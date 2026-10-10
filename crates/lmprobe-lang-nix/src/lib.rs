//! `lmprobe-lang-nix` — D5 capability impls for Nix.
//!
//! Phase 5B.16 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Nix => &lmprobe_lang_nix::Nix*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `NixExtractor` — bespoke: tries `attrpath` field first
//!   (`name = value;` bindings), then `name` field, then
//!   `first_named_child` with `identifier`/`attrpath` kinds.
//!   Ported from lang-core's exclusive `SgLang::Nix => &NixExtractor`
//!   arm.
//! - `NixCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Nix).
//! - `NixImportGrammar` — `import <name>` pattern. Ported from
//!   lang-core's exclusive arm.
//! - `NixInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Nix).
//! - `NixDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Nix).
//! - `NixTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Nix).
//! - `NixKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//!   Only `Const => ["binding"]`; all others empty.
//! - `NixTopLevelKinds` — empty (Nix isn't a data lang).
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

/// Nix declarations use `attrpath` for `name = value;` bindings
/// (`name = value;`).
pub struct NixExtractor;
impl NameExtractor for NixExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    node
      .field("attrpath")
      .or_else(|| node.field("name"))
      .map(|n| clean(&n.text()))
      .or_else(|| first_named_child(node, &["identifier", "attrpath"]).map(|n| clean(&n.text())))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Nix wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct NixCallGrammar;
impl CallGrammar for NixCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Nix: `import <path>` (file/path import) plus the scope-import
/// idioms that bring symbols from a module-like value into the
/// surrounding scope:
/// - `import EXPR` — direct file / path / channel import. Anchor =
///   `EXPR` (path string, `<channel>`, or expression).
/// - `with EXPR; BODY` — wildcard scope import. Anchor = `EXPR`.
/// - `inherit (SOURCE) attr1 attr2…;` — destructuring import from a
///   source expression. Anchor = `SOURCE`.
///
/// The bare `inherit a b c;` form (no `(SOURCE)`) is intentionally
/// skipped: with no named source there is no module anchor to
/// associate the captured symbols with. (Doctrine: "1 hit per
/// statement, module = anchor".)
///
/// PRECISION CAVEAT — inherit coverage:
///
/// `inherit (SOURCE) …;` is a binding form, NOT a top-level
/// expression in Nix's grammar. tree-sitter-nix only accepts it
/// inside a `binding_set` (let-block or attrset). Therefore we
/// can't write `inherit (X) $$$;` as a standalone ast-grep pattern
/// — `Pattern::try_new` returns `MultipleNode` for that snippet.
///
/// Workaround: wrap in `{ … }` so the pattern parses as a
/// single-binding attrset. This DOES match real `inherit_from`
/// nodes, but only when the `inherit` is the SOLE binding inside
/// some attrset. Multi-binding contexts (the common case in
/// practice — mid-let-block, mid-attrset) are missed by structural
/// match.
///
/// This is a doctrinal-correct under-match (we never produce a
/// false positive; we miss some real ones). A precision-tightening
/// fix would route through a `Pattern::contextual` extension to
/// the `ImportGrammar` API, not here. See solidity precedent
/// (`e48fcb42`) for analogous shape-only matching tradeoff.
///
/// Phase 6C #6 broadening.
pub struct NixImportGrammar;
impl ImportGrammar for NixImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("import {name}"),
      format!("with {name}; $$$"),
      format!("{{ inherit ({name}) $$$; }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Nix had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct NixInheritanceGrammar;
impl InheritanceGrammar for NixInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Nix had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct NixDecorationGrammar;
impl DecorationGrammar for NixDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Nix had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct NixTraceTemplate;
impl TraceTemplate for NixTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Nix taxonomy — ported from lang-core's `taxonomy!(NixTaxonomy { … })`.
/// tree-sitter-nix uses `binding` for `name = value;` attribute bindings
/// (the Const kind). All other portable kinds are unmapped.
pub struct NixKindTaxonomy;
impl KindTaxonomy for NixKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &[],
      PortableKind::Method => &[],
      PortableKind::Class => &[],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["binding"],
      PortableKind::Macro => &[],
      PortableKind::Import => &[],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Nix isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct NixTopLevelKinds;
impl TopLevelKinds for NixTopLevelKinds {}
