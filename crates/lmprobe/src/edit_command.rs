//! Preview-first edits of explicit UTF-8 files, independent of source parsers.
use crate::schema::{Diagnostic, ExitStatus};
use crate::text_edit::{transform, EditOperation};
use anyhow::{bail, Context};
use clap::Args;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Args, Debug)]
#[command(group(clap::ArgGroup::new("edit_operation").required(true).args(["pattern", "script", "insert_before", "insert_after", "delete_lines", "replace_lines", "prepend", "append"])))]
pub struct EditArgs {
  /// Explicit existing UTF-8 files; directories and symlinks are refused.
  #[arg(required = true, num_args = 1..)]
  pub files: Vec<PathBuf>,
  /// Literal search text (use --regex for Rust regular expressions).
  #[arg(short = 'p', long, requires = "replacement")]
  pub pattern: Option<String>,
  /// Replacement text; regex mode supports $1 and ${name} captures.
  #[arg(short = 'r', long, requires = "pattern")]
  pub replacement: Option<String>,
  /// Interpret --pattern as a Rust regex (no lookaround/backreferences).
  #[arg(long, requires = "pattern")]
  pub regex: bool,
  /// Match whole Unicode words only.
  #[arg(long, requires = "pattern")]
  pub word: bool,
  /// Replace only the first match per file.
  #[arg(long, requires = "pattern")]
  pub first: bool,
  /// JSON array of sequential edit operations; - reads stdin.
  #[arg(long)]
  pub script: Option<PathBuf>,
  /// Insert --text before this 1-based line.
  #[arg(long, requires = "text")]
  pub insert_before: Option<usize>,
  /// Insert --text after this 1-based line.
  #[arg(long, requires = "text")]
  pub insert_after: Option<usize>,
  /// Delete an inclusive 1-based START:END range (or one line).
  #[arg(long)]
  pub delete_lines: Option<String>,
  /// Replace an inclusive START:END range with --text.
  #[arg(long, requires = "text")]
  pub replace_lines: Option<String>,
  /// Add --text at the beginning, exactly as supplied.
  #[arg(long, requires = "text")]
  pub prepend: bool,
  /// Add --text at the end, exactly as supplied.
  #[arg(long, requires = "text")]
  pub append: bool,
  /// Text for line insertion/replacement or prepend/append; shell quoting supports newlines.
  #[arg(long, conflicts_with_all = ["pattern", "script", "delete_lines"])]
  pub text: Option<String>,
  /// Write changes (default: read-only before/after preview).
  #[arg(long)]
  pub apply: bool,
  /// Permit applying to a dirty Git tree or outside Git.
  #[arg(long, requires = "apply")]
  pub force: bool,
}

impl EditArgs {
  fn operations(&self, ignore_case: bool) -> anyhow::Result<Vec<EditOperation>> {
    if let Some(script) = &self.script {
      let content = if script == Path::new("-") {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content)?;
        content
      } else {
        fs::read_to_string(script)?
      };
      let operations: Vec<EditOperation> = serde_json::from_str(&content)?;
      if operations.is_empty() {
        bail!("edit script must contain at least one operation");
      }
      if ignore_case {
        bail!("use ignore_case inside each script replacement instead of -i");
      }
      return Ok(operations);
    }
    let text = self.text.clone().unwrap_or_default();
    let operation = if let Some(pattern) = &self.pattern {
      EditOperation::Replace {
        pattern: pattern.clone(),
        replacement: self.replacement.clone().unwrap(),
        regex: self.regex,
        word: self.word,
        ignore_case,
        first: self.first,
      }
    } else if let Some(line) = self.insert_before {
      EditOperation::InsertBefore { line, text }
    } else if let Some(line) = self.insert_after {
      EditOperation::InsertAfter { line, text }
    } else if let Some(range) = &self.delete_lines {
      let (start, end) = parse_range(range)?;
      EditOperation::DeleteLines { start, end }
    } else if let Some(range) = &self.replace_lines {
      let (start, end) = parse_range(range)?;
      EditOperation::ReplaceLines { start, end, text }
    } else if self.prepend {
      EditOperation::Prepend { text }
    } else {
      EditOperation::Append { text }
    };
    if ignore_case && self.pattern.is_none() {
      bail!("-i is only supported for replacement");
    }
    if self.text.is_some() && (self.pattern.is_some() || self.delete_lines.is_some()) {
      bail!("--text is not used by this operation");
    }
    Ok(vec![operation])
  }
}

