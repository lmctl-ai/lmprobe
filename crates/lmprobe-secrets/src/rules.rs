use crate::regex_compat::{
  compile_gitleaks_pattern, CompiledPattern, PatternCompileError, PatternKind, PatternStats,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, thiserror::Error)]
pub enum RuleLoadError {
  #[error("failed to parse gitleaks TOML: {0}")]
  Toml(#[from] toml::de::Error),
  #[error("rule id is missing or empty")]
  MissingRuleId,
  #[error("{rule_id}: both regex and path are empty")]
  EmptyRuleMatcher { rule_id: String },
  #[error("allowlist must contain at least one check for: commits, paths, regexes, or stopwords")]
  EmptyAllowlist,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Eq, PartialEq)]
pub struct Extend {
  #[serde(default)]
  pub path: String,
  #[serde(default)]
  pub url: String,
  #[serde(default, rename = "useDefault")]
  pub use_default: bool,
  #[serde(default, rename = "disabledRules")]
  pub disabled_rules: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Eq, PartialEq)]
pub struct RequiredRule {
  #[serde(default)]
  pub id: String,
  #[serde(default, rename = "withinLines")]
  pub within_lines: Option<i32>,
  #[serde(default, rename = "withinColumns")]
  pub within_columns: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Allowlist {
  #[serde(default)]
  pub description: String,
  #[serde(default)]
  pub condition: String,
  #[serde(default)]
  pub commits: Vec<String>,
  #[serde(default)]
  pub paths: Vec<String>,
  #[serde(default)]
  pub regexes: Vec<String>,
  #[serde(default, rename = "regexTarget")]
  pub regex_target: String,
  #[serde(default)]
  pub stopwords: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Rule {
  #[serde(default)]
  pub id: String,
  #[serde(default)]
  pub description: String,
  #[serde(default)]
  pub path: Option<String>,
  #[serde(default)]
  pub regex: Option<String>,
  #[serde(default, rename = "secretGroup")]
  pub secret_group: i32,
  #[serde(default)]
  pub entropy: f64,
  #[serde(default)]
  pub keywords: Vec<String>,
  #[serde(default)]
  pub tags: Vec<String>,
  #[serde(default, rename = "allowlist")]
  pub allowlist: Option<Allowlist>,
  #[serde(default)]
  pub allowlists: Vec<Allowlist>,
  #[serde(default)]
  pub required: Vec<RequiredRule>,
  #[serde(default, rename = "skipReport")]
  pub skip_report: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GitleaksConfig {
  #[serde(default)]
  pub title: String,
  #[serde(default)]
  pub description: String,
  #[serde(default, rename = "minVersion")]
  pub min_version: String,
  #[serde(default)]
  pub extend: Extend,
  #[serde(default, rename = "allowlist")]
  pub allowlist: Option<Allowlist>,
  #[serde(default)]
  pub allowlists: Vec<Allowlist>,
  #[serde(default)]
  pub rules: Vec<Rule>,
  #[serde(skip)]
  pub keywords: BTreeSet<String>,
}

impl GitleaksConfig {
  pub fn validate(&mut self) -> Result<(), RuleLoadError> {
    if let Some(allowlist) = &mut self.allowlist {
      allowlist.validate()?;
    }
    dedup_allowlists(&mut self.allowlists)?;
    for rule in &self.rules {
      if rule.id.trim().is_empty() {
        return Err(RuleLoadError::MissingRuleId);
      }
      if rule.regex.as_deref().unwrap_or("").is_empty()
        && rule.path.as_deref().unwrap_or("").is_empty()
      {
        return Err(RuleLoadError::EmptyRuleMatcher {
          rule_id: rule.id.clone(),
        });
      }
    }
    for rule in &mut self.rules {
      if let Some(allowlist) = &mut rule.allowlist {
        allowlist.validate()?;
      }
      dedup_allowlists(&mut rule.allowlists)?;
    }
    Ok(())
  }

  pub fn pattern_refs(&self) -> Vec<RulePatternRef<'_>> {
    let mut refs = Vec::new();
    for rule in &self.rules {
      if let Some(regex) = rule.regex.as_deref().filter(|s| !s.is_empty()) {
        refs.push(RulePatternRef {
          owner: rule.id.clone(),
          kind: PatternKind::RuleRegex,
          pattern: regex,
        });
      }
      if let Some(path) = rule.path.as_deref().filter(|s| !s.is_empty()) {
        refs.push(RulePatternRef {
          owner: rule.id.clone(),
          kind: PatternKind::RulePath,
          pattern: path,
        });
      }
      for (idx, allowlist) in rule.all_allowlists().iter().enumerate() {
        for regex in &allowlist.regexes {
          refs.push(RulePatternRef {
            owner: format!("{} allowlist[{idx}]", rule.id),
            kind: PatternKind::AllowlistRegex,
            pattern: regex,
          });
        }
      }
    }
    for (idx, allowlist) in self.all_allowlists().iter().enumerate() {
      for regex in &allowlist.regexes {
        refs.push(RulePatternRef {
          owner: format!("global allowlist[{idx}]"),
          kind: PatternKind::AllowlistRegex,
          pattern: regex,
        });
      }
    }
    refs
  }

  pub fn compile_patterns(&self) -> CompiledPatternSet {
    let mut patterns = Vec::new();
    let mut errors = Vec::new();

    for pattern_ref in self.pattern_refs() {
      match compile_gitleaks_pattern(
        pattern_ref.kind,
        pattern_ref.owner.clone(),
        pattern_ref.pattern.to_string(),
      ) {
        Ok(pattern) => patterns.push(pattern),
        Err(error) => errors.push(RulePatternError {
          owner: pattern_ref.owner,
          kind: pattern_ref.kind,
          pattern: pattern_ref.pattern.to_string(),
          error,
        }),
      }
    }

    let total = patterns.len() + errors.len();
    let fail = errors.len();
    CompiledPatternSet {
      patterns,
      errors,
      stats: PatternStats {
        total,
        ok: total - fail,
        fail,
      },
    }
  }

  pub fn all_allowlists(&self) -> Vec<&Allowlist> {
    let mut allowlists = Vec::new();
    if let Some(allowlist) = &self.allowlist {
      allowlists.push(allowlist);
    }
    allowlists.extend(self.allowlists.iter());
    allowlists
  }
}

impl Rule {
  pub fn all_allowlists(&self) -> Vec<&Allowlist> {
    let mut allowlists = Vec::new();
    if let Some(allowlist) = &self.allowlist {
      allowlists.push(allowlist);
    }
    allowlists.extend(self.allowlists.iter());
    allowlists
  }
}

impl Allowlist {
  pub fn validate(&mut self) -> Result<(), RuleLoadError> {
    self.commits = normalize_dedup_strings(&self.commits, true);
    self.paths = normalize_dedup_strings(&self.paths, false);
    self.regexes = normalize_dedup_strings(&self.regexes, false);
    self.stopwords = normalize_dedup_strings(&self.stopwords, true);
    if self.commits.is_empty()
      && self.paths.is_empty()
      && self.regexes.is_empty()
      && self.stopwords.is_empty()
    {
      return Err(RuleLoadError::EmptyAllowlist);
    }
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub struct RulePatternRef<'a> {
  pub owner: String,
  pub kind: PatternKind,
  pub pattern: &'a str,
}

#[derive(Debug)]
pub struct RulePatternError {
  pub owner: String,
  pub kind: PatternKind,
  pub pattern: String,
  pub error: PatternCompileError,
}

#[derive(Debug)]
pub struct CompiledPatternSet {
  pub patterns: Vec<CompiledPattern>,
  pub errors: Vec<RulePatternError>,
  pub stats: PatternStats,
}

pub fn load_gitleaks_config_str(input: &str) -> Result<GitleaksConfig, RuleLoadError> {
  let mut config: GitleaksConfig = toml::from_str(input)?;
  let mut keywords = BTreeSet::new();

  for rule in &mut config.rules {
    for keyword in &mut rule.keywords {
      *keyword = keyword.to_lowercase();
      keywords.insert(keyword.clone());
    }
  }
  config.keywords = keywords;
  config.validate()?;
  Ok(config)
}

fn dedup_allowlists(allowlists: &mut Vec<Allowlist>) -> Result<(), RuleLoadError> {
  for allowlist in allowlists.iter_mut() {
    allowlist.validate()?;
  }
  let mut seen = BTreeSet::new();
  allowlists.retain(|allowlist| seen.insert(allowlist_key(allowlist)));
  Ok(())
}

fn normalize_dedup_strings(values: &[String], lowercase: bool) -> Vec<String> {
  let mut seen = BTreeSet::new();
  let mut out = Vec::new();
  for value in values {
    let normalized = if lowercase {
      value.trim().to_lowercase()
    } else {
      value.trim().to_string()
    };
    if !normalized.is_empty() && seen.insert(normalized.clone()) {
      out.push(normalized);
    }
  }
  out
}

fn allowlist_key(allowlist: &Allowlist) -> String {
  format!(
    "{}\0{}\0{}\0{}\0{}\0{}",
    allowlist.condition,
    allowlist.commits.join("\0"),
    allowlist.paths.join("\0"),
    allowlist.regexes.join("\0"),
    allowlist.regex_target,
    allowlist.stopwords.join("\0")
  )
}

#[cfg(test)]
mod tests {
  use super::load_gitleaks_config_str;

  const DEFAULT_CONFIG: &str = include_str!("../tests/fixtures/gitleaks-default.toml");

  #[test]
  fn loads_default_config_rule_order_and_key_fields() {
    let config = load_gitleaks_config_str(DEFAULT_CONFIG).expect("load default gitleaks config");

    assert_eq!(config.title, "gitleaks config");
    assert_eq!(config.min_version, "v8.25.0");
    println!("rules={}", config.rules.len());
    assert_eq!(config.rules.len(), 222, "rules={}", config.rules.len());
    assert_eq!(config.rules.first().unwrap().id, "1password-secret-key");
    assert_eq!(config.rules.last().unwrap().id, "zendesk-secret-key");
    assert_eq!(config.allowlist.as_ref().unwrap().regexes.len(), 13);

    let aws = config
      .rules
      .iter()
      .find(|rule| rule.id == "aws-access-token")
      .expect("aws rule");
    assert_eq!(aws.entropy, 3.0);
    assert_eq!(aws.keywords, ["a3t", "akia", "asia", "abia", "acca"]);
    assert_eq!(aws.allowlists.len(), 1);

    let generic = config
      .rules
      .iter()
      .find(|rule| rule.id == "generic-api-key")
      .expect("generic rule");
    assert_eq!(generic.entropy, 3.5);
    assert_eq!(generic.allowlists.len(), 4);
    assert_eq!(generic.allowlists[1].regex_target, "match");
    assert_eq!(generic.allowlists[2].regex_target, "line");
  }

  #[test]
  fn compiles_default_config_patterns_with_brace_shim() {
    let config = load_gitleaks_config_str(DEFAULT_CONFIG).expect("load default gitleaks config");
    let compiled = config.compile_patterns();

    println!(
      "patterns total={} ok={} fail={}",
      compiled.stats.total, compiled.stats.ok, compiled.stats.fail
    );
    assert_eq!(
      (compiled.stats.total, compiled.stats.ok, compiled.stats.fail),
      (275, 275, 0),
      "patterns total={} ok={} fail={}",
      compiled.stats.total,
      compiled.stats.ok,
      compiled.stats.fail
    );
    assert!(compiled.errors.is_empty(), "{:#?}", compiled.errors);

    assert!(compiled
      .patterns
      .iter()
      .any(|pattern| pattern.original == r#"^\$(?:\d+|{\d+})$"#
        && pattern.normalized == r#"^\$(?:\d+|\{\d+\})$"#));
    assert!(compiled
      .patterns
      .iter()
      .any(|pattern| pattern.original == r#"^\${(?:[A-Z_]+|[a-z_]+)}$"#
        && pattern.normalized == r#"^\$\{(?:[A-Z_]+|[a-z_]+)\}$"#));
    assert!(compiled.patterns.iter().any(|pattern| pattern.original
      == r#"['"]?\$?{{[^}]+}}['"]?:['"]?\$?{{[^}]+}}['"]?"#
      && pattern.normalized == r#"['"]?\$?\{\{[^}]+\}\}['"]?:['"]?\$?\{\{[^}]+\}\}['"]?"#));
    assert!(compiled.patterns.iter().any(|pattern| pattern.original
      == r#"import[ \t]+{[ \t\w,]+}[ \t]+from[ \t]+['"][^'"]+['"]"#
      && pattern.normalized == r#"import[ \t]+\{[ \t\w,]+\}[ \t]+from[ \t]+['"][^'"]+['"]"#));
  }
}
