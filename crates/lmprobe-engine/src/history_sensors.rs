//! `lmprobe history <verb>` sensors (Phase 3A §3.1).
//!
//! All sensors share the repo-wide commit cache (`history::*`) at
//! `$XDG_CACHE_HOME/lmprobe/history.db`. Cold-cache walks libgit2;
//! warm-cache reads SQLite. Incremental: only commits since the last
//! cached HEAD are re-walked.
//!
//! Doctrine adherence:
//!   • Default lightweight CLI surface (no feature flags).
//!   • Live filesystem is truth; only the XDG cache grows.
//!   • Content-addressable per (blob_sha, lang, sensor_version,
//!     engine_version, schema_version) for blob-keyed entries; per
//!     (repo_root, schema_version, head_sha) for repo-wide.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ast_grep_core::matcher::Pattern;
use ast_grep_language::{Language, LanguageExt};
use git2::{Repository, Sort};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::types::{Types, TypesBuilder};

use crate::history::{
  init_history_db, insert_blob_signature, insert_commit_row, last_cached_head, query_commits,
  read_blob_signature, repo_root_key, set_cached_head, xdg_cache_dir, xdg_history_db_path,
  BlobSignature, CommitRow,
};
use lmprobe_lang_core::langs::SgLang;
use lmprobe_registry::name_extractor_for;
use lmprobe_types::error::{anyhow, Context, Result};
use lmprobe_types::schema::{
  BlameEntry, BlameResult, ChurnEntry, CoChangeEntry, CommitHit, Diagnostic, DiffHit, ExitStatus,
  FileRename, HistoryChurnResult, HistoryCoChangeResult, HistoryCommitResult, HistoryDiffResult,
  HistoryTodosResult, IntroducedResult, LineContext, TodoEntry,
};

const COCHANGE_CAP: usize = 1000;

// ───────────────────────── duration parsing ───────────────────────

/// Parse `--since` durations: `30d`, `12h`, `2w`, `90m`, `45s`. Falls
/// back to None on parse error so callers can warn.
pub fn parse_since(s: &str) -> Option<i64> {
  let s = s.trim();
  if s.is_empty() {
    return None;
  }
  let (num, unit) = s.split_at(s.len() - 1);
  let n: i64 = num.parse().ok()?;
  let secs = match unit {
    "s" => n,
    "m" => n * 60,
    "h" => n * 3600,
    "d" => n * 86400,
    "w" => n * 86400 * 7,
    _ => return None,
  };
  Some(chrono::Utc::now().timestamp() - secs)
}

fn rfc3339(ts: i64) -> String {
  chrono::DateTime::from_timestamp(ts, 0)
    .map(|d| d.to_rfc3339())
    .unwrap_or_else(|| ts.to_string())
}

// ─────────────────────── repo-cache refresh ───────────────────────

/// Ensure the commit cache is current for `repo`. Walks any commits
/// since the last cached HEAD and inserts them; updates the HEAD
/// pointer. Returns the number of new commits walked.
pub fn refresh_commit_cache(
  conn: &rusqlite::Connection,
  repo: &Repository,
  repo_root: &str,
) -> Result<usize> {
  let head = repo.head()?.peel_to_commit()?;
  let head_sha = head.id().to_string();
  let last = last_cached_head(conn, repo_root)?;
  if last.as_deref() == Some(head_sha.as_str()) {
    return Ok(0);
  }

  // Walk newest → oldest. Stop at the previous cached HEAD if any.
  let mut revwalk = repo.revwalk()?;
  revwalk.push_head()?;
  revwalk.set_sorting(Sort::TIME)?;
  let stop_at = last.as_deref();

  // Defer `files_changed` to lazy computation — populating per-
  // commit during cold-walk is the dominant cost (`diff_tree_to_
  // tree` ~10ms × commits = seconds-to-minutes on busy repos).
  // Each verb's query path computes files_changed for the small
  // time-windowed subset it actually needs (see `populate_files`).
  //
  // Wrap inserts in a single transaction — N round-trips with
  // autocommit fsync vs one batched commit is the difference
  // between 8s and 0.4s on a 1k-commit repo.
  let mut new_commits = 0usize;
  conn.execute_batch("BEGIN")?;
  for oid_res in revwalk {
    let oid = oid_res?;
    if let Some(stop) = stop_at {
      if oid.to_string() == stop {
        break;
      }
    }
    let commit = match repo.find_commit(oid) {
      Ok(c) => c,
      Err(_) => continue,
    };
    let author_sig = commit.author();
    let row = CommitRow {
      commit_hash: oid.to_string(),
      author: author_sig.name().unwrap_or("unknown").to_string(),
      email: author_sig.email().unwrap_or("").to_string(),
      timestamp: commit.time().seconds(),
      message: commit.message().unwrap_or("").trim().to_string(),
      files_changed: Vec::new(),
    };
    if let Err(e) = insert_commit_row(conn, repo_root, &row) {
      conn.execute_batch("ROLLBACK").ok();
      return Err(e);
    }
    new_commits += 1;
  }
  conn.execute_batch("COMMIT")?;
  set_cached_head(conn, repo_root, &head_sha)?;
  Ok(new_commits)
}

/// Lazily fill `files_changed` for the given rows. `populate` is the
/// max number of rows we'll diff in this call; saves time on large
/// windows when only the top-N matter.
fn populate_files(repo: &Repository, rows: &mut [CommitRow], populate: usize) {
  let limit = rows.len().min(populate);
  for r in rows.iter_mut().take(limit) {
    if !r.files_changed.is_empty() {
      continue;
    }
    let oid = match git2::Oid::from_str(&r.commit_hash) {
      Ok(o) => o,
      Err(_) => continue,
    };
    let commit = match repo.find_commit(oid) {
      Ok(c) => c,
      Err(_) => continue,
    };
    if let Ok(files) = commit_files_changed(repo, &commit) {
      r.files_changed = files;
    }
  }
}