fn parse_range(input: &str) -> anyhow::Result<(usize, usize)> {
  let (start, end) = input.split_once(':').unwrap_or((input, input));
  Ok((
    start.parse().context("invalid start line")?,
    end.parse().context("invalid end line")?,
  ))
}

#[derive(Serialize)]
pub struct EditFile {
  pub path: PathBuf,
  pub before: String,
  pub after: String,
  pub applied: bool,
  #[serde(skip)]
  original: fs::Metadata,
}
#[derive(Serialize)]
pub struct EditResult {
  pub schema: &'static str,
  pub mode: &'static str,
  pub files: Vec<EditFile>,
  pub audit_path: Option<PathBuf>,
  pub exit_status: ExitStatus,
  pub warnings: Vec<Diagnostic>,
}

fn safe_file(path: &Path) -> anyhow::Result<fs::Metadata> {
  let absolute = std::env::current_dir()?.join(path);
  for ancestor in absolute.ancestors() {
    if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
      bail!("symlinks are refused: {}", ancestor.display());
    }
  }
  let meta = fs::symlink_metadata(path)?;
  if !meta.is_file() {
    bail!("expected a regular file: {}", path.display());
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    if meta.nlink() != 1 {
      bail!("hard-linked files are refused: {}", path.display());
    }
  }
  Ok(meta)
}

fn git_safety(path: &Path, force: bool) -> anyhow::Result<()> {
  if force {
    return Ok(());
  }
  let repo = git2::Repository::discover(path.parent().unwrap_or(Path::new(".")))
    .context("EDIT_NON_GIT: --apply outside Git requires --force")?;
  let mut options = git2::StatusOptions::new();
  options.include_untracked(true).recurse_untracked_dirs(true);
  if !repo.statuses(Some(&mut options))?.is_empty() {
    bail!("EDIT_DIRTY_TREE: --apply in a dirty Git tree requires --force");
  }
  Ok(())
}

