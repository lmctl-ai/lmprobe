//! `lmprobe-lang-bash` — D5 capability impls for Bash.
//!
//! Phase 5B.14 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Bash => &lmprobe_lang_bash::Bash*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `BashExtractor` — delegates to `GenericExtractor` (Bash's
//!   `function_definition` nodes carry a `name` field, lang-core's
//!   catch-all already routed Bash here).
//! - `BashCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Bash).
//! - `BashImportGrammar` — `source <file>` / `. <file>` patterns
//!   (ported from lang-core's exclusive arm).
//! - `BashInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Bash).
//! - `BashDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Bash).
//! - `BashTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Bash).
//! - `BashKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `BashTopLevelKinds` — empty (Bash isn't a data lang).
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

/// Bash's `function_definition` nodes carry the identifier under the
/// `name` field — the generic strategy suffices (lang-core's catch-all
/// already routed Bash here).
pub struct BashExtractor;
impl NameExtractor for BashExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Bash wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct BashCallGrammar;
impl CallGrammar for BashCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Bash: `source <file>` and `. <file>`. The dot form needs an
/// explicit pattern variant for the engine. Ported from lang-core's
/// exclusive `SgLang::Bash => &BashImportGrammar` arm.
pub struct BashImportGrammar;
impl ImportGrammar for BashImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("source {name}"),
      format!(". {name}"),
      format!("source \"{name}\""),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Bash had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct BashInheritanceGrammar;
impl InheritanceGrammar for BashInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Bash had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct BashDecorationGrammar;
impl DecorationGrammar for BashDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Bash had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct BashTraceTemplate;
impl TraceTemplate for BashTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Bash taxonomy — ported from lang-core's `taxonomy!(BashTaxonomy { … })`.
/// tree-sitter-bash uses `function_definition` for shell functions and
/// `variable_assignment` for constants. Import maps to `command` (the
/// tree-sitter node that wraps `source`/`.` invocations).
pub struct BashKindTaxonomy;
impl KindTaxonomy for BashKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &[],
      PortableKind::Class => &[],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["variable_assignment"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["command"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Bash isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct BashTopLevelKinds;
impl TopLevelKinds for BashTopLevelKinds {}
