//! Phase 4B §4.1 — `lmprobe fix <pattern> <replacement>` dry-run.
//!
//! Read-only diff preview. Walks the workspace, parses each file
//! whose content contains the pattern, applies `replace_all` against
//! the AST, and emits a unified-diff-style envelope.
//!
//! `--apply` is intentionally NOT in this chunk — gated on §4.2's
//! worktree primitive (lands in 4C).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc, Mutex};

use ast_grep_language::{Language, LanguageExt};
use git2::{Repository, StatusOptions};
use ignore::WalkBuilder;
use std::fs;

use crate::sensors::{apply_walk_opts, walk_pre_check, PreCheck, WalkOpts};
use lmprobe_lang_core::langs::SgLang;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{Diagnostic, DiffHunk, FileDiff, FixResult};

pub fn fix_dry_run(
  pattern: &str,
  replacement: &str,
  root: &Path,
  lang_hint: Option<SgLang>,
  opts: &WalkOpts,
) -> Result<FixResult> {
  let (tx, rx) = mpsc::channel();
  let too_large: Arc<Mutex<Vec<Diagnostic>>> = Arc::new(Mutex::new(Vec::new()));
  let pattern_owned = pattern.to_string();
  let replacement_owned = replacement.to_string();

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  let walker = wb.build_parallel();
  walker.run(|| {
    let tx = tx.clone();
    let too_large = too_large.clone();
    let pattern = pattern_owned.clone();
    let replacement = replacement_owned.clone();
    let opts = opts;
    Box::new(move |result| {
      if let Ok(entry) = result {
        let (path_buf, lang) = match walk_pre_check(&entry, opts) {
          PreCheck::Process(p, l) => (p, l),
          PreCheck::Skip => return ignore::WalkState::Continue,
          PreCheck::SkipWithDiag(d) => {
            too_large.lock().unwrap().push(d);
            return ignore::WalkState::Continue;
          }
        };
        let path = path_buf.as_path();
        if let Some(hint) = lang_hint {
          if hint != lang {
            return ignore::WalkState::Continue;
          }
        }
        let content = match fs::read_to_string(path) {
          Ok(c) => c,
          Err(_) => return ignore::WalkState::Continue,
        };
        let grep = lang.ast_grep(content.clone());
        let edits = grep
          .root()
          .replace_all(pattern.as_str(), replacement.as_str());
        if edits.is_empty() {
          return ignore::WalkState::Continue;
        }

        // Build the rewritten source and the per-edit hunk list.
        let mut hunks: Vec<DiffHunk> = Vec::with_capacity(edits.len());
        let mut rewritten = grep.source().to_string();
        // Apply edits in reverse so byte-offsets remain valid.
        let mut sorted = edits;
        sorted.sort_by(|a, b| a.position.cmp(&b.position));
        for e in &sorted {
          let start = e.position;
          let end = e.position + e.deleted_length;
          let before = grep.source().get(start..end).unwrap_or("").to_string();
          let after = String::from_utf8_lossy(&e.inserted_text).into_owned();
          let line_start = (grep.source()[..start].matches('\n').count() as u32) + 1;
          let line_count = before.matches('\n').count().max(1) as u32;
          hunks.push(DiffHunk {
            line_start,
            line_count,
            before,
            after,
          });
        }
        for e in sorted.into_iter().rev() {
          rewritten.replace_range(
            e.position..e.position + e.deleted_length,
            &String::from_utf8_lossy(&e.inserted_text),
          );
        }
        let _ = rewritten; // currently unused for dry-run; populated for 4C apply.
        let matches_count = hunks.len() as u32;
        let _ = tx.send(FileDiff {
          file_path: path.to_string_lossy().into_owned(),
          hunks,
          matches_count,
        });
      }
      ignore::WalkState::Continue
    })
  });
  drop(tx);

  let mut diffs: Vec<FileDiff> = rx.into_iter().collect();
  diffs.sort_by(|a, b| a.file_path.cmp(&b.file_path));

  let mut env = FixResult::new("dry_run", diffs);
  for d in std::mem::take(&mut *too_large.lock().unwrap()) {
    env.push_warning(d);
  }
  Ok(env)
}

