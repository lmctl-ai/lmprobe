//! Evidence-only config key extractor with optional lexical code anchors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ast_grep_language::Language;
use ignore::WalkBuilder;

use lmprobe_lang_core::langs::{is_code_lang, SgLang};
use lmprobe_types::error::Result;
use lmprobe_types::schema::{
  ConfigCodeAnchor, ConfigKeyRecord, ConfigKeysResult, Diagnostic, GrepHit, LineContext,
};

use crate::sensors::{
  apply_walk_opts, attach_grep_roles_for_literal, attach_grep_structural_expansions,
  walk_pre_check, PreCheck, WalkOpts,
};

const MAX_GREP_KEYS: usize = 64;
const MAX_CODE_ANCHORS_PER_KEY: usize = 20;

pub fn config_keys(
  roots: &[PathBuf],
  key_filter: Option<&str>,
  grep_code: bool,
  around: Option<usize>,
  parents: bool,
  role: bool,
  opts: &WalkOpts,
) -> Result<ConfigKeysResult> {
  let mut warnings = Vec::new();
  let files = config_files(roots, opts, &mut warnings);
  let mut keys = Vec::new();

  for file in files {
    let Ok(content) = std::fs::read_to_string(&file) else {
      continue;
    };
    match parse_config_file(&file, &content) {
      Ok(mut records) => {
        if let Some(target_key) = key_filter {
          records.retain(|record| record.key_path == target_key);
        }
        keys.append(&mut records);
      }
      Err(message) => warnings.push(
        Diagnostic::new("CONFIG_PARSE_FAILED", message)
          .with_hint("fix the config syntax or omit that file from this config-keys run"),
      ),
    }
  }

  if grep_code && !keys.is_empty() {
    let code_roots = code_anchor_roots(roots);
    attach_code_anchors(
      &mut keys,
      &code_roots,
      around,
      parents,
      role,
      opts,
      &mut warnings,
    );
  }

  let mut res = ConfigKeysResult::new(keys);
  for warning in warnings {
    res.push_warning(warning);
  }
  Ok(res)
}

fn code_anchor_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
  roots
    .iter()
    .map(|root| {
      if root.is_file() && is_config_path(root) {
        let parent = root
          .parent()
          .filter(|parent| !parent.as_os_str().is_empty());
        parent.unwrap_or_else(|| Path::new(".")).to_path_buf()
      } else {
        root.clone()
      }
    })
    .collect()
}

fn config_files(
  roots: &[PathBuf],
  opts: &WalkOpts,
  warnings: &mut Vec<Diagnostic>,
) -> Vec<PathBuf> {
  let mut files = BTreeSet::new();
  for root in roots {
    if root.is_file() {
      if is_config_path(root) {
        files.insert(root.clone());
      }
      continue;
    }
    if !root.exists() {
      warnings.push(Diagnostic::new(
        "PATH_NOT_FOUND",
        format!("path not found: {}", root.display()),
      ));
      continue;
    }
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, opts, root);
    for entry in wb.build() {
      let Ok(entry) = entry else {
        continue;
      };
      match walk_pre_check(&entry, opts) {
        PreCheck::Process(path, _) if is_config_path(&path) => {
          files.insert(path);
        }
        PreCheck::SkipWithDiag(d) => warnings.push(d),
        PreCheck::Process(_, _) | PreCheck::Skip => {}
      }
    }
  }
  files.into_iter().collect()
}

fn parse_config_file(
  path: &Path,
  content: &str,
) -> std::result::Result<Vec<ConfigKeyRecord>, String> {
  match config_lang(path).as_deref() {
    Some("json") => serde_json::from_str::<serde_json::Value>(content)
      .map(|value| records_from_json(path, content, &value))
      .map_err(|err| format!("failed to parse JSON config {}: {err}", path.display())),
    Some("yaml") => serde_yaml::from_str::<serde_yaml::Value>(content)
      .map(|value| records_from_yaml(path, content, &value))
      .map_err(|err| format!("failed to parse YAML config {}: {err}", path.display())),
    Some("toml") => toml::from_str::<toml::Value>(content)
      .map(|value| records_from_toml(path, content, &value))
      .map_err(|err| format!("failed to parse TOML config {}: {err}", path.display())),
    _ => Ok(Vec::new()),
  }
}

fn records_from_json(
  path: &Path,
  content: &str,
  value: &serde_json::Value,
) -> Vec<ConfigKeyRecord> {
  let mut out = Vec::new();
  walk_json(path, content, value, &mut Vec::new(), None, &mut out);
  out
}

