//! Evidence-only resource acquire/release site scanner.
//!
//! This first slice is deliberately mechanical: user-provided regexes
//! identify acquire-like and release-like lines, then sites are grouped
//! by the nearest enclosing structural declaration.

use std::collections::BTreeMap;
use std::path::Path;

use ignore::WalkBuilder;
use regex::Regex;

use ast_grep_language::Language;
use lmprobe_lang_core::langs::SgLang;
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  Diagnostic, ExitStatus, LineContext, ResourceCallSite, ResourceSite, ResourceSitesResult,
  StructuralBody, StructuralParent,
};

use crate::sensors::{
  apply_walk_opts, structural_expansions_for_content, walk_pre_check, PreCheck, WalkOpts,
};

#[derive(Clone)]
struct ResourceAnchor {
  call: ResourceCallSite,
  parents: Vec<StructuralParent>,
  body: Option<StructuralBody>,
}

#[derive(Default)]
struct ResourceBucket {
  enclosing: Option<StructuralParent>,
  acquire_calls: Vec<ResourceCallSite>,
  release_calls: Vec<ResourceCallSite>,
  defer_calls: Vec<ResourceCallSite>,
  error_branches: Vec<ResourceCallSite>,
  acquire_timing: Option<String>,
  release_lifecycle: Option<String>,
  optional_acquire: Option<bool>,
  source_context: BTreeMap<usize, String>,
  parents: Vec<StructuralParent>,
  body: Option<StructuralBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
  file_path: String,
  line_start: usize,
  line_end: usize,
  kind: String,
  name: String,
}

