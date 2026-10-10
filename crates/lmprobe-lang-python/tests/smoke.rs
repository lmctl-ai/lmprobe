//! Smoke tests for `lmprobe-lang-python` against the in-crate
//! Python fixtures (`fixtures/*.py`).
//!
//! Asserts the trait impls produce the patterns Python needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_python::{
  PythonCallGrammar, PythonDecorationGrammar, PythonExtractor, PythonImportGrammar,
  PythonInheritanceGrammar, PythonKindTaxonomy, PythonTopLevelKinds, PythonTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = PythonExtractor;
  let _ = PythonCallGrammar;
  let _ = PythonImportGrammar;
  let _ = PythonInheritanceGrammar;
  let _ = PythonDecorationGrammar;
  let _ = PythonTraceTemplate;
  let _ = PythonKindTaxonomy;
  let _ = PythonTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_both_function_definition() {
  // Python folds top-level `def` and class-method `def` into the
  // single `function_definition` kind (no separate method node).
  assert_eq!(
    PythonKindTaxonomy.kind_for(PortableKind::Function),
    &["function_definition"]
  );
  assert_eq!(
    PythonKindTaxonomy.kind_for(PortableKind::Method),
    &["function_definition"]
  );
}

#[test]
fn taxonomy_class_definition() {
  assert_eq!(
    PythonKindTaxonomy.kind_for(PortableKind::Class),
    &["class_definition"]
  );
}

#[test]
fn taxonomy_no_struct_interface_trait_enum_typealias() {
  assert!(PythonKindTaxonomy.kind_for(PortableKind::Struct).is_empty());
  assert!(PythonKindTaxonomy
    .kind_for(PortableKind::Interface)
    .is_empty());
  assert!(PythonKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(PythonKindTaxonomy.kind_for(PortableKind::Enum).is_empty());
  assert!(PythonKindTaxonomy
    .kind_for(PortableKind::TypeAlias)
    .is_empty());
}

#[test]
fn taxonomy_const_assignment() {
  assert_eq!(
    PythonKindTaxonomy.kind_for(PortableKind::Const),
    &["assignment"]
  );
}

#[test]
fn taxonomy_import_statement_and_from() {
  assert_eq!(
    PythonKindTaxonomy.kind_for(PortableKind::Import),
    &["import_statement", "import_from_statement"]
  );
}

#[test]
fn import_grammar_import_and_from() {
  let p = PythonImportGrammar.import_patterns("os");
  assert!(p.iter().any(|s| s == "import os"));
  assert!(p.iter().any(|s| s == "from os import $$$"));
}

#[test]
fn import_grammar_export_def_class_and_dunder_all() {
  let p = PythonImportGrammar.export_patterns("foo");
  assert!(p.iter().any(|s| s == "def foo($$$): $$$"));
  assert!(p.iter().any(|s| s == "class foo: $$$"));
  assert!(p.iter().any(|s| s == "class foo($$$): $$$"));
  assert!(p.iter().any(|s| s == "__all__ = [$$$, \"foo\", $$$]"));
  assert!(p.iter().any(|s| s == "__all__ = [\"foo\"]"));
}

#[test]
fn inheritance_supertype_class_with_bases() {
  let p = PythonInheritanceGrammar.supertype_decl_patterns("Child");
  assert!(p.iter().any(|s| s == "class Child($SUPER): $$$"));
  assert!(p.iter().any(|s| s == "class Child($SUPER, $$$): $$$"));
}

#[test]
fn inheritance_subtype_first_middle_last_position() {
  let p = PythonInheritanceGrammar.subtype_patterns("Base");
  assert!(p.iter().any(|s| s == "class $C(Base): $$$"));
  assert!(p.iter().any(|s| s == "class $C(Base, $$$): $$$"));
  assert!(p.iter().any(|s| s == "class $C($$$, Base): $$$"));
  assert!(p.iter().any(|s| s == "class $C($$$, Base, $$$): $$$"));
}

#[test]
fn inheritance_override_empty_no_keyword() {
  // Python has no override keyword — empty Vec → NOT_IMPLEMENTED.
  assert!(PythonInheritanceGrammar.override_patterns("foo").is_empty());
}

#[test]
fn decoration_bare_called_and_dotted_forms() {
  let p = PythonDecorationGrammar.decoration_patterns("route");
  assert!(p.iter().any(|s| s == "@route"));
  assert!(p.iter().any(|s| s == "@route($$$)"));
  assert!(p.iter().any(|s| s == "@$X.route"));
  assert!(p.iter().any(|s| s == "@$X.route($$$)"));
}

#[test]
fn call_grammar_standard_shapes() {
  // Python uses StandardCallGrammar — same shapes as Rust/Go/Java.
  let p = PythonCallGrammar.call_patterns("parse");
  assert!(p.iter().any(|s| s == "parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.parse($$$ARGS)"));
}

#[test]
fn trace_template_print_to_stderr() {
  let t = PythonTraceTemplate.template_for("VAR").unwrap();
  assert_eq!(t, "print(f\"[TRACE] VAR = {$VAR}\", file=sys.stderr)");
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["models.py", "service.py"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
