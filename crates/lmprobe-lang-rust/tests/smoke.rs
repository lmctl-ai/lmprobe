//! Smoke tests for `lmprobe-lang-rust` against the in-crate
//! Rust fixture (`fixtures/lib.rs`).
//!
//! Asserts the trait impls produce reasonable patterns for a few
//! representative shapes. The binary's deep + smoke tiers are the
//! cross-language regression matrix; this is the per-crate gate
//! the upcoming xtask check-lang lint references.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_rust::{
  RustCallGrammar, RustDecorationGrammar, RustExtractor, RustImportGrammar, RustInheritanceGrammar,
  RustKindTaxonomy, RustTopLevelKinds, RustTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = RustExtractor;
  let _ = RustCallGrammar;
  let _ = RustImportGrammar;
  let _ = RustInheritanceGrammar;
  let _ = RustDecorationGrammar;
  let _ = RustTraceTemplate;
  let _ = RustKindTaxonomy;
  let _ = RustTopLevelKinds;
}

#[test]
fn taxonomy_maps_function_to_function_item() {
  let kinds = RustKindTaxonomy.kind_for(PortableKind::Function);
  assert_eq!(kinds, &["function_item"]);
}

#[test]
fn taxonomy_struct_kind() {
  let kinds = RustKindTaxonomy.kind_for(PortableKind::Struct);
  assert_eq!(kinds, &["struct_item"]);
}

#[test]
fn taxonomy_no_class_for_rust() {
  let kinds = RustKindTaxonomy.kind_for(PortableKind::Class);
  assert!(kinds.is_empty(), "Rust has no Class kind");
}

#[test]
fn import_grammar_use_pattern() {
  let p = RustImportGrammar.import_patterns("serde");
  assert!(p.iter().any(|s| s == "use serde;"));
  assert!(p.iter().any(|s| s == "use serde::$$$;"));
}

#[test]
fn import_grammar_phase_6c_pattern_set() {
  // Phase 6C #4: 5 shapes — plain, single-symbol continuation,
  // alias, group, glob. Order is not load-bearing (the sensor
  // tries each independently); set membership is.
  let p = RustImportGrammar.import_patterns("serde");
  assert_eq!(p.len(), 5, "exactly 5 patterns; got {p:?}");
  assert!(p.iter().any(|s| s == "use serde;"));
  assert!(p.iter().any(|s| s == "use serde::$$$;"));
  assert!(p.iter().any(|s| s == "use serde as $ALIAS;"));
  assert!(p.iter().any(|s| s == "use serde::{$$$};"));
  assert!(p.iter().any(|s| s == "use serde::*;"));
}

#[test]
fn import_grammar_multi_segment_name() {
  // Multi-segment names interpolate verbatim into every shape.
  let p = RustImportGrammar.import_patterns("std::io::Write");
  assert!(p.iter().any(|s| s == "use std::io::Write;"));
  assert!(p.iter().any(|s| s == "use std::io::Write::$$$;"));
  assert!(p.iter().any(|s| s == "use std::io::Write as $ALIAS;"));
  assert!(p.iter().any(|s| s == "use std::io::Write::{$$$};"));
  assert!(p.iter().any(|s| s == "use std::io::Write::*;"));
}

#[test]
fn export_grammar_pub_struct() {
  let p = RustImportGrammar.export_patterns("User");
  assert!(p.iter().any(|s| s.contains("pub struct User")));
}

#[test]
fn decoration_grammar_derive_chain() {
  let p = RustDecorationGrammar.decoration_patterns("Serialize");
  assert!(p.iter().any(|s| s == "#[Serialize]"));
  assert!(p.iter().any(|s| s == "#[derive(Serialize)]"));
  assert!(p.iter().any(|s| s == "#[derive($$$, Serialize, $$$)]"));
}

#[test]
fn trace_template_eprintln() {
  let t = RustTraceTemplate.template_for("x").expect("template");
  assert!(t.contains("eprintln!"));
  assert!(t.contains("[TRACE]"));
}

#[test]
fn inheritance_subtype_impl_for() {
  let p = RustInheritanceGrammar.subtype_patterns("MyTrait");
  assert!(p.iter().any(|s| s.contains("impl MyTrait for $T")));
}

#[test]
fn call_grammar_standard_shapes() {
  let p = RustCallGrammar.call_patterns("foo");
  assert!(p.iter().any(|s| s == "foo($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.foo($$$ARGS)"));
}

#[test]
fn fixture_file_exists() {
  // The in-crate fixture is reachable from this smoke binary
  // via CARGO_MANIFEST_DIR. Used by future cross-lang smoke
  // harnesses that pin against per-language fixtures.
  let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("fixtures")
    .join("lib.rs");
  assert!(
    path.exists(),
    "fixtures/lib.rs must exist for smoke harnesses"
  );
}
