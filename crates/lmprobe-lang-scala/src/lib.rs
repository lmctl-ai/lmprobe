//! `lmprobe-lang-scala` — D5 capability impls for Scala.
//!
//! Phase 5B.6 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Scala => &lmprobe_lang_scala::Scala*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `ScalaExtractor` — delegates to `GenericExtractor` (Scala's
//!   declaration nodes carry a `name` field, lang-core's catch-all
//!   already routed Scala here).
//! - `ScalaCallGrammar` — Scala call-site patterns (bare /
//!   no-new-constructor, method incl. multi-segment paths, explicit
//!   `new`, and canonical higher-order `obj.method(name)`) plus a
//!   wide-net `callee_decl_patterns` covering every common `def`
//!   shape (typed/untyped, abstract, no-paren, generic, curried,
//!   visibility/`override` modifiers).
//! - `ScalaImportGrammar` — `import X` / `import X._` / `import X.$$$`
//!   plus `def/class/object/trait` export forms.
//! - `ScalaInheritanceGrammar` — `extends $SUPER` / `with $$$` and
//!   `override def` patterns.
//! - `ScalaDecorationGrammar` — empty (lang-core had no Scala
//!   decoration grammar; catch-all `NoDecorationGrammar` applied).
//! - `ScalaTraceTemplate` — `None` (lang-core had no Scala trace
//!   template; catch-all `NoTraceTemplate` applied).
//! - `ScalaKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `ScalaTopLevelKinds` — empty (Scala isn't a data lang).
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

/// Scala's `class_definition` / `function_definition` / `trait_definition`
/// nodes carry the identifier under the `name` field — the generic
/// strategy suffices (lang-core's catch-all already routed Scala here).
pub struct ScalaExtractor;
impl NameExtractor for ScalaExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Scala call-site grammar (Phase 6D.1 — restores the cycle-1
/// regression flagged in the battle-test report; the 5B.6 extraction
/// preserved lang-core's pre-existing empty `NoCallGrammar`).
///
/// Mirrors `StandardCallGrammar`'s shape but adapted to Scala syntax:
/// Scala scopes via `.` (not `::`), and case-class apply makes
/// `Foo(args)` syntactically identical to a bare call (one
/// `call_expression` AST node covers both — no separate no-new-ctor
/// pattern needed).
///
/// Patterns:
/// - `{name}($$$ARGS)` — bare call AND no-new constructor (case-class
///   `apply`); same `call_expression` AST node.
/// - `$OBJ.{name}($$$ARGS)` — method call. `$OBJ` is greedy, so
///   `pkg.mod.{name}(args)` and `Foo.{name}(args)` (object-method)
///   are both covered without a separate path-prefixed pattern.
/// - `new {name}($$$ARGS)` — explicit-`new` constructor (parses as
///   `instance_expression`, distinct AST node).
/// - `$X.$M({name})` — canonical higher-order argument-passing
///   (`xs.map(foo)`); per locked doctrine, passing `name` as a
///   callable IS a caller-site. Bare-arg `bar({name})` is
///   intentionally not matched (would over-match every single-arg
///   call where a value happens to be named the same).
pub struct ScalaCallGrammar;
impl CallGrammar for ScalaCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("{name}($$$ARGS)"),
      format!("$OBJ.{name}($$$ARGS)"),
      format!("new {name}($$$ARGS)"),
      format!("$X.$M({name})"),
    ]
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    // Wide net across Scala's `def` shapes; the sensor walks each
    // match's body for descendant call sites. Each pattern targets
    // a structurally distinct `function_definition` shape — no
    // overlap, so no dedupe needed.
    vec![
      // typed return, concrete body
      format!("def {name}($$$): $RET = $$$"),
      // untyped return, concrete body
      format!("def {name}($$$) = $$$"),
      // abstract (trait method, no body)
      format!("def {name}($$$): $RET"),
      // no-paren typed (val-style: `def name: T = expr`)
      format!("def {name}: $RET = $$$"),
      // no-paren untyped
      format!("def {name} = $$$"),
      // generic
      format!("def {name}[$$$]($$$): $RET = $$$"),
      // curried (multiple parameter lists)
      format!("def {name}($$$)($$$): $RET = $$$"),
      // override
      format!("override def {name}($$$): $RET = $$$"),
      // visibility modifiers
      format!("private def {name}($$$): $RET = $$$"),
      format!("protected def {name}($$$): $RET = $$$"),
    ]
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Scala imports — one pattern suffices because tree-sitter-scala
/// flattens every `import` form (single, multi-segment, wildcard `._`,
/// group `{X, Y}`, Scala 2 alias `{X => Y}`, Scala 3 alias `as Y`) as
/// sibling children of `import_declaration`, and ast-grep is lenient
/// on extra trailing siblings — so `import {name}` matches all of:
///   import a.b.c.X
///   import a.b.c._
///   import a.b.c.{X, Y}
///   import a.b.c.{X => Y}
///   import a.b.c.X as Y
/// when name = `a.b.c`. Doctrine (Phase 6C): one hit per `import`
/// statement, module = anchor; aliases / wildcards / selectors are
/// metadata captured by the trailing siblings and ignored.
///
/// The pre-broadening pattern set (`import {name}`, `import {name}._`,
/// `import {name}.$$$`) emitted 2× duplicate hits per source line
/// because the bare pattern already lenient-matched the wildcard /
/// `$$$` forms. Collapsing to the single pattern restores 1 hit per
/// statement.
pub struct ScalaImportGrammar;
impl ImportGrammar for ScalaImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("import {name}")]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("def {name}($$$) = $$$"),
      format!("class {name}($$$) {{ $$$ }}"),
      format!("object {name} {{ $$$ }}"),
      format!("trait {name} {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct ScalaInheritanceGrammar;
impl InheritanceGrammar for ScalaInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} extends $SUPER {{ $$$ }}"),
      format!("class {name}($$$) extends $SUPER {{ $$$ }}"),
      format!("trait {name} extends $SUPER {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C extends {name} {{ $$$ }}"),
      format!("class $C extends {name} with $$$ {{ $$$ }}"),
      format!("trait $T extends {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("override def {name}($$$) = $$$")]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Scala had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct ScalaDecorationGrammar;
impl DecorationGrammar for ScalaDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

/// Scala had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct ScalaTraceTemplate;
impl TraceTemplate for ScalaTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct ScalaKindTaxonomy;
impl KindTaxonomy for ScalaKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &["function_definition"],
      PortableKind::Class => &["class_definition"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &["trait_definition"],
      PortableKind::Trait => &["trait_definition"],
      PortableKind::Enum => &["enum_definition"],
      PortableKind::TypeAlias => &["type_definition"],
      PortableKind::Const => &["val_definition", "var_definition"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Scala isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct ScalaTopLevelKinds;
impl TopLevelKinds for ScalaTopLevelKinds {}
