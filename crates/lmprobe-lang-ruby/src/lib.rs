//! `lmprobe-lang-ruby` — D5 capability impls for Ruby.
//!
//! Phase 5B.10 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Ruby => &lmprobe_lang_ruby::Ruby*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `RubyExtractor` — bespoke: `field("name")` then fall back to
//!   `identifier` / `constant` children walk. Ruby's methods / classes
//!   / modules expose `name`, but Sinatra-style `def Application`
//!   needs the fallback to find the identifier.
//! - `RubyCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Ruby; preserved for byte-equality).
//! - `RubyImportGrammar` — `require` / `require_relative` patterns
//!   (single-quoted and double-quoted variants).
//! - `RubyInheritanceGrammar` — `class Name < Super` supertype and
//!   `class $C < Name` subtype patterns.
//! - `RubyDecorationGrammar` — empty (Ruby has no annotation syntax;
//!   decorators are method calls, not a distinct grammar form).
//! - `RubyTraceTemplate` — `None` (lang-core's `NoTraceTemplate`
//!   catch-all applied; preserved here).
//! - `RubyKindTaxonomy` — portable-kind ↔ Ruby tree-sitter-kind map.
//!   Notably maps Trait → `module` (Ruby's mixin mechanism).
//! - `RubyTopLevelKinds` — empty (Ruby isn't a data lang).
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
// `clean` + `first_named_child` are the same trivial helpers
// lang-core uses internally. Replicated here (rather than re-
// exported) because they're 2-line utilities and per-lang crates
// stay self-contained — the alternative would be inflating
// lang-core's public API for every lang's bespoke extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn first_named_child<'a>(node: &SgNode<'a>, kinds: &[&str]) -> Option<SgNode<'a>> {
  node.children().find(|c| kinds.contains(&c.kind().as_ref()))
}

// ──────────────────────────── name extractor ──────────────────────

/// Ruby: methods / classes / modules expose `name`. Phase 0 adds the
/// per-language declaration kinds (`method`, `class`, `module`,
/// `singleton_method`) so `def Application` finds Sinatra-style
/// classes without `--kind`.
pub struct RubyExtractor;
impl NameExtractor for RubyExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    node
      .field("name")
      .map(|n| clean(&n.text()))
      .or_else(|| first_named_child(node, &["identifier", "constant"]).map(|n| clean(&n.text())))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Ruby's call shapes aren't yet modelled — lang-core fell through
/// to `NoCallGrammar`. Keeping the empty shape preserves byte-
/// equality with the pre-extraction behavior.
pub struct RubyCallGrammar;
impl CallGrammar for RubyCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct RubyImportGrammar;
impl ImportGrammar for RubyImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("require '{name}'"),
      format!("require \"{name}\""),
      format!("require_relative '{name}'"),
      format!("require_relative \"{name}\""),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct RubyInheritanceGrammar;
impl InheritanceGrammar for RubyInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("class {name} < $SUPER\n  $$$\nend")]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("class $C < {name}\n  $$$\nend")]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Ruby has no annotation/attribute syntax — decorators are method
/// calls (e.g. `attr_accessor`, `validates`), not a distinct grammar
/// form. Preserve the empty `NoDecorationGrammar` shape.
pub struct RubyDecorationGrammar;
impl DecorationGrammar for RubyDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

/// Ruby had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct RubyTraceTemplate;
impl TraceTemplate for RubyTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct RubyKindTaxonomy;
impl KindTaxonomy for RubyKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["method"],
      PortableKind::Method => &["method", "singleton_method"],
      PortableKind::Class => &["class"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &["module"],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &[],
      PortableKind::Macro => &[],
      PortableKind::Import => &["call"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Ruby isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct RubyTopLevelKinds;
impl TopLevelKinds for RubyTopLevelKinds {}
