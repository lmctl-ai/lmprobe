//! Phase 6C #3 — in-process integration test for PHP `use` import
//! patterns. Exercises `PhpImportGrammar::import_patterns`
//! end-to-end through ast-grep (the same path the engine's
//! `imports()` sensor takes), without taking a dev-dep on
//! `lmprobe-engine` (which would create a cycle since the engine
//! depends on this crate via the registry).
//!
//! The 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk regression, GraphQL parity) runs against the
//! binary in `crates/lmprobe/tests/*` and the qa1 fixtures. This
//! test pins the per-crate contract: each `use`-directive shape
//! produces ≥1 match against a representative fixture.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_php::PhpImportGrammar;
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
fn bare_root_prefix_match_round2() {
  // Phase 6C round-2 (TC-103): `imports Foo` matches every `use`
  // statement whose namespace begins with `Foo`. Round-1 only
  // matched the exact `use Foo;`; round-2 walks segments via
  // prefix-match.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("a.php");
  fs::write(
    &f,
    "<?php\nuse Foo;\nuse Foo\\Bar;\nuse Foo\\Bar\\Baz;\nclass X {}\n",
  )
  .expect("write a.php");
  let content = fs::read_to_string(&f).unwrap();

  // Bare-root: matches Foo, Foo\Bar, Foo\Bar\Baz → 3 hits.
  let p_foo = PhpImportGrammar.import_patterns("Foo");
  assert!(!p_foo.is_empty());
  assert_eq!(
    count_matches(&content, &p_foo, SgLang::Php),
    3,
    "round-2: `imports Foo` matches Foo, Foo\\Bar, Foo\\Bar\\Baz"
  );

  // Two-segment: matches Foo\Bar, Foo\Bar\Baz → 2 hits.
  let p_bar = PhpImportGrammar.import_patterns("Foo\\Bar");
  assert_eq!(
    count_matches(&content, &p_bar, SgLang::Php),
    2,
    "round-2: `imports Foo\\Bar` matches Foo\\Bar, Foo\\Bar\\Baz"
  );

  // Three-segment (full path): matches only Foo\Bar\Baz → 1 hit.
  let p_baz = PhpImportGrammar.import_patterns("Foo\\Bar\\Baz");
  assert_eq!(
    count_matches(&content, &p_baz, SgLang::Php),
    1,
    "exact match: `imports Foo\\Bar\\Baz`"
  );
}

#[test]
fn alias_use_captures_imported_target() {
  // `use Foo\Bar as Quux;` — per doctrine, the IMPORTED thing is
  // `Foo\Bar`. Querying `imports Foo\Bar` should match via the
  // alias pattern.
  let content = "<?php\nuse Foo\\Bar as Quux;\nclass X {}\n";
  let p_target = PhpImportGrammar.import_patterns("Foo\\Bar");
  assert_eq!(
    count_matches(content, &p_target, SgLang::Php),
    1,
    "alias form should match the imported target (Foo\\Bar)"
  );
}

#[test]
fn function_and_const_imports() {
  let content = "<?php\nuse function array_merge;\nuse const PHP_VERSION;\nclass X {}\n";

  // `use function array_merge;`
  let p_fn = PhpImportGrammar.import_patterns("array_merge");
  assert_eq!(
    count_matches(content, &p_fn, SgLang::Php),
    1,
    "exactly one `use function array_merge;`"
  );

  // `use const PHP_VERSION;`
  let p_const = PhpImportGrammar.import_patterns("PHP_VERSION");
  assert_eq!(
    count_matches(content, &p_const, SgLang::Php),
    1,
    "exactly one `use const PHP_VERSION;`"
  );
}

#[test]
fn group_use_anchored_to_module_prefix() {
  // `use Foo\{Bar, Baz};` — group-import. Doctrine: one hit per
  // `use` statement, module = anchor. Query `imports Foo` should
  // match the prefix.
  let content = "<?php\nuse Foo\\{Bar, Baz};\nclass X {}\n";
  let p = PhpImportGrammar.import_patterns("Foo");
  assert_eq!(
    count_matches(content, &p, SgLang::Php),
    1,
    "exactly one match for `use Foo\\{{Bar, Baz}};` via prefix"
  );
}

#[test]
fn no_falsematch_in_comment_or_string() {
  let content = "<?php\n// use Foo\\Bar;\n$s = \"use Foo\\Bar;\";\nclass X {}\n";
  let patterns = PhpImportGrammar.import_patterns("Foo\\Bar");
  assert_eq!(
    count_matches(content, &patterns, SgLang::Php),
    0,
    "should not match `use Foo\\Bar;` in comment or string literal"
  );
}

#[test]
fn all_shapes_in_one_file() {
  // A single file with every `use` shape — each name in the
  // canonical position should match exactly once across the
  // pattern set.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("AllForms.php");
  fs::write(
    &f,
    "<?php
use Foo\\Bar;
use Foo\\Bar\\Baz as Quux;
use function some_fn;
use const SOME_CONST;
use Grouped\\{X, Y};

class X {}
",
  )
  .expect("write");
  let content = fs::read_to_string(&f).unwrap();

  // Round-2 prefix-match:
  // `Foo\Bar` matches plain `use Foo\Bar;` AND alias `use Foo\Bar\Baz as Quux;`
  // (the alias target's prefix is Foo\Bar) → 2 hits.
  assert_eq!(
    count_matches(
      &content,
      &PhpImportGrammar.import_patterns("Foo\\Bar"),
      SgLang::Php
    ),
    2,
    "round-2 prefix: `use Foo\\Bar;` + alias `use Foo\\Bar\\Baz as Quux;`"
  );
  // `Foo\Bar\Baz` → exact alias target (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &PhpImportGrammar.import_patterns("Foo\\Bar\\Baz"),
      SgLang::Php
    ),
    1,
    "alias `use Foo\\Bar\\Baz as Quux;`"
  );
  // `some_fn` → function (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &PhpImportGrammar.import_patterns("some_fn"),
      SgLang::Php
    ),
    1,
    "`use function some_fn;`"
  );
  // `SOME_CONST` → const (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &PhpImportGrammar.import_patterns("SOME_CONST"),
      SgLang::Php
    ),
    1,
    "`use const SOME_CONST;`"
  );
  // `Grouped` → group prefix (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &PhpImportGrammar.import_patterns("Grouped"),
      SgLang::Php
    ),
    1,
    "`use Grouped\\{{X, Y}};` via prefix"
  );
}
