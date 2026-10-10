//! Smoke tests for `lmprobe-lang-csharp` against the in-crate
//! C# fixtures (`fixtures/*.cs`).
//!
//! Asserts the trait impls produce the patterns C# needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_csharp::{
  CSharpCallGrammar, CSharpDecorationGrammar, CSharpExtractor, CSharpImportGrammar,
  CSharpInheritanceGrammar, CSharpKindTaxonomy, CSharpTopLevelKinds, CSharpTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = CSharpExtractor;
  let _ = CSharpCallGrammar;
  let _ = CSharpImportGrammar;
  let _ = CSharpInheritanceGrammar;
  let _ = CSharpDecorationGrammar;
  let _ = CSharpTraceTemplate;
  let _ = CSharpKindTaxonomy;
  let _ = CSharpTopLevelKinds;
}

#[test]
fn taxonomy_function_includes_local_function() {
  let kinds = CSharpKindTaxonomy.kind_for(PortableKind::Function);
  assert!(kinds.contains(&"method_declaration"));
  assert!(
    kinds.contains(&"local_function_statement"),
    "C# Function maps to method + local function"
  );
}

#[test]
fn taxonomy_class_struct_interface_distinct() {
  assert_eq!(
    CSharpKindTaxonomy.kind_for(PortableKind::Class),
    &["class_declaration"]
  );
  assert_eq!(
    CSharpKindTaxonomy.kind_for(PortableKind::Struct),
    &["struct_declaration"]
  );
  assert_eq!(
    CSharpKindTaxonomy.kind_for(PortableKind::Interface),
    &["interface_declaration"]
  );
}

#[test]
fn taxonomy_no_trait_or_typealias() {
  assert!(CSharpKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(CSharpKindTaxonomy
    .kind_for(PortableKind::TypeAlias)
    .is_empty());
}

#[test]
fn taxonomy_import_using_directive() {
  let kinds = CSharpKindTaxonomy.kind_for(PortableKind::Import);
  assert_eq!(kinds, &["using_directive"]);
}

#[test]
fn taxonomy_const_field_declaration() {
  let kinds = CSharpKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["field_declaration"]);
}

#[test]
fn import_grammar_emits_using_shapes() {
  // Phase 6C #2 — `using` directive grammar is now wired. The
  // patterns cover plain, static, and the two alias forms.
  // `global using` rides through the plain shape (see test
  // `global_using_matches_via_plain_pattern` in tests/imports.rs).
  let p = CSharpImportGrammar.import_patterns("System.IO");
  assert!(p.iter().any(|s| s == "using System.IO;"), "plain shape");
  assert!(
    p.iter().any(|s| s == "using static System.IO;"),
    "static shape"
  );
  assert!(
    p.iter().any(|s| s == "using System.IO = $TARGET;"),
    "alias LHS shape"
  );
  assert!(
    p.iter().any(|s| s == "using $ALIAS = System.IO;"),
    "alias RHS shape"
  );
  // `global using {name};` is intentionally absent — the plain
  // `using {name};` pattern already matches the global form
  // because tree-sitter-csharp treats `global` as an outer
  // modifier on the same `using_directive` node.
}

#[test]
fn import_grammar_multi_segment_dotted_path() {
  let p = CSharpImportGrammar.import_patterns("A.B.C.D");
  assert!(p.iter().any(|s| s == "using A.B.C.D;"));
  assert!(p.iter().any(|s| s == "using static A.B.C.D;"));
}

#[test]
fn decoration_grammar_empty_csharp_unwired() {
  // C#'s attribute grammar (`[Attr]` / `[Attr(args)]`) isn't yet
  // wired; matches the pre-extraction NoDecorationGrammar behavior.
  assert!(CSharpDecorationGrammar
    .decoration_patterns("Serializable")
    .is_empty());
}

#[test]
fn inheritance_supertype_colon_form() {
  let p = CSharpInheritanceGrammar.supertype_decl_patterns("UserRepository");
  assert!(p
    .iter()
    .any(|s| s == "class UserRepository : $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class UserRepository : $SUPER, $$$ { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "interface UserRepository : $$$IFACES { $$$ }"));
}

#[test]
fn inheritance_subtype_colon_form() {
  let p = CSharpInheritanceGrammar.subtype_patterns("IRepository");
  assert!(p.iter().any(|s| s == "class $C : IRepository { $$$ }"));
  assert!(p.iter().any(|s| s == "class $C : IRepository, $$$ { $$$ }"));
  assert!(p.iter().any(|s| s == "interface $I : IRepository { $$$ }"));
}

#[test]
fn inheritance_override_public_and_bare() {
  let p = CSharpInheritanceGrammar.override_patterns("ToString");
  assert!(p
    .iter()
    .any(|s| s == "public override $RET ToString($$$) { $$$ }"));
  assert!(p.iter().any(|s| s == "override $RET ToString($$$) { $$$ }"));
}

#[test]
fn call_grammar_standard_shapes() {
  // C# uses StandardCallGrammar — same shapes as Rust/Java/Go.
  let p = CSharpCallGrammar.call_patterns("Parse");
  assert!(p.iter().any(|s| s == "Parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.Parse($$$ARGS)"));
}

#[test]
fn trace_template_none() {
  assert!(CSharpTraceTemplate.template_for("x").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["Repo.cs", "Models.cs"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
