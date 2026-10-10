//! Smoke tests for `lmprobe-lang-swift` against the in-crate
//! Swift fixtures (`fixtures/*.swift`).
//!
//! Asserts the trait impls produce the patterns Swift needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_swift::{
  SwiftCallGrammar, SwiftDecorationGrammar, SwiftExtractor, SwiftImportGrammar,
  SwiftInheritanceGrammar, SwiftKindTaxonomy, SwiftTopLevelKinds, SwiftTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = SwiftExtractor;
  let _ = SwiftCallGrammar;
  let _ = SwiftImportGrammar;
  let _ = SwiftInheritanceGrammar;
  let _ = SwiftDecorationGrammar;
  let _ = SwiftTraceTemplate;
  let _ = SwiftKindTaxonomy;
  let _ = SwiftTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_share_function_declaration() {
  // Swift's tree-sitter grammar collapses top-level functions and
  // class methods into the same `function_declaration` kind.
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Function),
    &["function_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Method),
    &["function_declaration"]
  );
}

#[test]
fn taxonomy_class_struct_enum_share_class_declaration() {
  // Swift reuses `class_declaration` for class / struct / enum —
  // disambiguation is structural (the leading keyword).
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Class),
    &["class_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Struct),
    &["class_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Enum),
    &["class_declaration"]
  );
}

#[test]
fn taxonomy_interface_and_trait_both_protocol() {
  // Swift's `protocol` covers both portable kinds — there's no
  // separate trait/interface distinction.
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Interface),
    &["protocol_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Trait),
    &["protocol_declaration"]
  );
}

#[test]
fn taxonomy_typealias_const_import_no_macro_no_route() {
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::TypeAlias),
    &["typealias_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Const),
    &["property_declaration"]
  );
  assert_eq!(
    SwiftKindTaxonomy.kind_for(PortableKind::Import),
    &["import_declaration"]
  );
  assert!(SwiftKindTaxonomy.kind_for(PortableKind::Macro).is_empty());
  assert!(SwiftKindTaxonomy.kind_for(PortableKind::Route).is_empty());
}

#[test]
fn import_grammar_attributed_and_kind_qualified() {
  // Phase 6C #9 — bare + 3 attributed + 8 kind-qualified shapes.
  // Each kind keyword (struct/class/func/enum/protocol/typealias/
  // var/let) requires its own pattern: tree-sitter-swift models
  // the kind as a specific anonymous-node slot, not an identifier
  // ast-grep can match through `$KIND`.
  let p = SwiftImportGrammar.import_patterns("Foundation");
  assert_eq!(p.len(), 12);
  assert!(p.iter().any(|s| s == "import Foundation"));
  assert!(p.iter().any(|s| s == "@testable import Foundation"));
  assert!(p.iter().any(|s| s == "@_spi($SPI) import Foundation"));
  assert!(p
    .iter()
    .any(|s| s == "@_implementationOnly import Foundation"));
  assert!(p.iter().any(|s| s == "import struct Foundation"));
  assert!(p.iter().any(|s| s == "import class Foundation"));
  assert!(p.iter().any(|s| s == "import func Foundation"));
  assert!(p.iter().any(|s| s == "import enum Foundation"));
  assert!(p.iter().any(|s| s == "import protocol Foundation"));
  assert!(p.iter().any(|s| s == "import typealias Foundation"));
  assert!(p.iter().any(|s| s == "import var Foundation"));
  assert!(p.iter().any(|s| s == "import let Foundation"));
}

#[test]
fn inheritance_supertype_colon_form() {
  let p = SwiftInheritanceGrammar.supertype_decl_patterns("Repository");
  assert!(p.iter().any(|s| s == "class Repository: $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class Repository: $SUPER, $$$ { $$$ }"));
}

#[test]
fn inheritance_subtype_colon_form() {
  let p = SwiftInheritanceGrammar.subtype_patterns("BaseRepo");
  assert!(p.iter().any(|s| s == "class $C: BaseRepo { $$$ }"));
  assert!(p.iter().any(|s| s == "class $C: BaseRepo, $$$ { $$$ }"));
}

#[test]
fn inheritance_override_func_with_and_without_return() {
  let p = SwiftInheritanceGrammar.override_patterns("load");
  assert!(p.iter().any(|s| s == "override func load($$$) { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "override func load($$$) -> $RET { $$$ }"));
}

#[test]
fn decoration_grammar_bare_and_called() {
  let p = SwiftDecorationGrammar.decoration_patterns("available");
  assert!(p.iter().any(|s| s == "@available"));
  assert!(p.iter().any(|s| s == "@available($$$)"));
}

#[test]
fn call_grammar_shape_coverage() {
  // Phase 6D.1 — restored callers grammar.
  let p = SwiftCallGrammar.call_patterns("foo");
  assert_eq!(p.len(), 4, "4 call-site patterns");
  assert!(p.iter().any(|s| s == "foo($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.foo($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$X.$M(foo)"));
  assert!(p.iter().any(|s| s == "foo { $$$BODY }"));

  let d = SwiftCallGrammar.callee_decl_patterns("foo");
  assert_eq!(
    d.len(),
    18,
    "18 callee-decl patterns (no-body × 9 modifier shapes)"
  );
  assert!(d.iter().any(|s| s == "func foo($$$) -> $RET"));
  assert!(d.iter().any(|s| s == "public func foo($$$) -> $RET"));
  assert!(d.iter().any(|s| s == "override func foo($$$)"));
  assert!(d.iter().any(|s| s == "static func foo($$$) -> $RET"));
}

#[test]
fn trace_template_none() {
  // Swift had no trace template in lang-core — None signals
  // NO_TRACE_TEMPLATE so callers know to pass `--template`.
  assert!(SwiftTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["Repo.swift", "Models.swift", "Decorators.swift"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
