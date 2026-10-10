//! Evidence-only OpenAPI, route, and HTTP-client literal extraction.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ast_grep_language::Language;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::Value;

use lmprobe_lang_core::langs::{is_code_lang, SgLang};
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  Diagnostic, HttpClientCallAnchor, HttpClientCallsResult, LineContext, OpenApiFactsResult,
  OpenApiOperationFact, RouteAnchor, RouteAnchorsResult, StructuralBody, StructuralParent,
};

use crate::sensors::{
  apply_walk_opts, structural_expansions_for_content, walk_pre_check, PreCheck, WalkOpts,
};

const METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

pub fn openapi_facts(root: &Path, opts: &WalkOpts) -> Result<OpenApiFactsResult> {
  let mut operations = Vec::new();
  let mut warnings = Vec::new();
  let (files, file_warnings) = openapi_candidate_files(root, opts);
  warnings.extend(file_warnings);
  for file in files {
    let Ok(content) = std::fs::read_to_string(&file) else {
      continue;
    };
    match parse_openapi_value(&file, &content) {
      Ok(value) => operations.extend(openapi_operations(&file, &value)),
      Err(message) => warnings.push(
        Diagnostic::new("OPENAPI_PARSE_FAILED", message)
          .with_hint("fix the OpenAPI JSON/YAML syntax or pass a narrower path"),
      ),
    }
  }
  let mut res = OpenApiFactsResult::new(operations);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

pub fn route_anchors(
  root: &Path,
  framework: Option<&str>,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<RouteAnchorsResult> {
  let mut routes = Vec::new();
  let (files, warnings) = code_files(root, opts);
  for file in files {
    if let Ok(content) = std::fs::read_to_string(&file) {
      routes.extend(route_anchors_in_file(
        &file, &content, framework, around, parents, body, opts,
      ));
    }
  }
  let mut res = RouteAnchorsResult::new(routes);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

pub fn http_client_calls(
  root: &Path,
  around: Option<usize>,
  parents: bool,
  body: bool,
  opts: &WalkOpts,
) -> Result<HttpClientCallsResult> {
  let mut calls = Vec::new();
  let (files, warnings) = code_files(root, opts);
  for file in files {
    if let Ok(content) = std::fs::read_to_string(&file) {
      calls.extend(http_client_calls_in_file(
        &file, &content, around, parents, body, opts,
      ));
    }
  }
  let mut res = HttpClientCallsResult::new(calls);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

fn openapi_candidate_files(root: &Path, opts: &WalkOpts) -> (Vec<PathBuf>, Vec<Diagnostic>) {
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
    if is_openapi_ext(root) {
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
      PreCheck::Process(path, _) if is_openapi_ext(&path) => {
        files.insert(path);
      }
      PreCheck::SkipWithDiag(diagnostic) => warnings.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  (files.into_iter().collect(), warnings)
}

fn parse_openapi_value(path: &Path, content: &str) -> std::result::Result<Value, String> {
  match path.extension().and_then(|ext| ext.to_str()) {
    Some("json") => serde_json::from_str::<Value>(content)
      .map_err(|err| format!("failed to parse OpenAPI JSON {}: {err}", path.display())),
    Some("yaml") | Some("yml") => {
      let yaml = serde_yaml::from_str::<serde_yaml::Value>(content)
        .map_err(|err| format!("failed to parse OpenAPI YAML {}: {err}", path.display()))?;
      serde_json::to_value(yaml)
        .map_err(|err| format!("failed to normalize OpenAPI YAML {}: {err}", path.display()))
    }
    _ => Ok(Value::Null),
  }
}

fn openapi_operations(path: &Path, value: &Value) -> Vec<OpenApiOperationFact> {
  let Some(paths) = value.get("paths").and_then(Value::as_object) else {
    return Vec::new();
  };
  let mut operations = Vec::new();
  for (route_path, path_item) in paths {
    let path_params = required_params(path_item, value);
    for method in METHODS {
      let Some(operation) = path_item.get(*method).and_then(Value::as_object) else {
        continue;
      };
      let mut params = path_params.clone();
      params.extend(required_params(&Value::Object(operation.clone()), value));
      params.sort();
      params.dedup();
      let response_schemas = operation
        .get("responses")
        .map(schema_refs)
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
      operations.push(OpenApiOperationFact {
        file_path: path.to_string_lossy().into_owned(),
        method: method.to_ascii_uppercase(),
        path: route_path.clone(),
        operation_id: operation
          .get("operationId")
          .and_then(Value::as_str)
          .map(str::to_string),
        required_params: params,
        response_schemas,
      });
    }
  }
  operations
}

fn required_params(value: &Value, root: &Value) -> Vec<String> {
  value
    .get("parameters")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|param| resolve_openapi_ref(param, root))
    .filter(|param| param.get("required").and_then(Value::as_bool) == Some(true))
    .filter_map(|param| {
      param
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
    })
    .collect()
}

fn resolve_openapi_ref<'a>(value: &'a Value, root: &'a Value) -> Option<&'a Value> {
  let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
    return Some(value);
  };
  let pointer = reference.strip_prefix('#')?;
  root.pointer(pointer)
}

fn schema_refs(value: &Value) -> BTreeSet<String> {
  let mut refs = BTreeSet::new();
  collect_schema_refs(value, &mut refs);
  refs
}

fn collect_schema_refs(value: &Value, refs: &mut BTreeSet<String>) {
  match value {
    Value::Object(map) => {
      if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
        refs.insert(
          reference
            .rsplit('/')
            .next()
            .unwrap_or(reference)
            .to_string(),
        );
      }
      if let Some(title) = map.get("title").and_then(Value::as_str) {
        refs.insert(title.to_string());
      }
      for child in map.values() {
        collect_schema_refs(child, refs);
      }
    }
    Value::Array(items) => {
      for item in items {
        collect_schema_refs(item, refs);
      }
    }
    _ => {}
  }
}

#[allow(clippy::too_many_arguments)]
fn route_anchors_in_file(
  path: &Path,
  content: &str,
  framework: Option<&str>,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> Vec<RouteAnchor> {
  let route_re = Regex::new(
    r#"(?i)(?:^|\s|@)([A-Za-z_][\w\.]*)\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#,
  )
  .expect("route regex");
  let fastapi_only = framework.is_some_and(|f| f.eq_ignore_ascii_case("fastapi"));
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let mut raw = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    let trimmed = line.trim_start();
    if fastapi_only && !trimmed.starts_with('@') {
      continue;
    }
    if let Some(caps) = route_re.captures(line) {
      let receiver = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
      if !is_route_receiver(trimmed, receiver, fastapi_only) {
        continue;
      }
      let (handler, expansion_line) = handler_after(&lines, idx + 1)
        .map(|(handler, line)| (Some(handler), line))
        .unwrap_or((None, idx + 1));
      raw.push((
        idx + 1,
        line.clone(),
        caps
          .get(2)
          .map(|m| m.as_str().to_ascii_uppercase())
          .unwrap_or_default(),
        caps
          .get(3)
          .map(|m| m.as_str().to_string())
          .unwrap_or_default(),
        handler,
        expansion_line,
      ));
    }
  }
  let expansions = expansions_by_line(
    content,
    path,
    opts,
    raw
      .iter()
      .map(|(_, _, _, _, _, expansion_line)| *expansion_line),
  );
  raw
    .into_iter()
    .map(
      |(line_number, line_content, method, route_path, handler, expansion_line)| {
        let (parents, found_body) = expansions.get(&expansion_line).cloned().unwrap_or_default();
        RouteAnchor {
          file_path: path.to_string_lossy().into_owned(),
          method,
          path: route_path,
          line_number,
          line_content,
          handler,
          source_context: around
            .map(|around| source_context(&lines, line_number, around))
            .unwrap_or_default(),
          parents: if include_parents { parents } else { Vec::new() },
          body: if include_body { found_body } else { None },
          confidence: "literal-evidence-only".to_string(),
        }
      },
    )
    .collect()
}

fn is_route_receiver(trimmed: &str, receiver: &str, fastapi_only: bool) -> bool {
  if fastapi_only {
    return trimmed.starts_with('@');
  }
  if trimmed.starts_with('@') {
    return true;
  }
  let last = receiver
    .rsplit('.')
    .next()
    .unwrap_or(receiver)
    .to_ascii_lowercase();
  matches!(
    last.as_str(),
    "app" | "router" | "routes" | "route" | "server" | "mux"
  )
}

fn http_client_calls_in_file(
  path: &Path,
  content: &str,
  around: Option<usize>,
  include_parents: bool,
  include_body: bool,
  opts: &WalkOpts,
) -> Vec<HttpClientCallAnchor> {
  let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
  let mut raw = Vec::new();
  for (idx, line) in lines.iter().enumerate() {
    raw.extend(client_calls_on_line(idx + 1, line));
  }
  let expansions = expansions_by_line(
    content,
    path,
    opts,
    raw.iter().map(|(line, _, _, _, _)| *line),
  );
  raw
    .into_iter()
    .map(|(line_number, line_content, method, url, client)| {
      let (parents, found_body) = expansions.get(&line_number).cloned().unwrap_or_default();
      HttpClientCallAnchor {
        file_path: path.to_string_lossy().into_owned(),
        method,
        url,
        line_number,
        line_content,
        client,
        source_context: around
          .map(|around| source_context(&lines, line_number, around))
          .unwrap_or_default(),
        parents: if include_parents { parents } else { Vec::new() },
        body: if include_body { found_body } else { None },
        confidence: "literal-evidence-only".to_string(),
      }
    })
    .collect()
}

fn client_calls_on_line(
  line_number: usize,
  line: &str,
) -> Vec<(usize, String, Option<String>, String, Option<String>)> {
  let mut out = Vec::new();
  if is_comment_line(line) {
    return out;
  }
  let direct =
    Regex::new(r#"(?i)\b(requests|httpx|axios)\.(get|post|put|delete|patch)\(\s*["']([^"']+)["']"#)
      .expect("direct client regex");
  for caps in direct.captures_iter(line) {
    if caps
      .get(0)
      .is_some_and(|m| is_inside_string_prefix(line, m.start()))
    {
      continue;
    }
    out.push((
      line_number,
      line.to_string(),
      caps.get(2).map(|m| m.as_str().to_ascii_uppercase()),
      caps
        .get(3)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default(),
      caps.get(1).map(|m| m.as_str().to_string()),
    ));
  }

  let request = Regex::new(
    r#"(?i)\b(requests|httpx)\.request\(\s*["'](GET|POST|PUT|DELETE|PATCH)["']\s*,\s*["']([^"']+)["']"#,
  )
  .expect("request client regex");
  for caps in request.captures_iter(line) {
    if caps
      .get(0)
      .is_some_and(|m| is_inside_string_prefix(line, m.start()))
    {
      continue;
    }
    out.push((
      line_number,
      line.to_string(),
      caps.get(2).map(|m| m.as_str().to_ascii_uppercase()),
      caps
        .get(3)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default(),
      caps.get(1).map(|m| m.as_str().to_string()),
    ));
  }

  let fetch = Regex::new(r#"(?i)\bfetch\(\s*["']([^"']+)["']"#).expect("fetch regex");
  if let Some(caps) = fetch.captures(line) {
    if !caps
      .get(0)
      .is_some_and(|m| is_inside_string_prefix(line, m.start()))
    {
      out.push((
        line_number,
        line.to_string(),
        fetch_method(line),
        caps
          .get(1)
          .map(|m| m.as_str().to_string())
          .unwrap_or_default(),
        Some("fetch".to_string()),
      ));
    }
  }
  out
}

fn is_comment_line(line: &str) -> bool {
  let trimmed = line.trim_start();
  trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.starts_with("/*")
}

fn is_inside_string_prefix(line: &str, end: usize) -> bool {
  let mut single = false;
  let mut double = false;
  let mut escaped = false;
  for ch in line[..end].chars() {
    if escaped {
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      continue;
    }
    match ch {
      '\'' if !double => single = !single,
      '"' if !single => double = !double,
      _ => {}
    }
  }
  single || double
}

fn fetch_method(line: &str) -> Option<String> {
  let method_re = Regex::new(r#"(?i)\bmethod\s*:\s*["'](GET|POST|PUT|DELETE|PATCH)["']"#)
    .expect("fetch method regex");
  method_re
    .captures(line)
    .and_then(|caps| caps.get(1).map(|m| m.as_str().to_ascii_uppercase()))
    .or_else(|| Some("GET".to_string()))
}

fn handler_after(lines: &[String], route_line: usize) -> Option<(String, usize)> {
  let def_re =
    Regex::new(r#"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\("#).expect("python def regex");
  for (idx, line) in lines.iter().enumerate().skip(route_line).take(6) {
    if line.trim().is_empty() || line.trim_start().starts_with('@') {
      continue;
    }
    return def_re
      .captures(line)
      .and_then(|caps| caps.get(1).map(|m| (m.as_str().to_string(), idx + 1)));
  }
  None
}

fn expansions_by_line(
  content: &str,
  path: &Path,
  opts: &WalkOpts,
  line_numbers: impl Iterator<Item = usize>,
) -> BTreeMap<usize, (Vec<StructuralParent>, Option<StructuralBody>)> {
  let line_numbers = line_numbers.collect::<Vec<_>>();
  let expansions = structural_expansions_for_content(content, path, opts, &line_numbers)
    .unwrap_or_else(|| vec![(Vec::new(), None); line_numbers.len()]);
  line_numbers.into_iter().zip(expansions).collect()
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

fn code_files(root: &Path, opts: &WalkOpts) -> (Vec<PathBuf>, Vec<Diagnostic>) {
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
    if is_code_path(root, opts) {
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
      PreCheck::Process(path, lang) if is_code_lang(lang) => {
        files.insert(path);
      }
      PreCheck::SkipWithDiag(diagnostic) => warnings.push(diagnostic),
      PreCheck::Process(_, _) | PreCheck::Skip => {}
    }
  }
  (files.into_iter().collect(), warnings)
}

fn is_code_path(path: &Path, opts: &WalkOpts) -> bool {
  opts
    .lang_filter
    .or_else(|| SgLang::from_path(path))
    .is_some_and(is_code_lang)
}

fn is_openapi_ext(path: &Path) -> bool {
  matches!(
    path.extension().and_then(|ext| ext.to_str()),
    Some("json" | "yaml" | "yml")
  )
}
