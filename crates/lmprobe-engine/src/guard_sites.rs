//! Evidence-only sensitive-action / local-guard anchor scanner.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ast_grep_language::Language;
use ignore::WalkBuilder;
use regex::Regex;

use lmprobe_lang_core::langs::{is_code_lang, SgLang};
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  AnchorScanStats, Diagnostic, ExitStatus, GuardCallSite, GuardSiteCandidate, GuardSitesResult,
  LineContext, StructuralBody, StructuralParent,
};

use crate::sensors::{
  apply_walk_opts, structural_expansions_for_content, walk_pre_check, PreCheck, WalkOpts,
};

#[derive(Clone)]
struct LineAnchor {
  line_number: usize,
  line_content: String,
  match_text: String,
  parents: Vec<StructuralParent>,
  body: Option<StructuralBody>,
}

struct GuardSitesFileResult {
  candidates: Vec<GuardSiteCandidate>,
  sensitive_anchor_count: usize,
  nearby_guard_count: usize,
}

pub fn guard_sites(
  root: &Path,
  sensitive: &str,
  guard: &str,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<GuardSitesResult> {
  let sensitive_re = match Regex::new(sensitive) {
    Ok(re) => re,
    Err(err) => return Ok(regex_error_result("sensitive", sensitive, &err)),
  };
  let guard_re = match Regex::new(guard) {
    Ok(re) => re,
    Err(err) => return Ok(regex_error_result("guard", guard, &err)),
  };

  let mut candidates = Vec::new();
  let mut scan_stats = AnchorScanStats::default();
  let (files, warnings, unsupported_files) = candidate_files(root, opts);
  scan_stats.unsupported_files = unsupported_files;
  for file in files {
    if let Ok(content) = std::fs::read_to_string(&file) {
      scan_stats.files_scanned += 1;
      let file_result = guard_sites_in_file(
        &file,
        &content,
        &sensitive_re,
        &guard_re,
        around,
        parents,
        body,
        opts,
      );
      scan_stats.anchors_found += file_result.sensitive_anchor_count;
      scan_stats.guard_hits += file_result.nearby_guard_count;
      candidates.extend(file_result.candidates);
    }
  }
  let mut res = GuardSitesResult::new(candidates);
  res.scan_stats = Some(scan_stats);
  for warning in warnings {
    res.push_warning(warning);
  }
  push_scan_shape_warnings(&mut res, scan_stats);
  Ok(res)
}

fn push_scan_shape_warnings(res: &mut GuardSitesResult, stats: AnchorScanStats) {
  if stats.unsupported_files > 0 {
    res.push_warning(Diagnostic::new(
      "LANGUAGE_UNSUPPORTED",
      format!(
        "{} file(s) had a recognized non-code language and were not scanned by guard-sites",
        stats.unsupported_files
      ),
    ));
  }
  if stats.files_scanned == 0 {
    res.push_warning(Diagnostic::new(
      "NO_FILES_SCANNED",
      "guard-sites did not scan any supported files",
    ));
    return;
  }
  if stats.anchors_found == 0 {
    res.push_warning(Diagnostic::new(
      "NO_ANCHORS_FOUND",
      "guard-sites scanned files but found no sensitive anchors for the supplied regex",
    ));
  } else if stats.guard_hits == 0 {
    res.push_warning(Diagnostic::new(
      "ANCHORS_WITHOUT_GUARD",
      "guard-sites found sensitive anchors but no nearby guard anchors for the supplied regex/window",
    ));
  }
}

fn candidate_files(root: &Path, opts: &WalkOpts) -> (Vec<PathBuf>, Vec<Diagnostic>, usize) {
  let mut files = Vec::new();
  let mut warnings = Vec::new();
  let mut unsupported_files = 0;
  if !root.exists() {
    warnings.push(Diagnostic::new(
      "PATH_NOT_FOUND",
      format!("path not found: {}", root.display()),
    ));
    return (files, warnings, unsupported_files);
  }
  if root.is_file() {
    if is_supported_path(root, opts) {
      files.push(root.to_path_buf());
    } else if opts
      .lang_filter
      .or_else(|| SgLang::from_path(root))
      .is_some_and(|lang| !is_code_lang(lang))
    {
      unsupported_files += 1;
    }
    return (files, warnings, unsupported_files);
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, lang) if is_code_lang(lang) => files.push(path),
      PreCheck::Process(_, _) => unsupported_files += 1,
      PreCheck::SkipWithDiag(diagnostic) => warnings.push(diagnostic),
      PreCheck::Skip => {}
    }
  }
  (files, warnings, unsupported_files)
}

