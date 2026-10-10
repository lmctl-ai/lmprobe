//! `lmprobe-lang-swift` — D5 capability impls for Swift.
//!
//! Phase 5B.7 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is
//! reachable from the registry's per-language dispatch arms
//! (`SgLang::Swift => &lmprobe_lang_swift::Swift*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `SwiftExtractor` — bespoke: `field("name")` then fall back to
//!   `simple_identifier` / `type_identifier` / `identifier` walk
//!   (Swift's class/protocol/struct/function/enum all expose `name`,
//!   but the children walk handles cases where the field is empty).
//! - `SwiftCallGrammar` — Swift call-site patterns (bare /
//!   no-new-constructor, method incl. multi-segment paths,
//!   higher-order `xs.map(name)`, trailing-closure `name { … }`)
//!   plus a wide-net `callee_decl_patterns` covering common `func`
//!   shapes (typed/untyped, visibility/`override`/`static` modifiers).
//! - `SwiftImportGrammar` — single `import {name}` pattern.
//! - `SwiftInheritanceGrammar` — `class C: Super { … }` (with and
//!   without trailing protocols), `class $C: Name`, `override func`.
//! - `SwiftDecorationGrammar` — bare `@Foo` and called `@Foo(…)`.
//! - `SwiftTraceTemplate` — `None` (lang-core's `NoTraceTemplate`
//!   catch-all applied; preserved here).
//! - `SwiftKindTaxonomy` — portable-kind ↔ Swift tree-sitter-kind
//!   map. Swift reuses `class_declaration` for class / struct / enum.
//! - `SwiftTopLevelKinds` — empty (Swift isn't a data lang).
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