pub fn resource_sites(
  root: &Path,
  acquire: &str,
  release: &str,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<ResourceSitesResult> {
  let acquire_re = match Regex::new(acquire) {
    Ok(re) => re,
    Err(err) => return Ok(resource_regex_error_result("acquire", acquire, &err)),
  };
  let release_re = match Regex::new(release) {
    Ok(re) => re,
    Err(err) => return Ok(resource_regex_error_result("release", release, &err)),
  };
  let mut sites = Vec::new();
  for file in candidate_files(root, opts) {
    if let Ok(content) = std::fs::read_to_string(&file) {
      sites.extend(resource_sites_in_file(
        &file,
        &content,
        &acquire_re,
        &release_re,
        around,
        parents,
        body,
        opts,
      ));
    }
  }
  Ok(ResourceSitesResult::new(sites))
}

fn candidate_files(root: &Path, opts: &WalkOpts) -> Vec<std::path::PathBuf> {
  let mut files = Vec::new();
  if root.is_file() {
    if is_supported_path(root, opts) {
      files.push(root.to_path_buf());
    }
    return files;
  }

  let mut wb = WalkBuilder::new(root);
  apply_walk_opts(&mut wb, opts, root);
  for entry in wb.build() {
    let Ok(entry) = entry else {
      continue;
    };
    match walk_pre_check(&entry, opts) {
      PreCheck::Process(path, lang) if is_resource_sites_lang(lang) => files.push(path),
      PreCheck::Process(_, _) | PreCheck::Skip | PreCheck::SkipWithDiag(_) => {}
    }
  }
  files
}

fn is_supported_path(path: &Path, opts: &WalkOpts) -> bool {
  opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(is_resource_sites_lang)
}

fn is_resource_sites_lang(lang: SgLang) -> bool {
  matches!(
    lang,
    SgLang::Go | SgLang::Rust | SgLang::JavaScript | SgLang::TypeScript | SgLang::Tsx
  )
}

fn is_js_ts_resource_lang(lang: SgLang) -> bool {
  matches!(lang, SgLang::JavaScript | SgLang::TypeScript | SgLang::Tsx)
}

#[allow(clippy::too_many_arguments)]
fn resource_sites_in_file(
  path: &Path,
  content: &str,
  acquire_re: &Regex,
  release_re: &Regex,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> Vec<ResourceSite> {
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let is_go = opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(|lang| lang == SgLang::Go);
  let is_js_ts = opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(is_js_ts_resource_lang);
  let mut anchor_records = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    if acquire_re.is_match(line)
      || release_re.is_match(line)
      || is_error_branch_line(line)
      || (is_go && is_go_return_line(line))
    {
      anchor_records.push((idx + 1, line.trim().to_string()));
    }
  }
  if anchor_records.is_empty() {
    return Vec::new();
  }

  let anchor_lines = anchor_records
    .iter()
    .map(|(line_number, _)| *line_number)
    .collect::<Vec<_>>();
  let expansions = structural_expansions_for_content(content, path, opts, &anchor_lines)
    .unwrap_or_else(|| vec![(Vec::new(), None); anchor_lines.len()]);
  let mut acquire_anchors = Vec::new();
  let mut release_anchors = Vec::new();
  let mut error_anchors = Vec::new();

  for ((line_number, line), (parents, body)) in
    anchor_records.into_iter().zip(expansions.into_iter())
  {
    if let Some(callee) = first_match(acquire_re, &line) {
      acquire_anchors.push(ResourceAnchor {
        call: ResourceCallSite {
          callee,
          line_number,
          line_content: line.clone(),
        },
        parents: parents.clone(),
        body: body.clone(),
      });
    }
    if let Some(callee) = first_match(release_re, &line) {
      release_anchors.push(ResourceAnchor {
        call: ResourceCallSite {
          callee,
          line_number,
          line_content: line.clone(),
        },
        parents: parents.clone(),
        body: body.clone(),
      });
    }
    if is_error_branch_line(&line) || (is_go && is_go_return_line(&line)) {
      error_anchors.push(ResourceAnchor {
        call: ResourceCallSite {
          callee: error_branch_label(&line).to_string(),
          line_number,
          line_content: line,
        },
        parents,
        body,
      });
    }
  }

  let file_path = path.to_string_lossy().into_owned();
  let mut buckets: BTreeMap<BucketKey, ResourceBucket> = BTreeMap::new();
  for acquire in acquire_anchors {
    let key = bucket_key(&file_path, &acquire);
    let bucket = buckets
      .entry(key.clone())
      .or_insert_with(|| ResourceBucket {
        enclosing: nearest_enclosing(&acquire.parents),
        parents: if include_parents {
          acquire.parents.clone()
        } else {
          Vec::new()
        },
        body: if include_body {
          acquire.body.clone()
        } else {
          None
        },
        ..Default::default()
      });
    bucket.acquire_calls.push(acquire.call.clone());
    if let Some(around) = around {
      add_context(
        &mut bucket.source_context,
        &lines,
        acquire.call.line_number,
        around,
      );
    }
  }

  for bucket in buckets.values_mut() {
    if let Some(enclosing) = bucket.enclosing.as_ref() {
      bucket.release_calls = release_anchors
        .iter()
        .filter(|anchor| enclosing_contains(enclosing, anchor.call.line_number))
        .map(|anchor| anchor.call.clone())
        .collect();
      bucket.defer_calls = release_anchors
        .iter()
        .filter(|anchor| enclosing_contains(enclosing, anchor.call.line_number))
        .filter(|anchor| is_defer_line(&anchor.call.line_content))
        .map(|anchor| anchor.call.clone())
        .collect();
      bucket.error_branches = if is_go {
        go_error_branches_after_acquire(bucket, enclosing, &error_anchors, &lines)
      } else {
        error_anchors
          .iter()
          .filter(|anchor| enclosing_contains(enclosing, anchor.call.line_number))
          .map(|anchor| anchor.call.clone())
          .collect()
      };
    }
    if is_js_ts {
      classify_js_ts_resource_bucket(bucket, &lines);
    }
  }

  buckets
    .into_values()
    .map(|bucket| ResourceSite {
      file_path: file_path.clone(),
      enclosing: bucket.enclosing,
      acquire_calls: bucket.acquire_calls,
      release_calls: bucket.release_calls,
      defer_calls: bucket.defer_calls,
      error_branches: bucket.error_branches,
      acquire_timing: bucket.acquire_timing,
      release_lifecycle: bucket.release_lifecycle,
      optional_acquire: bucket.optional_acquire,
      source_context: bucket
        .source_context
        .into_iter()
        .map(|(line_number, line_content)| LineContext {
          line_number,
          line_content,
        })
        .collect(),
      parents: bucket.parents,
      body: bucket.body,
      confidence: "local-evidence-only".to_string(),
    })
    .collect()
}

fn first_match(re: &Regex, line: &str) -> Option<String> {
  re.find(line).map(|m| m.as_str().to_string())
}

fn bucket_key(file_path: &str, anchor: &ResourceAnchor) -> BucketKey {
  let enclosing = nearest_enclosing(&anchor.parents);
  BucketKey {
    file_path: file_path.to_string(),
    line_start: enclosing
      .as_ref()
      .map(|p| p.line_start)
      .unwrap_or(anchor.call.line_number),
    line_end: enclosing
      .as_ref()
      .map(|p| p.line_end)
      .unwrap_or(anchor.call.line_number),
    kind: enclosing
      .as_ref()
      .map(|p| p.kind.clone())
      .unwrap_or_else(|| "line".to_string()),
    name: enclosing
      .as_ref()
      .and_then(|p| p.name.clone())
      .unwrap_or_default(),
  }
}

fn nearest_enclosing(parents: &[StructuralParent]) -> Option<StructuralParent> {
  parents
    .iter()
    .rev()
    .find(|parent| is_resource_enclosing_kind(&parent.kind))
    .cloned()
    .or_else(|| parents.last().cloned())
}

fn is_resource_enclosing_kind(kind: &str) -> bool {
  let kind = kind.to_ascii_lowercase();
  kind.contains("function")
    || kind.contains("method")
    || kind.contains("constructor")
    || kind == "class"
    || kind.ends_with("_class")
    || kind.ends_with("_item")
}

fn enclosing_contains(enclosing: &StructuralParent, line_number: usize) -> bool {
  enclosing.line_start <= line_number && line_number <= enclosing.line_end
}

fn add_context(
  source_context: &mut BTreeMap<usize, String>,
  lines: &[String],
  line_number: usize,
  around: usize,
) {
  let target_idx = line_number.saturating_sub(1);
  let start = target_idx.saturating_sub(around);
  let end = (target_idx + around + 1).min(lines.len());
  for idx in start..end {
    if let Some(line) = lines.get(idx) {
      source_context.insert(idx + 1, line.clone());
    }
  }
}

fn classify_js_ts_resource_bucket(bucket: &mut ResourceBucket, lines: &[String]) {
  bucket.acquire_timing = classify_js_ts_acquire_timing(bucket, lines);
  bucket.release_lifecycle = classify_js_ts_release_lifecycle(bucket, lines);
  if bucket
    .acquire_calls
    .iter()
    .any(|call| is_js_ts_optional_acquire(call, lines))
  {
    bucket.optional_acquire = Some(true);
  }
}

fn classify_js_ts_acquire_timing(bucket: &ResourceBucket, lines: &[String]) -> Option<String> {
  if bucket
    .acquire_calls
    .iter()
    .any(|call| call.line_content.contains("import("))
  {
    return Some("dynamic_import".to_string());
  }
  if bucket
    .acquire_calls
    .iter()
    .any(|call| js_ts_preceding_text(lines, call.line_number, 3).contains("addeventlistener"))
  {
    return Some("event_handler".to_string());
  }
  if bucket
    .acquire_calls
    .iter()
    .any(|call| is_js_ts_lazy_acquire_line(&call.line_content))
  {
    return Some("lazy_callback".to_string());
  }
  if bucket.enclosing.as_ref().map_or(true, |enclosing| {
    !is_resource_enclosing_kind(&enclosing.kind)
  }) {
    return Some("module_load".to_string());
  }
  None
}

fn classify_js_ts_release_lifecycle(bucket: &ResourceBucket, lines: &[String]) -> Option<String> {
  let release_texts = bucket
    .release_calls
    .iter()
    .map(|call| js_ts_nearby_text(lines, call.line_number, 3));
  best_js_ts_release_lifecycle(release_texts)
}

fn best_js_ts_release_lifecycle<I>(texts: I) -> Option<String>
where
  I: IntoIterator<Item = String>,
{
  let mut saw_dispose = false;
  let mut saw_visibility = false;
  let mut saw_beforeunload = false;
  for text in texts {
    if text.contains("pagehide") {
      return Some("pagehide".to_string());
    }
    saw_beforeunload |= text.contains("beforeunload");
    saw_visibility |= text.contains("visibilitychange");
    saw_dispose |=
      text.contains("dispose") || text.contains("unregister") || text.contains("registerhooks");
  }
  if saw_beforeunload {
    Some("beforeunload".to_string())
  } else if saw_visibility {
    Some("visibilitychange".to_string())
  } else if saw_dispose {
    Some("dispose_callback".to_string())
  } else {
    None
  }
}

fn is_js_ts_optional_acquire(call: &ResourceCallSite, lines: &[String]) -> bool {
  let line = call.line_content.as_str();
  is_js_ts_lazy_acquire_line(line)
    || line.contains("?.")
    || js_ts_nearby_text(lines, call.line_number, 2).contains("catch")
}

fn is_js_ts_lazy_acquire_line(line: &str) -> bool {
  line.contains("??=") || line.contains("||=") || line.contains("&&=")
}

fn js_ts_nearby_text(lines: &[String], line_number: usize, radius: usize) -> String {
  if lines.is_empty() {
    return String::new();
  }
  let idx = line_number.saturating_sub(1);
  let start = idx.saturating_sub(radius);
  let end = (idx + radius + 1).min(lines.len());
  lines[start..end].join("\n").to_ascii_lowercase()
}

fn js_ts_preceding_text(lines: &[String], line_number: usize, radius: usize) -> String {
  if lines.is_empty() {
    return String::new();
  }
  let idx = line_number.saturating_sub(1);
  let start = idx.saturating_sub(radius);
  lines[start..=idx].join("\n").to_ascii_lowercase()
}

fn is_defer_line(line: &str) -> bool {
  line.trim_start().starts_with("defer ")
}

fn is_error_branch_line(line: &str) -> bool {
  let trimmed = line.trim_start();
  trimmed.starts_with("return err")
    || trimmed.contains("return nil, err")
    || trimmed.contains("return nil, 0, err")
    || trimmed.contains("return Err(")
    || trimmed.contains("bail!(")
    || has_rust_try_operator(trimmed)
}

fn is_go_return_line(line: &str) -> bool {
  let trimmed = line.trim_start();
  trimmed == "return" || trimmed.starts_with("return ")
}

fn error_branch_label(line: &str) -> &'static str {
  if line.trim_start().starts_with("return") {
    "return"
  } else if line.contains("bail!(") {
    "bail"
  } else {
    "question_mark"
  }
}

