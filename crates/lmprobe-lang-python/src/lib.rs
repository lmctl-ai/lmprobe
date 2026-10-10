//! `lmprobe-lang-python` — D5 capability impls for Python.
//!
//! Phase 5B.5 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Python => &lmprobe_lang_python::Python*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `PythonExtractor` — delegates to `GenericExtractor` (Python's
//!   `function_definition` / `class_definition` carry a `name` field).
//! - `PythonCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)`).
//! - `PythonImportGrammar` — `import X` / `from X import $$$` and
//!   `def X` / `class X` / `__all__` export forms.
//! - `PythonInheritanceGrammar` — `class X($SUPER)` super/sub patterns
//!   (no override keyword in Python — empty override patterns).
//! - `PythonDecorationGrammar` — `@foo`, `@foo($$$)`, dotted
//!   `@app.route(...)`.
//! - `PythonTraceTemplate` — `print(f"[TRACE] var = {$var}", file=sys.stderr)`.
//! - `PythonKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `PythonTopLevelKinds` — empty (Python isn't a data lang).
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

/// Python's `function_definition` and `class_definition` nodes carry
/// the identifier under the `name` field — the generic strategy
/// suffices.
pub struct PythonExtractor;
impl NameExtractor for PythonExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Python uses standard `name(args)` / `obj.name(args)` shapes;
/// defer to `StandardCallGrammar`.
pub struct PythonCallGrammar;
impl CallGrammar for PythonCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("def {name}($$$): $$$"),
      format!("def {name}($$$) -> $RET: $$$"),
      format!("async def {name}($$$): $$$"),
      format!("async def {name}($$$) -> $RET: $$$"),
    ]
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct PythonImportGrammar;
impl ImportGrammar for PythonImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("import {name}"), format!("from {name} import $$$")]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    // Python: top-level `def/class` is implicitly exported (unless
    // `_`-prefixed); `__all__` is the explicit signal.
    vec![
      format!("def {name}($$$): $$$"),
      format!("class {name}: $$$"),
      format!("class {name}($$$): $$$"),
      format!("__all__ = [$$$, \"{name}\", $$$]"),
      format!("__all__ = [\"{name}\"]"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct PythonInheritanceGrammar;
impl InheritanceGrammar for PythonInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name}($SUPER): $$$"),
      format!("class {name}($SUPER, $$$): $$$"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C({name}): $$$"),
      format!("class $C({name}, $$$): $$$"),
      format!("class $C($$$, {name}): $$$"),
      format!("class $C($$$, {name}, $$$): $$$"),
    ]
  }
  fn override_patterns(&self, _name: &str) -> Vec<String> {
    // Python has no override keyword — the sensor reports
    // NOT_IMPLEMENTED for this verb specifically (handled via
    // empty Vec).
    Vec::new()
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Python decorators: bare, called, and dotted (`@app.route(...)`).
pub struct PythonDecorationGrammar;
impl DecorationGrammar for PythonDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("@{name}"),
      format!("@{name}($$$)"),
      format!("@$X.{name}"),
      format!("@$X.{name}($$$)"),
    ]
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct PythonTraceTemplate;
impl TraceTemplate for PythonTraceTemplate {
  fn template_for(&self, var: &str) -> Option<String> {
    Some(format!(
      "print(f\"[TRACE] {var} = {{${var}}}\", file=sys.stderr)"
    ))
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct PythonKindTaxonomy;
impl KindTaxonomy for PythonKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &["function_definition"],
      PortableKind::Class => &["class_definition"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["assignment"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_statement", "import_from_statement"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Python isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct PythonTopLevelKinds;
impl TopLevelKinds for PythonTopLevelKinds {}
