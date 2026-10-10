//! `lmprobe-lang-solidity` — D5 capability impls for Solidity.
//!
//! Phase 5B.20 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Solidity => &lmprobe_lang_solidity::Solidity*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `SolidityExtractor` — bespoke: `contract Foo { … }`,
//!   `function bar(…) { … }`. Returns the `name` field if present;
//!   else the first `identifier` child. Uses `clean` +
//!   `first_named_child` helpers (replicated privately per the
//!   playbook).
//! - `SolidityCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for Solidity).
//! - `SolidityImportGrammar` — `import "<path>";` and
//!   `import {Foo} from "<path>";` patterns. Ported from lang-core's
//!   exclusive arm.
//! - `SolidityInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for Solidity).
//! - `SolidityDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for Solidity).
//! - `SolidityTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for Solidity).
//! - `SolidityKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//!   Ported from lang-core's `taxonomy!(SolidityTaxonomy { … })`.
//! - `SolidityTopLevelKinds` — empty (Solidity isn't a data lang).
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

fn first_named_child<'a>(node: &SgNode<'a>, kinds: &[&str]) -> Option<SgNode<'a>> {
  node.children().find(|c| kinds.contains(&c.kind().as_ref()))
}

// ──────────────────────────── name extractor ──────────────────────

/// Solidity: `contract Foo { … }`, `function bar(…) { … }`,
/// `struct Baz { … }`. The `name` field covers most declaration
/// kinds; for nodes without a `name` field we fall back to the
/// first `identifier` child.
pub struct SolidityExtractor;
impl NameExtractor for SolidityExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    node
      .field("name")
      .map(|n| clean(&n.text()))
      .or_else(|| first_named_child(node, &["identifier"]).map(|n| clean(&n.text())))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Solidity wasn't wired to `StandardCallGrammar` in lang-core (it
/// fell through to `NoCallGrammar`). Keeping the empty shape
/// preserves byte-equality with the pre-extraction behavior.
pub struct SolidityCallGrammar;
impl CallGrammar for SolidityCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Solidity: 4 doctrinal `import` shapes. Doctrine: `{name}` is the
/// PATH STRING (e.g. `./Foo.sol`), not the imported symbol. The path
/// is the module identity; aliases are metadata. One hit per
/// statement; multi-symbol named imports (`{A, B, C}`) anchor to the
/// path via the `from` clause.
///
/// - `import "{name}";`                       (path-form simple)
/// - `import "{name}" as $ALIAS;`             (path-form alias)
/// - `import {{$$$}} from "{name}";`           (named — covers single,
///                                              multi, and per-symbol
///                                              `{X as Y}` since `$$$`
///                                              matches any tokens)
/// - `import * as $ALIAS from "{name}";`       (namespace wildcard)
///
/// The bare path-form without trailing `;` was previously a stale
/// defensive pattern; tree-sitter-solidity requires the semicolon
/// and the no-semi pattern over-matched (double-counting plain
/// statements and over-matching unrelated `import_directive` nodes).
pub struct SolidityImportGrammar;
impl ImportGrammar for SolidityImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("import \"{name}\";"),
      format!("import \"{name}\" as $ALIAS;"),
      format!("import {{$$$}} from \"{name}\";"),
      format!("import * as $ALIAS from \"{name}\";"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// Solidity had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct SolidityInheritanceGrammar;
impl InheritanceGrammar for SolidityInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// Solidity had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct SolidityDecorationGrammar;
impl DecorationGrammar for SolidityDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// Solidity had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct SolidityTraceTemplate;
impl TraceTemplate for SolidityTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// Solidity taxonomy — ported from lang-core's
/// `taxonomy!(SolidityTaxonomy { … })`.
pub struct SolidityKindTaxonomy;
impl KindTaxonomy for SolidityKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &["function_definition"],
      PortableKind::Class => &["contract_declaration"],
      PortableKind::Struct => &["struct_declaration"],
      PortableKind::Interface => &["interface_declaration"],
      PortableKind::Trait => &["library_declaration"],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["state_variable_declaration"],
      PortableKind::Macro => &["modifier_definition"],
      PortableKind::Import => &["import_directive"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Solidity isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct SolidityTopLevelKinds;
impl TopLevelKinds for SolidityTopLevelKinds {}