/// Compute the file-list touched by a commit relative to its first
/// parent (or to the empty tree for root commits).
fn commit_files_changed(repo: &Repository, commit: &git2::Commit) -> Result<Vec<String>> {
  let new_tree = commit.tree()?;
  let parent_tree = if commit.parent_count() > 0 {
    commit.parent(0).ok().and_then(|p| p.tree().ok())
  } else {
    None
  };
  let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None)?;
  let mut paths = Vec::new();
  diff.foreach(
    &mut |delta, _| {
      if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path()) {
        if let Some(s) = p.to_str() {
          paths.push(s.to_string());
        }
      }
      true
    },
    None,
    None,
    None,
  )?;
  Ok(paths)
}

/// Open the cache + ensure it's populated for the repo at `root`.
/// Returns the open repo, connection, and canonicalised root key.
fn open_cache_and_repo(root: &Path) -> Result<(Repository, rusqlite::Connection, String)> {
  let repo = Repository::discover(root).context("Could not find git repository")?;
  std::fs::create_dir_all(xdg_cache_dir())?;
  let conn = init_history_db(&xdg_history_db_path())?;
  let root_key = repo_root_key(root);
  Ok((repo, conn, root_key))
}

// ─────────────────────────── blame ────────────────────────────────

/// `history blame <symbol>` (symbol form) or `<file>:<line>` form.
/// Symbol form: collapse blame across the symbol's declaration block,
/// distinguishing INTRODUCED / LOGIC_CHANGE / FORMATTING via the
/// content-hash heuristic (Lead arbitration item E).
/// Back-compat wrapper used by `context_bundle::recent_history` —
/// equivalent to `blame_with_follow(target, root, false)`.
/// Back-compat wrapper used by `context_bundle::recent_history` —
/// equivalent to `blame_with_follow_semantic(target, root, false, false, false)`.
pub fn blame(target: &str, root: &Path) -> Result<BlameResult> {
  blame_with_follow_semantic(target, root, false, false, false)
}

/// `history blame <target> [--follow] [--semantic]`. When `semantic`
/// is set the LOGIC_CHANGE bucket is broken down into
/// BODY_REWRITE / PARAM_ADDED / PARAM_REMOVED / PARAM_RENAMED /
/// RETURN_TYPE_CHANGED. Best-effort heuristic on the AST-skeleton
/// string; cross-language parsing is intentionally pragmatic.
///
/// `no_cache=true` skips both reads from and writes to the
/// `blob_signatures` cache; behaviour is identical otherwise (just
/// slower, since every (blob, lang, symbol) re-parses).
pub fn blame_with_follow_semantic(
  target: &str,
  root: &Path,
  follow: bool,
  semantic: bool,
  no_cache: bool,
) -> Result<BlameResult> {
  blame_with_follow_semantic_around(target, root, follow, semantic, no_cache, None)
}

pub fn blame_with_follow_semantic_around(
  target: &str,
  root: &Path,
  follow: bool,
  semantic: bool,
  no_cache: bool,
  around: Option<usize>,
) -> Result<BlameResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;

  if let Some((file, line_str)) = target.rsplit_once(':') {
    if let Ok(line_num) = line_str.parse::<u32>() {
      let file = normalize_line_blame_file(&repo, file);
      return blame_line(&repo, &file, line_num, around);
    }
  }
  blame_symbol_with_follow_semantic(&repo, &conn, target, root, follow, semantic, no_cache)
}

fn normalize_line_blame_file(repo: &Repository, file: &str) -> String {
  let path = Path::new(file);
  if !path.is_absolute() {
    return file.to_string();
  }

  let Some(workdir) = repo.workdir() else {
    return file.to_string();
  };
  let Ok(root) = std::fs::canonicalize(workdir) else {
    return file.to_string();
  };
  let Ok(target) = std::fs::canonicalize(path) else {
    return file.to_string();
  };

  target
    .strip_prefix(root)
    .ok()
    .filter(|rel| !rel.as_os_str().is_empty())
    .map(|rel| rel.to_string_lossy().into_owned())
    .unwrap_or_else(|| file.to_string())
}

fn blame_line(
  repo: &Repository,
  file: &str,
  line: u32,
  around: Option<usize>,
) -> Result<BlameResult> {
  let path = Path::new(file);
  let mut opts = git2::BlameOptions::new();
  let blame = repo
    .blame_file(path, Some(&mut opts))
    .map_err(|e| anyhow!("git blame {}: {}", file, e))?;

  let mut entries: Vec<BlameEntry> = Vec::new();
  for hunk in blame.iter() {
    let final_start = hunk.final_start_line() as u32;
    let lines = hunk.lines_in_hunk() as u32;
    if line >= final_start && line < final_start + lines {
      let oid = hunk.final_commit_id();
      let commit = repo.find_commit(oid)?;
      let author = commit.author().name().unwrap_or("unknown").to_string();
      let timestamp = rfc3339(commit.time().seconds());
      let excerpt = read_file_at_commit(repo, &commit, path)
        .ok()
        .and_then(|content| {
          content
            .lines()
            .nth(line.saturating_sub(1) as usize)
            .map(|s| s.to_string())
        })
        .unwrap_or_default();
      entries.push(BlameEntry {
        commit_hash: oid.to_string(),
        author,
        timestamp,
        change_type: "LOGIC_CHANGE".to_string(),
        file_path: file.to_string(),
        line_start: Some(line),
        line_end: Some(line),
        excerpt,
      });
      break;
    }
  }
  let mut env = BlameResult::new(entries);
  if let Some(around) = around {
    env.source_context = line_source_context(repo, path, line, around);
  }
  Ok(env)
}

