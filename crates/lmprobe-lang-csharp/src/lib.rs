//! `lmprobe-lang-csharp` — D5 capability impls for C#.
//!
//! Phase 5B.2 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::CSharp => &lmprobe_lang_csharp::CSharp*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `CSharpExtractor` — delegates to `GenericExtractor` (C#'s
//!   declaration nodes carry a `name` field).
//! - `CSharpCallGrammar` — delegates to `StandardCallGrammar`.
//! - `CSharpImportGrammar` — `using` directives: plain, `static`,
//!   alias (LHS / RHS forms), and C# 10+ `global using`. Phase 6C #2
//!   wire-up.
//! - `CSharpInheritanceGrammar` — `class : Super` / `interface : Iface`
//!   plus `[public] override` method patterns.
//! - `CSharpDecorationGrammar` — empty (C# attribute grammar
//!   not yet wired; matched `NoDecorationGrammar` previously).
//! - `CSharpTraceTemplate` — `None` (no default template).
//! - `CSharpKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `CSharpTopLevelKinds` — empty (C# isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, GenericExtractor, ImportGrammar, InheritanceGrammar,
  KindTaxonomy, NameExtractor, PortableKind, SgNode, StandardCallGrammar, TopLevelKinds,
  TraceTemplate,
};

// ──────────────────────────── name extractor ──────────────────────

/// C# uses the generic `field("name") → field("key") → child(0)`
/// strategy — declarations like `class Foo`, `void Bar()`,
/// `interface IBaz` expose the identifier under the `name` field.
pub struct CSharpExtractor;
impl NameExtractor for CSharpExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// C# uses standard `name(args)` / `target.name(args)` /
/// `Type::name(args)` shapes; defer to `StandardCallGrammar`.
pub struct CSharpCallGrammar;
impl CallGrammar for CSharpCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// C# `using` directives, four shapes:
/// - `using {name};`                   — plain import (single or dotted path).
///                                        Also covers `global using {name};`
///                                        because tree-sitter-csharp treats
///                                        `global` as an outer modifier on
///                                        the same `using_directive` node.
/// - `using static {name};`            — static-member import.
/// - `using {name} = $TARGET;`         — alias form, `{name}` is the alias.
/// - `using $ALIAS = {name};`          — alias form, `{name}` is the imported target.
///
/// Doctrine: capture the imported module/symbol; alias is metadata.
/// For `using Foo = Bar.Baz;`, the imported thing is `Bar.Baz`, so a
/// query `imports Bar.Baz -l csharp` matches via the RHS pattern;
/// `imports Foo` matches via the LHS pattern (alias-as-symbol).
/// Wildcards/multi-symbol N/A — each `using` is one statement.
pub struct CSharpImportGrammar;
impl ImportGrammar for CSharpImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("using {name};"),
      format!("using static {name};"),
      format!("using {name} = $TARGET;"),
      format!("using $ALIAS = {name};"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct CSharpInheritanceGrammar;
impl InheritanceGrammar for CSharpInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} : $SUPER {{ $$$ }}"),
      format!("class {name} : $SUPER, $$$ {{ $$$ }}"),
      format!("interface {name} : $$$IFACES {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C : {name} {{ $$$ }}"),
      format!("class $C : {name}, $$$ {{ $$$ }}"),
      format!("interface $I : {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("public override $RET {name}($$$) {{ $$$ }}"),
      format!("override $RET {name}($$$) {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

pub struct CSharpDecorationGrammar;
impl DecorationGrammar for CSharpDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct CSharpTraceTemplate;
impl TraceTemplate for CSharpTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct CSharpKindTaxonomy;
impl KindTaxonomy for CSharpKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["method_declaration", "local_function_statement"],
      PortableKind::Method => &["method_declaration"],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &["struct_declaration"],
      PortableKind::Interface => &["interface_declaration"],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["field_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["using_directive"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// C# isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct CSharpTopLevelKinds;
impl TopLevelKinds for CSharpTopLevelKinds {}