fn has_rust_try_operator(trimmed: &str) -> bool {
  trimmed.ends_with('?') || trimmed.contains("?;") || trimmed.contains("?)")
}

fn go_error_branches_after_acquire(
  bucket: &ResourceBucket,
  enclosing: &StructuralParent,
  error_anchors: &[ResourceAnchor],
  lines: &[String],
) -> Vec<ResourceCallSite> {
  let mut branches = BTreeMap::new();
  for acquire in &bucket.acquire_calls {
    let release_limit = bucket
      .release_calls
      .iter()
      .chain(bucket.defer_calls.iter())
      .filter(|call| call.line_number > acquire.line_number)
      .map(|call| call.line_number)
      .min()
      .unwrap_or(enclosing.line_end + 1);
    for anchor in error_anchors {
      let line_number = anchor.call.line_number;
      if line_number <= acquire.line_number
        || line_number >= release_limit
        || !enclosing_contains(enclosing, line_number)
        || is_immediate_acquire_error_return(lines, acquire.line_number, line_number)
      {
        continue;
      }
      branches
        .entry(line_number)
        .or_insert_with(|| anchor.call.clone());
    }
  }
  branches.into_values().collect()
}

fn is_immediate_acquire_error_return(
  lines: &[String],
  acquire_line: usize,
  return_line: usize,
) -> bool {
  if return_line <= acquire_line || return_line.saturating_sub(acquire_line) > 3 {
    return false;
  }
  let Some(previous) = previous_non_empty_line(lines, return_line) else {
    return false;
  };
  let trimmed = previous.trim();
  trimmed.starts_with("if err != nil") && !trimmed.contains(":=")
}

fn previous_non_empty_line(lines: &[String], line_number: usize) -> Option<&str> {
  let mut idx = line_number.saturating_sub(2);
  loop {
    let line = lines.get(idx)?;
    if !line.trim().is_empty() {
      return Some(line);
    }
    if idx == 0 {
      return None;
    }
    idx -= 1;
  }
}

fn resource_regex_error_result(
  field: &str,
  pattern: &str,
  err: &regex::Error,
) -> ResourceSitesResult {
  let mut res = ResourceSitesResult::new(Vec::new());
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
