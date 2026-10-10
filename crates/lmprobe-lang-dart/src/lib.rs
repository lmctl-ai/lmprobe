//! `lmprobe-lang-dart` — D5 capability impls for Dart.
//!
//! Phase 5B.13 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Dart => &lmprobe_lang_dart::Dart*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `DartExtractor` — delegates to `GenericExtractor` (Dart's
//!   declaration nodes carry a `name` field, lang-core's catch-all
//!   already routed Dart here).
//! - `DartCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Dart).
//! - `DartImportGrammar` — empty (lang-core fell through to
//!   `NoImportGrammar` for Dart).
//! - `DartInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Dart).
//! - `DartDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Dart).
//! - `DartTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Dart).
//! - `DartKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `DartTopLevelKinds` — empty (Dart isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, GenericExtractor, ImportGrammar, InheritanceGrammar,
  KindTaxonomy, NameExtractor, PortableKind, SgNode, TopLevelKinds, TraceTemplate,
};

// ──────────────────────────── name extractor ──────────────────────

/// Dart's `class_declaration` / `function_signature` / `enum_declaration`
/// nodes carry the identifier under the `name` field — the generic
/// strategy suffices (lang-core's catch-all already routed Dart here).
pub struct DartExtractor;
impl NameExtractor for DartExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Dart wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct DartCallGrammar;
impl CallGrammar for DartCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Dart had no import grammar in lang-core — keep the empty shape
/// (matches `NoImportGrammar` catch-all).
pub struct DartImportGrammar;
impl ImportGrammar for DartImportGrammar {
  fn import_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Dart had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct DartInheritanceGrammar;
impl InheritanceGrammar for DartInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Dart had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct DartDecorationGrammar;
impl DecorationGrammar for DartDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Dart had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct DartTraceTemplate;
impl TraceTemplate for DartTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Dart taxonomy — ported from lang-core's `taxonomy!(DartTaxonomy { … })`.
/// tree-sitter-dart uses `class_declaration` (not `class_definition`);
/// top-level functions register as `function_signature` *with*
/// `function_body`. See plan_corrections.md entry #8.
pub struct DartKindTaxonomy;
impl KindTaxonomy for DartKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_signature", "function_body"],
      PortableKind::Method => &["method_signature", "function_signature"],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &["mixin_declaration"],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &["type_alias"],
      PortableKind::Const => &["initialized_variable_definition"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_or_export"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Dart isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct DartTopLevelKinds;
impl TopLevelKinds for DartTopLevelKinds {}