fn walk_json(
  path: &Path,
  content: &str,
  value: &serde_json::Value,
  prefix: &mut Vec<String>,
  key: Option<&str>,
  out: &mut Vec<ConfigKeyRecord>,
) {
  if let Some(key) = key {
    out.push(record(
      path,
      "json",
      prefix,
      key,
      content,
      json_kind(value),
      scalar_summary_json(value),
    ));
  }
  match value {
    serde_json::Value::Object(map) => {
      for (child_key, child_value) in map {
        prefix.push(child_key.clone());
        walk_json(path, content, child_value, prefix, Some(child_key), out);
        prefix.pop();
      }
    }
    serde_json::Value::Array(items) => {
      for (idx, item) in items.iter().enumerate() {
        prefix.push(format!("[{idx}]"));
        walk_json(path, content, item, prefix, None, out);
        prefix.pop();
      }
    }
    _ => {}
  }
}

fn records_from_yaml(
  path: &Path,
  content: &str,
  value: &serde_yaml::Value,
) -> Vec<ConfigKeyRecord> {
  let mut out = Vec::new();
  walk_yaml(path, content, value, &mut Vec::new(), None, &mut out);
  out
}

fn walk_yaml(
  path: &Path,
  content: &str,
  value: &serde_yaml::Value,
  prefix: &mut Vec<String>,
  key: Option<&str>,
  out: &mut Vec<ConfigKeyRecord>,
) {
  if let Some(key) = key {
    out.push(record(
      path,
      "yaml",
      prefix,
      key,
      content,
      yaml_kind(value),
      scalar_summary_yaml(value),
    ));
  }
  match value {
    serde_yaml::Value::Mapping(map) => {
      for (child_key, child_value) in map {
        let Some(child_key) = yaml_key(child_key) else {
          continue;
        };
        prefix.push(child_key.clone());
        walk_yaml(path, content, child_value, prefix, Some(&child_key), out);
        prefix.pop();
      }
    }
    serde_yaml::Value::Sequence(items) => {
      for (idx, item) in items.iter().enumerate() {
        prefix.push(format!("[{idx}]"));
        walk_yaml(path, content, item, prefix, None, out);
        prefix.pop();
      }
    }
    _ => {}
  }
}

fn records_from_toml(path: &Path, content: &str, value: &toml::Value) -> Vec<ConfigKeyRecord> {
  let mut out = Vec::new();
  walk_toml(path, content, value, &mut Vec::new(), None, &mut out);
  out
}

fn walk_toml(
  path: &Path,
  content: &str,
  value: &toml::Value,
  prefix: &mut Vec<String>,
  key: Option<&str>,
  out: &mut Vec<ConfigKeyRecord>,
) {
  if let Some(key) = key {
    out.push(record(
      path,
      "toml",
      prefix,
      key,
      content,
      toml_kind(value),
      scalar_summary_toml(value),
    ));
  }
  match value {
    toml::Value::Table(map) => {
      for (child_key, child_value) in map {
        prefix.push(child_key.clone());
        walk_toml(path, content, child_value, prefix, Some(child_key), out);
        prefix.pop();
      }
    }
    toml::Value::Array(items) => {
      for (idx, item) in items.iter().enumerate() {
        prefix.push(format!("[{idx}]"));
        walk_toml(path, content, item, prefix, None, out);
        prefix.pop();
      }
    }
    _ => {}
  }
}

fn record(
  path: &Path,
  lang: &str,
  prefix: &[String],
  key: &str,
  content: &str,
  value_kind: &str,
  value_summary: Option<String>,
) -> ConfigKeyRecord {
  let key_path = join_key_path(prefix);
  ConfigKeyRecord {
    file_path: path.to_string_lossy().into_owned(),
    lang: lang.to_string(),
    key_path: key_path.clone(),
    key: key.to_string(),
    line_number: best_effort_line(content, lang, &key_path, key),
    value_kind: value_kind.to_string(),
    value_summary,
    code_anchors: Vec::new(),
  }
}