fn line_source_context(
  repo: &Repository,
  path: &Path,
  line: u32,
  around: usize,
) -> Vec<LineContext> {
  let content = repo
    .workdir()
    .map(|workdir| workdir.join(path))
    .and_then(|path| std::fs::read_to_string(path).ok())
    .or_else(|| {
      repo
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .and_then(|commit| read_file_at_commit(repo, &commit, path).ok())
    });
  let Some(content) = content else {
    return Vec::new();
  };
  let lines: Vec<&str> = content.lines().collect();
  if line == 0 || lines.is_empty() {
    return Vec::new();
  }
  let target_idx = line.saturating_sub(1) as usize;
  if target_idx >= lines.len() {
    return Vec::new();
  }
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  (start..end)
    .map(|idx| LineContext {
      line_number: idx + 1,
      line_content: lines[idx].to_string(),
    })
    .collect()
}

/// Look up `(blob_sha, lang, name)` in the `blob_signatures` cache;
/// on miss, parse the blob, extract the symbol's signature +
/// content-hash, persist the result, and return it. Phase 4A.
///
/// Returns `(content_hash, signature)`. Empty signature means "the
/// symbol is absent from this blob" — that absence is also cached
/// so subsequent walks skip the parse.
fn blob_signature_cached(
  conn: &rusqlite::Connection,
  blob: &git2::Blob<'_>,
  lang: SgLang,
  name: &str,
  no_cache: bool,
) -> (String, String, Option<u32>, Option<u32>) {
  let blob_sha = blob.id().to_string();
  let lang_label = format!("{:?}", lang);
  // §4.A — read path. `no_cache` skips both reads + writes so a
  // user passing `--no-cache` gets a true cold parse on every blob.
  if !no_cache {
    if let Ok(Some(cached)) = read_blob_signature(conn, &blob_sha, &lang_label, name) {
      return (
        cached.content_hash,
        cached.signature,
        cached.line_start,
        cached.line_end,
      );
    }
  }

  // Cache miss — parse + compute.
  let content = String::from_utf8_lossy(blob.content()).to_string();
  let grep = lang.ast_grep(content);
  let extractor = name_extractor_for(lang);
  let kind_set = lmprobe_registry::taxonomy_for(lang).all_declaration_kinds();

  let mut sig = String::new();
  let mut content_hash = String::new();
  let mut line_start: Option<u32> = None;
  let mut line_end: Option<u32> = None;
  for n in grep.root().dfs() {
    let nk: String = n.kind().to_string();
    if !kind_set.iter().any(|k| *k == nk.as_str()) {
      continue;
    }
    let m = ast_grep_core::NodeMatch::from(n);
    let extracted = extractor.extract(&m).unwrap_or_default();
    if extracted == name {
      sig = crate::index::create_skeleton(&m);
      content_hash = format!("{:016x}", fnv1a64(m.text().as_bytes()));
      // Phase 6B #3 — capture 1-based line range. ast-grep's
      // `start_pos().line()` is 0-based; +1 anchors to the
      // chapter-17 LineNumber ≥ 1 contract.
      line_start = Some(m.start_pos().line() as u32 + 1);
      line_end = Some(m.end_pos().line() as u32 + 1);
      break;
    }
  }

  // Persist (including the absence sentinel — empty signature).
  // `no_cache` short-circuits the write too.
  if !no_cache {
    let _ = insert_blob_signature(
      conn,
      &blob_sha,
      &lang_label,
      name,
      &BlobSignature {
        content_hash: content_hash.clone(),
        signature: sig.clone(),
        line_start,
        line_end,
      },
    );
  }
  (content_hash, sig, line_start, line_end)
}

// `blame_symbol` once existed as the no-follow entry; both arms now
// route through `blame_symbol_with_follow` directly so the wrapper
// would be dead code.

