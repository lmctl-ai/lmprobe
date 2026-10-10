//! §1.2 relational rule plumbing.
//!
//! Bridges user-supplied YAML (`--has '<yaml>'`) into the rule
//! engine in `ast_grep_config`. We accept both flow-style YAML
//! (`{kind: x, regex: y}`) and the inline shorthand
//! (`kind: x, regex: y`) by normalising to flow style if no `{`/
//! newline is present. Failures degrade to a `Diagnostic` with code
//! `RULE_PARSE_FAILED`.
//!
//! Phase 1A only wires `--has`; the other relational rules
//! (`--inside`, `--precedes`, `--follows`, `--all`, `--any`,
//! `--not`, `-r/--rule`) come in 1B/1C — the schema scaffolding
//! here is forward-compatible.

use ast_grep_config::{DeserializeEnv, Rule, SerializableRule};
use ast_grep_core::matcher::MatcherExt;
use ast_grep_core::NodeMatch;

use lmprobe_lang_core::langs::SgLang;

pub type SgNodeMatch<'a> = NodeMatch<'a, ast_grep_core::tree_sitter::StrDoc<SgLang>>;

/// Parse the user's `--has` YAML and materialise a Rule for the
/// given language.
pub fn build_has_rule(yaml: &str, lang: SgLang) -> Result<Rule, String> {
  let normalised = normalise_inline_yaml(yaml);
  let serialised: SerializableRule = match serde_yaml::from_str(&normalised) {
    Ok(r) => r,
    Err(e) => return Err(format!("invalid rule YAML: {}", e)),
  };
  let env = DeserializeEnv::new(lang);
  env
    .deserialize_rule(serialised)
    .map_err(|e| format!("rule build failed: {}", e))
}

/// Test whether `candidate` has a descendant matching `rule`.
pub fn node_has_descendant(candidate: &SgNodeMatch<'_>, rule: &Rule, _lang: SgLang) -> bool {
  for descendant in candidate.dfs() {
    // skip self — we want descendants, not the candidate itself.
    if descendant.range() == candidate.range() {
      continue;
    }
    if rule.match_node(descendant).is_some() {
      return true;
    }
  }
  false
}

/// Accept both flow-style YAML and bare `key: value, key: value`
/// shorthand (the form the brief's exit-criterion test uses).
fn normalise_inline_yaml(s: &str) -> String {
  let trimmed = s.trim();
  if trimmed.starts_with('{') || trimmed.contains('\n') {
    return trimmed.to_string();
  }
  if trimmed.contains(':') {
    return format!("{{ {} }}", trimmed);
  }
  trimmed.to_string()
}
