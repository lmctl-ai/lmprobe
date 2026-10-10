//! `lmprobe-lang-toml` — D5 capability impls for TOML.
//!
//! Phase 5B.3 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Toml => &lmprobe_lang_toml::Toml*`).
//!
//! TOML is a **data-tier** language (alongside JSON / YAML / XML /
//! HTML / CSS): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `TomlExtractor` — bespoke: `table` / `table_array_element`
//!   headers expose the path via `bare_key` / `dotted_key` /
//!   `quoted_key` children (the generic extractor stops at the
//!   `[` bracket); `pair` carries the key under a `key` field.
//! - `TomlCallGrammar` — empty (data-tier; no call shape).
//! - `TomlImportGrammar` — empty (data-tier; no import shape).
//! - `TomlInheritanceGrammar` — empty (data-tier).
//! - `TomlDecorationGrammar` — empty (data-tier).
//! - `TomlTraceTemplate` — `None` (no trace instrumentation).
//! - `TomlKindTaxonomy` — `Class` ↔ table containers, `Const` ↔
//!   `pair` (the leaf key/value declaration).
//! - `TomlTopLevelKinds` — `pair` is the direct-child entry inside
//!   a table; what `--has-child` gates against.
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
// Same trivial `clean` lang-core uses internally. Replicated here
// rather than re-exported — per-lang crates stay self-contained, and
// the alternative would inflate lang-core's public API for every
// bespoke extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

// ──────────────────────────── name extractor ──────────────────────

/// TOML: `table` / `table_array_element` headers carry the path via
/// `bare_key` / `dotted_key` / `quoted_key` children — the generic
/// extractor's `field("name")` / `child(0)` strategy stops at the
/// `[` bracket. `pair` carries the key under a `key` field.
pub struct TomlExtractor;
impl NameExtractor for TomlExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    for c in node.children() {
      let k = c.kind();
      if k == "bare_key" || k == "dotted_key" || k == "quoted_key" {
        return Some(clean(&c.text())).filter(|s| !s.is_empty());
      }
    }
    if let Some(key) = node.field("key") {
      let s = clean(&key.text());
      if !s.is_empty() {
        return Some(s);
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// TOML is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct TomlCallGrammar;
impl CallGrammar for TomlCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct TomlImportGrammar;
impl ImportGrammar for TomlImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct TomlInheritanceGrammar;
impl InheritanceGrammar for TomlInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct TomlDecorationGrammar;
impl DecorationGrammar for TomlDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct TomlTraceTemplate;
impl TraceTemplate for TomlTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: TOML's `table` / `table_array_element` / `inline_table`
/// nodes are container-shaped (Class); `pair` is the leaf key/value
/// declaration (Const). Mirrors lang-core's `taxonomy!(TomlTaxonomy
/// { … })` macro invocation pre-extraction.
pub struct TomlKindTaxonomy;
impl KindTaxonomy for TomlKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["table", "table_array_element", "inline_table"],
      PortableKind::Const => &["pair"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `pair` is the direct-child entry inside a table — what
/// `--has-child` gates against for TOML.
pub struct TomlTopLevelKinds;
impl TopLevelKinds for TomlTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["pair"]
  }
}
