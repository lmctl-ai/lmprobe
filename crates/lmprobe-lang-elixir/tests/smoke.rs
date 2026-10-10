//! Smoke + Phase 6C #10 import-pattern audit for `lmprobe-lang-elixir`.

use lmprobe_lang_core::langs::ImportGrammar;
use lmprobe_lang_elixir::{
  ElixirCallGrammar, ElixirDecorationGrammar, ElixirExtractor, ElixirImportGrammar,
  ElixirInheritanceGrammar, ElixirKindTaxonomy, ElixirTopLevelKinds, ElixirTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = ElixirExtractor;
  let _ = ElixirCallGrammar;
  let _ = ElixirImportGrammar;
  let _ = ElixirInheritanceGrammar;
  let _ = ElixirDecorationGrammar;
  let _ = ElixirTraceTemplate;
  let _ = ElixirKindTaxonomy;
  let _ = ElixirTopLevelKinds;
}

/// Phase 6C #10: audit the import-pattern set covers the five
/// shapes (4 base macros + 1 group form). Lenient matching on
/// the call's first argument means `Foo, opt: val` and
/// Phase 6C round-3: round-1's narrow patterns dropped — broad
/// `$X` patterns + engine `needs_prefix_match` filter cover both
/// exact-match and prefix-match. Group form retained.
#[test]
fn import_patterns_cover_phase_6c_shapes() {
  let pats = ElixirImportGrammar.import_patterns("Foo");
  assert_eq!(pats.len(), 5, "expected 5 patterns, got {pats:?}");
  assert!(pats.iter().any(|p| p == "alias Foo.{$$$}"), "group");
  assert!(pats.iter().any(|p| p == "import $X"), "broad import");
  assert!(pats.iter().any(|p| p == "alias $X"), "broad alias");
  assert!(pats.iter().any(|p| p == "use $X"), "broad use");
  assert!(pats.iter().any(|p| p == "require $X"), "broad require");
}

#[test]
fn needs_prefix_match_is_true_round3() {
  // Phase 6C round-3: Elixir opts into the engine's content
  // filter to enable bare-root prefix-match.
  assert!(ElixirImportGrammar.needs_prefix_match());
}

/// Multi-segment dotted module names: round-3 group pattern
/// preserves the `{$$$}` shape. The exact-name patterns dropped;
/// the engine's filter handles full-path queries via the broad
/// `$X` patterns + content match.
#[test]
fn import_patterns_handle_multi_segment_names() {
  let pats = ElixirImportGrammar.import_patterns("Plug.Conn.Unfetched");
  assert!(pats.iter().any(|p| p == "alias Plug.Conn.Unfetched.{$$$}"));
  // Broad patterns are name-independent.
  assert!(pats.iter().any(|p| p == "alias $X"));
}
