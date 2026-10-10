//! `lmprobe-lang-lua` — D5 capability impls for Lua.
//!
//! Phase 5B.15 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Lua => &lmprobe_lang_lua::Lua*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `LuaExtractor` — bespoke: walks to the last `.`/`:`-segmented
//!   identifier so dotted names (`M.setup`, `lazy.config.options`)
//!   hit on their tail component. Ported from lang-core's exclusive
//!   `SgLang::Lua => &LuaExtractor` arm.
//! - `LuaCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Lua).
//! - `LuaImportGrammar` — `require 'mod'` / `require "mod"` /
//!   `require('mod')` / `require("mod")` patterns. Ported from
//!   lang-core's exclusive arm.
//! - `LuaInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Lua).
//! - `LuaDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Lua).
//! - `LuaTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Lua).
//! - `LuaKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//! - `LuaTopLevelKinds` — empty (Lua isn't a data lang).
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

// ──────────────────────────── name extractor ──────────────────────

/// Lua's `function_declaration` / `function_definition` nodes may
/// carry dotted (`M.setup`) or colon (`M:method`) names in the
/// `name` field. Walk to the last `.`/`:`-segmented identifier so
/// the tail component becomes the match target.
pub struct LuaExtractor;
impl NameExtractor for LuaExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if let Some(name) = node.field("name") {
      let s = clean(&name.text());
      // Walk to the last `.`-segmented identifier so dotted
      // names (`M.setup`, `lazy.config.options`) hit on their
      // tail.
      let tail = s.rsplit('.').next().unwrap_or(&s).trim();
      let tail = tail.rsplit(':').next().unwrap_or(tail).trim();
      if !tail.is_empty() {
        return Some(tail.to_string());
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Lua wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct LuaCallGrammar;
impl CallGrammar for LuaCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Lua: `require 'mod'`, `require "mod"`, `require('mod')`,
/// `require("mod")`. Ported from lang-core's exclusive
/// `SgLang::Lua => &LuaImportGrammar` arm.
pub struct LuaImportGrammar;
impl ImportGrammar for LuaImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("require '{name}'"),
      format!("require \"{name}\""),
      format!("require('{name}')"),
      format!("require(\"{name}\")"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Lua had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct LuaInheritanceGrammar;
impl InheritanceGrammar for LuaInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Lua had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct LuaDecorationGrammar;
impl DecorationGrammar for LuaDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Lua had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct LuaTraceTemplate;
impl TraceTemplate for LuaTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Lua taxonomy — ported from lang-core's `taxonomy!(LuaTaxonomy { … })`.
/// tree-sitter-lua uses `function_declaration` for named functions and
/// `function_definition` for anonymous function expressions.
/// `function_call` wraps `require(...)` invocations (the Import kind).
pub struct LuaKindTaxonomy;
impl KindTaxonomy for LuaKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_declaration", "function_definition"],
      PortableKind::Method => &["function_declaration"],
      PortableKind::Class => &[],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["variable_declaration", "assignment_statement"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["function_call"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Lua isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct LuaTopLevelKinds;
impl TopLevelKinds for LuaTopLevelKinds {}