/// Swift: protocol / class / struct / function / enum all expose
/// `name`. Similar to Kotlin in that child(0) is the keyword — but
/// unlike Kotlin we don't filter the field path against a keyword
/// set (Swift's tree-sitter grammar doesn't surface the keyword
/// through the `name` field).
pub struct SwiftExtractor;
impl NameExtractor for SwiftExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    node
      .field("name")
      .map(|n| clean(&n.text()))
      .or_else(|| {
        first_named_child(
          node,
          &["simple_identifier", "type_identifier", "identifier"],
        )
        .map(|n| clean(&n.text()))
      })
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Swift call-site grammar (Phase 6D.1 — restores the cycle-1
/// regression flagged in the battle-test report; the 5B.7 extraction
/// preserved lang-core's pre-existing empty `NoCallGrammar`).
///
/// Swift specifics:
/// - No `new` keyword for constructors: `Foo(x: 1)` is the
///   constructor-call form, syntactically identical to a bare
///   function call. The same `call_expression` AST node covers both
///   shapes — one pattern handles M1 + M3.
/// - Trailing-closure syntax: `foo { x in … }` — Swift admits a
///   no-paren call form when the sole argument is a closure (also
///   the basis for SwiftUI's Result Builder syntax `VStack { … }`).
///   Empirically, tree-sitter-swift differentiates this from
///   `if foo { … }` / `while foo { … }` / `for foo in xs { … }`,
///   so the trailing-closure pattern doesn't false-match those.
///
/// Patterns:
/// - `{name}($$$ARGS)` — bare call AND no-new constructor; same
///   `call_expression` AST node.
/// - `$OBJ.{name}($$$ARGS)` — method call. `$OBJ` is greedy, so
///   `pkg.mod.{name}(args)` and `Type.{name}(args)` (static method)
///   are both covered without a separate path-prefixed pattern.
/// - `$X.$M({name})` — canonical higher-order argument-passing
///   (`xs.map(foo)`); per locked doctrine, passing `name` as a
///   callable IS a caller-site. Bare-arg `bar({name})` is
///   intentionally not matched (would over-match every single-arg
///   call where a value happens to be named the same).
/// - `{name} {{ $$$BODY }}` — trailing-closure invocation. Also
///   matches Result Builder constructions (`Form { … }`,
///   `VStack { … }`); per doctrine these ARE caller-sites for the
///   builder type.
pub struct SwiftCallGrammar;
impl CallGrammar for SwiftCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("{name}($$$ARGS)"),
      format!("$OBJ.{name}($$$ARGS)"),
      format!("$X.$M({name})"),
      format!("{name} {{ $$$BODY }}"),
    ]
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    // Wide net across Swift's `func` shapes; the sensor walks
    // each match's body for descendant call sites. Patterns
    // omit the explicit `{ $$$ }` body brace because the
    // combination `func {name}($$$) -> $RET { $$$ }` fails to
    // match in tree-sitter-swift (empirically: `$RET` metavar
    // doesn't bind in return-type position when params are
    // `$$$` AND the body is `{ $$$ }` — a known Swift-grammar
    // quirk; isolated body-less or concrete-return forms work
    // fine, and lenient sibling matching covers the body).
    // Visibility modifiers don't lenient-match either — each
    // explicit modifier needs its own pattern.
    vec![
      format!("func {name}($$$) -> $RET"),
      format!("func {name}($$$)"),
      format!("public func {name}($$$) -> $RET"),
      format!("public func {name}($$$)"),
      format!("private func {name}($$$) -> $RET"),
      format!("private func {name}($$$)"),
      format!("internal func {name}($$$) -> $RET"),
      format!("internal func {name}($$$)"),
      format!("fileprivate func {name}($$$) -> $RET"),
      format!("fileprivate func {name}($$$)"),
      format!("open func {name}($$$) -> $RET"),
      format!("open func {name}($$$)"),
      format!("static func {name}($$$) -> $RET"),
      format!("static func {name}($$$)"),
      format!("class func {name}($$$) -> $RET"),
      format!("class func {name}($$$)"),
      format!("override func {name}($$$) -> $RET"),
      format!("override func {name}($$$)"),
    ]
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct SwiftImportGrammar;
impl ImportGrammar for SwiftImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    // Swift's import grammar is structurally divergent across
    // attributed forms (`@testable`, `@_spi(...)`,
    // `@_implementationOnly`) and kind-qualified forms
    // (`import struct Foundation.Data` etc.). Each requires its
    // own pattern — metavariable collapse (`import $KIND {name}`,
    // `import $$$ {name}`) fails to parse because tree-sitter-swift
    // models the kind keyword as a specific anonymous-node slot,
    // not an identifier ast-grep can match through `$KIND`.
    //
    // Bare `import {name}` covers canonical module imports.
    // The kind-qualified patterns also cover the same shape via
    // ast-grep's lenient sibling matching when the queried name
    // is the module portion (e.g. querying `Foundation` matches
    // `import struct Foundation.Data` because `Foundation` is the
    // prefix of the identifier path).
    vec![
      format!("import {name}"),
      format!("@testable import {name}"),
      format!("@_spi($SPI) import {name}"),
      format!("@_implementationOnly import {name}"),
      format!("import struct {name}"),
      format!("import class {name}"),
      format!("import func {name}"),
      format!("import enum {name}"),
      format!("import protocol {name}"),
      format!("import typealias {name}"),
      format!("import var {name}"),
      format!("import let {name}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct SwiftInheritanceGrammar;
impl InheritanceGrammar for SwiftInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name}: $SUPER {{ $$$ }}"),
      format!("class {name}: $SUPER, $$$ {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C: {name} {{ $$$ }}"),
      format!("class $C: {name}, $$$ {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("override func {name}($$$) {{ $$$ }}"),
      format!("override func {name}($$$) -> $RET {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Swift attributes: `@available`, `@MainActor`, `@objc(...)`, etc.
pub struct SwiftDecorationGrammar;
impl DecorationGrammar for SwiftDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("@{name}"), format!("@{name}($$$)")]
  }
}

// ─────────────────────────── trace template ───────────────────────

/// Swift had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct SwiftTraceTemplate;
impl TraceTemplate for SwiftTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct SwiftKindTaxonomy;
impl KindTaxonomy for SwiftKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_declaration"],
      PortableKind::Method => &["function_declaration"],
      // Swift's tree-sitter grammar emits `class_declaration`
      // for class, struct, and enum — disambiguation is
      // structural (the leading keyword), not at the kind level.
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &["class_declaration"],
      PortableKind::Interface => &["protocol_declaration"],
      PortableKind::Trait => &["protocol_declaration"],
      PortableKind::Enum => &["class_declaration"],
      PortableKind::TypeAlias => &["typealias_declaration"],
      PortableKind::Const => &["property_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Swift isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct SwiftTopLevelKinds;
impl TopLevelKinds for SwiftTopLevelKinds {}