/// §4.E — when `follow` is set, walk the file's history allowing
/// renames. libgit2's `Diff::find_similar` (50% similarity default)
/// rewrites the diff so a renamed file's old path is reported via
/// `delta.old_file()`. We track those as we walk and re-target the
/// per-commit blob lookup at the historical name.
fn blame_symbol_with_follow_semantic(
  repo: &Repository,
  conn: &rusqlite::Connection,
  name: &str,
  user_root: &Path,
  follow: bool,
  semantic: bool,
  no_cache: bool,
) -> Result<BlameResult> {
  // Locate the symbol in HEAD. Phase 6B #1 — when the user passed
  // a FILE path, use it directly. Previously the search ran against
  // the whole workdir and `matches[0]` could land in a different
  // file containing the same symbol name, causing the WRONG file's
  // history to be walked — which manifested as 1-entry results on
  // real-world multi-commit files (battle-test report:
  // Oban/oban.ex, pandoc/HTML.hs:writeHtml, terraform main.tf etc).
  let workdir = repo.workdir().ok_or_else(|| anyhow!("bare repo"))?;
  let canon_workdir = std::fs::canonicalize(workdir)?;
  let canon_user = std::fs::canonicalize(user_root).unwrap_or_else(|_| user_root.to_path_buf());
  let walk_opts = crate::sensors::WalkOpts::default();

  let file_path: String = if canon_user.is_file() {
    // User pointed at a specific file: trust them; skip the
    // multi-file search entirely.
    canon_user
      .strip_prefix(&canon_workdir)
      .unwrap_or(&canon_user)
      .to_string_lossy()
      .into_owned()
  } else {
    // Directory mode: search within the user's directory (which
    // may be a sub-tree of the repo, not necessarily the workdir
    // root). matches[0]'s file becomes the walked file.
    let search_root = if canon_user.is_dir() {
      canon_user.clone()
    } else {
      canon_workdir.clone()
    };
    let (matches, _) = crate::sensors::search_symbol_hits(
      &search_root,
      true,
      crate::sensors::SearchConstraints {
        query: Some(name.to_string()),
        exact_name: true,
        ..Default::default()
      },
      &walk_opts,
    )?;
    if matches.is_empty() {
      return Ok(BlameResult::new(Vec::new()));
    }
    let abs_path = std::path::Path::new(&matches[0].file_path);
    abs_path
      .strip_prefix(&canon_workdir)
      .unwrap_or(abs_path)
      .to_string_lossy()
      .into_owned()
  };

  // §4.E — pre-pass: when `follow` is set, walk newest→oldest
  // along the file's history and record every rename hop libgit2
  // detects. Returns:
  //   • a path_at_commit lookup mapping commit_sha → path-in-that-
  //     commit (or None if file didn't exist there);
  //   • a follows: Vec<FileRename> chain for the envelope.
  let (path_at_commit, follows) = if follow {
    build_rename_chain(repo, &file_path)?
  } else {
    (HashMap::new(), Vec::new())
  };

  // Walk only commits that touch this file. libgit2's revwalk
  // doesn't natively path-filter; we walk all commits and skip
  // those whose tree doesn't contain the file or whose parent's
  // version of the file matches.
  let mut revwalk = repo.revwalk()?;
  revwalk.push_head()?;
  revwalk.set_sorting(Sort::TIME | Sort::REVERSE)?;

  let head_path_buf = std::path::PathBuf::from(&file_path);
  let mut entries: Vec<BlameEntry> = Vec::new();
  let mut last_signature = String::new();
  let mut last_content_hash = String::new();
  let mut last_blob_oid: Option<git2::Oid> = None;

  // Batch all `blob_signatures` writes into a single transaction.
  // Per-blob INSERT autocommit is the dominant cold-walk cost
  // (~2-3ms × N blobs); one BEGIN/COMMIT is microseconds.
  let _ = conn.execute_batch("BEGIN");

  for oid_res in revwalk {
    let oid = oid_res?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    // §4.E — when following renames, look up the historical
    // path for this commit; otherwise use the HEAD path.
    let effective_path: std::path::PathBuf = if follow {
      match path_at_commit.get(&oid.to_string()) {
        Some(Some(p)) => std::path::PathBuf::from(p),
        Some(None) => continue,        // file absent in this era
        None => head_path_buf.clone(), // commit pre-dates renames; use HEAD path
      }
    } else {
      head_path_buf.clone()
    };
    let entry = match tree.get_path(&effective_path) {
      Ok(e) => e,
      Err(_) => continue,
    };
    // Skip commits that didn't change this file (same blob).
    if Some(entry.id()) == last_blob_oid {
      continue;
    }
    last_blob_oid = Some(entry.id());

    let lang = match SgLang::from_path(&effective_path) {
      Some(l) => l,
      None => continue,
    };
    let blob = match repo.find_blob(entry.id()) {
      Ok(b) => b,
      Err(_) => continue,
    };
    // Phase 4A — `blob_signatures` cache. The first walk on a
    // given (blob, lang, symbol) parses; every subsequent walk
    // (same blob across commits, OR same blob across CLI
    // invocations via the persistent SQLite store) is a single
    // SQLite read.
    let (content_hash, sig, line_start, line_end) =
      blob_signature_cached(conn, &blob, lang, name, no_cache);
    if sig.is_empty() {
      // The file changed but the symbol isn't here yet (or got
      // removed). If it was previously present, that earlier
      // entry stands; we don't emit a removal milestone in 3A.
      continue;
    }
    let entry_path = effective_path.to_string_lossy().into_owned();
    if last_signature.is_empty() {
      entries.push(make_entry(
        &commit,
        oid,
        "INTRODUCED",
        &entry_path,
        &sig,
        line_start,
        line_end,
      ));
    } else if content_hash != last_content_hash {
      let change = if sig != last_signature {
        if semantic {
          classify_signature_change(&last_signature, &sig)
        } else {
          "LOGIC_CHANGE"
        }
      } else if semantic {
        // Same skeleton, different content-hash → body-only edit.
        "BODY_REWRITE"
      } else {
        "FORMATTING"
      };
      entries.push(make_entry(
        &commit,
        oid,
        change,
        &entry_path,
        &sig,
        line_start,
        line_end,
      ));
    }
    last_signature = sig;
    last_content_hash = content_hash;
  }

  let _ = conn.execute_batch("COMMIT");
  entries.reverse(); // newest-first for output
  let mut env = BlameResult::new(entries);
  env.follows = follows;
  Ok(env)
}

