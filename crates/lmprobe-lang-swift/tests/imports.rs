//! Phase 6C #9 — in-process integration test for Swift's
//! attributed (`@testable`, `@_spi(...)`, `@_implementationOnly`)
//! and kind-qualified (`import struct Foundation.Data` etc.) import
//! shapes. Exercises `SwiftImportGrammar::import_patterns` end-to-
//! end through ast-grep (the same path the engine's `imports()`
//! sensor takes), without taking a dev-dep on `lmprobe-engine`
//! (which would create a cycle since the engine depends on this
//! crate via the registry).
//!
//! Mirrors the C / C++ / C# / PHP / Rust / Solidity / Nix / HCL /
//! Scala precedents (43f0e340 / d2496bd4 / 919a38b9 / f424a1ae /
//! 35a91997 / e48fcb42 / 77624641 / 14d3d9d1 / 738d3424). The
//! 5-marker engine-level verification (NOT_IMPLEMENTED gone, cross-
//! lang walk, GraphQL parity) runs against the binary and the qa1
//! fixtures.
//!
//! Doctrine for Swift:
//! - `import {name}` — canonical module import.
//! - `@testable import {name}`, `@_spi($SPI) import {name}`,
//!   `@_implementationOnly import {name}` — attributed imports.
//! - `import {kind} {name}` for kind ∈ struct / class / func / enum
//!   / protocol / typealias / let / var — kind-qualified sub-symbol
//!   imports (`import struct Foundation.Data` imports the struct
//!   `Data` from the `Foundation` module). Anchor = the dotted
//!   identifier path; the kind keyword is metadata, not the anchor.
//! - No wildcards (Swift has no `import X.*`-equivalent).
//! - No aliasing (Swift has no `import X as Y`).

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_swift::SwiftImportGrammar;

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
fn patterns_non_empty() {
  let p = SwiftImportGrammar.import_patterns("Foundation");
  assert!(
    !p.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  assert_eq!(p.len(), 12, "expected 12 doctrinal shapes");
}

#[test]
fn canonical_bare_import() {
  let content = "import Foundation\n";
  let p = SwiftImportGrammar.import_patterns("Foundation");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    1,
    "bare canonical import hits exactly once"
  );
}

#[test]
fn testable_attributed_import() {
  let content = "@testable import App\n";
  let p = SwiftImportGrammar.import_patterns("App");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    1,
    "@testable import hits exactly once"
  );
}

#[test]
fn spi_attributed_import() {
  let content = "@_spi(WebSocket) import Alamofire\n";
  let p = SwiftImportGrammar.import_patterns("Alamofire");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    1,
    "@_spi(...) import hits exactly once"
  );
}

#[test]
fn implementation_only_attributed_import() {
  let content = "@_implementationOnly import Bar\n";
  let p = SwiftImportGrammar.import_patterns("Bar");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    1,
    "@_implementationOnly import hits exactly once"
  );
}

#[test]
fn kind_qualified_struct_import() {
  let content = "import struct Foundation.Data\n";
  let p = SwiftImportGrammar.import_patterns("Foundation.Data");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    1,
    "import struct X.Y hits exactly once when anchored on X.Y"
  );
}

#[test]
fn kind_qualified_class_func_enum_protocol_typealias() {
  // Each kind keyword is structurally distinct in tree-sitter-swift,
  // so each requires its own pattern. Anchor on the dotted identifier.
  let cases = [
    ("import class UIKit.UIView\n", "UIKit.UIView"),
    ("import func Foundation.print\n", "Foundation.print"),
    ("import enum Foundation.Decimal\n", "Foundation.Decimal"),
    (
      "import protocol Foundation.Sequence\n",
      "Foundation.Sequence",
    ),
    ("import typealias Foundation.URL\n", "Foundation.URL"),
  ];
  for (content, name) in cases {
    let p = SwiftImportGrammar.import_patterns(name);
    assert_eq!(
      count_matches(content, &p, SgLang::Swift),
      1,
      "kind-qualified import {name:?} hits exactly once"
    );
  }
}

#[test]
fn no_match_unrelated_module() {
  let content = "import Foundation\nimport Combine\n";
  let p = SwiftImportGrammar.import_patterns("UIKit");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    0,
    "no false positives for unrelated module"
  );
}

#[test]
fn no_match_in_comment_or_string() {
  let content = r#"// import Foundation
let s = "import Foundation"
"#;
  let p = SwiftImportGrammar.import_patterns("Foundation");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    0,
    "comment- and string-embedded text not matched"
  );
}

#[test]
fn lenient_module_anchor_matches_kind_qualified() {
  // When the user queries the MODULE name and the file has kind-
  // qualified imports of sub-symbols from that module, those count
  // as imports of the module — ast-grep's lenient sibling matching
  // means `import struct Foundation` matches `import struct
  // Foundation.Data`. One hit per source line, no per-line dup.
  let content = r#"import Foundation
import struct Foundation.Data
import enum Foundation.Decimal
"#;
  let p = SwiftImportGrammar.import_patterns("Foundation");
  assert_eq!(
    count_matches(content, &p, SgLang::Swift),
    3,
    "module anchor hits bare + 2 kind-qualified sub-symbol imports"
  );
}

#[test]
fn real_qa1_fixture_alamofire_attributed() {
  // Alamofire's Tests/AuthenticationInterceptorTests.swift uses
  // `@testable import Alamofire` — exercises the attributed path
  // end-to-end against a real-world fixture.
  // Skipped unless `LMPROBE_QA_CORPUS` points at a qa1 corpus checkout.
  let Some(root) = std::env::var_os("LMPROBE_QA_CORPUS") else {
    eprintln!("skipping: LMPROBE_QA_CORPUS is not set");
    return;
  };
  let path =
    std::path::Path::new(&root).join("swift/Alamofire/Tests/AuthenticationInterceptorTests.swift");
  let Ok(content) = std::fs::read_to_string(&path) else {
    eprintln!("skipping: {} is missing", path.display());
    return;
  };
  let p = SwiftImportGrammar.import_patterns("Alamofire");
  assert_eq!(
    count_matches(&content, &p, SgLang::Swift),
    1,
    "single @testable import Alamofire on line 25"
  );
}

#[test]
fn all_patterns_parse() {
  // Smoke that every pattern shape `SwiftImportGrammar` emits parses
  // through `Pattern::try_new` for representative anchor strings.
  for name in &["Foundation", "App", "Foundation.Data", "UIKit.UIView"] {
    for pat in SwiftImportGrammar.import_patterns(name) {
      Pattern::try_new(&pat, SgLang::Swift)
        .unwrap_or_else(|e| panic!("pattern {pat:?} failed to parse: {e:?}"));
    }
  }
}