fn attach_code_anchors(
  keys: &mut [ConfigKeyRecord],
  roots: &[PathBuf],
  around: Option<usize>,
  parents: bool,
  role: bool,
  opts: &WalkOpts,
  warnings: &mut Vec<Diagnostic>,
) {
  let mut leaf_keys = keys
    .iter()
    .map(|record| record.key.clone())
    .filter(|key| key.chars().count() >= 2)
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>();
  if leaf_keys.len() > MAX_GREP_KEYS {
    leaf_keys.truncate(MAX_GREP_KEYS);
    warnings.push(
      Diagnostic::new(
        "CONFIG_KEYS_GREP_KEY_LIMIT",
        format!("config-keys --grep-code searched the first {MAX_GREP_KEYS} unique leaf keys"),
      )
      .with_hint("run config-keys on a narrower path to inspect more keys"),
    );
  }

  let code_files = code_files(roots, opts);
  let mut hits_by_key: BTreeMap<String, Vec<GrepHit>> = BTreeMap::new();
  let mut truncated_keys = BTreeSet::new();
  for file in code_files {
    let Ok(content) = std::fs::read_to_string(&file) else {
      continue;
    };
    let file_path = file.to_string_lossy().into_owned();
    for key in &leaf_keys {
      for (idx, line) in content.lines().enumerate() {
        if !line.contains(key) {
          continue;
        }
        let hits = hits_by_key.entry(key.clone()).or_default();
        if hits.len() >= MAX_CODE_ANCHORS_PER_KEY {
          truncated_keys.insert(key.clone());
          break;
        }
        hits.push(GrepHit {
          file_path: file_path.clone(),
          line_number: idx + 1,
          line_content: line.to_string(),
          ..Default::default()
        });
      }
    }
  }

  for (key, hits) in &mut hits_by_key {
    enrich_hits(hits, key, around, parents, role, opts);
  }
  for key in truncated_keys {
    warnings.push(
      Diagnostic::new(
        "CONFIG_KEYS_GREP_ANCHOR_LIMIT",
        format!(
          "config-keys --grep-code kept the first {MAX_CODE_ANCHORS_PER_KEY} code anchors for key {key}"
        ),
      )
      .with_hint("run grep for that key directly to inspect every lexical anchor"),
    );
  }

  for record in keys {
    if let Some(hits) = hits_by_key.get(&record.key) {
      record.code_anchors = hits
        .iter()
        .map(|hit| ConfigCodeAnchor {
          file_path: hit.file_path.clone(),
          line_number: hit.line_number,
          line_content: hit.line_content.clone(),
          role: hit.role.clone(),
          parents: hit.parents.clone(),
          source_context: merge_context(hit),
        })
        .collect();
    }
  }
}

fn enrich_hits(
  hits: &mut [GrepHit],
  key: &str,
  around: Option<usize>,
  parents: bool,
  role: bool,
  opts: &WalkOpts,
) {
  if parents {
    attach_grep_structural_expansions(hits, opts, true, false);
  }
  if role {
    attach_grep_roles_for_literal(hits, key, opts);
  }
  if let Some(around) = around {
    attach_source_context(hits, around);
  }
}