/// §4.E — pre-pass to detect file renames along the history of
/// `head_path`. Walks newest→oldest, diffing each commit vs its
/// parent with rename detection enabled (`Diff::find_similar` at
/// libgit2's default 50% similarity threshold). When the current
/// path appears as the "new file" of a rename delta, the older
/// path becomes the new `current_path`.
///
/// Returns:
///   • `path_at_commit` — sparse map keyed on commit_sha. A commit
///     present in the map at value `Some(p)` means "the historical
///     path during this commit/era is `p`"; absent means "use the
///     HEAD path or earlier-era path"; `None` means "the file did
///     not exist yet at this commit" (treated as skip in the walk).
///   • `follows` — the chain of FileRename hops detected.
///
/// Heuristic limitation: this catches whole-file renames but NOT
/// cross-symbol-name renames (e.g. `Pattern` → `AstPattern` in the
/// same file). Documented in plan_corrections.md.
fn build_rename_chain(
  repo: &Repository,
  head_path: &str,
) -> Result<(HashMap<String, Option<String>>, Vec<FileRename>)> {
  let mut path_at_commit: HashMap<String, Option<String>> = HashMap::new();
  let mut follows: Vec<FileRename> = Vec::new();

  let mut current_path = head_path.to_string();
  let mut revwalk = repo.revwalk()?;
  revwalk.push_head()?;
  revwalk.set_sorting(Sort::TIME)?; // newest → oldest

  for oid_res in revwalk {
    let oid = oid_res?;
    let commit = repo.find_commit(oid)?;
    path_at_commit.insert(oid.to_string(), Some(current_path.clone()));

    if commit.parent_count() == 0 {
      break;
    }
    let parent = match commit.parent(0) {
      Ok(p) => p,
      Err(_) => break,
    };
    let new_tree = commit.tree()?;
    let old_tree = parent.tree()?;
    let mut diff = match repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None) {
      Ok(d) => d,
      Err(_) => continue,
    };
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true).copies(false);
    let _ = diff.find_similar(Some(&mut find_opts));

    let mut renamed_to: Option<String> = None;
    let _ = diff.foreach(
      &mut |delta, _| {
        let new_p = delta
          .new_file()
          .path()
          .and_then(|p| p.to_str())
          .unwrap_or("");
        let old_p = delta
          .old_file()
          .path()
          .and_then(|p| p.to_str())
          .unwrap_or("");
        if delta.status() == git2::Delta::Renamed && new_p == current_path && !old_p.is_empty() {
          renamed_to = Some(old_p.to_string());
        }
        true
      },
      None,
      None,
      None,
    );
    if let Some(older) = renamed_to {
      follows.push(FileRename {
        from: older.clone(),
        to: current_path.clone(),
        commit: oid.to_string(),
      });
      current_path = older;
    }
  }
  // The chain we've built reads newest→oldest; expose
  // oldest→newest for human-friendly rendering.
  follows.reverse();
  Ok((path_at_commit, follows))
}

/// §4.3 — semantic classification of a signature-changing commit.
/// Best-effort heuristic on the AST-skeleton string (e.g.
/// `pub fn foo(x: i32) -> bool { ... }`):
///
///   • RETURN_TYPE_CHANGED — text after `->` differs.
///   • PARAM_ADDED         — paren-balanced argument count grew.
///   • PARAM_REMOVED       — count shrank.
///   • PARAM_RENAMED       — count same but param substring differs.
///   • LOGIC_CHANGE        — fallback when none of the above fit.
///
/// Cross-language: works on Rust / Java / Go / C / C++ / JS / TS /
/// Python / Kotlin / Scala-shaped signatures. Heuristic returns
/// "LOGIC_CHANGE" on languages whose skeletons don't conform.
fn classify_signature_change(prev: &str, curr: &str) -> &'static str {
  let prev_ret = ret_type_after_arrow(prev);
  let curr_ret = ret_type_after_arrow(curr);
  let ret_changed = prev_ret != curr_ret;

  let prev_params = param_list(prev);
  let curr_params = param_list(curr);
  let prev_count = count_top_level_commas(&prev_params)
    .saturating_add(if prev_params.trim().is_empty() { 0 } else { 1 });
  let curr_count = count_top_level_commas(&curr_params)
    .saturating_add(if curr_params.trim().is_empty() { 0 } else { 1 });

  if ret_changed && prev_count == curr_count {
    return "RETURN_TYPE_CHANGED";
  }
  if curr_count > prev_count {
    return "PARAM_ADDED";
  }
  if curr_count < prev_count {
    return "PARAM_REMOVED";
  }
  if prev_params != curr_params {
    return "PARAM_RENAMED";
  }
  "LOGIC_CHANGE"
}

/// Return the substring between the OUTERMOST balanced parens, or
/// "" if no parens are present (e.g. constants / type aliases).
fn param_list(sig: &str) -> String {
  let bytes = sig.as_bytes();
  let mut depth = 0i32;
  let mut start = None;
  for (i, &b) in bytes.iter().enumerate() {
    if b == b'(' {
      if depth == 0 {
        start = Some(i + 1);
      }
      depth += 1;
    } else if b == b')' {
      depth -= 1;
      if depth == 0 {
        if let Some(s) = start {
          return sig[s..i].to_string();
        }
      }
    }
  }
  String::new()
}

/// Count top-level commas (i.e. ignore commas inside nested
/// generics / types like `HashMap<K, V>`).
fn count_top_level_commas(s: &str) -> usize {
  let mut depth = 0i32;
  let mut n = 0;
  for c in s.chars() {
    match c {
      '<' | '[' | '(' | '{' => depth += 1,
      '>' | ']' | ')' | '}' => depth -= 1,
      ',' if depth == 0 => n += 1,
      _ => {}
    }
  }
  n
}

/// Extract the substring after the first top-level `->` arrow, or
/// "" if the language doesn't use one (Python's annotation syntax,
/// Java's pre-method type, etc.).
fn ret_type_after_arrow(sig: &str) -> String {
  if let Some(idx) = sig.find("->") {
    sig[idx + 2..]
      .trim()
      .split('{')
      .next()
      .unwrap_or("")
      .trim()
      .to_string()
  } else {
    String::new()
  }
}

fn make_entry(
  commit: &git2::Commit,
  oid: git2::Oid,
  change_type: &str,
  file_path: &str,
  sig: &str,
  line_start: Option<u32>,
  line_end: Option<u32>,
) -> BlameEntry {
  BlameEntry {
    commit_hash: oid.to_string(),
    author: commit.author().name().unwrap_or("unknown").to_string(),
    timestamp: rfc3339(commit.time().seconds()),
    change_type: change_type.to_string(),
    file_path: file_path.to_string(),
    line_start,
    line_end,
    excerpt: sig.to_string(),
  }
}

