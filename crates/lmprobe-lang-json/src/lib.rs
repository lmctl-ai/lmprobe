//! `lmprobe-lang-json` — D5 capability impls for JSON.
//!
//! Phase 5B.21 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Json => &lmprobe_lang_json::Json*`).
//!
//! JSON is a **data-tier** language (alongside TOML / YAML / XML /
//! HTML / CSS): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `JsonExtractor` — bespoke: `pair` has a `key` field that is a
//!   `string`; strip the surrounding quotes for a clean name.
//! - `JsonCallGrammar` — empty (data-tier; no call shape).
//! - `JsonImportGrammar` — empty (data-tier; no import shape).
//! - `JsonInheritanceGrammar` — empty (data-tier).
//! - `JsonDecorationGrammar` — empty (data-tier).
//! - `JsonTraceTemplate` — `None` (no trace instrumentation).
//! - `JsonKindTaxonomy` — `Class` ↔ `object` / `array` containers,
//!   `Const` ↔ `pair` (the leaf key/value declaration).
//! - `JsonTopLevelKinds` — `pair` is the direct-child entry inside
//!   an object; what `--has-child` gates against.
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

/// JSON: `pair` has a `key` field that is a `string`. Strip the
/// surrounding quotes for a clean name.
pub struct JsonExtractor;
impl NameExtractor for JsonExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if let Some(key) = node.field("key") {
      let raw = clean(&key.text());
      return Some(strip_outer_quotes(&raw)).filter(|s| !s.is_empty());
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// JSON is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct JsonCallGrammar;
impl CallGrammar for JsonCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct JsonImportGrammar;
impl ImportGrammar for JsonImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct JsonInheritanceGrammar;
impl InheritanceGrammar for JsonInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct JsonDecorationGrammar;
impl DecorationGrammar for JsonDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct JsonTraceTemplate;
impl TraceTemplate for JsonTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: JSON's `object` / `array` nodes are container-shaped
/// (Class); `pair` is the leaf key/value declaration (Const).
/// Mirrors lang-core's `taxonomy!(JsonTaxonomy { … })` macro
/// invocation pre-extraction.
pub struct JsonKindTaxonomy;
impl KindTaxonomy for JsonKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["object", "array"],
      PortableKind::Const => &["pair"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `pair` is the direct-child entry inside an object — what
/// `--has-child` gates against for JSON.
pub struct JsonTopLevelKinds;
impl TopLevelKinds for JsonTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["pair"]
  }
}
