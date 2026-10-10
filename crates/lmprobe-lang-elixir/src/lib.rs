//! `lmprobe-lang-elixir` — D5 capability impls for Elixir.
//!
//! Phase 5B.17 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Elixir => &lmprobe_lang_elixir::Elixir*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `ElixirExtractor` — bespoke: Elixir parses every macro form
//!   (`def`, `defp`, `defmodule`, `use`, etc.) as a single `call`
//!   node. The extractor disambiguates by checking `target` against
//!   `ELIXIR_DEF_MACROS` and walking children to find the actual
//!   symbol name (alias / identifier / nested call target).
//!   Ported from lang-core's exclusive `SgLang::Elixir => &ElixirExtractor`
//!   arm.
//! - `ElixirCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Elixir).
//! - `ElixirImportGrammar` — `import`, `alias`, `use`, `require`
//!   patterns. Ported from lang-core's exclusive arm.
//! - `ElixirInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Elixir).
//! - `ElixirDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Elixir).
//! - `ElixirTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Elixir).
//! - `ElixirKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//!   Function/Method/Trait/Macro/Import all map to `["call"]`
//!   (everything in Elixir is a macro/call).
//! - `ElixirTopLevelKinds` — empty (Elixir isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, NameExtractor,
  PortableKind, SgNode, TopLevelKinds, TraceTemplate,
};

// ─────────────────────────── helpers ──────────────────────────────

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Elixir def-macros: `def`/`defp`/`defmodule`/`defmacro` etc. are
/// all parsed as a single `call` node with the macro name as `target`
/// and the actual symbol as the first argument. `use`, `import`,
/// `alias`, `require` are also call-shaped macros in tree-sitter-elixir.
const ELIXIR_DEF_MACROS: &[&str] = &[
  "def",
  "defp",
  "defmacro",
  "defmacrop",
  "defmodule",
  "defprotocol",
  "defimpl",
  "defstruct",
  "defexception",
  "defguard",
  "defguardp",
  "defdelegate",
  "use",
  "import",
  "alias",
  "require",
];

// ──────────────────────────── name extractor ──────────────────────

