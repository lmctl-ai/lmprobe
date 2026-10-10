//! `lmprobe-lang-css` — D5 capability impls for CSS.
//!
//! Phase 5B.25 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Css => &lmprobe_lang_css::Css*`).
//!
//! CSS is a **data-tier** language (alongside TOML / JSON / YAML /
//! XML / HTML): only `KindTaxonomy` + `TopLevelKinds` carry live
//! per-language data. The four code-tier grammars
//! (`CallGrammar` / `ImportGrammar` / `InheritanceGrammar` /
//! `DecorationGrammar`) and `TraceTemplate` return the same empty /
//! `None` shapes lang-core's defaults produced pre-extraction.
//!
//! Structs exported (per the registry's expected naming):
//! - `CssExtractor` — bespoke: selectors aren't classical identifiers.
//!   We use the trimmed selector text as the "name" so `def html`
//!   finds `html { … }`. `rule_set` is the canonical declaration kind.
//!   For `rule_set`, selectors live on the `selectors` field; otherwise
//!   fall back to the first child's text.
//! - `CssCallGrammar` — empty (data-tier; no call shape).
//! - `CssImportGrammar` — empty (data-tier; no import shape).
//! - `CssInheritanceGrammar` — empty (data-tier).
//! - `CssDecorationGrammar` — empty (data-tier).
//! - `CssTraceTemplate` — `None` (no trace instrumentation).
//! - `CssKindTaxonomy` — `Class` ↔ `rule_set` (structural container),
//!   `Const` ↔ `declaration` (named entry),
//!   `Import` ↔ `import_statement`.
//! - `CssTopLevelKinds` — `declaration` / `rule_set` are the
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

/// CSS: selectors aren't classical identifiers. We use the trimmed
/// selector text as the "name" so `def html` finds `html { … }`.
/// `rule_set` is the canonical declaration kind.
pub struct CssExtractor;
impl NameExtractor for CssExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    // For rule_set, the selectors live on `selectors`; otherwise
    // fall back to the first child's text.
    node
      .field("selectors")
      .or_else(|| node.child(0))
      .map(|n| clean(&n.text()))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// CSS is data-tier — no call shape. Matches lang-core's
/// pre-extraction `NoCallGrammar` fallback (empty patterns surface
/// as `NOT_IMPLEMENTED` from the sensor).
pub struct CssCallGrammar;
impl CallGrammar for CssCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct CssImportGrammar;
impl ImportGrammar for CssImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct CssInheritanceGrammar;
impl InheritanceGrammar for CssInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

pub struct CssDecorationGrammar;
impl DecorationGrammar for CssDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct CssTraceTemplate;
impl TraceTemplate for CssTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Live: CSS's `rule_set` nodes are container-shaped (Class);
/// `declaration` nodes are named entries (Const);
/// `import_statement` nodes are imports (Import). Mirrors lang-core's
/// `taxonomy!(CssTaxonomy { … })` macro invocation pre-extraction.
pub struct CssKindTaxonomy;
impl KindTaxonomy for CssKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Class => &["rule_set"],
      PortableKind::Const => &["declaration"],
      PortableKind::Import => &["import_statement"],
      _ => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Live: `declaration` / `rule_set` are the direct-child entries —
/// what `--has-child` gates against for CSS.
pub struct CssTopLevelKinds;
impl TopLevelKinds for CssTopLevelKinds {
  fn child_kinds(&self) -> &'static [&'static str] {
    &["declaration", "rule_set"]
  }
}