/// §4.1 — `lmprobe fix --apply`. Provisions a fresh `git
/// worktree`, runs `replace_all` there, captures the diff as a
/// `.patch`, and applies it atomically to the user's tree.
/// Refuses dirty tree without `--force` (mirrors trace's contract).
/// Audit log written to `$XDG_CACHE_HOME/lmprobe/fix-<ts>.log`.
pub fn fix_apply(
  pattern: &str,
  replacement: &str,
  root: &Path,
  lang_hint: Option<SgLang>,
  force: bool,
  opts: &WalkOpts,
) -> Result<FixResult> {
  // 1. Safety: refuse dirty / non-git unless forced.
  if let Err(diag) = check_fix_safety(root, force) {
    let mut env = FixResult::new("applied", Vec::new());
    env.exit_status = lmprobe_types::schema::ExitStatus::Error;
    env.push_warning(diag);
    return Ok(env);
  }

  // 2. Dry-run first to collect diffs (errors out before any
  //    worktree work if the pattern is malformed).
  let preview = fix_dry_run(pattern, replacement, root, lang_hint, opts)?;
  if preview.matches_count == 0 {
    let mut env = FixResult::new("applied", Vec::new());
    env.exit_status = lmprobe_types::schema::ExitStatus::NoMatch;
    return Ok(env);
  }

  // 3. Audit log BEFORE any tree mutation.
  let audit_path = match write_fix_audit_log(&preview) {
    Ok(p) => Some(p),
    Err(d) => {
      let mut env = preview.clone();
      env.mode = "applied".to_string();
      env.exit_status = lmprobe_types::schema::ExitStatus::Error;
      env.push_warning(d);
      return Ok(env);
    }
  };

  // 4. Provision worktree at HEAD.
  let wt = match provision_fix_worktree(root) {
    Ok(w) => w,
    Err(d) => {
      let mut env = preview.clone();
      env.mode = "applied".to_string();
      env.exit_status = lmprobe_types::schema::ExitStatus::Error;
      env.push_warning(d);
      return Ok(env);
    }
  };

  // 5. Apply rewrites in the worktree.
  let mut applied_in_wt: u32 = 0;
  for diff in &preview.diffs {
    let rel = diff
      .file_path
      .strip_prefix(&format!("{}/", root.display()))
      .or_else(|| diff.file_path.strip_prefix("./"))
      .unwrap_or(&diff.file_path);
    let wt_path = wt.path.join(rel);
    let lang = match SgLang::from_path(&wt_path) {
      Some(l) => l,
      None => continue,
    };
    if let Some(hint) = lang_hint {
      if hint != lang {
        continue;
      }
    }
    let content = match fs::read_to_string(&wt_path) {
      Ok(c) => c,
      Err(_) => continue,
    };
    let grep = lang.ast_grep(content);
    let edits = grep.root().replace_all(pattern, replacement);
    if edits.is_empty() {
      continue;
    }
    let mut sorted = edits;
    sorted.sort_by(|a, b| a.position.cmp(&b.position));
    let mut rewritten = grep.source().to_string();
    for e in sorted.into_iter().rev() {
      rewritten.replace_range(
        e.position..e.position + e.deleted_length,
        &String::from_utf8_lossy(&e.inserted_text),
      );
    }
    if fs::write(&wt_path, rewritten.as_bytes()).is_ok() {
      applied_in_wt += 1;
    }
  }

  // 6. Capture patch (worktree → user tree) and apply atomically.
  let patch_path = match capture_fix_patch(&wt) {
    Ok(p) => p,
    Err(d) => {
      let _ = remove_fix_worktree(&wt);
      let mut env = preview.clone();
      env.mode = "applied".to_string();
      env.exit_status = lmprobe_types::schema::ExitStatus::Error;
      env.push_warning(d);
      return Ok(env);
    }
  };
  let apply_outcome = apply_patch_to_root(root, &patch_path);

  // 7. Cleanup worktree.
  let _ = remove_fix_worktree(&wt);

  let mut env = preview.clone();
  env.mode = "applied".to_string();
  env.applied_files_count = applied_in_wt;
  env.patch_path = Some(patch_path.to_string_lossy().into_owned());
  env.audit_log_path = audit_path.map(|p| p.to_string_lossy().into_owned());

  if let Err(diag) = apply_outcome {
    env.exit_status = lmprobe_types::schema::ExitStatus::Error;
    env.push_warning(diag);
  }
  Ok(env)
}

fn check_fix_safety(root: &Path, force: bool) -> std::result::Result<(), Diagnostic> {
  if force {
    return Ok(());
  }
  let repo = match Repository::discover(root) {
    Ok(r) => r,
    Err(_) => {
      return Err(Diagnostic::new(
        "NOT_GIT_REPO",
        "fix --apply requires a git working tree (or pass --force)",
      ));
    }
  };
  let mut opts = StatusOptions::new();
  opts.include_untracked(true);
  opts.include_ignored(false);
  let statuses = match repo.statuses(Some(&mut opts)) {
    Ok(s) => s,
    Err(e) => {
      return Err(Diagnostic::new(
        "GIT_STATUS_FAILED",
        format!("could not read git status: {}", e),
      ));
    }
  };
  if !statuses.is_empty() {
    return Err(
      Diagnostic::new("DIRTY_TREE", "fix --apply refuses a dirty working tree")
        .with_hint("commit/stash your changes, or pass --force"),
    );
  }
  Ok(())
}

struct FixWorktree {
  path: PathBuf,
  parent: PathBuf,
}

