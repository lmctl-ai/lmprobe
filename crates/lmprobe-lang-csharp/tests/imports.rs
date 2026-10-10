//! Phase 6C #2 — in-process integration test for C# `using` import
//! patterns. Exercises `CSharpImportGrammar::import_patterns`
//! end-to-end through ast-grep (the same path the engine's
//! `imports()` sensor takes), without taking a dev-dep on
//! `lmprobe-engine` (which would create a cycle since the engine
//! depends on this crate via the registry).
//!
//! The 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk regression, GraphQL parity) runs against the
//! binary in `crates/lmprobe/tests/*` and the qa1 fixtures. This
//! test pins the per-crate contract: each of the 5 `using` shapes
//! produces ≥1 match against a representative fixture.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_csharp::CSharpImportGrammar;
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
fn plain_using_single_and_multi_segment() {
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("a.cs");
  fs::write(
    &f,
    "using System;\nusing System.IO;\nusing A.B.C.D;\nclass X {}\n",
  )
  .expect("write a.cs");
  let content = fs::read_to_string(&f).unwrap();

  // Single-segment.
  let p_system = CSharpImportGrammar.import_patterns("System");
  assert!(
    !p_system.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  assert_eq!(
    count_matches(&content, &p_system, SgLang::CSharp),
    1,
    "exactly one `using System;`"
  );

  // Multi-segment dotted path.
  let p_io = CSharpImportGrammar.import_patterns("System.IO");
  assert_eq!(
    count_matches(&content, &p_io, SgLang::CSharp),
    1,
    "exactly one `using System.IO;`"
  );

  // Deep multi-segment.
  let p_deep = CSharpImportGrammar.import_patterns("A.B.C.D");
  assert_eq!(
    count_matches(&content, &p_deep, SgLang::CSharp),
    1,
    "exactly one `using A.B.C.D;`"
  );
}

#[test]
fn static_using_matches_static_form() {
  let content = "using static System.Math;\nclass X { double t = Sqrt(2.0); }\n";
  let patterns = CSharpImportGrammar.import_patterns("System.Math");
  // The `using static {name};` shape should match here. The plain
  // `using {name};` shape should NOT (different AST structure).
  let n = count_matches(content, &patterns, SgLang::CSharp);
  assert_eq!(
    n, 1,
    "exactly one match for `using static System.Math;` across the 5 shapes (got {n})"
  );
}

#[test]
fn alias_using_captures_imported_target_not_alias_name() {
  // `using Foo = Bar.Baz;` — per doctrine, the IMPORTED thing is
  // `Bar.Baz`. Querying `imports Bar.Baz` should match via the RHS
  // alias pattern.
  let content = "using LinqEnumerable = System.Linq.Enumerable;\nclass X {}\n";
  let p_target = CSharpImportGrammar.import_patterns("System.Linq.Enumerable");
  assert_eq!(
    count_matches(content, &p_target, SgLang::CSharp),
    1,
    "RHS alias form should match the imported target (System.Linq.Enumerable)"
  );

  // `imports LinqEnumerable` (alias name) should also match via the
  // LHS alias pattern — alias-as-symbol is supported.
  let p_alias = CSharpImportGrammar.import_patterns("LinqEnumerable");
  assert_eq!(
    count_matches(content, &p_alias, SgLang::CSharp),
    1,
    "LHS alias form should match the alias name (LinqEnumerable)"
  );
}

#[test]
fn global_using_matches_via_plain_pattern() {
  // C# 10+ `global using` is treated by tree-sitter-csharp as a
  // `using_directive` node with an outer `global` modifier. The
  // plain `using {name};` pattern matches it without needing a
  // dedicated `global using` shape.
  let content = "global using System;\nclass X {}\n";
  let patterns = CSharpImportGrammar.import_patterns("System");
  assert_eq!(
    count_matches(content, &patterns, SgLang::CSharp),
    1,
    "exactly one match for `global using System;` via plain shape"
  );
}

#[test]
fn no_falsematch_in_comment_or_string() {
  let content = r#"
// using System.IO; — this is a comment, not an import
class X { string s = "using System.IO;"; }
"#;
  let patterns = CSharpImportGrammar.import_patterns("System.IO");
  assert_eq!(
    count_matches(content, &patterns, SgLang::CSharp),
    0,
    "should not match `using System.IO;` in comment or string literal"
  );
}

#[test]
fn all_five_shapes_in_one_file() {
  // A single file with all 5 `using` forms — the patterns produced
  // for a name appearing in the canonical position should match
  // exactly once per shape's slot.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("AllForms.cs");
  fs::write(
    &f,
    r#"using System;
using static System.Math;
using LinqEnumerable = System.Linq.Enumerable;
using TheBaz = Bar.Baz;
global using System.Collections.Generic;

class X {}
"#,
  )
  .expect("write");
  let content = fs::read_to_string(&f).unwrap();

  // `System` → plain `using System;` (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &CSharpImportGrammar.import_patterns("System"),
      SgLang::CSharp
    ),
    1,
    "plain `using System;`"
  );
  // `System.Math` → static form (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &CSharpImportGrammar.import_patterns("System.Math"),
      SgLang::CSharp
    ),
    1,
    "static `using System.Math;`"
  );
  // `System.Linq.Enumerable` → RHS alias (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &CSharpImportGrammar.import_patterns("System.Linq.Enumerable"),
      SgLang::CSharp
    ),
    1,
    "alias RHS `System.Linq.Enumerable`"
  );
  // `TheBaz` → LHS alias (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &CSharpImportGrammar.import_patterns("TheBaz"),
      SgLang::CSharp
    ),
    1,
    "alias LHS `TheBaz`"
  );
  // `System.Collections.Generic` → global form (1 hit).
  assert_eq!(
    count_matches(
      &content,
      &CSharpImportGrammar.import_patterns("System.Collections.Generic"),
      SgLang::CSharp
    ),
    1,
    "global `using System.Collections.Generic;`"
  );
}
