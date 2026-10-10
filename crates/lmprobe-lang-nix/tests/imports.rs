//! Phase 6C #6 — in-process integration test for Nix `import` /
//! `with` / `inherit` patterns. Exercises
//! `NixImportGrammar::import_patterns` end-to-end through ast-grep
//! (the same path the engine's `imports()` sensor takes), without
//! taking a dev-dep on `lmprobe-engine` (which would create a cycle
//! since the engine depends on this crate via the registry).
//!
//! Mirrors the C / C++ / C# / PHP / Rust / Solidity precedents
//! (43f0e340 / d2496bd4 / 919a38b9 / f424a1ae / 35a91997 /
//! e48fcb42). The 5-marker engine-level verification (NOT_IMPLEMENTED
//! gone, cross-lang walk, GraphQL parity) runs against the binary
//! and the qa1 fixtures.
//!
//! Doctrine for Nix:
//! - `import EXPR` — anchor = `EXPR` (path / `<channel>` / expr).
//! - `with EXPR; BODY` — anchor = `EXPR`. Wildcard scope import.
//! - `inherit (SOURCE) a b c;` — anchor = `SOURCE`. Captured under
//!   the structural caveat documented in `lib.rs`: only matches
//!   when the `inherit` is the SOLE binding in some enclosing
//!   attrset. Multi-binding contexts are a known under-match.
//! - `inherit a b c;` (no source) — skipped intentionally.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_nix::NixImportGrammar;

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
  let p = NixImportGrammar.import_patterns("pkgs");
  assert!(
    !p.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  assert_eq!(p.len(), 3, "expected 3 doctrinal shapes");
}

#[test]
fn import_path_form() {
  // `import ./foo.nix` — direct file import. Anchor = path.
  let content = "{ pkgs }: let\n  foo = import ./foo.nix;\nin foo\n";
  let p = NixImportGrammar.import_patterns("./foo.nix");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "exactly one `import ./foo.nix`"
  );
}

#[test]
fn import_channel_path() {
  // `import <nixpkgs>` — system search-path. Anchor = `<nixpkgs>`.
  let content = "let pkgs = import <nixpkgs> {}; in pkgs\n";
  let p = NixImportGrammar.import_patterns("<nixpkgs>");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "exactly one `import <nixpkgs>`"
  );
}

#[test]
fn with_wildcard_scope() {
  // `with pkgs; [...]` — wildcard scope import. Anchor = `pkgs`.
  let content = "{ pkgs }: {\n  buildInputs = with pkgs; [ jq coreutils findutils ];\n}\n";
  let p = NixImportGrammar.import_patterns("pkgs");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "exactly one `with pkgs; [...]`"
  );
}

#[test]
fn with_dotted_source() {
  // `with config.lib;` — dotted source expression as wildcard scope.
  let content = "{ config }: {\n  result = with config.lib; mkOption { type = types.str; };\n}\n";
  let p = NixImportGrammar.import_patterns("config.lib");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "exactly one `with config.lib;`"
  );
}

#[test]
fn inherit_sole_binding_in_attrset() {
  // `inherit (lib) mkForce;` as the SOLE binding in an attrset —
  // matches the `{ inherit (X) $$$; }` wrapper pattern.
  let content = "{ lib }: {\n  result = {\n    inherit (lib) mkForce;\n  };\n}\n";
  let p = NixImportGrammar.import_patterns("lib");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "sole-binding `inherit (lib) mkForce;` in inner attrset"
  );
}

#[test]
fn inherit_sole_binding_multi_symbol() {
  // `inherit (lib) mkForce mkOverride;` — multiple captured
  // symbols, sole binding. Doctrine: 1 hit per statement,
  // module = source = anchor.
  let content = "{ lib }: {\n  result = {\n    inherit (lib) mkForce mkOverride;\n  };\n}\n";
  let p = NixImportGrammar.import_patterns("lib");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "sole-binding `inherit (lib) mkForce mkOverride;` (1 hit, multi-symbol)"
  );
}

#[test]
fn inherit_dotted_source() {
  // `inherit (config.virtualisation) diskSize;` — sole binding,
  // dotted source expression. Real-world pattern from
  // nixos-generators/formats/raw.nix.
  let content =
    "{ config }: {\n  result = {\n    inherit (config.virtualisation) diskSize;\n  };\n}\n";
  let p = NixImportGrammar.import_patterns("config.virtualisation");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    1,
    "sole-binding `inherit (config.virtualisation) diskSize;`"
  );
}

#[test]
fn inherit_multi_binding_under_matched() {
  // KNOWN UNDER-MATCH: when `inherit (X) ...;` is one of MULTIPLE
  // bindings in an enclosing attrset / let-block, the structural
  // wrapper `{ inherit (X) $$$; }` cannot match. This is the
  // documented limitation in `lib.rs` — pinning the behavior so a
  // future contextual-pattern fix flips this assertion.
  let content =
    "{ lib }: {\n  result = {\n    a = 1;\n    inherit (lib) mkForce;\n    b = 2;\n  };\n}\n";
  let p = NixImportGrammar.import_patterns("lib");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    0,
    "under-match: multi-binding inherit is structurally missed (documented)"
  );
}

#[test]
fn inherit_no_source_skipped() {
  // `inherit a b c;` (no `(SOURCE)`) — intentionally NOT matched.
  // Doctrine: no clear module anchor, so no hit.
  let content = "{ a, b, c }: {\n  result = {\n    inherit a b c;\n  };\n}\n";
  let p = NixImportGrammar.import_patterns("a");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    0,
    "bare `inherit a b c;` (no source) intentionally skipped"
  );
}

#[test]
fn no_falsematch_in_comment_or_string() {
  let content = "{ pkgs }: {\n  # with pkgs; fakebody\n  result = \"with pkgs; not real\";\n}\n";
  let p = NixImportGrammar.import_patterns("pkgs");
  assert_eq!(
    count_matches(content, &p, SgLang::Nix),
    0,
    "should not match `with pkgs;` in comment or string"
  );
}

#[test]
fn all_shapes_in_one_file() {
  // A single file with each doctrinal shape (each placed so the
  // structural patterns match). Per-anchor must produce ≥1 hit.
  let content = "{ pkgs, lib, config }:\n\
        let\n\
          foo = import ./foo.nix;\n\
        in {\n\
          result = with pkgs; [ jq coreutils ];\n\
          libImports = {\n\
            inherit (lib) mkForce mkOverride;\n\
          };\n\
          inner = {\n\
            inherit (config.virtualisation) diskSize;\n\
          };\n\
        }\n";

  for anchor in ["./foo.nix", "pkgs", "lib", "config.virtualisation"] {
    let p = NixImportGrammar.import_patterns(anchor);
    let n = count_matches(content, &p, SgLang::Nix);
    assert!(
      n >= 1,
      "expected ≥1 match for `{anchor}` across all shapes, got {n}"
    );
  }
}
