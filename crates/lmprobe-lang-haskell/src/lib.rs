//! `lmprobe-lang-haskell` — D5 capability impls for Haskell.
//!
//! Phase 5B.18 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Haskell => &lmprobe_lang_haskell::Haskell*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `HaskellExtractor` — bespoke: `function`/`signature`/`bind`
//!   declarations carry the identifier as `name` or as a
//!   `variable`/`constructor` child. Uses `clean` + `first_named_child`
//!   helpers (replicated privately per the playbook).
//! - `HaskellCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Haskell).
//! - `HaskellImportGrammar` — `import`, `import qualified`, `import …
//!   ($$$)` patterns. Ported from lang-core's exclusive arm.
//! - `HaskellInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Haskell).
//! - `HaskellDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Haskell).
//! - `HaskellTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Haskell).
//! - `HaskellKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//!   Ported from lang-core's `taxonomy!(HaskellTaxonomy { … })`.
//! - `HaskellTopLevelKinds` — empty (Haskell isn't a data lang).
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

// ─────────────────────────── helpers ──────────────────────────────

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn first_named_child<'a>(node: &SgNode<'a>, kinds: &[&str]) -> Option<SgNode<'a>> {
  node.children().find(|c| kinds.contains(&c.kind().as_ref()))
}

// ──────────────────────────── name extractor ──────────────────────

/// Haskell: `function`/`signature`/`bind` declarations carry the
/// identifier as `name` or as a `variable`/`constructor` child. The
/// per-language kind list lifts the QA `--kind signature` workaround
/// (findings/haskell.md) into the default search path.
pub struct HaskellExtractor;
impl NameExtractor for HaskellExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    node
      .field("name")
      .map(|n| clean(&n.text()))
      .or_else(|| first_named_child(node, &["variable", "constructor"]).map(|n| clean(&n.text())))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Haskell wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct HaskellCallGrammar;
impl CallGrammar for HaskellCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Haskell: `import`, `import qualified`, and explicit import lists.
/// Ported from lang-core's exclusive `SgLang::Haskell => &HaskellImportGrammar`
/// arm.
pub struct HaskellImportGrammar;
impl ImportGrammar for HaskellImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("import {name}"),
      format!("import qualified {name}"),
      format!("import {name} ($$$)"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Haskell had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct HaskellInheritanceGrammar;
impl InheritanceGrammar for HaskellInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Haskell had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct HaskellDecorationGrammar;
impl DecorationGrammar for HaskellDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Haskell had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct HaskellTraceTemplate;
impl TraceTemplate for HaskellTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Haskell taxonomy — ported from lang-core's `taxonomy!(HaskellTaxonomy { … })`.
pub struct HaskellKindTaxonomy;
impl KindTaxonomy for HaskellKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function", "signature", "bind"],
      PortableKind::Method => &[],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &["class_declaration"],
      PortableKind::Enum => &["data_type"],
      PortableKind::TypeAlias => &["type_synonym", "newtype"],
      PortableKind::Const => &["bind"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Haskell isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct HaskellTopLevelKinds;
impl TopLevelKinds for HaskellTopLevelKinds {}