fn read_file_at_commit(repo: &Repository, commit: &git2::Commit, path: &Path) -> Result<String> {
  let entry = commit.tree()?.get_path(path)?;
  let blob = repo.find_blob(entry.id())?;
  Ok(String::from_utf8_lossy(blob.content()).to_string())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
  let mut h: u64 = 0xcbf2_9ce4_8422_2325;
  for b in bytes {
    h ^= *b as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
  }
  h
}

// ───────────────────────── introduced ─────────────────────────────

/// `history introduced <name>` — first AST appearance commit. Same
/// underlying walk as `blame_symbol`; we just keep only the first
/// entry. Phase 3A is exact-name; rename-aware tracking is Phase 4.
pub fn introduced_with_follow(
  name: &str,
  root: &Path,
  _lang_hint: Option<&str>,
  follow: bool,
  no_cache: bool,
) -> Result<IntroducedResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let blame = blame_symbol_with_follow_semantic(&repo, &conn, name, root, follow, false, no_cache)?;
  // INTRODUCED is the OLDEST entry (after newest-first reversal,
  // it's at the tail).
  let intro: Vec<_> = blame
    .entries
    .into_iter()
    .filter(|e| e.change_type == "INTRODUCED")
    .collect();
  Ok(IntroducedResult::new(intro))
}

// ─────────────────────────── commit ───────────────────────────────

/// `history commit -p <pat> [--since X] [--author A]`. Filters cached
/// commits by message-substring, then optionally by AST pattern in
/// any file diff. With no `-p`, returns all commits in the window.
pub fn history_commit(
  pattern: Option<&str>,
  since: Option<&str>,
  author: Option<&str>,
  root: &Path,
) -> Result<HistoryCommitResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let since_ts = since.and_then(parse_since);
  let mut rows = query_commits(&conn, &root_key, since_ts, author)?;
  // Pattern-in-diff matching needs files_changed; message-only
  // matching doesn't. Be conservative and populate when -p is set.
  if pattern.is_some() {
    let n = rows.len();
    populate_files(&repo, &mut rows, n);
  }
  let pat_lower = pattern.map(|p| p.to_lowercase());

  let mut hits: Vec<CommitHit> = Vec::new();
  for r in rows {
    let mut matched_in_msg = false;
    if let Some(ref p) = pat_lower {
      if r.message.to_lowercase().contains(p) {
        matched_in_msg = true;
      }
    }
    let matched_files = if !matched_in_msg && pattern.is_some() {
      let pat = pattern.unwrap();
      commit_files_with_pattern(&repo, &r.commit_hash, &r.files_changed, pat)?
    } else if matched_in_msg {
      r.files_changed.clone()
    } else {
      // No pattern → keep all.
      r.files_changed.clone()
    };
    let keep = pattern.is_none() || matched_in_msg || !matched_files.is_empty();
    if !keep {
      continue;
    }
    hits.push(CommitHit {
      commit_hash: r.commit_hash,
      author: r.author,
      timestamp: rfc3339(r.timestamp),
      message: r.message,
      matched_files,
    });
  }
  Ok(HistoryCommitResult::new(hits))
}

/// Return paths in `files` that contain `pattern` in their content at
/// `commit_sha`. Uses ast-grep when the path's lang is known, else
/// falls back to a literal substring match.
fn commit_files_with_pattern(
  repo: &Repository,
  commit_sha: &str,
  files: &[String],
  pattern: &str,
) -> Result<Vec<String>> {
  let oid = git2::Oid::from_str(commit_sha)?;
  let commit = repo.find_commit(oid)?;
  let tree = commit.tree()?;
  let mut out = Vec::new();
  for f in files {
    let path = std::path::Path::new(f);
    let entry = match tree.get_path(path) {
      Ok(e) => e,
      Err(_) => continue,
    };
    let blob = match repo.find_blob(entry.id()) {
      Ok(b) => b,
      Err(_) => continue,
    };
    let content = String::from_utf8_lossy(blob.content()).to_string();
    let matched = if let Some(lang) = SgLang::from_path(path) {
      let pat = match Pattern::try_new(pattern, lang) {
        Ok(p) => p,
        Err(_) => continue,
      };
      let grep = lang.ast_grep(content);
      let any = grep.root().find_all(pat).next().is_some();
      any
    } else {
      false
    };
    if matched {
      out.push(f.clone());
    } else if path
      .extension()
      .and_then(|e| e.to_str())
      .map(|e| !is_text_ext(e))
      .unwrap_or(true)
    {
      // unknown lang — skip literal-match fallback to keep the
      // pattern semantic (no false positives on binary blobs).
    }
  }
  Ok(out)
}

fn is_text_ext(ext: &str) -> bool {
  matches!(
    ext,
    "rs"
      | "py"
      | "go"
      | "js"
      | "ts"
      | "tsx"
      | "jsx"
      | "java"
      | "kt"
      | "rb"
      | "php"
      | "swift"
      | "scala"
      | "lua"
      | "ex"
      | "exs"
      | "hs"
      | "c"
      | "cc"
      | "cpp"
      | "h"
      | "hpp"
      | "cs"
      | "sh"
      | "bash"
      | "nix"
      | "tf"
      | "sol"
      | "dart"
      | "json"
      | "yaml"
      | "yml"
      | "toml"
      | "xml"
      | "html"
      | "css"
      | "md"
      | "txt"
  )
}

// ──────────────────────────── diff ────────────────────────────────

