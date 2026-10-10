use regex::{Regex, RegexBuilder};
use std::fmt;

const REGEX_SIZE_LIMIT: usize = 256 * 1024 * 1024;
const REGEX_DFA_SIZE_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PatternKind {
  RuleRegex,
  RulePath,
  AllowlistRegex,
}

impl fmt::Display for PatternKind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::RuleRegex => f.write_str("rule regex"),
      Self::RulePath => f.write_str("rule path"),
      Self::AllowlistRegex => f.write_str("allowlist regex"),
    }
  }
}

#[derive(Debug)]
pub struct CompiledPattern {
  pub kind: PatternKind,
  pub owner: String,
  pub original: String,
  pub normalized: String,
  pub regex: Regex,
}

#[derive(Debug, Clone)]
pub struct PatternCompileError {
  pub kind: PatternKind,
  pub owner: String,
  pub original: String,
  pub normalized: String,
  pub message: String,
}

impl fmt::Display for PatternCompileError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(
      f,
      "{} for {} failed to compile: {}",
      self.kind, self.owner, self.message
    )
  }
}

impl std::error::Error for PatternCompileError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct PatternStats {
  pub total: usize,
  pub ok: usize,
  pub fail: usize,
}

pub fn compile_gitleaks_pattern(
  kind: PatternKind,
  owner: impl Into<String>,
  pattern: impl Into<String>,
) -> Result<CompiledPattern, PatternCompileError> {
  let owner = owner.into();
  let original = pattern.into();
  let normalized = normalize_go_re2_literal_braces(&original);
  let regex = RegexBuilder::new(&normalized)
    .size_limit(REGEX_SIZE_LIMIT)
    .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
    .build()
    .map_err(|err| PatternCompileError {
      kind,
      owner: owner.clone(),
      original: original.clone(),
      normalized: normalized.clone(),
      message: err.to_string(),
    })?;

  Ok(CompiledPattern {
    kind,
    owner,
    original,
    normalized,
    regex,
  })
}

/// Normalize Go RE2 patterns for Rust `regex`.
///
/// Go's regexp accepts literal braces in positions where they are not valid
/// repetition operators. Rust's regex parser rejects those same unescaped
/// braces. This pass escapes only unescaped braces that are outside character
/// classes and are not valid `{n}`, `{n,}`, or `{n,m}` quantifiers.
pub fn normalize_go_re2_literal_braces(pattern: &str) -> String {
  let chars: Vec<char> = pattern.chars().collect();
  let mut out = String::with_capacity(pattern.len());
  let mut i = 0;
  let mut in_class = false;

  while i < chars.len() {
    let c = chars[i];

    if c == '\\' {
      out.push(c);
      if let Some(next) = chars.get(i + 1) {
        out.push(*next);
        i += 2;
      } else {
        i += 1;
      }
      continue;
    }

    if c == '[' {
      in_class = true;
      out.push(c);
      i += 1;
      continue;
    }

    if c == ']' {
      in_class = false;
      out.push(c);
      i += 1;
      continue;
    }

    if !in_class && c == '{' {
      if let Some(end) = valid_quantifier_end(&chars, i) {
        for ch in &chars[i..=end] {
          out.push(*ch);
        }
        i = end + 1;
      } else {
        out.push('\\');
        out.push('{');
        i += 1;
      }
      continue;
    }

    if !in_class && c == '}' {
      out.push('\\');
      out.push('}');
      i += 1;
      continue;
    }

    out.push(c);
    i += 1;
  }

  out
}

fn valid_quantifier_end(chars: &[char], start: usize) -> Option<usize> {
  debug_assert_eq!(chars[start], '{');
  let mut i = start + 1;
  let first_digits = take_ascii_digits(chars, &mut i);
  if first_digits == 0 {
    return None;
  }

  if chars.get(i) == Some(&'}') {
    return Some(i);
  }

  if chars.get(i) != Some(&',') {
    return None;
  }
  i += 1;
  let _second_digits = take_ascii_digits(chars, &mut i);

  if chars.get(i) == Some(&'}') {
    Some(i)
  } else {
    None
  }
}

fn take_ascii_digits(chars: &[char], i: &mut usize) -> usize {
  let start = *i;
  while chars.get(*i).is_some_and(|c| c.is_ascii_digit()) {
    *i += 1;
  }
  *i - start
}

#[cfg(test)]
mod tests {
  use super::{compile_gitleaks_pattern, normalize_go_re2_literal_braces, PatternKind};

  #[test]
  fn normalizes_literal_braces_without_touching_quantifiers_or_escapes() {
    let input = r#"^\$(?:\d+|{\d+})$|a{2,4}|b{3,}|c{5}|\{\{ok}}"#;
    let normalized = normalize_go_re2_literal_braces(input);

    assert!(normalized.contains(r#"\{\d+\}"#));
    assert!(normalized.contains("a{2,4}"));
    assert!(normalized.contains("b{3,}"));
    assert!(normalized.contains("c{5}"));
    assert!(normalized.contains(r#"\{\{ok\}"#));
    compile_gitleaks_pattern(PatternKind::AllowlistRegex, "test", normalized)
      .expect("normalized pattern compiles");
  }
}
