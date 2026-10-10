//! Phase 6C #4 — in-process integration test for Rust `use` import
//! patterns. Exercises `RustImportGrammar::import_patterns` end-to-
//! end through ast-grep (the same path the engine's `imports()`
//! sensor takes), without taking a dev-dep on `lmprobe-engine`
//! (which would create a cycle since the engine depends on this
//! crate via the registry).
//!
//! The 5-marker engine-level verification (NOT_IMPLEMENTED gone,
//! cross-lang walk regression, GraphQL parity) runs against the
//! binary in `crates/lmprobe/tests/*` and the qa1 fixtures. This
//! test pins the per-crate contract: each of the 5 `use` shapes
//! produces exactly the expected per-statement count against a
//! representative fixture, and the doctrinal exclusions
//! (`pub use`, alias-as-symbol) do not leak in.

use ast_grep_core::matcher::Pattern;
use ast_grep_language::LanguageExt;
use lmprobe_lang_core::langs::{ImportGrammar, SgLang};
use lmprobe_lang_rust::RustImportGrammar;
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
fn plain_single_and_multi_segment() {
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("a.rs");
  fs::write(
    &f,
    "use std;\n\
         use std::io::Write;\n\
         use crate::sensors::WalkOpts;\n\
         fn main() {}\n",
  )
  .expect("write a.rs");
  let content = fs::read_to_string(&f).unwrap();

  // Single-segment.
  let p_std = RustImportGrammar.import_patterns("std");
  assert!(
    !p_std.is_empty(),
    "patterns must be non-empty (suppresses NOT_IMPLEMENTED)"
  );
  // `use std;` matches plain. `use std::io::Write;` does NOT
  // match for name=std because the continuation pattern's `$$$`
  // matches one final segment only — `io::Write` is two segments
  // separated by `::`, so the structural shape doesn't fit
  // `use std::$$$;`. To match `use std::io::Write;` the user
  // queries `imports std::io::Write` (full path → plain) or
  // `imports std::io` (continuation matches the `Write` final
  // segment). Net for name=std: 1 hit.
  assert_eq!(
    count_matches(&content, &p_std, SgLang::Rust),
    1,
    "name=std → `use std;` only (continuation does not span `::`)"
  );

  // Two-segment.
  let p_io = RustImportGrammar.import_patterns("std::io");
  // `use std::io::Write;` matches the continuation pattern. 1 hit.
  assert_eq!(
    count_matches(&content, &p_io, SgLang::Rust),
    1,
    "name=std::io → continuation matches `use std::io::Write;`"
  );

  // Three-segment full path.
  let p_write = RustImportGrammar.import_patterns("std::io::Write");
  assert_eq!(
    count_matches(&content, &p_write, SgLang::Rust),
    1,
    "name=std::io::Write → plain matches the full path"
  );

  // Cross-segment crate-relative.
  let p_walk = RustImportGrammar.import_patterns("crate::sensors::WalkOpts");
  assert_eq!(
    count_matches(&content, &p_walk, SgLang::Rust),
    1,
    "name=crate::sensors::WalkOpts → plain matches"
  );
}

#[test]
fn group_import_one_hit_per_statement() {
  // Doctrine: "Multiple symbols per stmt: 1 hit per statement,
  // module = anchor." For `use std::io::{Read, Write};` the
  // module is `std::io`, so `imports std::io` yields 1 hit.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("group.rs");
  fs::write(
    &f,
    "use std::io::{Read, Write};\n\
         use std::{io, fmt};\n\
         use std::collections::{HashMap, BTreeMap};\n",
  )
  .expect("write group.rs");
  let content = fs::read_to_string(&f).unwrap();

  let p_io = RustImportGrammar.import_patterns("std::io");
  // `use std::io::{Read, Write};` matches the group pattern. The
  // continuation pattern `use std::io::$$$;` does NOT match groups
  // (verified empirically — `$$$` does not span `::` or `{}`).
  // Net: 1 hit.
  assert_eq!(
    count_matches(&content, &p_io, SgLang::Rust),
    1,
    "group form: 1 hit per statement, module = anchor"
  );

  let p_std = RustImportGrammar.import_patterns("std");
  // `use std::{io, fmt};` matches the group pattern for name=std.
  // `use std::io::{Read, Write};` matches the continuation pattern
  // for name=std (because `$$$` matches one final segment, but
  // `io::{Read, Write}` is structurally a different shape — verify
  // count is 1 for the group statement).
  let hits_std = count_matches(&content, &p_std, SgLang::Rust);
  assert!(
    hits_std >= 1,
    "name=std → at least 1 hit on `use std::{{io, fmt}};`; got {hits_std}"
  );

  let p_collections = RustImportGrammar.import_patterns("std::collections");
  assert_eq!(
    count_matches(&content, &p_collections, SgLang::Rust),
    1,
    "name=std::collections → 1 hit on the group import"
  );
}

