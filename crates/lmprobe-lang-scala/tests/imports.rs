//! Phase 6C #8 — in-process integration test for Scala `import`
//! patterns. Exercises `ScalaImportGrammar::import_patterns`
//! end-to-end through ast-grep (the same path the engine's
//! `imports()` sensor takes), without taking a dev-dep on
//! `lmprobe-engine` (which would create a cycle since the engine
//! depends on this crate via the registry).
//!
//! Pins the per-crate contract:
//! 1. Single pattern matches every Scala `import` shape (single,
//!    multi-segment, wildcard, group, Scala 2 alias `=>`, Scala 3
//!    alias `as`) — one hit per source line.
//! 2. NO duplicate hits per source line (the pre-broadening 3-pattern
//!    set emitted 2× per line because the bare pattern lenient-matched
//!    the wildcard / `$$$` forms).
//! 3. Doctrine: module = anchor; aliases / wildcards / selectors are
//!    metadata captured by the trailing siblings.
//!
//! The 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk, GraphQL parity) runs against the binary in
//! `crates/lmprobe/tests/*` and the qa1 fixtures.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_scala::ScalaImportGrammar;
use std::collections::HashSet;
use std::fs;

/// Total match count across all patterns for `name` in `content`.
/// Mirrors the engine's per-pattern emit loop in `sensors::imports`.
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

/// Distinct (line_start, text) pairs across all pattern matches —
/// what the engine SHOULD produce after dedupe (currently doesn't, but
/// the single-pattern design ensures total == distinct). Used to
/// assert no duplicate hits per source line.
fn distinct_match_lines(content: &str, patterns: &[String], lang: SgLang) -> usize {
  let grep = lang.ast_grep(content);
  let root_node = grep.root();
  let mut seen: HashSet<(usize, String)> = HashSet::new();
  for pattern_str in patterns {
    let pat = Pattern::try_new(pattern_str, lang).expect("pattern parses");
    for m in root_node.find_all(pat) {
      seen.insert((m.start_pos().line(), m.text().to_string()));
    }
  }
  seen.len()
}

#[test]
fn single_pattern_no_dedupe_gap() {
  // Phase 6C #8 root cause: tree-sitter-scala's flat sibling tree
  // + ast-grep lenient-matching meant 3 patterns emitted 2× hits
  // per source line. The fix is collapsing to one pattern; this
  // asserts the contract.
  let p = ScalaImportGrammar.import_patterns("scopt");
  assert_eq!(
    p.len(),
    1,
    "single-pattern design closes the dedupe gap; \
         additional patterns reintroduce the 2× over-count"
  );
  assert_eq!(p[0], "import scopt");
}

#[test]
fn plain_single_and_multi_segment() {
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("a.scala");
  fs::write(
    &f,
    "import scopt\n\
         import scopt.OParser\n\
         import scala.collection.mutable\n\
         import a.b.c.X\n\
         class X {}\n",
  )
  .expect("write a.scala");
  let content = fs::read_to_string(&f).unwrap();

  // Single-segment.
  let p = ScalaImportGrammar.import_patterns("scopt");
  assert!(
    !p.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  // `import scopt` matches `import scopt` AND prefix-matches
  // `import scopt.OParser` per doctrine (module = anchor).
  assert_eq!(count_matches(&content, &p, SgLang::Scala), 2);

  // Two-segment dotted path.
  let p = ScalaImportGrammar.import_patterns("scopt.OParser");
  assert_eq!(count_matches(&content, &p, SgLang::Scala), 1);

  // Three-segment deep path.
  let p = ScalaImportGrammar.import_patterns("scala.collection.mutable");
  assert_eq!(count_matches(&content, &p, SgLang::Scala), 1);

  // Four-segment deep path.
  let p = ScalaImportGrammar.import_patterns("a.b.c.X");
  assert_eq!(count_matches(&content, &p, SgLang::Scala), 1);
}

#[test]
fn wildcard_form_matches_via_prefix() {
  // `import a.b.c._` — wildcard. Per doctrine, module = anchor,
  // so `imports a.b.c` matches; the `_` is captured as metadata.
  let content = "import a.b.c._\nclass X {}\n";
  let p = ScalaImportGrammar.import_patterns("a.b.c");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "wildcard `import a.b.c._` matches via prefix"
  );
}