pub fn run(args: &EditArgs, ignore_case: bool, max_size: u64) -> anyhow::Result<EditResult> {
  crate::program_output::protect(
    args.files.iter().cloned().chain(
      args
        .script
        .iter()
        .filter(|p| p.as_path() != Path::new("-"))
        .cloned(),
    ),
  )
  .map_err(anyhow::Error::msg)?;
  let operations = args.operations(ignore_case)?;
  let mut files = Vec::new();
  let mut seen = std::collections::BTreeSet::new();
  for path in &args.files {
    let meta = safe_file(path)?;
    if meta.len() > max_size {
      bail!("file exceeds --max-filesize: {}", path.display());
    }
    let path = path.canonicalize()?;
    if !seen.insert(path.clone()) {
      continue;
    }
    let before = fs::read_to_string(&path)
      .with_context(|| format!("expected UTF-8 text: {}", path.display()))?;
    let after = transform(&before, &operations)
      .with_context(|| format!("invalid edit for {}", path.display()))?;
    if before != after {
      files.push(EditFile {
        path,
        before,
        after,
        applied: false,
        original: meta,
      });
    }
  }
  let mut result = EditResult {
    schema: "lmprobe.edit.v1",
    mode: "dry_run",
    exit_status: if files.is_empty() {
      ExitStatus::NoMatch
    } else {
      ExitStatus::Matched
    },
    files,
    audit_path: None,
    warnings: vec![],
  };
  if !args.apply || result.files.is_empty() {
    return Ok(result);
  }
  for file in &result.files {
    git_safety(&file.path, args.force)?;
    if safe_file(&file.path)?.permissions().readonly() {
      bail!("read-only edit target: {}", file.path.display());
    }
  }
  // Complete recovery data is durable before the first source write.
  let cache = std::env::var_os("XDG_CACHE_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
  fs::create_dir_all(cache.join("lmprobe"))?;
  let audit = cache.join("lmprobe").join(format!(
    "edit-{}-{}.json",
    std::process::id(),
    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
  ));
  let mut options = OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let mut log = options.open(&audit)?;
  log.write_all(&serde_json::to_vec_pretty(&result)?)?;
  log.sync_all()?;
  result.audit_path = Some(audit);
  result.mode = "applied";
  for file in &mut result.files {
    if let Err(error) = write_file(file) {
      result.exit_status = ExitStatus::Error;
      result.warnings.push(Diagnostic {
        code: "EDIT_WRITE_FAILED".into(),
        message: format!(
          "{}: {error}; recovery data: {}",
          file.path.display(),
          result.audit_path.as_ref().unwrap().display()
        ),
        hint: None,
        lang: None,
      });
      result.mode = "incomplete";
      break;
    }
    file.applied = true;
  }
  Ok(result)
}

fn write_file(file: &EditFile) -> anyhow::Result<()> {
  let temporary = file.path.with_file_name(format!(
    ".lmprobe-edit-{}-{}",
    std::process::id(),
    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
  ));
  write_file_at(file, &temporary)
}

fn write_file_at(file: &EditFile, temporary: &Path) -> anyhow::Result<()> {
  let original = &file.original;
  let current = safe_file(&file.path)?;
  check_identity(original, &current)?;
  if fs::read_to_string(&file.path)? != file.before {
    bail!("file changed after preview");
  }
  let mut created = false;
  let result = (|| -> anyhow::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let mut output = options.open(temporary)?;
    created = true;
    output.write_all(file.after.as_bytes())?;
    output.set_permissions(original.permissions())?;
    output.sync_all()?;
    let current = safe_file(&file.path)?;
    check_identity(original, &current)?;
    if fs::read_to_string(&file.path)? != file.before
      || current.modified()? != original.modified()?
    {
      bail!("file changed after preview");
    }
    fs::rename(temporary, &file.path)?;
    Ok(())
  })();
  if result.is_err() && created {
    let _ = fs::remove_file(temporary);
  }
  result
}

fn check_identity(original: &fs::Metadata, current: &fs::Metadata) -> anyhow::Result<()> {
  if original.len() != current.len()
    || original.modified()? != current.modified()?
    || original.permissions() != current.permissions()
  {
    bail!("file metadata changed after preview");
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    if current.dev() != original.dev()
      || current.ino() != original.ino()
      || current.ctime() != original.ctime()
      || current.ctime_nsec() != original.ctime_nsec()
    {
      bail!("file identity changed after preview");
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn failed_create_does_not_remove_an_unowned_temporary() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.txt");
    fs::write(&path, "before").unwrap();
    let file = EditFile {
      original: fs::metadata(&path).unwrap(),
      path: path.clone(),
      before: "before".into(),
      after: "after".into(),
      applied: false,
    };
    let temporary = root.path().join("collision");
    fs::write(&temporary, "sentinel").unwrap();
    assert!(write_file_at(&file, &temporary).is_err());
    assert_eq!(fs::read_to_string(temporary).unwrap(), "sentinel");
    assert_eq!(fs::read_to_string(path).unwrap(), "before");
  }
  #[test]
  fn changed_snapshot_is_refused_before_temp_creation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.txt");
    fs::write(&path, "before").unwrap();
    let file = EditFile {
      original: fs::metadata(&path).unwrap(),
      path: path.clone(),
      before: "before".into(),
      after: "after".into(),
      applied: false,
    };
    fs::write(&path, "changed").unwrap();
    let temporary = root.path().join("temporary");
    assert!(write_file_at(&file, &temporary).is_err());
    assert!(!temporary.exists());
    assert_eq!(fs::read_to_string(path).unwrap(), "changed");
  }
}
