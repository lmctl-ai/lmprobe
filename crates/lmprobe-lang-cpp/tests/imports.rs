//! Phase 6C #1 (C++ half) — in-process integration test for C++
//! `#include` import patterns. Exercises
//! `CppImportGrammar::import_patterns` end-to-end through ast-grep
//! (the same path the engine's `imports()` sensor takes), without
//! taking a dev-dep on `lmprobe-engine` (which would create a cycle
//! since the engine depends on this crate via the registry).
//!
//! The 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk regression, GraphQL parity) runs against the
//! binary in `crates/lmprobe/tests/*` and the qa1 fixtures. This
//! test pins the per-crate contract: patterns produce ≥2 matches
//! against a fixture containing both `#include <hdr>` and
//! `#include "hdr"` lines.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_cpp::CppImportGrammar;
use std::fs;

fn count_matches(content: &str, patterns: &[String], lang: SgLang) -> usize {
  let grep = lang.ast_grep(content);
  let root_node = grep.root();
  let mut total = 0;
  for pattern_str in patterns {
    let pat = Pattern::try_new(pattern_str, lang).expect("pattern parses");
    total += root_node.find_all(pat).count();
  }
  total
}

#[test]
fn include_patterns_match_both_shapes_in_tempdir() {
  let dir = tempfile::tempdir().expect("tempdir");
  let a = dir.path().join("a.cpp");
  let b = dir.path().join("b.cpp");
  fs::write(&a, "#include <iostream>\nint main() { return 0; }\n").expect("write a.cpp");
  fs::write(&b, "#include \"local.hpp\"\nvoid foo() {}\n").expect("write b.cpp");

  // Marker 1 (canonical-form parse): patterns parse + match the
  // angle-bracket form against a real `.cpp` file.
  let iostream_patterns = CppImportGrammar.import_patterns("iostream");
  assert!(
    !iostream_patterns.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  let a_content = fs::read_to_string(&a).unwrap();
  assert_eq!(
    count_matches(&a_content, &iostream_patterns, SgLang::Cpp),
    1,
    "a.cpp should match exactly one #include <iostream>",
  );

  // Local-header form against b.cpp.
  let local_patterns = CppImportGrammar.import_patterns("local.hpp");
  let b_content = fs::read_to_string(&b).unwrap();
  assert_eq!(
    count_matches(&b_content, &local_patterns, SgLang::Cpp),
    1,
    "b.cpp should match exactly one #include \"local.hpp\"",
  );

  // Combined ≥2 hit assertion across both fixtures.
  let total = count_matches(&a_content, &iostream_patterns, SgLang::Cpp)
    + count_matches(&b_content, &local_patterns, SgLang::Cpp);
  assert!(
    total >= 2,
    "expected ≥2 hits across a.cpp+b.cpp, got {total}"
  );
}

#[test]
fn include_patterns_dont_falsematch_unrelated_text() {
  // Confirm the patterns are anchored to `#include` and don't grab
  // the bare header name appearing in a comment or string literal.
  let content = r#"
// iostream is great
const char *msg = "iostream";
int x = 1;
"#;
  let patterns = CppImportGrammar.import_patterns("iostream");
  assert_eq!(
    count_matches(content, &patterns, SgLang::Cpp),
    0,
    "should not match iostream in comment or string literal",
  );
}

#[test]
fn include_patterns_match_subdir_paths() {
  // Common C++ pattern: `#include <sys/types.h>` or project-local
  // nested headers like `#include "core/widget.hpp"`.
  let content = "#include <sys/types.h>\n";
  let patterns = CppImportGrammar.import_patterns("sys/types.h");
  assert_eq!(
    count_matches(content, &patterns, SgLang::Cpp),
    1,
    "subdir header path should match",
  );
}
