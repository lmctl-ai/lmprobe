//! Smoke tests for `lmprobe-lang-go` against the in-crate Go
//! fixtures (`fixtures/*.go`).
//!
//! Asserts the trait impls produce the patterns Go needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_go::{
  GoCallGrammar, GoDecorationGrammar, GoExtractor, GoImportGrammar, GoInheritanceGrammar,
  GoKindTaxonomy, GoTopLevelKinds, GoTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = GoExtractor;
  let _ = GoCallGrammar;
  let _ = GoImportGrammar;
  let _ = GoInheritanceGrammar;
  let _ = GoDecorationGrammar;
  let _ = GoTraceTemplate;
  let _ = GoKindTaxonomy;
  let _ = GoTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_distinct() {
  // Unlike Java, Go separates top-level `func` from receiver
  // methods at the tree-sitter level.
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Function),
    &["function_declaration"]
  );
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Method),
    &["method_declaration"]
  );
}

#[test]
fn taxonomy_struct_interface_typealias_share_kind() {
  // Go folds all three under `type_declaration`; the inner
  // `type_spec` carries the actual shape.
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Struct),
    &["type_declaration"]
  );
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Interface),
    &["type_declaration"]
  );
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::TypeAlias),
    &["type_declaration"]
  );
}

#[test]
fn taxonomy_no_class_trait_enum_macro() {
  assert!(GoKindTaxonomy.kind_for(PortableKind::Class).is_empty());
  assert!(GoKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(GoKindTaxonomy.kind_for(PortableKind::Enum).is_empty());
  assert!(GoKindTaxonomy.kind_for(PortableKind::Macro).is_empty());
}

#[test]
fn taxonomy_const_covers_var() {
  // Go has no `const` / `var` distinction at the portable layer;
  // both surface under PortableKind::Const.
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Const),
    &["const_declaration", "var_declaration"]
  );
}

#[test]
fn taxonomy_import_declaration() {
  assert_eq!(
    GoKindTaxonomy.kind_for(PortableKind::Import),
    &["import_declaration"]
  );
}

#[test]
fn import_grammar_quoted_path() {
  let p = GoImportGrammar.import_patterns("fmt");
  assert!(p.iter().any(|s| s == "import \"fmt\""));
}

#[test]
fn import_grammar_export_func_type_var_const() {
  let p = GoImportGrammar.export_patterns("Repository");
  assert!(p.iter().any(|s| s == "func Repository($$$) { $$$ }"));
  assert!(p.iter().any(|s| s == "func Repository($$$) $RET { $$$ }"));
  assert!(p.iter().any(|s| s == "type Repository struct { $$$ }"));
  assert!(p.iter().any(|s| s == "type Repository interface { $$$ }"));
  assert!(p.iter().any(|s| s == "var Repository = $RHS"));
  assert!(p.iter().any(|s| s == "const Repository = $RHS"));
}

#[test]
fn inheritance_grammar_empty() {
  // Go has no `class extends` shape — empty across all three
  // sub-traits (struct embedding + structural interface
  // satisfaction don't reduce to single patterns).
  assert!(GoInheritanceGrammar.supertype_decl_patterns("X").is_empty());
  assert!(GoInheritanceGrammar.subtype_patterns("X").is_empty());
  assert!(GoInheritanceGrammar.override_patterns("X").is_empty());
}

#[test]
fn decoration_grammar_empty() {
  // Go has no annotation syntax; build tags are line comments
  // and handled elsewhere.
  assert!(GoDecorationGrammar.decoration_patterns("Foo").is_empty());
}

#[test]
fn call_grammar_standard_shapes() {
  // Go uses StandardCallGrammar — same shapes as Rust/Java/C#.
  let p = GoCallGrammar.call_patterns("Find");
  assert!(p.iter().any(|s| s == "Find($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.Find($$$ARGS)"));
}

#[test]
fn trace_template_fmt_fprintln() {
  let t = GoTraceTemplate.template_for("VAR").unwrap();
  assert_eq!(t, "fmt.Fprintln(os.Stderr, \"[TRACE] VAR =\", $VAR)");
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["repository.go", "types.go"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
