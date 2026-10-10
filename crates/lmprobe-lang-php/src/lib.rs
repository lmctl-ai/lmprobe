//! `lmprobe-lang-php` — D5 capability impls for PHP.
//!
//! Phase 5B.12 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Php => &lmprobe_lang_php::Php*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `PhpExtractor` — delegates to `GenericExtractor` (PHP's
//!   `function_definition` / `class_declaration` / `method_declaration`
//!   carry a `name` field).
//! - `PhpCallGrammar` — empty (PHP call grammar not wired; returns
//!   empty vecs matching `NoCallGrammar` behavior).
//! - `PhpImportGrammar` — `use X;` (plain + lenient on `function`/
//!   `const` modifier and `as $A` alias suffix) and `use X\{$$$};`
//!   group-import forms.
//! - `PhpInheritanceGrammar` — `class X extends $SUPER` /
//!   `class X implements $$$IFACES` super/sub patterns.
//! - `PhpDecorationGrammar` — PHP 8 attributes: `#[Foo]`, `#[Foo(...)]`.
//! - `PhpTraceTemplate` — empty (no trace template for PHP).
//! - `PhpKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `PhpTopLevelKinds` — empty (PHP isn't a data lang).
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

/// PHP's `function_definition`, `class_declaration`, and
/// `method_declaration` nodes carry the identifier under the `name`
/// field — the generic strategy suffices.
pub struct PhpExtractor;
impl NameExtractor for PhpExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// PHP call grammar not wired — produces empty vecs (matches
/// `NoCallGrammar` behavior from lang-core catch-all).
pub struct PhpCallGrammar;
impl CallGrammar for PhpCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// PHP namespace imports. Phase 6C round-2 — bare-root prefix-match
/// added per TC-103 PASS bar: `imports Foo /tree` should match
/// `use Foo\Bar`, `use Foo\Bar\Baz`, etc. Round-1 only matched the
/// full path; the `\$$$` continuation pattern was dropped as a
/// dedupe fix but that removed exactly the prefix-match capability
/// users want.
///
/// Round-2 patterns:
/// 1. `use {name};` — exact match (plain / function / const / alias
///    via tree-sitter-php's lenient sibling matching).
/// 2. `use {name}\$$$;` — bare-root prefix continuation. For
///    `name="Foo"`, matches `use Foo\Bar;`, `use Foo\Bar\Baz;`, etc.
///    Doesn't match `use Foo;` (the `\` requires at least one
///    continuation segment), so #1 covers the exact case without
///    overlap on this pattern.
/// 3. `use {name}\{{$$$}};` — group form: `use Foo\{X, Y};`.
pub struct PhpImportGrammar;
impl ImportGrammar for PhpImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("use {name};"),
      format!("use {name}\\$$$;"),
      format!("use {name}\\{{$$$}};"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// PHP class inheritance: `extends` for single parent,
/// `implements` for interface adoption.
pub struct PhpInheritanceGrammar;
impl InheritanceGrammar for PhpInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} extends $SUPER {{ $$$ }}"),
      format!("class {name} implements $$$IFACES {{ $$$ }}"),
      format!("class {name} extends $SUPER implements $$$ {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C extends {name} {{ $$$ }}"),
      format!("class $C implements {name} {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// PHP 8 attributes: `#[Foo]` and `#[Foo(...)]`. Pre-8 doc-comment
/// annotations are out of scope.
pub struct PhpDecorationGrammar;
impl DecorationGrammar for PhpDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("#[{name}]"), format!("#[{name}($$$)]")]
  }
}

// ─────────────────────────── trace template ───────────────────────

/// PHP has no trace template wired — returns None (matches
/// `NoTraceTemplate` catch-all from lang-core).
pub struct PhpTraceTemplate;
impl TraceTemplate for PhpTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct PhpKindTaxonomy;
impl KindTaxonomy for PhpKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &["method_declaration"],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &["interface_declaration"],
      PortableKind::Trait => &["trait_declaration"],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["const_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["namespace_use_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// PHP isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct PhpTopLevelKinds;
impl TopLevelKinds for PhpTopLevelKinds {}
