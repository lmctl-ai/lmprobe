//! `lmprobe-lang-html` — D5 capability impls for HTML.
//!
//! Phase 5B.24 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Html => &lmprobe_lang_html::Html*`).
//!
//! HTML is a **data-tier** language (alongside TOML / JSON / YAML /
//! XML / CSS): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `HtmlExtractor` — bespoke: `element` wraps a `start_tag` whose
//!   first `tag_name` child is the element name. `self_closing_tag`
//!   uses the same structure. `attribute` nodes expose `attribute_name`
//!   as a direct child.
//! - `HtmlCallGrammar` — empty (data-tier; no call shape).
//! - `HtmlImportGrammar` — empty (data-tier; no import shape).
//! - `HtmlInheritanceGrammar` — empty (data-tier).
//! - `HtmlDecorationGrammar` — empty (data-tier).
//! - `HtmlTraceTemplate` — `None` (no trace instrumentation).
//! - `HtmlKindTaxonomy` — `Class` ↔ `element` (structural container),
//!   `Const` ↔ `attribute` (named entry).
//! - `HtmlTopLevelKinds` — `element` / `attribute` are the
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

/// HTML: `element` wraps a `start_tag` whose first child is `tag_name`.
pub struct HtmlExtractor;
impl NameExtractor for HtmlExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    for c in node.children() {
      let k = c.kind();
      if k == "start_tag" || k == "self_closing_tag" {
        for cc in c.children() {
          if cc.kind() == "tag_name" {
            return Some(clean(&cc.text())).filter(|s| !s.is_empty());
          }
        }
      }
      if k == "tag_name" || k == "attribute_name" {
        return Some(clean(&c.text())).filter(|s| !s.is_empty());
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// HTML is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct HtmlCallGrammar;
impl CallGrammar for HtmlCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct HtmlImportGrammar;
impl ImportGrammar for HtmlImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct HtmlInheritanceGrammar;
impl InheritanceGrammar for HtmlInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct HtmlDecorationGrammar;
impl DecorationGrammar for HtmlDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct HtmlTraceTemplate;
impl TraceTemplate for HtmlTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: HTML's `element` nodes are container-shaped (Class);
/// `attribute` nodes are named entries (Const). Mirrors lang-core's
/// `taxonomy!(HtmlTaxonomy { … })` macro invocation pre-extraction.
pub struct HtmlKindTaxonomy;
impl KindTaxonomy for HtmlKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["element"],
      PortableKind::Const => &["attribute"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `element` / `attribute` are the direct-child entries —
/// what `--has-child` gates against for HTML.
pub struct HtmlTopLevelKinds;
impl TopLevelKinds for HtmlTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["element", "attribute"]
  }
}