fn is_supported_path(path: &Path, opts: &WalkOpts) -> bool {
  opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(is_code_lang)
}

#[allow(clippy::too_many_arguments)]
fn guard_sites_in_file(
  path: &Path,
  content: &str,
  sensitive_re: &Regex,
  guard_re: &Regex,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> GuardSitesFileResult {
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let mut raw_sensitive = Vec::new();
  let mut raw_guards = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    if let Some(m) = sensitive_re.find(line) {
      raw_sensitive.push((idx + 1, line.clone(), m.as_str().to_string()));
    }
    if let Some(m) = guard_re.find(line) {
      raw_guards.push((idx + 1, line.clone(), m.as_str().to_string()));
    }
  }
  let sensitive_anchor_count = raw_sensitive.len();
  if raw_sensitive.is_empty() {
    return GuardSitesFileResult {
      candidates: Vec::new(),
      sensitive_anchor_count,
      nearby_guard_count: 0,
    };
  }

  let line_numbers = raw_sensitive
    .iter()
    .map(|(line_number, _, _)| *line_number)
    .chain(raw_guards.iter().map(|(line_number, _, _)| *line_number))
    .collect::<Vec<_>>();
  let expansions = structural_expansions_for_content(content, path, opts, &line_numbers)
    .unwrap_or_else(|| vec![(Vec::new(), None); line_numbers.len()]);

  let mut expansion_by_line: BTreeMap<usize, (Vec<StructuralParent>, Option<StructuralBody>)> =
    BTreeMap::new();
  for (line_number, expansion) in line_numbers.into_iter().zip(expansions.into_iter()) {
    expansion_by_line.entry(line_number).or_insert(expansion);
  }

  let sensitive_anchors = raw_sensitive
    .into_iter()
    .map(|(line_number, line_content, match_text)| {
      anchor(line_number, line_content, match_text, &expansion_by_line)
    })
    .collect::<Vec<_>>();
  let guard_anchors = raw_guards
    .into_iter()
    .map(|(line_number, line_content, match_text)| {
      anchor(line_number, line_content, match_text, &expansion_by_line)
    })
    .collect::<Vec<_>>();

  let window = around.unwrap_or(0);
  let file_path = path.to_string_lossy().into_owned();
  let mut nearby_guard_count = 0;
  let candidates = sensitive_anchors
    .iter()
    .map(|sensitive| {
      let nearby_guards = guard_anchors
        .iter()
        .filter(|guard| guard.line_number.abs_diff(sensitive.line_number) <= window)
        .filter(|guard| same_enclosing_if_available(sensitive, guard, include_parents))
        .map(call_site)
        .collect::<Vec<_>>();
      nearby_guard_count += nearby_guards.len();
      let parents = if include_parents {
        sensitive.parents.clone()
      } else {
        Vec::new()
      };
      let decorations = decorations_for_anchor(&lines, sensitive)
        .into_iter()
        .map(|(line_number, line_content)| GuardCallSite {
          match_text: decoration_match_text(&line_content),
          line_number,
          line_content,
        })
        .collect();
      GuardSiteCandidate {
        file_path: file_path.clone(),
        sensitive: call_site(sensitive),
        nearby_guards,
        decorations,
        parents,
        body: if include_body {
          sensitive.body.clone()
        } else {
          None
        },
        source_context: around
          .map(|around| source_context(&lines, sensitive.line_number, around))
          .unwrap_or_default(),
        confidence: "local-evidence-only".to_string(),
      }
    })
    .collect();
  GuardSitesFileResult {
    candidates,
    sensitive_anchor_count,
    nearby_guard_count,
  }
}

