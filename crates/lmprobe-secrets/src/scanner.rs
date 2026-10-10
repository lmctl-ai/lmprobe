use crate::regex_compat::{compile_gitleaks_pattern, CompiledPattern, PatternKind};
use crate::rules::{load_gitleaks_config_str, Allowlist, GitleaksConfig, Rule};
#[cfg(test)]
use crate::{SemanticScanResult, SINK_CATALOG_VERSION};
use aho_corasick::AhoCorasick;
use base64::Engine;
use lmprobe_types::schema::{ScanRegion, Secret};
pub use lmprobe_types::schema::{
  SecretDiagnostic, SecretFinding, SecretsExitStatus, SecretsResult, SuppressedSecretFinding,
};
use regex::Regex;
use std::collections::HashSet;

const DETECTOR_GITLEAKS_RULE: &str = "gitleaks-rule";
pub const CONFIDENCE_RULE_COMPATIBLE: &str = "rule-compatible";
pub const CONFIDENCE_RULE_COMPATIBLE_LOW: &str = "rule-compatible-low";

pub const DEFAULT_GITLEAKS_CONFIG: &str = include_str!("../tests/fixtures/gitleaks-default.toml");
const MAX_DECODE_DEPTH: usize = 7;

#[derive(Debug, thiserror::Error)]
pub enum Layer1Error {
  #[error(transparent)]
  RuleLoad(#[from] crate::rules::RuleLoadError),
  #[error("{0}")]
  Pattern(#[from] crate::regex_compat::PatternCompileError),
  #[error("failed to build keyword prefilter: {0}")]
  KeywordPrefilter(String),
}

#[cfg(test)]
fn new_secrets_result(findings: Vec<SecretFinding>) -> SecretsResult {
  SecretsResult::with_sink_catalog_version(findings, SINK_CATALOG_VERSION)
}

#[cfg(test)]
fn extend_semantic_result(result: &mut SecretsResult, semantic: SemanticScanResult) {
  result
    .semantic_findings
    .extend(semantic.symbol_reachability);
  result.source_evidence.extend(semantic.source_evidence);
  result.confirmed_flows.extend(semantic.confirmed_flows);
  result.sink_evidence.extend(semantic.sink_evidence);
  result.sink_catalog_version = semantic.sink_catalog_version;
}

#[derive(Debug, Clone, Copy)]
pub struct FileScanInput<'a> {
  pub file_path: &'a str,
  pub content: &'a str,
  pub source_mode: &'a str,
}

#[derive(Debug)]
pub struct Layer1Engine {
  rules: Vec<CompiledRule>,
  global_allowlists: Vec<CompiledAllowlist>,
  prefilter: Option<AhoCorasick>,
}

#[derive(Debug)]
struct CompiledRule {
  id: String,
  description: String,
  path: Option<CompiledPattern>,
  regex: Option<CompiledPattern>,
  secret_group: i32,
  entropy: f64,
  tags: Vec<String>,
  keywords: Vec<String>,
  allowlists: Vec<CompiledAllowlist>,
  skip_report: bool,
}

#[derive(Debug)]
struct CompiledAllowlist {
  condition: AllowlistCondition,
  commits: Vec<String>,
  paths: Vec<Regex>,
  regexes: Vec<Regex>,
  regex_target: AllowlistRegexTarget,
  stopwords: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AllowlistCondition {
  Or,
  And,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AllowlistRegexTarget {
  Secret,
  Match,
  Line,
}

impl Layer1Engine {
  pub fn from_default_config() -> Result<Self, Layer1Error> {
    Self::from_config_str(DEFAULT_GITLEAKS_CONFIG)
  }

  pub fn from_config_str(input: &str) -> Result<Self, Layer1Error> {
    let config = load_gitleaks_config_str(input)?;
    Self::from_config(config)
  }

  pub fn from_config(config: GitleaksConfig) -> Result<Self, Layer1Error> {
    let mut all_keywords = Vec::new();
    let mut seen_keywords = HashSet::new();
    let global_allowlists = compile_allowlists(config.all_allowlists())?;
    let mut rules = Vec::with_capacity(config.rules.len());

    for rule in config.rules {
      for keyword in &rule.keywords {
        if seen_keywords.insert(keyword.clone()) {
          all_keywords.push(keyword.clone());
        }
      }
      rules.push(compile_rule(rule)?);
    }

    let prefilter = if all_keywords.is_empty() {
      None
    } else {
      Some(
        AhoCorasick::new(all_keywords)
          .map_err(|err| Layer1Error::KeywordPrefilter(err.to_string()))?,
      )
    };

    Ok(Self {
      rules,
      global_allowlists,
      prefilter,
    })
  }

  pub fn scan_file(&self, input: FileScanInput<'_>) -> Vec<SecretFinding> {
    let mut findings = self.scan_file_pass(input, None);
    if !findings.is_empty() {
      dedup_findings(&mut findings);
      return filter_generic_findings(findings);
    }
    let mut current = input.content.to_string();
    for depth in 1..=MAX_DECODE_DEPTH {
      let Some(decoded) = decode_once(&current) else {
        break;
      };
      if decoded == current {
        break;
      }
      let mut decoded_findings = self.scan_file_pass(
        FileScanInput {
          file_path: input.file_path,
          content: &decoded,
          source_mode: input.source_mode,
        },
        Some(depth),
      );
      findings.append(&mut decoded_findings);
      current = decoded;
    }
    dedup_findings(&mut findings);
    filter_generic_findings(findings)
  }

  fn scan_file_pass(
    &self,
    input: FileScanInput<'_>,
    decode_depth: Option<usize>,
  ) -> Vec<SecretFinding> {
    if file_path_allowed(input.file_path, &self.global_allowlists) {
      return Vec::new();
    }

    let lower = input.content.to_lowercase();
    let keywords = self.matched_keywords(&lower);
    let line_starts = line_starts(input.content);
    let mut findings = Vec::new();

    for rule in &self.rules {
      if rule.skip_report {
        continue;
      }
      if !rule_should_run(rule, &keywords) {
        continue;
      }
      if file_path_allowed(input.file_path, &rule.allowlists) {
        continue;
      }
      findings.extend(self.scan_rule(rule, input, &line_starts, decode_depth));
    }

    findings
  }

  fn matched_keywords(&self, lower_content: &str) -> HashSet<String> {
    let mut keywords = HashSet::new();
    if let Some(prefilter) = &self.prefilter {
      for mat in prefilter.find_iter(lower_content) {
        keywords.insert(lower_content[mat.start()..mat.end()].to_string());
      }
    }
    keywords
  }

  fn scan_rule(
    &self,
    rule: &CompiledRule,
    input: FileScanInput<'_>,
    line_starts: &[usize],
    decode_depth: Option<usize>,
  ) -> Vec<SecretFinding> {
    if let Some(path) = &rule.path {
      let path_matches = path.regex.is_match(input.file_path);
      if rule.regex.is_none() {
        if path_matches {
          return vec![path_only_finding(rule, input)];
        }
        return Vec::new();
      }
      if !path_matches {
        return Vec::new();
      }
    }

    let Some(regex) = rule.regex.as_ref().map(|pattern| &pattern.regex) else {
      return Vec::new();
    };

    let mut findings = Vec::new();
    for mat in regex.find_iter(input.content) {
      let mut match_range = mat.range();
      let full_match = input.content[match_range.clone()].trim_end_matches('\n');
      match_range.end = match_range.start + full_match.len();
      let Some(secret) = extract_secret(regex, full_match, rule.secret_group) else {
        continue;
      };
      let entropy = shannon_entropy(&secret);
      if rule.entropy != 0.0 && entropy <= rule.entropy {
        continue;
      }

      let location = byte_range_location(
        input.content,
        line_starts,
        match_range.start,
        match_range.end,
      );
      let line = line_text(input.content, line_starts, location.start_line);
      if line.contains("gitleaks:allow") {
        continue;
      }
      if finding_allowed(
        input.file_path,
        full_match,
        &secret,
        line,
        &self.global_allowlists,
      ) || finding_allowed(input.file_path, full_match, &secret, line, &rule.allowlists)
      {
        continue;
      }

      let mut tags = rule.tags.clone();
      if let Some(depth) = decode_depth {
        tags.push("decoded:recursive".to_string());
        tags.push(format!("decode-depth:{depth}"));
      }
      findings.push(SecretFinding {
        detector: DETECTOR_GITLEAKS_RULE.into(),
        confidence: CONFIDENCE_RULE_COMPATIBLE.into(),
        ast_context: None,
        rule_id: rule.id.clone(),
        description: rule.description.clone(),
        file_path: input.file_path.to_string(),
        line_number: location.start_line,
        start_line: location.start_line,
        end_line: location.end_line,
        start_column: location.start_column,
        end_column: location.end_column,
        redacted_secret: Secret::new(secret.clone()),
        match_text: Secret::new(full_match.to_string()),
        entropy,
        tags,
        fingerprint: fingerprint(input.file_path, &rule.id, location.start_line),
        source_mode: input.source_mode.to_string(),
        evidence: vec![ScanRegion {
          file_path: input.file_path.to_string(),
          start_line: location.start_line,
          end_line: location.end_line,
          start_column: location.start_column,
          end_column: location.end_column,
        }],
      });
    }
    findings
  }
}

fn compile_rule(rule: Rule) -> Result<CompiledRule, Layer1Error> {
  let path = rule
    .path
    .as_ref()
    .filter(|pattern| !pattern.is_empty())
    .map(|pattern| compile_gitleaks_pattern(PatternKind::RulePath, &rule.id, pattern))
    .transpose()?;
  let regex = rule
    .regex
    .as_ref()
    .filter(|pattern| !pattern.is_empty())
    .map(|pattern| compile_gitleaks_pattern(PatternKind::RuleRegex, &rule.id, pattern))
    .transpose()?;
  let allowlists = compile_allowlists(rule.all_allowlists())?;

  Ok(CompiledRule {
    id: rule.id,
    description: rule.description,
    path,
    regex,
    secret_group: rule.secret_group,
    entropy: rule.entropy,
    tags: rule.tags,
    keywords: rule.keywords,
    allowlists,
    skip_report: rule.skip_report,
  })
}

fn decode_once(input: &str) -> Option<String> {
  let mut decoded = input.to_string();
  decoded = decode_percent(&decoded);
  decoded = decode_unicode_escapes(&decoded);
  decoded = decode_hex_words(&decoded);
  decoded = decode_base64_words(&decoded);
  (decoded != input).then_some(decoded)
}

fn decode_percent(input: &str) -> String {
  let bytes = input.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut idx = 0;
  while idx < bytes.len() {
    if bytes[idx] == b'%' && idx + 2 < bytes.len() {
      if let (Some(a), Some(b)) = (hex_val(bytes[idx + 1]), hex_val(bytes[idx + 2])) {
        out.push((a << 4) | b);
        idx += 3;
        continue;
      }
    }
    out.push(bytes[idx]);
    idx += 1;
  }
  String::from_utf8_lossy(&out).into_owned()
}

fn decode_unicode_escapes(input: &str) -> String {
  let mut out = String::with_capacity(input.len());
  let chars: Vec<_> = input.chars().collect();
  let mut idx = 0;
  while idx < chars.len() {
    if chars[idx] == '\\' && chars.get(idx + 1) == Some(&'u') && idx + 5 < chars.len() {
      let hex: String = chars[idx + 2..idx + 6].iter().collect();
      if let Ok(value) = u32::from_str_radix(&hex, 16) {
        if let Some(ch) = char::from_u32(value) {
          out.push(ch);
          idx += 6;
          continue;
        }
      }
    }
    out.push(chars[idx]);
    idx += 1;
  }
  out
}

fn decode_hex_words(input: &str) -> String {
  input
    .split_whitespace()
    .map(|word| {
      if word.len() >= 16 && word.len() % 2 == 0 && word.bytes().all(|b| hex_val(b).is_some()) {
        let bytes: Vec<_> = word
          .as_bytes()
          .chunks(2)
          .filter_map(|pair| Some((hex_val(pair[0])? << 4) | hex_val(pair[1])?))
          .collect();
        if is_printable_ascii(&bytes) {
          return String::from_utf8_lossy(&bytes).into_owned();
        }
      }
      word.to_string()
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn decode_base64_words(input: &str) -> String {
  input
    .split_whitespace()
    .map(|word| {
      let trimmed = word.trim_matches(['"', '\'', ',', ';']);
      if trimmed.len() >= 16 {
        for engine in [
          &base64::engine::general_purpose::STANDARD,
          &base64::engine::general_purpose::STANDARD_NO_PAD,
          &base64::engine::general_purpose::URL_SAFE,
          &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        ] {
          if let Ok(bytes) = engine.decode(trimmed) {
            if is_printable_ascii(&bytes) {
              return word.replace(trimmed, &String::from_utf8_lossy(&bytes));
            }
          }
        }
      }
      word.to_string()
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn hex_val(byte: u8) -> Option<u8> {
  match byte {
    b'0'..=b'9' => Some(byte - b'0'),
    b'a'..=b'f' => Some(byte - b'a' + 10),
    b'A'..=b'F' => Some(byte - b'A' + 10),
    _ => None,
  }
}

fn is_printable_ascii(bytes: &[u8]) -> bool {
  bytes
    .iter()
    .all(|byte| matches!(byte, b'\n' | b'\r' | b'\t' | 0x20..=0x7e))
}

fn compile_allowlists(allowlists: Vec<&Allowlist>) -> Result<Vec<CompiledAllowlist>, Layer1Error> {
  allowlists.into_iter().map(compile_allowlist).collect()
}

fn compile_allowlist(allowlist: &Allowlist) -> Result<CompiledAllowlist, Layer1Error> {
  let paths = allowlist
    .paths
    .iter()
    .enumerate()
    .map(|(idx, pattern)| {
      compile_gitleaks_pattern(
        PatternKind::RulePath,
        format!("allowlist path[{idx}]"),
        pattern,
      )
      .map(|compiled| compiled.regex)
    })
    .collect::<Result<Vec<_>, _>>()?;
  let regexes = allowlist
    .regexes
    .iter()
    .enumerate()
    .map(|(idx, pattern)| {
      compile_gitleaks_pattern(
        PatternKind::AllowlistRegex,
        format!("allowlist regex[{idx}]"),
        pattern,
      )
      .map(|compiled| compiled.regex)
    })
    .collect::<Result<Vec<_>, _>>()?;

  Ok(CompiledAllowlist {
    condition: match allowlist.condition.to_uppercase().as_str() {
      "AND" | "&&" => AllowlistCondition::And,
      _ => AllowlistCondition::Or,
    },
    commits: allowlist
      .commits
      .iter()
      .map(|commit| commit.trim().to_lowercase())
      .collect(),
    paths,
    regexes,
    regex_target: match allowlist.regex_target.as_str() {
      "match" => AllowlistRegexTarget::Match,
      "line" => AllowlistRegexTarget::Line,
      _ => AllowlistRegexTarget::Secret,
    },
    stopwords: allowlist
      .stopwords
      .iter()
      .map(|stopword| stopword.to_lowercase())
      .collect(),
  })
}

fn rule_should_run(rule: &CompiledRule, matched_keywords: &HashSet<String>) -> bool {
  rule.keywords.is_empty()
    || rule
      .keywords
      .iter()
      .any(|keyword| matched_keywords.contains(keyword))
}

fn extract_secret(regex: &Regex, full_match: &str, secret_group: i32) -> Option<String> {
  let Some(captures) = regex.captures(full_match) else {
    return Some(full_match.to_string());
  };
  if captures.len() < 2 {
    return Some(full_match.to_string());
  }
  if secret_group > 0 {
    return captures
      .get(secret_group as usize)
      .map(|m| m.as_str().to_string());
  }
  Some(
    captures
      .iter()
      .skip(1)
      .flatten()
      .find(|m| !m.as_str().is_empty())
      .map(|m| m.as_str().to_string())
      .unwrap_or_else(|| full_match.to_string()),
  )
}

pub fn shannon_entropy(data: &str) -> f64 {
  if data.is_empty() {
    return 0.0;
  }
  let mut counts = std::collections::HashMap::new();
  for ch in data.chars() {
    *counts.entry(ch).or_insert(0usize) += 1;
  }
  let inv_len = 1.0 / data.len() as f64;
  counts.values().fold(0.0, |entropy, count| {
    let freq = *count as f64 * inv_len;
    entropy - freq * freq.log2()
  })
}

fn file_path_allowed(path: &str, allowlists: &[CompiledAllowlist]) -> bool {
  allowlists.iter().any(|allowlist| {
    let path_allowed = path_allowed(path, allowlist);
    match allowlist.condition {
      AllowlistCondition::Or => path_allowed,
      AllowlistCondition::And => {
        let mut checks = Vec::new();
        if !allowlist.commits.is_empty() {
          checks.push(false);
        }
        if !allowlist.paths.is_empty() {
          checks.push(path_allowed);
        }
        if !allowlist.regexes.is_empty() || !allowlist.stopwords.is_empty() {
          return false;
        }
        !checks.is_empty() && checks.into_iter().all(|check| check)
      }
    }
  })
}

fn finding_allowed(
  path: &str,
  full_match: &str,
  secret: &str,
  line: &str,
  allowlists: &[CompiledAllowlist],
) -> bool {
  allowlists.iter().any(|allowlist| {
    let regex_target = match allowlist.regex_target {
      AllowlistRegexTarget::Secret => secret,
      AllowlistRegexTarget::Match => full_match,
      AllowlistRegexTarget::Line => line,
    };
    let regex_allowed = !allowlist.regexes.is_empty()
      && allowlist
        .regexes
        .iter()
        .any(|regex| regex.is_match(regex_target));
    let stopword_allowed = contains_stopword(secret, &allowlist.stopwords);
    let path_allowed = path_allowed(path, allowlist);

    match allowlist.condition {
      AllowlistCondition::Or => regex_allowed || stopword_allowed,
      AllowlistCondition::And => {
        let mut checks = Vec::new();
        if !allowlist.commits.is_empty() {
          checks.push(false);
        }
        if !allowlist.paths.is_empty() {
          checks.push(path_allowed);
        }
        if !allowlist.regexes.is_empty() {
          checks.push(regex_allowed);
        }
        if !allowlist.stopwords.is_empty() {
          checks.push(stopword_allowed);
        }
        !checks.is_empty() && checks.into_iter().all(|check| check)
      }
    }
  })
}

fn path_allowed(path: &str, allowlist: &CompiledAllowlist) -> bool {
  !allowlist.paths.is_empty() && allowlist.paths.iter().any(|regex| regex.is_match(path))
}

fn contains_stopword(secret: &str, stopwords: &[String]) -> bool {
  let lower = secret.to_lowercase();
  stopwords
    .iter()
    .any(|stopword| lower.contains(stopword.as_str()))
}

#[derive(Debug, Clone, Copy)]
struct Location {
  start_line: usize,
  end_line: usize,
  start_column: usize,
  end_column: usize,
}

fn line_starts(content: &str) -> Vec<usize> {
  let mut starts = vec![0];
  for (idx, byte) in content.bytes().enumerate() {
    if byte == b'\n' {
      starts.push(idx + 1);
    }
  }
  starts
}

fn byte_range_location(content: &str, starts: &[usize], start: usize, end: usize) -> Location {
  let start_line_idx = starts.partition_point(|line_start| *line_start <= start) - 1;
  let end_line_idx = starts
    .partition_point(|line_start| *line_start < end)
    .saturating_sub(1);
  let start_line_start = starts[start_line_idx];
  let end_line_start = starts[end_line_idx];
  Location {
    start_line: start_line_idx + 1,
    end_line: end_line_idx + 1,
    start_column: content[start_line_start..start].chars().count() + 1,
    end_column: content[end_line_start..end].chars().count(),
  }
}

fn line_text<'a>(content: &'a str, starts: &[usize], one_based_line: usize) -> &'a str {
  let idx = one_based_line.saturating_sub(1);
  let Some(start) = starts.get(idx).copied() else {
    return "";
  };
  let end = starts
    .get(idx + 1)
    .copied()
    .map(|next| next.saturating_sub(1))
    .unwrap_or(content.len());
  &content[start..end]
}

fn fingerprint(path: &str, rule_id: &str, line: usize) -> String {
  format!("{path}:{rule_id}:{line}")
}

fn path_only_finding(rule: &CompiledRule, input: FileScanInput<'_>) -> SecretFinding {
  let match_text = format!("file detected: {}", input.file_path);
  SecretFinding {
    detector: DETECTOR_GITLEAKS_RULE.into(),
    confidence: CONFIDENCE_RULE_COMPATIBLE.into(),
    ast_context: None,
    rule_id: rule.id.clone(),
    description: rule.description.clone(),
    file_path: input.file_path.to_string(),
    line_number: 1,
    start_line: 1,
    end_line: 1,
    start_column: 1,
    end_column: 1,
    redacted_secret: Secret::new(match_text.clone()),
    match_text: Secret::new(match_text),
    entropy: 0.0,
    tags: rule.tags.clone(),
    fingerprint: fingerprint(input.file_path, &rule.id, 1),
    source_mode: input.source_mode.to_string(),
    evidence: vec![ScanRegion {
      file_path: input.file_path.to_string(),
      start_line: 1,
      end_line: 1,
      start_column: 1,
      end_column: 1,
    }],
  }
}

fn filter_generic_findings(findings: Vec<SecretFinding>) -> Vec<SecretFinding> {
  findings
    .iter()
    .filter(|finding| {
      if !finding.rule_id.to_lowercase().contains("generic") {
        return true;
      }
      !findings.iter().any(|other| {
        finding.start_line == other.start_line
          && finding.rule_id != other.rule_id
          && !other.rule_id.to_lowercase().contains("generic")
          && other
            .redacted_secret
            .expose_raw()
            .contains(finding.redacted_secret.expose_raw())
      })
    })
    .cloned()
    .collect()
}

fn dedup_findings(findings: &mut Vec<SecretFinding>) {
  let mut seen = HashSet::new();
  findings.retain(|finding| {
    seen.insert((
      finding.file_path.clone(),
      finding.start_line,
      finding.start_column,
      finding.rule_id.clone(),
      finding.redacted_secret.expose_raw().to_string(),
    ))
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn keyword_prefilter_entropy_and_secret_group_detect_generic_api_key() {
    let config = r#"
title = "test"
[[rules]]
id = "generic-api-key"
description = "generic"
regex = '''(?i)(api_key)\s*=\s*["']?([A-Za-z0-9_]{10,})["']?'''
secretGroup = 2
entropy = 3.5
keywords = ["api_key"]
"#;
    let engine = Layer1Engine::from_config_str(config).expect("engine");
    let low = engine.scan_file(FileScanInput {
      file_path: "app.py",
      content: "api_key = \"abc123\"\n",
      source_mode: "filesystem",
    });
    assert!(low.is_empty());

    let high = engine.scan_file(FileScanInput {
      file_path: "app.py",
      content: "api_key = \"S3cr3tV4lue_kJ8shdfLKJ23lkjsdf09sdfkjh23\"\n",
      source_mode: "filesystem",
    });
    assert_eq!(high.len(), 1);
    assert_eq!(high[0].rule_id, "generic-api-key");
    assert_eq!(
      high[0].redacted_secret.expose_raw(),
      "S3cr3tV4lue_kJ8shdfLKJ23lkjsdf09sdfkjh23"
    );
    assert!(high[0].entropy > 3.5);
  }

  #[test]
  fn allowlists_support_path_regex_line_match_secret_and_stopwords() {
    let config = r#"
title = "test"
[[allowlists]]
paths = ['''ignored\.txt''']

[[rules]]
id = "demo"
description = "demo"
regex = '''secret\s*=\s*([A-Za-z0-9_]+)'''
secretGroup = 1

[[rules.allowlists]]
regexTarget = "line"
regexes = ['''test fixture''']

[[rules.allowlists]]
stopwords = ["example"]
"#;
    let engine = Layer1Engine::from_config_str(config).expect("engine");
    assert!(engine
      .scan_file(FileScanInput {
        file_path: "ignored.txt",
        content: "secret = abcdef12345\n",
        source_mode: "filesystem",
      })
      .is_empty());
    assert!(engine
      .scan_file(FileScanInput {
        file_path: "app.txt",
        content: "secret = abcdef12345 # test fixture\n",
        source_mode: "filesystem",
      })
      .is_empty());
    assert!(engine
      .scan_file(FileScanInput {
        file_path: "app.txt",
        content: "secret = example12345\n",
        source_mode: "filesystem",
      })
      .is_empty());
    assert_eq!(
      engine
        .scan_file(FileScanInput {
          file_path: "app.txt",
          content: "secret = abcdef12345\n",
          source_mode: "filesystem",
        })
        .len(),
      1
    );
  }

  #[test]
  fn serialized_layer1_finding_redacts_match_and_secret() {
    let engine = Layer1Engine::from_config_str(
      r#"
title = "test"
[[rules]]
id = "demo"
description = "demo"
regex = '''token=([A-Za-z0-9]+)'''
secretGroup = 1
"#,
    )
    .expect("engine");
    let finding = engine
      .scan_file(FileScanInput {
        file_path: "app.txt",
        content: "token=rawsecret123\n",
        source_mode: "filesystem",
      })
      .pop()
      .expect("finding");

    let json = serde_json::to_string(&finding).expect("serialize finding");
    assert!(!json.contains("rawsecret123"));
    assert!(json.contains("REDACTED"));
  }

  #[test]
  fn out_of_range_secret_group_skips_finding() {
    let engine = Layer1Engine::from_config_str(
      r#"
title = "test"
[[rules]]
id = "demo"
description = "demo"
regex = '''token=([A-Za-z0-9]+)'''
secretGroup = 2
"#,
    )
    .expect("engine");

    let findings = engine.scan_file(FileScanInput {
      file_path: "app.txt",
      content: "token=rawsecret123\n",
      source_mode: "filesystem",
    });

    assert!(findings.is_empty());
  }

  #[test]
  fn canonical_findings_merge_layer1_and_symbol_reachability() {
    let engine = Layer1Engine::from_config_str(
      r#"
title = "test"
[[rules]]
id = "aws-access-token"
description = "aws"
regex = '''AKIA[0-9A-Z]{16}'''
"#,
    )
    .expect("engine");
    let content = "const key = \"AKIAZ3M7Q6YB2N4P5R6S\";\nclient.login(key);\n";
    let findings = engine.scan_file(FileScanInput {
      file_path: "case.js",
      content,
      source_mode: "test",
    });
    let semantic = crate::analyze_source("case.js", content, crate::SinkEvidenceMode::On);
    let mut result = new_secrets_result(findings);
    extend_semantic_result(&mut result, semantic);
    result.rebuild_canonical_findings();

    assert_eq!(result.canonical_findings.len(), 1);
    assert_eq!(result.canonical_findings[0].detector, "gitleaks-rule");
    assert_eq!(result.canonical_findings[0].line_number, 2);
    assert!(result.canonical_findings[0]
      .evidence
      .iter()
      .any(|evidence| evidence.detector == "symbol-reachability"));
    assert!(result.canonical_findings[0]
      .evidence
      .iter()
      .any(|evidence| evidence.detector == "gitleaks-rule"));
    assert_eq!(result.summary.canonical, 1);
  }

  #[test]
  fn recursive_decoders_find_base64_encoded_secret_and_redact() {
    let engine = Layer1Engine::from_config_str(
      r#"
title = "test"
[[rules]]
id = "aws-access-token"
description = "aws"
regex = '''AKIA[0-9A-Z]{16}'''
"#,
    )
    .expect("engine");
    let findings = engine.scan_file(FileScanInput {
      file_path: "encoded.txt",
      content: "QUtJQVozTTdRNllCMk40UDVSNlM=\n",
      source_mode: "filesystem",
    });
    assert_eq!(findings.len(), 1);
    assert!(findings[0].tags.iter().any(|tag| tag == "decode-depth:1"));
    let json = serde_json::to_string(&findings).expect("serialize");
    assert!(!json.contains("AKIAZ3M7Q6YB2N4P5R6S"));
    assert!(json.contains("REDACTED"));
  }

  #[test]
  fn recursive_decoder_stops_at_depth_bound() {
    let mut encoded = "AKIAZ3M7Q6YB2N4P5R6S".to_string();
    for _ in 0..(MAX_DECODE_DEPTH + 2) {
      encoded = base64::engine::general_purpose::STANDARD.encode(encoded);
    }
    let decoded = decode_once(&encoded).expect("decode once");
    assert_ne!(decoded, "AKIAZ3M7Q6YB2N4P5R6S");
  }
}