fn attach_source_context(hits: &mut [GrepHit], around: usize) {
  let mut file_cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
  for hit in hits {
    let lines = file_cache.entry(hit.file_path.clone()).or_insert_with(|| {
      std::fs::read_to_string(&hit.file_path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
    });
    let target_idx = hit.line_number.saturating_sub(1);
    let start = target_idx.saturating_sub(around);
    let end = (target_idx + around + 1).min(lines.len());
    hit.before_context = (start..target_idx)
      .filter_map(|idx| {
        lines.get(idx).map(|line_content| LineContext {
          line_number: idx + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
    hit.after_context = ((target_idx + 1)..end)
      .filter_map(|idx| {
        lines.get(idx).map(|line_content| LineContext {
          line_number: idx + 1,
          line_content: line_content.clone(),
        })
      })
      .collect();
  }
}

fn merge_context(hit: &GrepHit) -> Vec<LineContext> {
  hit
    .before_context
    .iter()
    .cloned()
    .chain(std::iter::once(LineContext {
      line_number: hit.line_number,
      line_content: hit.line_content.clone(),
    }))
    .chain(hit.after_context.iter().cloned())
    .collect()
}

fn code_files(roots: &[PathBuf], opts: &WalkOpts) -> Vec<PathBuf> {
  let mut files = BTreeSet::new();
  for root in roots {
    if root.is_file() {
      if is_code_path(root, opts) {
        files.insert(root.clone());
      }
      continue;
    }
    if !root.exists() {
      continue;
    }
    let mut wb = WalkBuilder::new(root);
    apply_walk_opts(&mut wb, opts, root);
    for entry in wb.build() {
      let Ok(entry) = entry else {
        continue;
      };
      match walk_pre_check(&entry, opts) {
        PreCheck::Process(path, lang) if is_code_lang(lang) && !is_config_path(&path) => {
          files.insert(path);
        }
        PreCheck::Process(_, _) | PreCheck::Skip | PreCheck::SkipWithDiag(_) => {}
      }
    }
  }
  files.into_iter().collect()
}

fn is_code_path(path: &Path, opts: &WalkOpts) -> bool {
  !is_config_path(path)
    && opts
      .lang_filter
      .or_else(|| SgLang::from_path(path))
      .is_some_and(is_code_lang)
}

fn is_config_path(path: &Path) -> bool {
  config_lang(path).is_some()
}

fn config_lang(path: &Path) -> Option<String> {
  match path.extension().and_then(|ext| ext.to_str()) {
    Some("json") => Some("json".to_string()),
    Some("yaml") | Some("yml") => Some("yaml".to_string()),
    Some("toml") => Some("toml".to_string()),
    _ => None,
  }
}

fn best_effort_line(content: &str, lang: &str, key_path: &str, key: &str) -> usize {
  match lang {
    "json" => {
      best_effort_json_line(content, key_path).unwrap_or_else(|| fallback_key_line(content, key))
    }
    "yaml" => {
      best_effort_yaml_line(content, key_path).unwrap_or_else(|| fallback_key_line(content, key))
    }
    "toml" => best_effort_toml_line(content, key_path, key)
      .unwrap_or_else(|| fallback_key_line(content, key)),
    _ => fallback_key_line(content, key),
  }
}

fn fallback_key_line(content: &str, key: &str) -> usize {
  let quoted_double = format!("\"{key}\"");
  let quoted_single = format!("'{key}'");
  let bare_assign = format!("{key} =");
  let bare_colon = format!("{key}:");
  for (idx, line) in content.lines().enumerate() {
    let trimmed = line.trim_start();
    if trimmed.contains(&quoted_double)
      || trimmed.contains(&quoted_single)
      || trimmed.starts_with(&bare_assign)
      || trimmed.starts_with(&bare_colon)
    {
      return idx + 1;
    }
  }
  0
}

fn best_effort_json_line(content: &str, key_path: &str) -> Option<usize> {
  let target = split_key_path(key_path);
  let mut stack: Vec<String> = Vec::new();
  for (idx, line) in content.lines().enumerate() {
    let trimmed = line.trim_start();
    while trimmed.starts_with('}') && !stack.is_empty() {
      stack.pop();
    }
    let Some((key, after_colon)) = json_line_key(trimmed) else {
      continue;
    };
    let mut candidate = stack.clone();
    candidate.push(key.clone());
    if candidate == target {
      return Some(idx + 1);
    }
    if after_colon.trim_start().starts_with('{') {
      stack.push(key);
    }
  }
  None
}

fn json_line_key(trimmed: &str) -> Option<(String, &str)> {
  let rest = trimmed.strip_prefix('"')?;
  let end = rest.find('"')?;
  let key = rest[..end].to_string();
  let after_key = rest[end + 1..].trim_start();
  let after_colon = after_key.strip_prefix(':')?;
  Some((key, after_colon))
}

fn best_effort_yaml_line(content: &str, key_path: &str) -> Option<usize> {
  let target = split_key_path(key_path);
  let mut stack: Vec<(usize, String)> = Vec::new();
  for (idx, line) in content.lines().enumerate() {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
      continue;
    }
    let Some((key, _after_colon)) = yaml_line_key(trimmed) else {
      continue;
    };
    let indent = line.len().saturating_sub(trimmed.len());
    while stack
      .last()
      .is_some_and(|(prev_indent, _)| *prev_indent >= indent)
    {
      stack.pop();
    }
    let mut candidate = stack.iter().map(|(_, key)| key.clone()).collect::<Vec<_>>();
    candidate.push(key.clone());
    if candidate == target {
      return Some(idx + 1);
    }
    stack.push((indent, key));
  }
  None
}

fn yaml_line_key(trimmed: &str) -> Option<(String, &str)> {
  let colon = trimmed.find(':')?;
  let key = trimmed[..colon]
    .trim()
    .trim_matches('"')
    .trim_matches('\'')
    .to_string();
  if key.is_empty() {
    return None;
  }
  Some((key, &trimmed[colon + 1..]))
}

fn best_effort_toml_line(content: &str, key_path: &str, key: &str) -> Option<usize> {
  let key_parts = split_key_path(key_path);
  let parent_path = if key_parts.len() > 1 {
    &key_parts[..key_parts.len() - 1]
  } else {
    &[][..]
  };
  let mut current_table: Vec<String> = Vec::new();
  for (idx, line) in content.lines().enumerate() {
    let trimmed = line.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
      let table = trimmed
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_start_matches('[')
        .trim_end_matches(']');
      current_table = table
        .split('.')
        .filter(|part| !part.is_empty())
        .map(|part| part.trim().trim_matches('"').to_string())
        .collect();
      if current_table == key_parts {
        return Some(idx + 1);
      }
      continue;
    }
    if current_table == parent_path
      && toml_line_key(trimmed).is_some_and(|line_key| line_key == key)
    {
      return Some(idx + 1);
    }
  }
  None
}

fn toml_line_key(trimmed: &str) -> Option<String> {
  let equals = trimmed.find('=')?;
  let key = trimmed[..equals].trim().trim_matches('"').to_string();
  if key.is_empty() {
    None
  } else {
    Some(key)
  }
}

fn split_key_path(key_path: &str) -> Vec<String> {
  key_path
    .split('.')
    .filter(|part| !part.starts_with('[') && !part.is_empty())
    .map(str::to_string)
    .collect()
}

fn join_key_path(parts: &[String]) -> String {
  let mut out = String::new();
  for part in parts {
    if part.starts_with('[') {
      out.push_str(part);
    } else {
      if !out.is_empty() {
        out.push('.');
      }
      out.push_str(part);
    }
  }
  out
}

fn yaml_key(value: &serde_yaml::Value) -> Option<String> {
  match value {
    serde_yaml::Value::String(s) => Some(s.clone()),
    serde_yaml::Value::Number(n) => Some(n.to_string()),
    serde_yaml::Value::Bool(b) => Some(b.to_string()),
    _ => None,
  }
}

fn json_kind(value: &serde_json::Value) -> &'static str {
  match value {
    serde_json::Value::Object(_) => "object",
    serde_json::Value::Array(_) => "array",
    serde_json::Value::String(_) => "string",
    serde_json::Value::Number(_) => "number",
    serde_json::Value::Bool(_) => "bool",
    serde_json::Value::Null => "null",
  }
}

fn yaml_kind(value: &serde_yaml::Value) -> &'static str {
  match value {
    serde_yaml::Value::Mapping(_) => "object",
    serde_yaml::Value::Sequence(_) => "array",
    serde_yaml::Value::String(_) => "string",
    serde_yaml::Value::Number(_) => "number",
    serde_yaml::Value::Bool(_) => "bool",
    serde_yaml::Value::Null => "null",
    _ => "unknown",
  }
}

fn toml_kind(value: &toml::Value) -> &'static str {
  match value {
    toml::Value::Table(_) => "object",
    toml::Value::Array(_) => "array",
    toml::Value::String(_) => "string",
    toml::Value::Integer(_) | toml::Value::Float(_) => "number",
    toml::Value::Boolean(_) => "bool",
    toml::Value::Datetime(_) => "datetime",
  }
}

