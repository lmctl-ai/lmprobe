//! Evidence-only JS/TS async teardown call scanner.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ast_grep_language::Language;
use ignore::WalkBuilder;
use regex::Regex;

use lmprobe_lang_core::langs::SgLang;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{AsyncLeakSite, AsyncLeaksResult, Diagnostic, ExitStatus, LineContext};

use crate::sensors::{
  apply_walk_opts, structural_expansions_for_content, walk_pre_check, PreCheck, WalkOpts,
};

struct CallMatcher {
  re: Regex,
}

pub fn async_leaks(
  root: &Path,
  calls: &[String],
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<AsyncLeaksResult> {
  let call_names = calls
    .iter()
    .map(|call| call.trim())
    .filter(|call| !call.is_empty())
    .collect::<Vec<_>>();
  if call_names.is_empty() {
    let mut res = AsyncLeaksResult::new(Vec::new());
    res.exit_status = ExitStatus::Error;
    res.push_warning(Diagnostic::new(
      "ASYNC_LEAKS_NO_CALLS",
      "async-leaks requires at least one non-empty --call value",
    ));
    return Ok(res);
  }

  let matcher = CallMatcher::new(&call_names);
  let (files, warnings) = candidate_files(root, opts);
  let mut sites = Vec::new();
  for file in files {
    if let Ok(content) = std::fs::read_to_string(&file) {
      sites.extend(async_leaks_in_file(
        &file, &content, &matcher, around, parents, body, opts,
      ));
    }
  }
  let mut res = AsyncLeaksResult::new(sites);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

impl CallMatcher {
  fn new(call_names: &[&str]) -> Self {
    let calls = call_names
      .iter()
      .map(|call| regex::escape(call))
      .collect::<Vec<_>>()
      .join("|");
    let ident = r"(?:[A-Za-z_$][A-Za-z0-9_$]*|this)";
    let member_chain = format!(r"{ident}(?:\??\.{ident})*");
    let re = Regex::new(&format!(
      r"(?:^|[^A-Za-z0-9_$])(?P<callee>(?:{member_chain}\??\.)?(?:{calls}))\s*\("
    ))
    .expect("escaped call-name regex should compile");
    Self { re }
  }

  fn find_line_calls(&self, line: &str) -> Vec<LineCall> {
    self
      .re
      .captures_iter(line)
      .filter_map(|caps| {
        let callee = caps.name("callee")?;
        Some(LineCall {
          callee: callee.as_str().to_string(),
          start: callee.start(),
        })
      })
      .collect()
  }
}

struct LineCall {
  callee: String,
  start: usize,
}

#[allow(clippy::too_many_arguments)]
fn async_leaks_in_file(
  path: &Path,
  content: &str,
  matcher: &CallMatcher,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> Vec<AsyncLeakSite> {
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let mut anchors = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    for call in matcher.find_line_calls(line) {
      anchors.push((idx + 1, line.clone(), call));
    }
  }
  if anchors.is_empty() {
    return Vec::new();
  }

  let anchor_lines = anchors
    .iter()
    .map(|(line_number, _, _)| *line_number)
    .collect::<Vec<_>>();
  let expansions = structural_expansions_for_content(content, path, opts, &anchor_lines)
    .unwrap_or_else(|| vec![(Vec::new(), None); anchor_lines.len()]);
  let file_path = path.to_string_lossy().into_owned();

  anchors
    .into_iter()
    .zip(expansions)
    .map(|((line_number, line_content, call), (parents, body))| {
      let prefix = line_content.get(..call.start).unwrap_or_default();
      AsyncLeakSite {
        file_path: file_path.clone(),
        line_number,
        line_content: line_content.clone(),
        callee: call.callee,
        awaited: is_awaited_prefix(prefix),
        returned: is_returned_prefix(prefix),
        source_context: around
          .map(|around| source_context(&lines, line_number, around))
          .unwrap_or_default(),
        parents: if include_parents { parents } else { Vec::new() },
        body: if include_body { body } else { None },
        confidence: "syntax-evidence-only".to_string(),
      }
    })
    .collect()
}

fn is_awaited_prefix(prefix: &str) -> bool {
  prefix
    .trim_end()
    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
    .next_back()
    .is_some_and(|token| token == "await")
}

fn is_returned_prefix(prefix: &str) -> bool {
  prefix
    .trim_end()
    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
    .any(|token| token == "return")
}

fn source_context(lines: &[String], line_number: usize, around: usize) -> Vec<LineContext> {
  let target_idx = line_number.saturating_sub(1);
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  (start..end)
    .filter_map(|idx| {
      lines.get(idx).map(|line_content| LineContext {
        line_number: idx + 1,
        line_content: line_content.clone(),
      })
    })
    .collect()
}

fn candidate_files(root: &Path, opts: &WalkOpts) -> (Vec<PathBuf>, Vec<Diagnostic>) {
  let mut files = BTreeSet::new();
  let mut warnings = Vec::new();
  if !root.exists() {
    warnings.push(Diagnostic::new(
      "PATH_NOT_FOUND",
      format!("path not found: {}", root.display()),
    ));
    return (Vec::new(), warnings);
  }
  if root.is_file() {
    if is_supported_path(root, opts) {
      files.insert(root.to_path_buf());
    }
    return (files.into_iter().collect(), warnings);
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, lang) if is_async_leaks_lang(lang) => {
        files.insert(path);
      }
      PreCheck::SkipWithDiag(diagnostic) => warnings.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  (files.into_iter().collect(), warnings)
}

fn is_supported_path(path: &Path, opts: &WalkOpts) -> bool {
  opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(is_async_leaks_lang)
}

fn is_async_leaks_lang(lang: SgLang) -> bool {
  matches!(lang, SgLang::JavaScript | SgLang::TypeScript | SgLang::Tsx)
}