/// `history diff -p <pat> [--since X]` — for each commit in window,
/// pair the matched-pattern's before/after text from the commit's
/// parent → new-tree diff.
pub fn history_diff(pattern: &str, since: Option<&str>, root: &Path) -> Result<HistoryDiffResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let since_ts = since.and_then(parse_since);
  let mut rows = query_commits(&conn, &root_key, since_ts, None)?;
  let n = rows.len();
  populate_files(&repo, &mut rows, n);

  let mut hits: Vec<DiffHit> = Vec::new();
  for r in rows {
    let oid = match git2::Oid::from_str(&r.commit_hash) {
      Ok(o) => o,
      Err(_) => continue,
    };
    let commit = match repo.find_commit(oid) {
      Ok(c) => c,
      Err(_) => continue,
    };
    let new_tree = match commit.tree() {
      Ok(t) => t,
      Err(_) => continue,
    };
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    for f in &r.files_changed {
      let path = std::path::Path::new(f);
      let lang = match SgLang::from_path(path) {
        Some(l) => l,
        None => continue,
      };
      let after = blob_text_at(&repo, &new_tree, path).unwrap_or_default();
      let before = parent_tree
        .as_ref()
        .and_then(|t| blob_text_at(&repo, t, path).ok())
        .unwrap_or_default();
      let pat_after = match Pattern::try_new(pattern, lang) {
        Ok(p) => p,
        Err(_) => continue,
      };
      let pat_before = match Pattern::try_new(pattern, lang) {
        Ok(p) => p,
        Err(_) => continue,
      };
      let after_match: Option<String> = {
        let grep = lang.ast_grep(after.clone());
        let s = grep
          .root()
          .find_all(pat_after)
          .next()
          .map(|m| m.text().to_string());
        s
      };
      let before_match: Option<String> = {
        let grep = lang.ast_grep(before.clone());
        let s = grep
          .root()
          .find_all(pat_before)
          .next()
          .map(|m| m.text().to_string());
        s
      };
      if after_match.is_some() || before_match.is_some() {
        hits.push(DiffHit {
          commit_hash: r.commit_hash.clone(),
          author: r.author.clone(),
          timestamp: rfc3339(r.timestamp),
          file_path: f.clone(),
          before: before_match.unwrap_or_default(),
          after: after_match.unwrap_or_default(),
        });
      }
    }
  }
  Ok(HistoryDiffResult::new(hits))
}

fn blob_text_at(repo: &Repository, tree: &git2::Tree, path: &Path) -> Result<String> {
  let entry = tree.get_path(path)?;
  let blob = repo.find_blob(entry.id())?;
  Ok(String::from_utf8_lossy(blob.content()).to_string())
}

// ──────────────────────────── churn ───────────────────────────────

/// `history churn --top N --since X` — files with the most commits.
pub fn churn(top: usize, since: Option<&str>, root: &Path) -> Result<HistoryChurnResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let since_ts = since.and_then(parse_since);
  let mut rows = query_commits(&conn, &root_key, since_ts, None)?;
  let n = rows.len();
  populate_files(&repo, &mut rows, n);

  let mut counts: HashMap<String, (u32, i64)> = HashMap::new();
  for r in rows {
    for f in r.files_changed {
      let entry = counts.entry(f).or_insert((0, 0));
      entry.0 += 1;
      if r.timestamp > entry.1 {
        entry.1 = r.timestamp;
      }
    }
  }
  let mut entries: Vec<ChurnEntry> = counts
    .into_iter()
    .map(|(path, (count, last))| ChurnEntry {
      path,
      commit_count: count,
      last_touched: rfc3339(last),
    })
    .collect();
  entries.sort_by(|a, b| {
    b.commit_count
      .cmp(&a.commit_count)
      .then_with(|| a.path.cmp(&b.path))
  });
  entries.truncate(top);
  let mut env = HistoryChurnResult::new(entries);
  push_shallow_history_warning(&repo, &mut env, "history churn");
  Ok(env)
}

// ────────────────────────── co-change ─────────────────────────────

/// `history co-change <file> [--since X]` — files most often changing
/// together with `file`. Cap of 1000 commits (Lead arbitration B);
/// emits HISTORY_TRUNCATED when exceeded.
pub fn co_change(file: &str, since: Option<&str>, root: &Path) -> Result<HistoryCoChangeResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let since_ts = since.and_then(parse_since);
  let mut rows = query_commits(&conn, &root_key, since_ts, None)?;

  let truncated = rows.len() > COCHANGE_CAP;
  if truncated {
    rows.truncate(COCHANGE_CAP);
  }
  let n = rows.len();
  populate_files(&repo, &mut rows, n);

  let mut target_commits: HashSet<String> = HashSet::new();
  let mut co_counts: HashMap<String, u32> = HashMap::new();
  let mut all_for_target = 0u32;
  for r in &rows {
    if r.files_changed.iter().any(|f| f == file) {
      target_commits.insert(r.commit_hash.clone());
      all_for_target += 1;
      for f in &r.files_changed {
        if f != file {
          *co_counts.entry(f.clone()).or_insert(0) += 1;
        }
      }
    }
  }
  let mut entries: Vec<CoChangeEntry> = co_counts
    .into_iter()
    .map(|(path, n)| {
      let score = if all_for_target == 0 {
        0.0
      } else {
        n as f32 / all_for_target as f32
      };
      CoChangeEntry {
        path,
        co_occurrences: n,
        score,
      }
    })
    .collect();
  entries.sort_by(|a, b| {
    b.co_occurrences
      .cmp(&a.co_occurrences)
      .then_with(|| a.path.cmp(&b.path))
  });
  let mut env = HistoryCoChangeResult::new(entries);
  if truncated {
    env.push_warning(
      Diagnostic::new(
        "HISTORY_TRUNCATED",
        format!(
          "co-change scan capped at {} commits; pass `--since` to narrow",
          COCHANGE_CAP
        ),
      )
      .with_hint("see plan_corrections.md item B"),
    );
  }
  push_shallow_history_warning(&repo, &mut env, "history co-change");
  Ok(env)
}