#[test]
fn group_selector_form_matches_via_prefix() {
  // `import a.b.c.{X, Y}` — group selector. Per doctrine, one hit
  // per statement, module = anchor.
  let content = "import a.b.c.{X, Y}\nclass X {}\n";
  let p = ScalaImportGrammar.import_patterns("a.b.c");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "group `import a.b.c.{{X, Y}}` matches via prefix"
  );
}

#[test]
fn scala2_alias_form_captures_imported_target() {
  // `import a.b.c.{X => Y}` — Scala 2 rename. The IMPORTED thing
  // is `a.b.c`; the rename `X => Y` is metadata inside the
  // `namespace_selectors` sibling.
  let content = "import a.b.c.{X => Y}\nclass X {}\n";
  let p = ScalaImportGrammar.import_patterns("a.b.c");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "Scala 2 alias `import a.b.c.{{X => Y}}` matches via prefix"
  );
}

#[test]
fn scala3_alias_form_captures_imported_target() {
  // `import a.b.c.X as Y` — Scala 3 rename keyword. Tree-sitter
  // emits `as_renamed_identifier` as a sibling of the
  // `identifier`s; lenient-matching skips it.
  let content = "import a.b.c.X as Y\nclass X {}\n";
  let p = ScalaImportGrammar.import_patterns("a.b.c");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "Scala 3 alias `import a.b.c.X as Y` matches via prefix"
  );
}

#[test]
fn no_falsematch_on_unrelated_prefix() {
  // `import other.unrelated` should NOT match `imports scopt` —
  // first identifier differs.
  let content = "import other.unrelated\nimport scopt\nclass X {}\n";
  let p = ScalaImportGrammar.import_patterns("scopt");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    1,
    "only the `import scopt` line should match"
  );
}

#[test]
fn no_falsematch_in_comment_or_string() {
  let content = "// import a.b.c.X\n\
                   val s = \"import a.b.c.X\"\n\
                   class X {}\n";
  let p = ScalaImportGrammar.import_patterns("a.b.c.X");
  assert_eq!(
    count_matches(content, &p, SgLang::Scala),
    0,
    "should not match `import a.b.c.X` in a comment or string"
  );
}

#[test]
fn no_duplicate_hits_per_source_line() {
  // The 2× dedupe gap: pre-fix, `import scopt.OParser` was emitted
  // twice (once via `import {name}`, once via `import {name}.$$$`).
  // After the fix, total matches == distinct (line, text) pairs.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("AllForms.scala");
  fs::write(
    &f,
    "import scopt\n\
         import scopt.OParser\n\
         import scopt._\n\
         import scopt.{X, Y}\n\
         import scopt.{X => Y}\n\
         import scopt.OEffect._\n\
         class App {}\n",
  )
  .expect("write");
  let content = fs::read_to_string(&f).unwrap();

  let p = ScalaImportGrammar.import_patterns("scopt");
  let total = count_matches(&content, &p, SgLang::Scala);
  let distinct = distinct_match_lines(&content, &p, SgLang::Scala);
  assert_eq!(
    total, distinct,
    "no duplicate hits per source line (was 2× pre-fix)"
  );
  // 6 import lines, all anchored on `scopt`.
  assert_eq!(total, 6, "exactly one hit per import statement");
}

#[test]
fn all_shapes_in_one_file_one_hit_each() {
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("AllShapes.scala");
  fs::write(
    &f,
    "import a.b.c.X\n\
         import a.b.c.{X, Y}\n\
         import a.b.c._\n\
         import a.b.c.{X => Y}\n\
         import a.b.c.X as Z\n\
         import other.unrelated\n\
         class App {}\n",
  )
  .expect("write");
  let content = fs::read_to_string(&f).unwrap();

  // `a.b.c` prefix matches the first 5 lines; the 6th has a
  // different first segment.
  let p = ScalaImportGrammar.import_patterns("a.b.c");
  assert_eq!(count_matches(&content, &p, SgLang::Scala), 5);
}
