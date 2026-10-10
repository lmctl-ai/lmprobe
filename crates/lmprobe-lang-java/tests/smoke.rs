//! Smoke tests for `lmprobe-lang-java` against the in-crate
//! Java fixtures (`fixtures/*.java`).
//!
//! Asserts the trait impls produce the patterns Java needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_java::{
  JavaCallGrammar, JavaDecorationGrammar, JavaExtractor, JavaImportGrammar, JavaInheritanceGrammar,
  JavaKindTaxonomy, JavaTopLevelKinds, JavaTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = JavaExtractor;
  let _ = JavaCallGrammar;
  let _ = JavaImportGrammar;
  let _ = JavaInheritanceGrammar;
  let _ = JavaDecorationGrammar;
  let _ = JavaTraceTemplate;
  let _ = JavaKindTaxonomy;
  let _ = JavaTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_both_method_declaration() {
  // Java folds top-level + class-method declarations into the
  // single `method_declaration` kind.
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Function),
    &["method_declaration"]
  );
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Method),
    &["method_declaration"]
  );
}

#[test]
fn taxonomy_class_interface_enum_distinct() {
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Class),
    &["class_declaration"]
  );
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Interface),
    &["interface_declaration"]
  );
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Enum),
    &["enum_declaration"]
  );
}

#[test]
fn taxonomy_no_struct_trait_typealias() {
  assert!(JavaKindTaxonomy.kind_for(PortableKind::Struct).is_empty());
  assert!(JavaKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(JavaKindTaxonomy
    .kind_for(PortableKind::TypeAlias)
    .is_empty());
}

#[test]
fn taxonomy_import_declaration() {
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Import),
    &["import_declaration"]
  );
}

#[test]
fn taxonomy_const_field_declaration() {
  assert_eq!(
    JavaKindTaxonomy.kind_for(PortableKind::Const),
    &["field_declaration"]
  );
}

#[test]
fn import_grammar_simple_and_wildcard() {
  let p = JavaImportGrammar.import_patterns("java.util.List");
  assert!(p.iter().any(|s| s == "import java.util.List;"));
  assert!(p.iter().any(|s| s == "import java.util.List.$$$;"));
}

#[test]
fn import_grammar_export_public_class_interface_enum() {
  let p = JavaImportGrammar.export_patterns("UserRepository");
  assert!(p.iter().any(|s| s == "public class UserRepository { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "public interface UserRepository { $$$ }"));
  assert!(p.iter().any(|s| s == "public enum UserRepository { $$$ }"));
}

#[test]
fn inheritance_supertype_extends_implements() {
  let p = JavaInheritanceGrammar.supertype_decl_patterns("UserRepository");
  assert!(p
    .iter()
    .any(|s| s == "class UserRepository extends $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class UserRepository extends $SUPER implements $$$ { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class UserRepository implements $$$IFACES { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "interface UserRepository extends $$$IFACES { $$$ }"));
}

#[test]
fn inheritance_subtype_extends_implements() {
  let p = JavaInheritanceGrammar.subtype_patterns("Repository");
  assert!(p.iter().any(|s| s == "class $C extends Repository { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class $C implements Repository { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "interface $I extends Repository { $$$ }"));
}

#[test]
fn inheritance_override_with_at_override() {
  let p = JavaInheritanceGrammar.override_patterns("toString");
  assert!(p
    .iter()
    .any(|s| s == "@Override\n$RET toString($$$) { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "@Override public $RET toString($$$) { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "@Override protected $RET toString($$$) { $$$ }"));
}

#[test]
fn decoration_at_annotation_forms() {
  let p = JavaDecorationGrammar.decoration_patterns("Override");
  assert!(p.iter().any(|s| s == "@Override"));
  assert!(p.iter().any(|s| s == "@Override($$$)"));
}

#[test]
fn call_grammar_standard_shapes() {
  // Java uses StandardCallGrammar — same shapes as Rust/Go/C#.
  let p = JavaCallGrammar.call_patterns("parse");
  assert!(p.iter().any(|s| s == "parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.parse($$$ARGS)"));
}

#[test]
fn trace_template_system_err() {
  let t = JavaTraceTemplate.template_for("VAR").unwrap();
  assert_eq!(t, "System.err.println(\"[TRACE] VAR = \" + $VAR);");
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["UserRepository.java", "Models.java"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
