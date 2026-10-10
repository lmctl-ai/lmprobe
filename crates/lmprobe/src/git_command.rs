//! Read-only, structured access to the Git operations most useful to agents.
//!
//! This module deliberately uses libgit2 directly.  It never invokes the Git
//! executable, updates the index, writes refs, or contacts a remote.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use git2::{BranchType, DiffOptions, ObjectType, Repository, Status, StatusOptions};
use serde_json::{json, Value};

#[derive(Args, Debug, Clone)]
pub struct GitArgs {
  /// Repository or a path inside a repository.
  #[arg(long, global = true, default_value = ".")]
  pub repo: PathBuf,
  #[command(subcommand)]
  pub command: GitCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum GitCommand {
  /// Show staged and worktree changes.
  #[command(after_help = "Example:\n  lmprobe git status --repo . --json")]
  Status {
    #[arg(long)]
    ignored: bool,
  },
  /// Show a structured patch and file statistics.
  #[command(after_help = "Example:\n  lmprobe git diff --repo . --path src/lib.rs")]
  Diff {
    /// Compare this commit/tree to TO, or to the worktree when TO is absent.
    #[arg(value_name = "FROM")]
    from: Option<String>,
    #[arg(value_name = "TO")]
    to: Option<String>,
    /// Compare HEAD to the index instead of the worktree.
    #[arg(long)]
    staged: bool,
    /// Limit the patch to a path (repeatable).
    #[arg(long = "path")]
    paths: Vec<String>,
  },
  /// List commits newest first.
  #[command(after_help = "Example:\n  lmprobe git log --repo . --max-count 10")]
  Log {
    #[arg(default_value = "HEAD")]
    revision: String,
    #[arg(long = "max-count", default_value_t = 20)]
    max_count: usize,
  },
  /// Show a commit, or a file from its tree.
  #[command(after_help = "Example:\n  lmprobe git show HEAD --file README.md")]
  Show {
    #[arg(default_value = "HEAD")]
    revision: String,
    #[arg(long)]
    file: Option<String>,
  },
  /// List local branches and their upstream state.
  #[command(after_help = "Example:\n  lmprobe git branch --repo . --all")]
  Branch {
    #[arg(long)]
    all: bool,
  },
  /// List configured remotes with redacted URLs.
  #[command(after_help = "Example:\n  lmprobe git remote --repo .")]
  Remote,
  /// Resolve a revision and optionally peel it to a commit.
  #[command(after_help = "Example:\n  lmprobe git rev-parse HEAD")]
  RevParse {
    #[arg(default_value = "HEAD")]
    revision: String,
  },
  /// List index entries, optionally limited to paths.
  #[command(after_help = "Example:\n  lmprobe git ls-files --repo . --path src")]
  LsFiles {
    #[arg(long = "path")]
    paths: Vec<String>,
  },
  /// Attribute lines to commits.
  #[command(after_help = "Example:\n  lmprobe git blame src/lib.rs --revision HEAD")]
  Blame {
    file: String,
    #[arg(long, default_value = "HEAD")]
    revision: String,
  },
}

pub fn run(args: &GitArgs) -> Result<Value> {
  let repo = Repository::discover(&args.repo)
    .with_context(|| format!("GIT_REPOSITORY_ERROR: {}", args.repo.display()))?;
  let head = match repo.head() {
    Ok(head) => Some(head),
    Err(e)
      if matches!(
        e.code(),
        git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
      ) =>
    {
      None
    }
    Err(e) => return Err(e.into()),
  };
  let (verb, mut payload, hierarchical) = match &args.command {
    GitCommand::Status { ignored } => ("status", status(&repo, *ignored)?, false),
    GitCommand::Diff {
      from,
      to,
      staged,
      paths,
    } => (
      "diff",
      diff(&repo, from.as_deref(), to.as_deref(), *staged, paths)?,
      false,
    ),
    GitCommand::Log {
      revision,
      max_count,
    } => ("log", log(&repo, revision, *max_count)?, false),
    GitCommand::Show { revision, file } => ("show", show(&repo, revision, file.as_deref())?, true),
    GitCommand::Branch { all } => ("branch", branches(&repo, *all)?, false),
    GitCommand::Remote => ("remote", remotes(&repo)?, false),
    GitCommand::RevParse { revision } => ("rev_parse", rev_parse(&repo, revision)?, true),
    GitCommand::LsFiles { paths } => ("ls_files", ls_files(&repo, paths)?, false),
    GitCommand::Blame { file, revision } => ("blame", blame(&repo, file, revision)?, false),
  };
  let no_match =
    !hierarchical && verb != "status" && payload["rows"].as_array().is_some_and(Vec::is_empty);
  let warnings = payload
    .as_object_mut()
    .unwrap()
    .remove("warnings")
    .unwrap_or_else(|| json!([]));
  let partial = payload
    .as_object_mut()
    .unwrap()
    .remove("partial")
    .unwrap_or(json!(false));
  let mut result = json!({
    "schema":format!("lmprobe.git_{verb}.v1"), "exit_status":if no_match {"no_match"} else {"matched"},
    "warnings":warnings, "partial":partial,
    "repo":path_text(repo.path()), "workdir":repo.workdir().map(path_text),
    "head":head.as_ref().and_then(|head| head.target()).map(|oid| oid.to_string()),
    "head_name":head.as_ref().filter(|head| head.is_branch()).map(|head| bytes_text(head.shorthand_bytes())),
    "unborn":head.is_none(), "detached":head.as_ref().is_some_and(|head| !head.is_branch()),
    "bare":repo.is_bare(), "shallow":repo.is_shallow()
  });
  if hierarchical {
    result["data"] = payload;
  } else {
    result
      .as_object_mut()
      .unwrap()
      .extend(payload.as_object().unwrap().clone());
  }
  Ok(result)
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn bytes_text(bytes: &[u8]) -> Value {
  match std::str::from_utf8(bytes) {
    Ok(text) => json!(text),
    Err(_) => json!({"bytes_hex":hex(bytes)}),
  }
}
fn path_text(path: &Path) -> Value {
  #[cfg(unix)]
  {
    use std::os::unix::ffi::OsStrExt;
    bytes_text(path.as_os_str().as_bytes())
  }
  #[cfg(not(unix))]
  {
    json!(path.to_string_lossy())
  }
}
fn optional_path(bytes: Option<&[u8]>) -> Value {
  bytes.map(bytes_text).unwrap_or(Value::Null)
}
fn diagnostic(code: &str, error: impl std::fmt::Display) -> Value {
  json!({"code":code,"message":error.to_string(),"hint":null,"lang":null})
}

fn status(repo: &Repository, ignored: bool) -> Result<Value> {
  let mut options = StatusOptions::new();
  options
    .include_untracked(true)
    .recurse_untracked_dirs(true)
    .include_ignored(ignored)
    .recurse_ignored_dirs(true)
    .renames_head_to_index(true)
    .renames_index_to_workdir(true);
  let entries = repo.statuses(Some(&mut options))?;
  let rows: Vec<_> = entries.iter().map(|entry| {
    let flags = entry.status();
    let staged = entry.head_to_index(); let worktree = entry.index_to_workdir();
    let path = worktree.as_ref().and_then(|delta| delta.new_file().path_bytes())
      .or_else(|| staged.as_ref().and_then(|delta| delta.new_file().path_bytes())).unwrap_or(entry.path_bytes());
    json!({"path":bytes_text(path), "index_status":status_side(flags,true), "worktree_status":status_side(flags,false),
      "index_old_path":optional_path(staged.as_ref().and_then(|delta| delta.old_file().path_bytes())),
      "index_new_path":optional_path(staged.as_ref().and_then(|delta| delta.new_file().path_bytes())),
      "worktree_old_path":optional_path(worktree.as_ref().and_then(|delta| delta.old_file().path_bytes())),
      "worktree_new_path":optional_path(worktree.as_ref().and_then(|delta| delta.new_file().path_bytes())),
      "untracked":flags.contains(Status::WT_NEW), "ignored":flags.contains(Status::IGNORED),
      "conflicted":flags.contains(Status::CONFLICTED), "flags":flags.bits()})
  }).collect();
  let clean = entries
    .iter()
    .all(|entry| entry.status() == Status::CURRENT || entry.status() == Status::IGNORED);
  Ok(json!({"rows":rows,"clean":clean}))
}
fn status_side(flags: Status, index: bool) -> &'static str {
  if flags.contains(Status::CONFLICTED) {
    return "conflicted";
  }
  if !index && flags.contains(Status::IGNORED) {
    return "ignored";
  }
  let entries = if index {
    [
      (Status::INDEX_RENAMED, "renamed"),
      (Status::INDEX_NEW, "added"),
      (Status::INDEX_MODIFIED, "modified"),
      (Status::INDEX_DELETED, "deleted"),
      (Status::INDEX_TYPECHANGE, "typechanged"),
    ]
  } else {
    [
      (Status::WT_RENAMED, "renamed"),
      (Status::WT_NEW, "untracked"),
      (Status::WT_MODIFIED, "modified"),
      (Status::WT_DELETED, "deleted"),
      (Status::WT_TYPECHANGE, "typechanged"),
    ]
  };
  entries
    .iter()
    .find(|(flag, _)| flags.contains(*flag))
    .map(|(_, name)| *name)
    .unwrap_or("clean")
}

fn rev<'repo>(repo: &'repo Repository, revision: &str) -> Result<git2::Object<'repo>> {
  repo
    .revparse_single(revision)
    .with_context(|| format!("GIT_REVISION_ERROR: {revision}"))
}
fn head_tree(repo: &Repository) -> Result<Option<git2::Tree<'_>>> {
  match repo.head() {
    Ok(head) => Ok(Some(head.peel_to_tree()?)),
    Err(e)
      if matches!(
        e.code(),
        git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
      ) =>
    {
      Ok(None)
    }
    Err(e) => Err(e.into()),
  }
}
fn diff(
  repo: &Repository,
  from: Option<&str>,
  to: Option<&str>,
  staged: bool,
  paths: &[String],
) -> Result<Value> {
  if staged && to.is_some() {
    return Err(anyhow!("--staged cannot be combined with TO"));
  }
  let mut options = DiffOptions::new();
  for path in paths {
    options.pathspec(path);
  }
  let diff = if staged {
    let tree = if let Some(from) = from {
      Some(rev(repo, from)?.peel_to_tree()?)
    } else {
      head_tree(repo)?
    };
    repo.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
  } else if let Some(from) = from {
    let old = rev(repo, from)?.peel_to_tree()?;
    if let Some(to) = to {
      let new = rev(repo, to)?.peel_to_tree()?;
      repo.diff_tree_to_tree(Some(&old), Some(&new), Some(&mut options))?
    } else {
      repo.diff_tree_to_workdir_with_index(Some(&old), Some(&mut options))?
    }
  } else {
    repo.diff_index_to_workdir(None, Some(&mut options))?
  };
  diff_records(diff)
}
fn diff_records(mut diff: git2::Diff<'_>) -> Result<Value> {
  let mut find = git2::DiffFindOptions::new();
  find.renames(true);
  diff.find_similar(Some(&mut find))?;
  let mut rows = Vec::new();
  for (index, delta) in diff.deltas().enumerate() {
    let patch = git2::Patch::from_diff(&diff, index)?;
    let mut hunks = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;
    if let Some(patch) = patch {
      let (_, added, deleted) = patch.line_stats()?;
      additions = added;
      deletions = deleted;
      for hunk_index in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_index)?;
        let mut lines = Vec::new();
        for line_index in 0..line_count {
          let line = patch.line_in_hunk(hunk_index, line_index)?;
          lines.push(json!({"origin":line.origin().to_string(),"old_line":line.old_lineno(),"new_line":line.new_lineno(),"content":bytes_text(line.content())}));
        }
        hunks.push(json!({"old_start":hunk.old_start(),"old_lines":hunk.old_lines(),"new_start":hunk.new_start(),"new_lines":hunk.new_lines(),"header":bytes_text(hunk.header()),"lines":lines}));
      }
    }
    rows.push(json!({"path":optional_path(delta.new_file().path_bytes().or_else(||delta.old_file().path_bytes())),
      "old_path":optional_path(delta.old_file().path_bytes()),"new_path":optional_path(delta.new_file().path_bytes()),
      "status":format!("{:?}",delta.status()).to_ascii_lowercase(),
      "old_oid":delta.old_file().id().to_string(),"new_oid":delta.new_file().id().to_string(),
      "old_mode":format!("{:?}",delta.old_file().mode()),"new_mode":format!("{:?}",delta.new_file().mode()),
      "binary":delta.old_file().is_binary() || delta.new_file().is_binary(),
      "additions":additions,"deletions":deletions,"hunks":hunks}));
  }
  let stats = diff.stats()?;
  Ok(
    json!({"rows":rows,"stats":{"files":stats.files_changed(),"insertions":stats.insertions(),"deletions":stats.deletions()}}),
  )
}

