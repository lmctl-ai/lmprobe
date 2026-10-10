//! Changed-line anchors from git diffs.
//!
//! `diff-context` is intentionally not a semantic diff engine. It
//! parses new-side changed line ranges from `git diff --unified=0`
//! and then reuses the same source/structural expansion primitives
//! as grep-style anchors.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  Diagnostic, DiffContextAnchor, DiffContextResult, ExitStatus, LineContext,
};

use crate::sensors::WalkOpts;

#[derive(Debug, Clone)]
struct HunkMeta {
  old_start: usize,
  old_lines: usize,
  new_start: usize,
  new_lines: usize,
}

pub fn diff_context(
  from: &str,
  to: &str,
  path: &Path,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<DiffContextResult> {
  let diff = run_git_diff(from, to, path)?;
  let mut anchors = anchors_from_diff(&diff);
  let (source_lines, used_worktree_fallback) = if around.is_some() || parents || body {
    attach_source_context(&mut anchors, to, around)
  } else {
    (BTreeMap::new(), false)
  };
  attach_structural_expansions(&mut anchors, parents, body, opts, &source_lines);

  let mut res = DiffContextResult::new(anchors);
  if used_worktree_fallback {
    res.push_warning(
      Diagnostic::new(
        "DIFF_CONTEXT_SOURCE_FALLBACK",
        "diff-context could not read at least one target revision blob and fell back to the current worktree for expansion",
      )
      .with_hint("Changed-line text still comes from git diff; source_context/parents/body may reflect the worktree for fallback files."),
    );
  }
  if res.anchors.is_empty() {
    res.exit_status = ExitStatus::NoMatch;
  }
  Ok(res)
}

fn run_git_diff(from: &str, to: &str, path: &Path) -> Result<String> {
  let pathspec = path.to_string_lossy().to_string();
  let output = Command::new("git")
    .args(["diff", "--unified=0", "--no-color", from, to, "--"])
    .arg(pathspec)
    .output()
    .map_err(|e| lmprobe_types::error::anyhow!("git diff spawn failed: {}", e))?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(lmprobe_types::error::anyhow!(
      "git diff failed: {}",
      stderr.trim()
    ));
  }
  Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn anchors_from_diff(diff: &str) -> Vec<DiffContextAnchor> {
  let mut anchors = Vec::new();
  let mut file_path: Option<String> = None;
  let mut hunk: Option<HunkMeta> = None;
  let mut new_line = 0usize;

  for line in diff.lines() {
    if let Some(path) = parse_new_file_path(line) {
      file_path = path;
      hunk = None;
      continue;
    }
    if let Some(parsed) = parse_hunk_header(line) {
      new_line = parsed.new_start;
      hunk = Some(parsed);
      continue;
    }
    if line.starts_with("diff --git ")
      || line.starts_with("index ")
      || line.starts_with("--- ")
      || line.starts_with("new file mode ")
      || line.starts_with("deleted file mode ")
      || line.starts_with("similarity index ")
      || line.starts_with("rename from ")
      || line.starts_with("rename to ")
    {
      continue;
    }
    let Some(file_path) = file_path.as_ref() else {
      continue;
    };
    let Some(hunk) = hunk.as_ref() else {
      continue;
    };
    if line.starts_with('+') && !line.starts_with("+++") {
      anchors.push(DiffContextAnchor {
        file_path: file_path.clone(),
        line_number: new_line,
        line_content: line.strip_prefix('+').unwrap_or("").to_string(),
        anchor_kind: "changed_line".to_string(),
        old_start: hunk.old_start,
        old_lines: hunk.old_lines,
        new_start: hunk.new_start,
        new_lines: hunk.new_lines,
        source_context: Vec::new(),
        parents: Vec::new(),
        body: None,
      });
      new_line += 1;
    } else if line.starts_with('-') && !line.starts_with("---") {
      // Deleted-only lines have no new-side source anchor in this
      // first slice.
    } else {
      new_line += 1;
    }
  }
  anchors
}

fn parse_new_file_path(line: &str) -> Option<Option<String>> {
  let rest = line.strip_prefix("+++ ")?;
  if rest == "/dev/null" {
    return Some(None);
  }
  let cleaned = rest
    .split('\t')
    .next()
    .unwrap_or(rest)
    .trim()
    .trim_matches('"');
  Some(Some(
    cleaned.strip_prefix("b/").unwrap_or(cleaned).to_string(),
  ))
}

fn parse_hunk_header(line: &str) -> Option<HunkMeta> {
  let rest = line.strip_prefix("@@ -")?;
  let (old_part, rest) = rest.split_once(" +")?;
  let (new_part, _) = rest.split_once(" @@")?;
  let (old_start, old_lines) = parse_range(old_part)?;
  let (new_start, new_lines) = parse_range(new_part)?;
  Some(HunkMeta {
    old_start,
    old_lines,
    new_start,
    new_lines,
  })
}

fn parse_range(value: &str) -> Option<(usize, usize)> {
  if let Some((start, len)) = value.split_once(',') {
    Some((start.parse().ok()?, len.parse().ok()?))
  } else {
    Some((value.parse().ok()?, 1))
  }
}

fn attach_source_context(
  anchors: &mut [DiffContextAnchor],
  revision: &str,
  around: Option<usize>,
) -> (BTreeMap<String, Vec<String>>, bool) {
  let mut content_by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
  let mut used_worktree_fallback = false;
  for anchor in anchors {
    if !content_by_file.contains_key(&anchor.file_path) {
      let (lines, fallback) = read_lines_at_revision(revision, &anchor.file_path);
      used_worktree_fallback |= fallback;
      content_by_file.insert(anchor.file_path.clone(), lines);
    }
    let lines = content_by_file
      .get(&anchor.file_path)
      .expect("source lines inserted");
    if let Some(around) = around {
      anchor.source_context = line_context_around(lines, anchor.line_number, around);
    }
  }
  (content_by_file, used_worktree_fallback)
}

fn read_lines_at_revision(revision: &str, path: &str) -> (Vec<String>, bool) {
  let spec = format!("{}:{}", revision, path);
  let output = Command::new("git").args(["show", &spec]).output();
  let (content, used_worktree_fallback) = match output {
    Ok(output) if output.status.success() => {
      (String::from_utf8_lossy(&output.stdout).into_owned(), false)
    }
    _ => (std::fs::read_to_string(path).unwrap_or_default(), true),
  };
  (
    content.lines().map(|line| line.to_string()).collect(),
    used_worktree_fallback,
  )
}

fn line_context_around(lines: &[String], line_number: usize, around: usize) -> Vec<LineContext> {
  let target_idx = line_number.saturating_sub(1);
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  (start..end)
    .filter_map(|idx| {
      lines.get(idx).map(|line| LineContext {
        line_number: idx + 1,
        line_content: line.clone(),
      })
    })
    .collect()
}

fn attach_structural_expansions(
  anchors: &mut [DiffContextAnchor],
  parents: bool,
  body: bool,
  opts: &WalkOpts,
  source_lines: &BTreeMap<String, Vec<String>>,
) {
  if !parents && !body {
    return;
  }

  let mut by_file: BTreeMap<String, Vec<usize>> = BTreeMap::new();
  for (idx, anchor) in anchors.iter().enumerate() {
    by_file
      .entry(anchor.file_path.clone())
      .or_default()
      .push(idx);
  }

  for (file_path, indices) in by_file {
    let Some(lines) = source_lines.get(&file_path) else {
      continue;
    };
    let content = lines.join("\n");
    let line_numbers = indices
      .iter()
      .map(|idx| anchors[*idx].line_number)
      .collect::<Vec<_>>();
    let Some(expansions) = crate::sensors::structural_expansions_for_content(
      &content,
      Path::new(&file_path),
      opts,
      &line_numbers,
    ) else {
      continue;
    };
    for (idx, (anchor_parents, anchor_body)) in indices.into_iter().zip(expansions) {
      if parents {
        anchors[idx].parents = anchor_parents;
      }
      if body {
        anchors[idx].body = anchor_body;
      }
    }
  }
}
