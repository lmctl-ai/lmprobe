//! `lmprobe-lang-xml` — D5 capability impls for XML.
//!
//! Phase 5B.23 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Xml => &lmprobe_lang_xml::Xml*`).
//!
//! XML is a **data-tier** language (alongside TOML / JSON / YAML /
//! HTML / CSS): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `XmlExtractor` — bespoke: `element` carries start-tag text;
//!   tree-sitter-xml exposes the start tag as `STag`/`EmptyElemTag`
//!   child whose own first `Name` child is the tag name. `Attribute`
//!   nodes use `child(0)` for the name.
//! - `XmlCallGrammar` — empty (data-tier; no call shape).
//! - `XmlImportGrammar` — empty (data-tier; no import shape).
//! - `XmlInheritanceGrammar` — empty (data-tier).
//! - `XmlDecorationGrammar` — empty (data-tier).
//! - `XmlTraceTemplate` — `None` (no trace instrumentation).
//! - `XmlKindTaxonomy` — `Class` ↔ `element` (structural container),
//!   `Const` ↔ `Attribute` (named entry).
//! - `XmlTopLevelKinds` — `element` / `Attribute` are the
//!   direct-child entries; what `--has-child` gates against.
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

/// XML: `element` carries the start-tag text; we want the tag name
/// alone. tree-sitter-xml exposes the start tag as `STag`/`EmptyElemTag`
/// child whose own first identifier child is the name.
pub struct XmlExtractor;
impl NameExtractor for XmlExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    for c in node.children() {
      let k = c.kind();
      if k == "STag" || k == "EmptyElemTag" {
        for cc in c.children() {
          if cc.kind() == "Name" {
            return Some(clean(&cc.text())).filter(|s| !s.is_empty());
          }
        }
      }
      if k == "Name" {
        return Some(clean(&c.text())).filter(|s| !s.is_empty());
      }
    }
    // Attribute: first child is the Name.
    if let Some(c0) = node.child(0) {
      return Some(clean(&c0.text())).filter(|s| !s.is_empty());
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// XML is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct XmlCallGrammar;
impl CallGrammar for XmlCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct XmlImportGrammar;
impl ImportGrammar for XmlImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct XmlInheritanceGrammar;
impl InheritanceGrammar for XmlInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct XmlDecorationGrammar;
impl DecorationGrammar for XmlDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct XmlTraceTemplate;
impl TraceTemplate for XmlTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: XML's `element` nodes are container-shaped (Class);
/// `Attribute` nodes are named entries (Const). Mirrors lang-core's
/// `taxonomy!(XmlTaxonomy { … })` macro invocation pre-extraction.
pub struct XmlKindTaxonomy;
impl KindTaxonomy for XmlKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["element"],
      PortableKind::Const => &["Attribute"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `element` / `Attribute` are the direct-child entries —
/// what `--has-child` gates against for XML.
pub struct XmlTopLevelKinds;
impl TopLevelKinds for XmlTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["element", "Attribute"]
  }
}