fn anchor(
  line_number: usize,
  line_content: String,
  match_text: String,
  expansion_by_line: &BTreeMap<usize, (Vec<StructuralParent>, Option<StructuralBody>)>,
) -> LineAnchor {
  let (parents, body) = expansion_by_line
    .get(&line_number)
    .cloned()
    .unwrap_or_default();
  LineAnchor {
    line_number,
    line_content,
    match_text,
    parents,
    body,
  }
}

fn call_site(anchor: &LineAnchor) -> GuardCallSite {
  GuardCallSite {
    match_text: anchor.match_text.clone(),
    line_number: anchor.line_number,
    line_content: anchor.line_content.clone(),
  }
}

fn same_enclosing_if_available(
  sensitive: &LineAnchor,
  guard: &LineAnchor,
  include_parents: bool,
) -> bool {
  if !include_parents {
    return true;
  }
  match (sensitive.parents.last(), guard.parents.last()) {
    (Some(left), Some(right)) => {
      left.kind == right.kind
        && left.name == right.name
        && left.line_start == right.line_start
        && left.line_end == right.line_end
    }
    _ => true,
  }
}

fn decorations_for_anchor(lines: &[String], anchor: &LineAnchor) -> Vec<(usize, String)> {
  let start_line = anchor
    .parents
    .last()
    .map(|parent| parent.line_start)
    .unwrap_or(anchor.line_number);
  let mut out = Vec::new();
  let mut idx = start_line.saturating_sub(2);
  while let Some(line) = lines.get(idx) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
      if idx == 0 {
        break;
      }
      idx -= 1;
      continue;
    }
    if is_decoration_line(trimmed) {
      out.push((idx + 1, line.clone()));
      if idx == 0 {
        break;
      }
      idx -= 1;
      continue;
    }
    break;
  }
  out.reverse();
  out
}

fn is_decoration_line(trimmed: &str) -> bool {
  trimmed.starts_with('@')
    || (!is_comment_line(trimmed) && decoration_match_text(trimmed) != trimmed)
}

fn is_comment_line(trimmed: &str) -> bool {
  trimmed.starts_with('#')
    || trimmed.starts_with("//")
    || trimmed.starts_with("/*")
    || trimmed.starts_with('*')
}

fn decoration_match_text(line: &str) -> String {
  let trimmed = line.trim();
  let lower = trimmed.to_ascii_lowercase();
  for token in ["route", "middleware", "auth", "authorize", "permission"] {
    if lower.contains(token) {
      return token.to_string();
    }
  }
  trimmed.to_string()
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

fn regex_error_result(field: &str, pattern: &str, err: &regex::Error) -> GuardSitesResult {
  let mut res = GuardSitesResult::new(Vec::new());
  res.exit_status = ExitStatus::Error;
  res.push_warning(
    Diagnostic::new(
      "REGEX_PARSE_FAILED",
      format!("{field} regex pattern failed to parse: {err}"),
    )
    .with_hint(regex_parse_hint(pattern)),
  );
  res
}

fn regex_parse_hint(pattern: &str) -> &'static str {
  if pattern.contains(r"\\(") || pattern.contains(r"\\)") {
    "Rust regex uses `\\(` or `\\)` for literal parentheses; remove one backslash if you meant a literal parenthesis"
  } else {
    "use Rust regex syntax; escape literal metacharacters with one backslash, for example `\\(` for a literal `(`, and use bare `|` for alternation"
  }
}