fn shallow_history_diagnostic(verb: &str) -> Diagnostic {
  Diagnostic::new(
    "HISTORY_SHALLOW",
    format!("{verb} is running on a shallow or grafted checkout; history evidence may be incomplete"),
  )
  .with_hint("fetch more history before relying on churn or co-change evidence, for example `git fetch --unshallow`")
}

trait HistoryWarningSink {
  fn push_history_warning(&mut self, diagnostic: Diagnostic);
}

impl HistoryWarningSink for HistoryChurnResult {
  fn push_history_warning(&mut self, diagnostic: Diagnostic) {
    self.push_warning(diagnostic);
  }
}

impl HistoryWarningSink for HistoryCoChangeResult {
  fn push_history_warning(&mut self, diagnostic: Diagnostic) {
    self.push_warning(diagnostic);
  }
}

fn push_shallow_history_warning<T: HistoryWarningSink>(repo: &Repository, env: &mut T, verb: &str) {
  if repo.is_shallow() {
    env.push_history_warning(shallow_history_diagnostic(verb));
  }
}

pub fn filter_co_change_result(
  mut env: HistoryCoChangeResult,
  top: Option<usize>,
  min_score: Option<f32>,
  exclude_globs: &[String],
  type_filter: Option<&str>,
) -> HistoryCoChangeResult {
  if let Some(min_score) = min_score {
    env.rows.retain(|entry| entry.score >= min_score);
  }
  let exclude_matcher = build_co_change_exclude_matcher(exclude_globs);
  let type_matcher = build_co_change_type_matcher(type_filter);
  if exclude_matcher.is_some() || type_matcher.is_some() {
    env.rows.retain(|entry| {
      co_change_path_allowed_by_excludes(&entry.path, exclude_matcher.as_ref())
        && co_change_path_allowed_by_type(&entry.path, type_matcher.as_ref())
    });
  }
  if let Some(top) = top {
    env.rows.truncate(top);
  }
  env.exit_status = ExitStatus::from_count(env.rows.len());
  env
}

fn build_co_change_exclude_matcher(exclude_globs: &[String]) -> Option<Override> {
  if exclude_globs.is_empty() {
    return None;
  }
  let mut builder = OverrideBuilder::new(Path::new(""));
  for glob in exclude_globs {
    let _ = builder.add(&format!("!{}", glob));
  }
  builder.build().ok()
}

fn build_co_change_type_matcher(type_filter: Option<&str>) -> Option<Types> {
  let type_filter = type_filter?.trim();
  if type_filter.is_empty() {
    return None;
  }
  let mut builder = TypesBuilder::new();
  builder.add_defaults();
  let _ = builder.select(type_filter);
  builder.build().ok()
}

fn co_change_path_allowed_by_excludes(path: &str, matcher: Option<&Override>) -> bool {
  !matcher
    .map(|matcher| matcher.matched(Path::new(path), false).is_ignore())
    .unwrap_or(false)
}

fn co_change_path_allowed_by_type(path: &str, matcher: Option<&Types>) -> bool {
  matcher
    .map(|matcher| matcher.matched(Path::new(path), false).is_whitelist())
    .unwrap_or(true)
}

// ──────────────────────────── todos ───────────────────────────────

/// `history todos [--since X]` — TODO/FIXME/XXX/HACK markers added
/// in the window. Walk each commit's diff; capture `+`-side lines.
pub fn todos(since: Option<&str>, root: &Path) -> Result<HistoryTodosResult> {
  let (repo, conn, root_key) = open_cache_and_repo(root)?;
  let _ = refresh_commit_cache(&conn, &repo, &root_key)?;
  let since_ts = since.and_then(parse_since);
  let rows = query_commits(&conn, &root_key, since_ts, None)?;

  let re = regex::Regex::new(r"\b(TODO|FIXME|XXX|HACK)\b[:(]?\s*(.*)$").unwrap();
  let mut hits: Vec<TodoEntry> = Vec::new();

  for r in rows {
    let oid = match git2::Oid::from_str(&r.commit_hash) {
      Ok(o) => o,
      Err(_) => continue,
    };
    let commit = match repo.find_commit(oid) {
      Ok(c) => c,
      Err(_) => continue,
    };
    let new_tree = match commit.tree() {
      Ok(t) => t,
      Err(_) => continue,
    };
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None) {
      Ok(d) => d,
      Err(_) => continue,
    };
    let r_ref = &r;
    let _ = diff.foreach(
      &mut |_, _| true,
      None,
      None,
      Some(&mut |delta, _hunk, line| {
        if line.origin() != '+' {
          return true;
        }
        let text = String::from_utf8_lossy(line.content());
        let path = delta
          .new_file()
          .path()
          .and_then(|p| p.to_str())
          .unwrap_or("")
          .to_string();
        let lineno = line.new_lineno().unwrap_or(0);
        if let Some(cap) = re.captures(&text) {
          let marker = cap.get(1).map(|m| m.as_str()).unwrap_or("TODO").to_string();
          let body = cap
            .get(2)
            .map(|m| m.as_str().trim())
            .unwrap_or("")
            .to_string();
          hits.push(TodoEntry {
            commit_hash: r_ref.commit_hash.clone(),
            author: r_ref.author.clone(),
            timestamp: rfc3339(r_ref.timestamp),
            file_path: path,
            line_number: lineno,
            marker,
            text: body,
          });
        }
        true
      }),
    );
  }
  Ok(HistoryTodosResult::new(hits))
}

// ────────────────────── path-cwd resolver ─────────────────────────

/// Convert a CLI `path: PathBuf` to an absolute root suitable for
/// `Repository::discover` + cache keying.
pub fn resolve_root(path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::env::current_dir()
      .map(|d| d.join(path))
      .unwrap_or_else(|_| path.to_path_buf())
  }
}