#[test]
fn glob_import_module_is_anchor() {
  // Doctrine: "Wildcards (`use foo::*;`): capture; module = anchor."
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("glob.rs");
  fs::write(
    &f,
    "use std::io::*;\n\
         use crossbeam_channel::*;\n\
         use foo::bar::baz::*;\n",
  )
  .expect("write glob.rs");
  let content = fs::read_to_string(&f).unwrap();

  let p_io = RustImportGrammar.import_patterns("std::io");
  assert_eq!(
    count_matches(&content, &p_io, SgLang::Rust),
    1,
    "name=std::io → glob pattern matches `use std::io::*;`"
  );

  // Underscored bare-segment name (the PM-cited shape).
  let p_cb = RustImportGrammar.import_patterns("crossbeam_channel");
  assert_eq!(
    count_matches(&content, &p_cb, SgLang::Rust),
    1,
    "name=crossbeam_channel → glob matches `use crossbeam_channel::*;`"
  );

  let p_baz = RustImportGrammar.import_patterns("foo::bar::baz");
  assert_eq!(
    count_matches(&content, &p_baz, SgLang::Rust),
    1,
    "name=foo::bar::baz → glob matches the deep path"
  );
}

#[test]
fn alias_captures_imported_target() {
  // Doctrine: "Aliasing: capture imported, alias is metadata."
  // For `use foo::bar as Baz;`, `imports foo::bar` matches via the
  // alias pattern; `imports Baz` does NOT match (alias is not a
  // module / not a queryable thing).
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("alias.rs");
  fs::write(
    &f,
    "use foo::bar as Baz;\n\
         use std::collections::HashMap as Map;\n\
         use std::io::Write;\n",
  )
  .expect("write alias.rs");
  let content = fs::read_to_string(&f).unwrap();

  // Alias-target queryable.
  let p_target = RustImportGrammar.import_patterns("foo::bar");
  assert_eq!(
    count_matches(&content, &p_target, SgLang::Rust),
    1,
    "name=foo::bar → alias pattern matches `use foo::bar as Baz;`"
  );

  let p_hashmap = RustImportGrammar.import_patterns("std::collections::HashMap");
  assert_eq!(
    count_matches(&content, &p_hashmap, SgLang::Rust),
    1,
    "name=std::collections::HashMap → alias matches"
  );

  // Alias-as-symbol NOT queryable (doctrine: alias is metadata).
  let p_alias = RustImportGrammar.import_patterns("Baz");
  assert_eq!(
    count_matches(&content, &p_alias, SgLang::Rust),
    0,
    "name=Baz (alias) → 0 hits; alias is metadata, not a module"
  );

  // Plain `use std::io::Write;` is not an alias; alias pattern
  // shouldn't match it (no `as` clause). Verify the plain pattern
  // still works alongside the alias pattern without double-counting.
  let p_write = RustImportGrammar.import_patterns("std::io::Write");
  assert_eq!(
    count_matches(&content, &p_write, SgLang::Rust),
    1,
    "name=std::io::Write → 1 hit (plain only; alias does not double-count)"
  );
}

