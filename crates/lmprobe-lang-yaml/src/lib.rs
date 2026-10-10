//! `lmprobe-lang-yaml` — D5 capability impls for YAML.
//!
//! Phase 5B.22 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Yaml => &lmprobe_lang_yaml::Yaml*`).
//!
//! YAML is a **data-tier** language (alongside TOML / JSON / XML /
//! HTML / CSS): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `YamlExtractor` — bespoke: `block_mapping_pair` and `flow_pair`
//!   have a `key` field; strip surrounding quotes for a clean name.
//! - `YamlCallGrammar` — empty (data-tier; no call shape).
//! - `YamlImportGrammar` — empty (data-tier; no import shape).
//! - `YamlInheritanceGrammar` — empty (data-tier).
//! - `YamlDecorationGrammar` — empty (data-tier).
//! - `YamlTraceTemplate` — `None` (no trace instrumentation).
//! - `YamlKindTaxonomy` — `Class` ↔ `block_mapping` / `flow_mapping`
//!   / `block_sequence` containers, `Const` ↔ `block_mapping_pair` /
//!   `flow_pair` (the leaf key/value declaration).
//! - `YamlTopLevelKinds` — `block_mapping_pair` / `flow_pair` are
//!   the direct-child entries; what `--has-child` gates against.
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, NameExtractor,
  PortableKind, SgNode, TopLevelKinds, TraceTemplate,
};

// ──────────────────────────── helpers ─────────────────────────────
//
// Same trivial `clean` + `strip_outer_quotes` lang-core uses
// internally. Replicated here rather than re-exported — per-lang
// crates stay self-contained, and the alternative would inflate
// lang-core's public API for every bespoke extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn strip_outer_quotes(s: &str) -> String {
  let t = s.trim();
  if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
    || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
  {
    t[1..t.len() - 1].to_string()
  } else {
    t.to_string()
  }
}

// ──────────────────────────── name extractor ──────────────────────

/// YAML: `block_mapping_pair` and `flow_pair` have a `key` field.
/// Strip surrounding quotes for a clean name.
pub struct YamlExtractor;
impl NameExtractor for YamlExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if let Some(key) = node.field("key") {
      let s = clean(&key.text());
      if !s.is_empty() {
        return Some(strip_outer_quotes(&s));
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// YAML is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct YamlCallGrammar;
impl CallGrammar for YamlCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct YamlImportGrammar;
impl ImportGrammar for YamlImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct YamlInheritanceGrammar;
impl InheritanceGrammar for YamlInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct YamlDecorationGrammar;
impl DecorationGrammar for YamlDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct YamlTraceTemplate;
impl TraceTemplate for YamlTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: YAML's `block_mapping` / `flow_mapping` / `block_sequence`
/// nodes are container-shaped (Class); `block_mapping_pair` /
/// `flow_pair` are the leaf key/value declarations (Const). Mirrors
/// lang-core's `taxonomy!(YamlTaxonomy { … })` macro invocation
/// pre-extraction.
pub struct YamlKindTaxonomy;
impl KindTaxonomy for YamlKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["block_mapping", "flow_mapping", "block_sequence"],
      PortableKind::Const => &["block_mapping_pair", "flow_pair"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `block_mapping_pair` / `flow_pair` are the direct-child
/// entries — what `--has-child` gates against for YAML.
pub struct YamlTopLevelKinds;
impl TopLevelKinds for YamlTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["block_mapping_pair", "flow_pair"]
  }
}