fn provision_fix_worktree(root: &Path) -> std::result::Result<FixWorktree, Diagnostic> {
  let parent = match Repository::discover(root) {
    Ok(repo) => repo
      .workdir()
      .map(|p| p.to_path_buf())
      .unwrap_or_else(|| root.to_path_buf()),
    Err(_) => {
      return Err(Diagnostic::new(
        "NOT_GIT_REPO",
        "fix --apply requires a git working tree",
      ));
    }
  };
  let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
  let target = crate::history::xdg_cache_dir()
    .join("worktrees")
    .join(format!("fix-{}", ts));
  if let Some(parent_dir) = target.parent() {
    let _ = fs::create_dir_all(parent_dir);
  }
  let out = Command::new("git")
    .args(["worktree", "add", "--detach", target.to_str().unwrap_or("")])
    .current_dir(&parent)
    .output()
    .map_err(|e| {
      Diagnostic::new(
        "WORKTREE_PROVISION_FAILED",
        format!("git worktree add: {}", e),
      )
    })?;
  if !out.status.success() {
    return Err(Diagnostic::new(
      "WORKTREE_PROVISION_FAILED",
      format!(
        "git worktree add: {}",
        String::from_utf8_lossy(&out.stderr).trim()
      ),
    ));
  }
  Ok(FixWorktree {
    path: target,
    parent,
  })
}

fn remove_fix_worktree(wt: &FixWorktree) -> std::result::Result<(), Diagnostic> {
  let out = Command::new("git")
    .args([
      "worktree",
      "remove",
      "--force",
      wt.path.to_str().unwrap_or(""),
    ])
    .current_dir(&wt.parent)
    .output();
  if out.map(|o| !o.status.success()).unwrap_or(true) {
    let _ = fs::remove_dir_all(&wt.path);
  }
  Ok(())
}

fn capture_fix_patch(wt: &FixWorktree) -> std::result::Result<PathBuf, Diagnostic> {
  let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
  let target = crate::history::xdg_cache_dir()
    .join("patches")
    .join(format!("fix-{}.patch", ts));
  if let Some(parent_dir) = target.parent() {
    let _ = fs::create_dir_all(parent_dir);
  }
  let out = Command::new("git")
    .args(["diff", "--no-color"])
    .current_dir(&wt.path)
    .output()
    .map_err(|e| Diagnostic::new("PATCH_CAPTURE_FAILED", format!("git diff: {}", e)))?;
  if let Err(e) = fs::write(&target, &out.stdout) {
    return Err(Diagnostic::new(
      "PATCH_CAPTURE_FAILED",
      format!("write patch: {}", e),
    ));
  }
  Ok(target)
}

fn apply_patch_to_root(root: &Path, patch: &Path) -> std::result::Result<(), Diagnostic> {
  let out = Command::new("git")
    .args(["apply", "--whitespace=nowarn", patch.to_str().unwrap_or("")])
    .current_dir(root)
    .output()
    .map_err(|e| Diagnostic::new("PATCH_APPLY_FAILED", format!("git apply spawn: {}", e)))?;
  if !out.status.success() {
    return Err(Diagnostic::new(
      "PATCH_APPLY_FAILED",
      format!("git apply: {}", String::from_utf8_lossy(&out.stderr).trim()),
    ));
  }
  Ok(())
}

fn write_fix_audit_log(preview: &FixResult) -> std::result::Result<PathBuf, Diagnostic> {
  let ts = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
  let path = crate::history::xdg_cache_dir().join(format!("fix-{}.log", ts));
  if let Some(parent_dir) = path.parent() {
    let _ = fs::create_dir_all(parent_dir);
  }
  let mut lines: Vec<String> = Vec::new();
  lines.push(format!("# lmprobe fix audit log {}", ts));
  lines.push(format!(
    "# {} matches across {} files",
    preview.matches_count, preview.files_count
  ));
  for d in &preview.diffs {
    lines.push(format!("{}  hunks={}", d.file_path, d.hunks.len()));
  }
  fs::write(&path, lines.join("\n"))
    .map_err(|e| Diagnostic::new("AUDIT_LOG_WRITE_FAILED", format!("write: {}", e)))?;
  Ok(path)
}

/// Render a unified-diff string for one FileDiff. Used by markdown
/// output. JSON output ships the structured envelope as-is.
pub fn render_unified_diff(file_diff: &FileDiff) -> String {
  let mut out = String::new();
  out.push_str(&format!("--- a/{}\n", file_diff.file_path));
  out.push_str(&format!("+++ b/{}\n", file_diff.file_path));
  for h in &file_diff.hunks {
    out.push_str(&format!(
      "@@ -{},{} +{},{} @@\n",
      h.line_start,
      h.line_count,
      h.line_start,
      h.after.matches('\n').count().max(1) as u32
    ));
    for line in h.before.lines() {
      out.push_str(&format!("-{}\n", line));
    }
    for line in h.after.lines() {
      out.push_str(&format!("+{}\n", line));
    }
  }
  out
}