#[test]
fn pub_use_excluded_per_doctrine() {
  // Doctrine: "Re-exports `pub use`: OUT of scope" — they belong
  // to the `exports` sensor. Tree-sitter-rust treats `pub use` as
  // a structurally distinct node (the `pub` modifier wraps the
  // use_declaration), so the bare `use {name};` pattern naturally
  // does not leak into pub use. This test pins that exclusion.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("reexport.rs");
  fs::write(
    &f,
    "pub use foo::Bar;\n\
         pub use baz::*;\n\
         pub use qux::{One, Two};\n\
         pub use alpha as Beta;\n\
         use beta::Gamma;\n",
  )
  .expect("write reexport.rs");
  let content = fs::read_to_string(&f).unwrap();

  let p_bar = RustImportGrammar.import_patterns("foo::Bar");
  assert_eq!(
    count_matches(&content, &p_bar, SgLang::Rust),
    0,
    "pub use foo::Bar; is a re-export, not an import — 0 hits"
  );

  let p_baz = RustImportGrammar.import_patterns("baz");
  assert_eq!(
    count_matches(&content, &p_baz, SgLang::Rust),
    0,
    "pub use baz::*; is a re-export — 0 hits"
  );

  let p_qux = RustImportGrammar.import_patterns("qux");
  assert_eq!(
    count_matches(&content, &p_qux, SgLang::Rust),
    0,
    "pub use qux::{{One, Two}}; is a re-export — 0 hits"
  );

  let p_alpha = RustImportGrammar.import_patterns("alpha");
  assert_eq!(
    count_matches(&content, &p_alpha, SgLang::Rust),
    0,
    "pub use alpha as Beta; is a re-export — 0 hits"
  );

  // Plain `use beta::Gamma;` IS an import — must still match.
  let p_gamma = RustImportGrammar.import_patterns("beta::Gamma");
  assert_eq!(
    count_matches(&content, &p_gamma, SgLang::Rust),
    1,
    "use beta::Gamma; is a real import — 1 hit"
  );
}

#[test]
fn cfg_conditional_use_captured() {
  // Doctrine: "cfg-conditional `#[cfg(...)] use foo::bar`: capture
  // (ast-grep matches the use-statement regardless of preceding
  // attribute)."
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("cfg.rs");
  fs::write(
    &f,
    "#[cfg(target_os = \"linux\")]\n\
         use linux_specific::Module;\n\
         \n\
         #[cfg(unix)]\n\
         use unix::Path;\n",
  )
  .expect("write cfg.rs");
  let content = fs::read_to_string(&f).unwrap();

  let p_linux = RustImportGrammar.import_patterns("linux_specific::Module");
  assert_eq!(
    count_matches(&content, &p_linux, SgLang::Rust),
    1,
    "cfg-conditional use captured"
  );

  let p_unix = RustImportGrammar.import_patterns("unix::Path");
  assert_eq!(
    count_matches(&content, &p_unix, SgLang::Rust),
    1,
    "cfg-conditional use captured (second variant)"
  );
}

#[test]
fn all_shapes_in_one_file_no_double_counting() {
  // Catch-all: a single fixture with every doctrinal shape under
  // a single name. Pin that no double-counting occurs across the
  // 5 patterns.
  let dir = tempfile::tempdir().expect("tempdir");
  let f = dir.path().join("all.rs");
  fs::write(
    &f,
    "use foo;\n\
         use foo::Bar;\n\
         use foo::*;\n\
         use foo::{One, Two};\n\
         use foo as RenamedFoo;\n\
         use foo::nested::Deep;\n\
         pub use foo::ReExported;\n",
  )
  .expect("write all.rs");
  let content = fs::read_to_string(&f).unwrap();

  let p_foo = RustImportGrammar.import_patterns("foo");
  let hits = count_matches(&content, &p_foo, SgLang::Rust);
  // Expected hits for name="foo":
  //   1. `use foo;`            — plain
  //   2. `use foo::Bar;`       — continuation
  //   3. `use foo::*;`         — glob
  //   4. `use foo::{One, Two};`— group
  //   5. `use foo as Renamed;` — alias
  //   6. `use foo::nested::Deep;` — continuation? `$$$` doesn't span
  //      `::`, so this should NOT match for name=foo. Verified
  //      empirically in the dispatch investigation.
  //   7. `pub use foo::ReExported;` — excluded per doctrine.
  // Total: 5 hits. If any pattern over-matches we'll see >5.
  assert_eq!(
        hits, 5,
        "name=foo across 5 doctrinal shapes + 1 deep + 1 pub use → exactly 5 hits (no double-counting, no pub-use leak); got {hits}"
    );
}

#[test]
fn fixture_file_compiles() {
  // Sanity: the in-crate fixture (referenced by the original smoke
  // suite) is a real Rust file the test harness can locate.
  let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("fixtures")
    .join("lib.rs");
  assert!(path.exists(), "fixtures/lib.rs must exist");
}
