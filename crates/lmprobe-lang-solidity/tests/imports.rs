//! Phase 6C #5 — in-process integration test for Solidity `import`
//! patterns. Exercises `SolidityImportGrammar::import_patterns`
//! end-to-end through ast-grep (the same path the engine's
//! `imports()` sensor takes), without taking a dev-dep on
//! `lmprobe-engine` (which would create a cycle since the engine
//! depends on this crate via the registry).
//!
//! Mirrors the C / C++ / C# / PHP / Rust precedents
//! (43f0e340 / d2496bd4 / 919a38b9 / f424a1ae / 35a91997). The
//! 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk, GraphQL parity) runs against the binary in
//! `crates/lmprobe/tests/*` and the qa1 fixtures. This test pins
//! the per-crate contract: each `import`-directive shape produces
//! ≥1 match against a representative fixture.
//!
//! Doctrine for Solidity: in `import "<path>";` and friends,
//! `{name}` is the PATH STRING (e.g. `./Foo.sol`), not the
//! imported symbol. The path is the module identity.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_solidity::SolidityImportGrammar;
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
fn path_form_simple() {
  // `import "./Foo.sol";` — bare path-form import. Doctrine:
  // module identity is the path string.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import \"./Foo.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Foo.sol");
  assert!(
    !p.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one `import \"./Foo.sol\";`"
  );
}

#[test]
fn path_form_with_alias() {
  // `import "./Foo.sol" as Foo;` — path-form with alias. The
  // imported module is the path; the alias is metadata.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import \"./Foo.sol\" as Foo;\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Foo.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one match for `import \"./Foo.sol\" as Foo;`"
  );
}

#[test]
fn named_form_single_symbol() {
  // `import {Bar} from "./Baz.sol";` — named import.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import {Bar} from \"./Baz.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Baz.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one `import {{Bar}} from \"./Baz.sol\";`"
  );
}

#[test]
fn named_form_multi_symbol() {
  // `import {A, B, C} from "./Baz.sol";` — multi-symbol named
  // import. Doctrine: 1 hit per statement, module = anchor.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import {A, B, C} from \"./Baz.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Baz.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one match for multi-symbol named import"
  );
}

#[test]
fn named_form_with_per_symbol_alias() {
  // `import {Bar as Quux} from "./Baz.sol";` — per-symbol alias.
  // Doctrine: capture imported, alias is metadata. Querying the
  // path matches via the named pattern.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import {Bar as Quux} from \"./Baz.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Baz.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one match for per-symbol alias named import"
  );
}

#[test]
fn namespace_wildcard_form() {
  // `import * as Foo from "./Bar.sol";` — namespace import.
  // Doctrine: capture; module = anchor (path string).
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import * as Foo from \"./Bar.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("./Bar.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    1,
    "exactly one match for `import * as Foo from \"./Bar.sol\";`"
  );
}

#[test]
fn multi_segment_path() {
  // `import "@openzeppelin/contracts/token/ERC20/IERC20.sol";`
  // — npm-style scoped path. Doctrine: path is the module.
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import \"@openzeppelin/contracts/token/ERC20/IERC20.sol\";\n\
        import {IERC20} from \"@openzeppelin/contracts/token/ERC20/IERC20.sol\";\n\
        contract X {}\n";
  let p = SolidityImportGrammar.import_patterns("@openzeppelin/contracts/token/ERC20/IERC20.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    2,
    "should match both path-form and named-form on multi-segment path"
  );
}

#[test]
fn no_falsematch_in_comment_or_string() {
  let content = "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        // import \"./Foo.sol\";\n\
        contract X { string s = \"import \\\"./Foo.sol\\\";\"; }\n";
  let p = SolidityImportGrammar.import_patterns("./Foo.sol");
  assert_eq!(
    count_matches(content, &p, SgLang::Solidity),
    0,
    "should not match `import \"./Foo.sol\";` in comment or string"
  );
}

#[test]
fn all_shapes_in_one_file() {
  // A single file with every doctrinal `import` shape — the
  // pattern set must produce at least one hit per queried path.
  //
  // PRECISION CAVEAT: ast-grep treats string literals as opaque
  // when a pattern contains metavariables (e.g. `$$$`, `$ALIAS`) —
  // i.e. the SHAPE is matched but the path-string content is
  // ignored. This means querying `./Plain.sol` against the named-
  // form pattern `import {$$$} from "{name}";` ALSO matches every
  // other `{X} from "Y";` line in the same file regardless of `Y`.
  // The first pattern (no metavar) does respect string content.
  //
  // This was a pre-existing characteristic of the original 3-arm
  // lang-core grammar (its pattern 3 had the same behavior). The
  // engine does not post-filter by content. A precision-tightening
  // fix would live in the engine's `imports()` sensor (post-match
  // `m.text().contains(name)`), not here.
  //
  // Per-statement contract: every shape produces ≥1 structural
  // match for any path that appears in the query. The strict-1
  // assertion holds for SINGLE-shape files (the per-shape tests
  // above), not the mixed file below.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("AllForms.sol");
  fs::write(
    &f,
    "// SPDX-License-Identifier: MIT\n\
        pragma solidity ^0.8.0;\n\
        import \"./Plain.sol\";\n\
        import \"./Aliased.sol\" as Aliased;\n\
        import {Named} from \"./Named.sol\";\n\
        import {A as B} from \"./Renamed.sol\";\n\
        import * as Star from \"./Star.sol\";\n\
        contract X {}\n",
  )
  .expect("write");
  let content = fs::read_to_string(&f).unwrap();

  for path in [
    "./Plain.sol",
    "./Aliased.sol",
    "./Named.sol",
    "./Renamed.sol",
    "./Star.sol",
  ] {
    let p = SolidityImportGrammar.import_patterns(path);
    let n = count_matches(&content, &p, SgLang::Solidity);
    assert!(
      n >= 1,
      "expected ≥1 match for `{path}` across all shapes, got {n}"
    );
  }
}