fn signature(signature: git2::Signature<'_>) -> Value {
  json!({"name":bytes_text(signature.name_bytes()),"email":bytes_text(signature.email_bytes()),"time":signature.when().seconds(),"offset_minutes":signature.when().offset_minutes()})
}
fn commit_info(commit: &git2::Commit<'_>) -> Value {
  json!({"id":commit.id().to_string(),"short_id":commit.id().to_string()[..7],"summary":commit.summary_bytes().map(bytes_text),"message":bytes_text(commit.message_bytes()),
    "author":signature(commit.author()),"committer":signature(commit.committer()),"parents":commit.parent_ids().map(|oid|oid.to_string()).collect::<Vec<_>>(),"tree":commit.tree_id().to_string()})
}
fn log(repo: &Repository, revision: &str, max_count: usize) -> Result<Value> {
  if max_count == 0 {
    return Err(anyhow!("--max-count must be positive"));
  }
  let mut walk = repo.revwalk()?;
  walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
  if revision.contains("..") {
    walk
      .push_range(revision)
      .with_context(|| format!("GIT_REVISION_ERROR: {revision}"))?;
  } else {
    let object = match rev(repo, revision) {
      Ok(object) => object,
      Err(_) if revision == "HEAD" && head_tree(repo)?.is_none() => {
        return Ok(json!({"rows":[],"truncated":false}))
      }
      Err(e) => return Err(e),
    };
    walk.push(object.peel_to_commit()?.id())?;
  }
  let mut rows = Vec::new();
  for oid in walk.by_ref().take(max_count) {
    rows.push(commit_info(&repo.find_commit(oid?)?));
  }
  let truncated = match walk.next() {
    Some(Ok(_)) => true,
    Some(Err(e)) => return Err(e.into()),
    None => false,
  };
  Ok(json!({"rows":rows,"truncated":truncated,"max_count":max_count}))
}
fn show(repo: &Repository, revision: &str, file: Option<&str>) -> Result<Value> {
  let object = rev(repo, revision)?;
  if let Some(file) = file {
    let tree = object.peel_to_tree()?;
    let entry = tree.get_path(Path::new(file))?;
    let blob = repo.find_blob(entry.id())?;
    return Ok(
      json!({"revision":object.id().to_string(),"file":file,"oid":blob.id().to_string(),"mode":entry.filemode(),"binary":blob.is_binary(),"content":bytes_text(blob.content())}),
    );
  }
  if object.kind() == Some(ObjectType::Blob) {
    let blob = object.as_blob().unwrap();
    return Ok(
      json!({"oid":blob.id().to_string(),"kind":"blob","binary":blob.is_binary(),"content":bytes_text(blob.content())}),
    );
  }
  let commit = object.peel_to_commit()?;
  let tree = commit.tree()?;
  let old = if commit.parent_count() > 0 {
    Some(commit.parent(0)?.tree()?)
  } else {
    None
  };
  let changes = diff_records(repo.diff_tree_to_tree(old.as_ref(), Some(&tree), None)?)?;
  Ok(
    json!({"commit":commit_info(&commit),"comparison":"first_parent","files":changes["rows"],"stats":changes["stats"]}),
  )
}
fn branches(repo: &Repository, all: bool) -> Result<Value> {
  let mut rows = Vec::new();
  let mut warnings = Vec::new();
  for item in repo.branches(if all { None } else { Some(BranchType::Local) })? {
    let (branch, kind) = item?;
    let upstream = if kind == BranchType::Remote {
      None
    } else {
      match branch.upstream() {
        Ok(upstream) => Some(upstream),
        Err(e) if e.code() == git2::ErrorCode::NotFound => None,
        Err(e) => return Err(e.into()),
      }
    };
    let mut ahead = None;
    let mut behind = None;
    if let (Some(local), Some(upstream_oid)) = (
      branch.get().target(),
      upstream.as_ref().and_then(|up| up.get().target()),
    ) {
      match repo.graph_ahead_behind(local, upstream_oid) {
        Ok((a, b)) => {
          ahead = Some(a);
          behind = Some(b);
        }
        Err(e) => warnings.push(diagnostic("GIT_GRAPH_INCOMPLETE", e)),
      }
    }
    rows.push(json!({"name":bytes_text(branch.name_bytes()?),"remote":kind==BranchType::Remote,"current":branch.is_head(),"target":branch.get().target().map(|oid|oid.to_string()),
      "upstream":upstream.as_ref().map(|up|bytes_text(up.get().shorthand_bytes())),"ahead":ahead,"behind":behind}));
  }
  Ok(json!({"rows":rows,"partial":!warnings.is_empty(),"warnings":warnings}))
}
fn redact_url_bytes(url: &[u8]) -> Value {
  let Some(scheme_end) = url.windows(3).position(|bytes| bytes == b"://") else {
    return bytes_text(url);
  };
  let rest = &url[scheme_end + 3..];
  let authority_end = rest
    .iter()
    .position(|byte| matches!(byte, b'/' | b'?' | b'#'))
    .unwrap_or(rest.len());
  let authority = &rest[..authority_end];
  let host_start = authority
    .iter()
    .rposition(|byte| *byte == b'@')
    .map(|pos| pos + 1)
    .unwrap_or(0);
  let suffix = &rest[authority_end..];
  let suffix_end = suffix
    .iter()
    .position(|byte| matches!(byte, b'?' | b'#'))
    .unwrap_or(suffix.len());
  let mut redacted = url[..scheme_end + 3].to_vec();
  redacted.extend_from_slice(&authority[host_start..]);
  redacted.extend_from_slice(&suffix[..suffix_end]);
  bytes_text(&redacted)
}
fn configured_urls(config: &git2::Config, name: &str, suffix: &str) -> Result<Vec<Value>> {
  let key = format!("remote.{name}.{suffix}");
  let mut entries = config.entries(None)?;
  let mut values = Vec::new();
  while let Some(entry) = entries.next() {
    let entry = entry?;
    if entry.name_bytes() == key.as_bytes() {
      values.push(redact_url_bytes(entry.value_bytes()));
    }
  }
  Ok(values)
}
fn refspec_values(values: git2::string_array::StringArray) -> Result<Vec<Value>> {
  (0..values.len())
    .map(|index| {
      values
        .get_bytes(index)
        .map(bytes_text)
        .ok_or_else(|| anyhow!("missing refspec value"))
    })
    .collect()
}
fn remotes(repo: &Repository) -> Result<Value> {
  let mut rows = Vec::new();
  let mut warnings = Vec::new();
  let config = repo.config()?;
  for name in repo.remotes()?.iter() {
    let Some(name) = name else {
      warnings.push(diagnostic(
        "GIT_REMOTE_NAME_ENCODING",
        "remote name is not UTF-8",
      ));
      continue;
    };
    let remote = repo.find_remote(name)?;
    let fetch_url = redact_url_bytes(remote.url_bytes());
    let push_url = remote
      .pushurl_bytes()
      .map(redact_url_bytes)
      .unwrap_or_else(|| fetch_url.clone());
    rows.push(json!({"name":name,"fetch_url":fetch_url,"push_url":push_url,
      "configured_fetch_urls":configured_urls(&config,name,"url")?,"configured_push_urls":configured_urls(&config,name,"pushurl")?,
      "fetch_refspecs":refspec_values(remote.fetch_refspecs()?)?,"push_refspecs":refspec_values(remote.push_refspecs()?)?}));
  }
  Ok(json!({"rows":rows,"partial":!warnings.is_empty(),"warnings":warnings}))
}
fn rev_parse(repo: &Repository, revision: &str) -> Result<Value> {
  let object = rev(repo, revision)?;
  let kind = object
    .kind()
    .map(|kind| format!("{kind:?}").to_ascii_lowercase());
  let peeled = object.peel(ObjectType::Any)?;
  let commit = if peeled.kind() == Some(ObjectType::Commit) {
    Some(peeled.id().to_string())
  } else {
    None
  };
  Ok(json!({"revision":revision,"oid":object.id().to_string(),"kind":kind,"commit":commit}))
}
fn ls_files(repo: &Repository, paths: &[String]) -> Result<Value> {
  let index = repo.index()?;
  let spec = if paths.is_empty() {
    None
  } else {
    Some(git2::Pathspec::new(paths)?)
  };
  let mut rows = Vec::new();
  for entry in index.iter() {
    if let Some(spec) = &spec {
      #[cfg(unix)]
      let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(entry.path.clone()))
      };
      #[cfg(not(unix))]
      let path = PathBuf::from(std::str::from_utf8(&entry.path)?);
      if !spec.matches_path(&path, git2::PathspecFlags::DEFAULT) {
        continue;
      }
    }
    rows.push(json!({"path":bytes_text(&entry.path),"oid":entry.id.to_string(),"mode":entry.mode,"stage":(entry.flags>>12)&3}));
  }
  Ok(json!({"rows":rows}))
}
fn blame(repo: &Repository, file: &str, revision: &str) -> Result<Value> {
  let commit = rev(repo, revision)?.peel_to_commit()?;
  let tree = commit.tree()?;
  let entry = tree.get_path(Path::new(file))?;
  let blob = repo.find_blob(entry.id())?;
  if blob.is_binary() {
    return Err(anyhow!(
      "GIT_BINARY_BLAME: binary files cannot be attributed as text lines"
    ));
  }
  let contents = blob
    .content()
    .split_inclusive(|byte| *byte == b'\n')
    .collect::<Vec<_>>();
  let mut options = git2::BlameOptions::new();
  options.newest_commit(commit.id());
  let blame = repo.blame_file(Path::new(file), Some(&mut options))?;
  let mut rows = Vec::new();
  for hunk in blame.iter() {
    for offset in 0..hunk.lines_in_hunk() {
      let line = hunk.final_start_line() + offset;
      rows.push(json!({"line":line,"original_line":hunk.orig_start_line()+offset,"commit":hunk.final_commit_id().to_string(),"original_commit":hunk.orig_commit_id().to_string(),
        "author":signature(hunk.final_signature()),"path":hunk.path().map(path_text),"content":contents.get(line-1).map(|text|bytes_text(text)),"boundary":hunk.is_boundary()}));
    }
  }
  Ok(json!({"rows":rows,"revision":commit.id().to_string(),"file":file}))
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn redact_remote_secrets_without_rewriting_local_paths() {
    assert_eq!(
      redact_url_bytes(b"https://user:password@example.com/repo.git?token=secret#fragment"),
      "https://example.com/repo.git"
    );
    assert_eq!(
      redact_url_bytes(b"git@example.com:repo.git"),
      "git@example.com:repo.git"
    );
    assert_eq!(redact_url_bytes(b"/local/repo?#name"), "/local/repo?#name");
    assert_eq!(
      redact_url_bytes(b"https://user:secret@example.com/\xff?token=secret"),
      json!({"bytes_hex":hex(b"https://example.com/\xff")})
    );
  }

  #[test]
  fn bytes_and_paths_preserve_non_utf8_as_hex() {
    assert_eq!(bytes_text(b"name$"), "name$");
    assert_eq!(bytes_text(b"name\xff"), json!({"bytes_hex":"6e616d65ff"}));
    #[cfg(unix)]
    {
      use std::ffi::OsString;
      use std::os::unix::ffi::OsStringExt;
      assert_eq!(
        path_text(Path::new(&OsString::from_vec(b"dir/\xff".to_vec()))),
        json!({"bytes_hex":"6469722fff"})
      );
    }
  }

  #[test]
  fn redact_url_handles_ipv6_ports_and_dollar_credentials() {
    assert_eq!(
      redact_url_bytes(b"https://user:p$ss@[2001:db8::1]:8443/repo.git?token=x"),
      "https://[2001:db8::1]:8443/repo.git"
    );
    assert_eq!(
      redact_url_bytes(b"ssh://$user:$pass@[::1]:22/repo.git#frag"),
      "ssh://[::1]:22/repo.git"
    );
  }

  #[test]
  fn redact_url_returns_hex_when_remaining_url_is_not_utf8() {
    assert_eq!(
      redact_url_bytes(b"https://user@example.com/repo/\xff"),
      json!({"bytes_hex":"68747470733a2f2f6578616d706c652e636f6d2f7265706f2fff"})
    );
  }

  #[test]
  fn redact_url_keeps_local_and_scp_style_paths_unchanged() {
    assert_eq!(redact_url_bytes(b"/tmp/repo$one?x#y"), "/tmp/repo$one?x#y");
    assert_eq!(
      redact_url_bytes(b"user@example.com:repo$one.git"),
      "user@example.com:repo$one.git"
    );
  }

  #[test]
  fn status_side_prioritizes_conflicts_and_ignored_worktree_entries() {
    let conflicted = Status::CONFLICTED | Status::INDEX_MODIFIED | Status::WT_DELETED;
    assert_eq!(status_side(conflicted, true), "conflicted");
    assert_eq!(status_side(conflicted, false), "conflicted");
    assert_eq!(
      status_side(Status::IGNORED | Status::WT_MODIFIED, false),
      "ignored"
    );
    assert_eq!(
      status_side(Status::IGNORED | Status::INDEX_MODIFIED, true),
      "modified"
    );
  }

  #[test]
  fn renamed_and_modified_files_keep_independent_index_and_worktree_states() {
    let flags = Status::INDEX_RENAMED | Status::INDEX_MODIFIED | Status::WT_DELETED;
    assert_eq!(status_side(flags, true), "renamed");
    assert_eq!(status_side(flags, false), "deleted");
    let flags = Status::INDEX_NEW | Status::WT_RENAMED | Status::WT_MODIFIED;
    assert_eq!(status_side(flags, true), "added");
    assert_eq!(status_side(flags, false), "renamed");
    assert_eq!(status_side(Status::WT_MODIFIED, true), "clean");
    assert_eq!(status_side(Status::INDEX_MODIFIED, false), "clean");
  }
}