/// Elixir: `def`/`defp`/`defmodule`/`defmacro` are all parsed as a
/// single `call` node with the macro name as `target` and the actual
/// symbol as the first argument. §1.4 closes the §0.5 outstanding by
/// disambiguating: when `target` is one of the recognised def-macros,
/// we extract the first argument's identifier text.
pub struct ElixirExtractor;
impl NameExtractor for ElixirExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    // Pull the macro name from `target`.
    let target_text = node.field("target").map(|n| clean(&n.text()));
    let is_def_macro = target_text
      .as_deref()
      .map(|t| ELIXIR_DEF_MACROS.iter().any(|m| *m == t))
      .unwrap_or(false);
    if is_def_macro {
      // Walk children skipping the `target` and find the first
      // alias / identifier / nested call (for `def foo(args)`).
      for c in node.children() {
        if !c.is_named() {
          continue;
        }
        let k = c.kind();
        let kref: &str = k.as_ref();
        // Skip the target itself.
        if kref == "identifier" {
          if let Some(t) = node.field("target") {
            if t.range() == c.range() {
              continue;
            }
          }
        }
        if kref == "alias" || kref == "atom" {
          return Some(clean(&c.text()));
        }
        if kref == "identifier" {
          return Some(clean(&c.text()));
        }
        if kref == "call" {
          // Nested call: `def foo(args) do ... end`.
          if let Some(t) = c.field("target") {
            return Some(clean(&t.text()));
          }
        }
        if kref == "arguments" {
          for cc in c.children() {
            if !cc.is_named() {
              continue;
            }
            let kk = cc.kind();
            let kkref: &str = kk.as_ref();
            if kkref == "alias" || kkref == "identifier" || kkref == "atom" {
              return Some(clean(&cc.text()));
            }
            if kkref == "call" {
              if let Some(t) = cc.field("target") {
                return Some(clean(&t.text()));
              }
            }
          }
        }
      }
      return target_text;
    }
    // Non-def `call` — return None so the search loop's name
    // filter skips it. This stops every `Foo.bar()` invocation
    // from polluting `def`/`search` output.
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Elixir wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct ElixirCallGrammar;
impl CallGrammar for ElixirCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Elixir: every macro form parses as `call`. ast-grep matches
/// `import $MOD` etc. against the call's target+first-argument —
/// the patterns below cover the four common module-introduction
/// macros. ElixirExtractor's call-target disambiguation (§1.4)
/// keeps non-import calls out of the result.
///
/// Phase 6C #10: lenient-match audit confirmed all four base
/// patterns already cover keyword-list options (the call's extra
/// args parse as siblings of the first arg, which ast-grep
/// allows): `import Foo, only: [...]`, `use Foo, opt: val`,
/// `alias Foo, as: Bar`, `use Foo, unquote(opts)`. The one shape
/// they don't cover is the group form `alias Foo.{Bar, Baz}` —
/// added as the fifth pattern below. Per doctrine, group forms
/// produce 1 hit anchored on the prefix module (`Foo`).
/// Phase 6C round-3 (TC-104) — bare-root prefix-match via the engine's
/// `needs_prefix_match` filter (designer-arbitrated path-A waiver).
/// tree-sitter-elixir's qualified alias is atomic from the pattern
/// matcher's perspective (`alias Foo.$X` returns 0 hits — see round-2
/// blocker doctrine). We emit intentionally-broad patterns matching
/// the keyword + ANY single argument; the engine's post-match
/// `content.contains(name)` filter eliminates false positives.
///
/// `imports Foo /tree` now matches:
///   - `import Foo`             (exact)
///   - `alias Foo.Bar`          (qualified — broad pattern + filter)
///   - `alias Foo.Bar.Baz`      (qualified — broad pattern + filter)
///   - `use Foo.Bar`            (qualified — broad + filter)
///   - `alias Foo.{X, Y}`       (group)
///
/// Documented false-positive class: `imports Foo` matches `alias
/// FooBar.Baz` because content `alias FooBar.Baz` contains "Foo".
/// PM accepted this tradeoff in round-3 ("pick whichever is simpler"
/// for Go; same logic applies to Elixir). Path-aware regex would be
/// more precise but is engine-side scope-creep beyond what round-3
/// authorized.
pub struct ElixirImportGrammar;
impl ImportGrammar for ElixirImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    // Phase 6C round-3: round-1 narrow patterns (`import {name}`,
    // `alias {name}`, etc.) dropped — they're redundant when the
    // engine's `needs_prefix_match` filter is active. The broad
    // `$X` patterns + filter cover both exact-match (e.g. `imports
    // Foo` against `import Foo`) and prefix-match (`imports Foo`
    // against `alias Foo.Bar`). Group form `.{$$$}` kept because
    // it has a structurally distinct AST shape.
    vec![
      format!("alias {name}.{{$$$}}"), // group: alias Foo.{X, Y}
      "import $X".to_string(),
      "alias $X".to_string(),
      "use $X".to_string(),
      "require $X".to_string(),
    ]
  }
  fn needs_prefix_match(&self) -> bool {
    true
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Elixir had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct ElixirInheritanceGrammar;
impl InheritanceGrammar for ElixirInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Elixir had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct ElixirDecorationGrammar;
impl DecorationGrammar for ElixirDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Elixir had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct ElixirTraceTemplate;
impl TraceTemplate for ElixirTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Elixir taxonomy — ported from lang-core's `taxonomy!(ElixirTaxonomy { … })`.
/// Elixir parses every macro form as `call`. The narrower
/// disambiguation (def vs defmodule vs use vs whatever) is
/// applied by `ElixirExtractor` at name-extraction time.
pub struct ElixirKindTaxonomy;
impl KindTaxonomy for ElixirKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["call"],
      PortableKind::Method => &["call"],
      PortableKind::Class => &[],
      PortableKind::Struct => &[],
      PortableKind::Interface => &[],
      PortableKind::Trait => &["call"],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &[],
      PortableKind::Macro => &["call"],
      PortableKind::Import => &["call"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Elixir isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct ElixirTopLevelKinds;
impl TopLevelKinds for ElixirTopLevelKinds {}