fn scalar_summary_json(value: &serde_json::Value) -> Option<String> {
  match value {
    serde_json::Value::String(s) => Some(truncate_summary(s)),
    serde_json::Value::Number(n) => Some(n.to_string()),
    serde_json::Value::Bool(b) => Some(b.to_string()),
    serde_json::Value::Null => Some("null".to_string()),
    _ => None,
  }
}

fn scalar_summary_yaml(value: &serde_yaml::Value) -> Option<String> {
  match value {
    serde_yaml::Value::String(s) => Some(truncate_summary(s)),
    serde_yaml::Value::Number(n) => Some(n.to_string()),
    serde_yaml::Value::Bool(b) => Some(b.to_string()),
    serde_yaml::Value::Null => Some("null".to_string()),
    _ => None,
  }
}

fn scalar_summary_toml(value: &toml::Value) -> Option<String> {
  match value {
    toml::Value::String(s) => Some(truncate_summary(s)),
    toml::Value::Integer(n) => Some(n.to_string()),
    toml::Value::Float(n) => Some(n.to_string()),
    toml::Value::Boolean(b) => Some(b.to_string()),
    toml::Value::Datetime(d) => Some(d.to_string()),
    _ => None,
  }
}

fn truncate_summary(value: &str) -> String {
  const MAX: usize = 80;
  if value.chars().count() <= MAX {
    value.to_string()
  } else {
    let mut out = value.chars().take(MAX).collect::<String>();
    out.push_str("...");
    out
  }
}
