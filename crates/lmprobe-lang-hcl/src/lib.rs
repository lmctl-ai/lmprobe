//! `lmprobe-lang-hcl` — D5 capability impls for HCL.
//!
//! Phase 5B.19 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Hcl => &lmprobe_lang_hcl::Hcl*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `HclExtractor` — bespoke: block-style `resource "aws_vpc" "this"
//!   { … }`. Returns the first `string_lit` child (block label) or
//!   falls back to the first `identifier`/`block_label` child. Uses
//!   `clean` + `first_named_child` helpers (replicated privately per
//!   the playbook).
//! - `HclCallGrammar` — empty (lang-core fell through to
//!   `NoCallGrammar` for HCL).
//! - `HclImportGrammar` — terraform module `source = "<NAME>"` patterns.
//!   Ported from lang-core's exclusive arm.
//! - `HclInheritanceGrammar` — empty (lang-core fell through to
//!   `NoInheritanceGrammar` for HCL).
//! - `HclDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar` for HCL).
//! - `HclTraceTemplate` — `None` (lang-core fell through to
//!   `NoTraceTemplate` for HCL).
//! - `HclKindTaxonomy` — portable-kind -> tree-sitter-kind map.
//!   Ported from lang-core's `taxonomy!(HclTaxonomy { … })`.
//! - `HclTopLevelKinds` — empty (HCL isn't a data lang).
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

/// HCL: block-style `resource "aws_vpc" "this" { … }`. The first
/// child is the block-type identifier (`resource`); the labels follow
/// as `string_lit` children. We return the first `string_lit` if any
/// (the canonical "name" for resource/data/module blocks); else the
/// first identifier (for label-less blocks like `locals { … }`).
pub struct HclExtractor;
impl NameExtractor for HclExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    // Prefer first string_lit (block label).
    if let Some(lit) = first_named_child(node, &["string_lit"]) {
      let s = clean(&lit.text());
      if !s.is_empty() {
        return Some(s);
      }
    }
    // Else first identifier (block type for label-less blocks).
    first_named_child(node, &["identifier", "block_label"])
      .map(|n| clean(&n.text()))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// HCL wasn't wired to `StandardCallGrammar` in lang-core (it fell
/// through to `NoCallGrammar`). Keeping the empty shape preserves
/// byte-equality with the pre-extraction behavior.
pub struct HclCallGrammar;
impl CallGrammar for HclCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// HCL: terraform's idiomatic "import" is the `module "X" { source =
/// "<NAME>" … }` block. Anchor doctrine: `{name}` is the source path
/// (relative `../foo`, registry `terraform-aws-modules/...`, or git
/// `git::https://…`); the module name is metadata bound by the
/// `$NAME` ast-grep metavar.
///
/// Patterns:
/// 1. `module "$NAME" { source = "<name>" }` — block context, the
///    canonical terraform "import" idiom. Ast-grep's tree-sitter-hcl
///    matcher relax-matches the body, so this hits blocks where
///    `source` is the first attribute (the convention used in every
///    real terraform fixture audited), regardless of trailing attrs.
/// 2. `source = "<name>"` — direct attribute. Broader anchor; also
///    matches `required_providers { aws = { source = "hashicorp/aws"
///    … } }`. Kept for users querying just the source string and as
///    a fallback when the block-form misses (under-match below).
///
/// Documented under-match: HCL's tree-sitter doesn't allow `$$$`
/// body wildcards (the matcher fails to parse `module "$NAME" {
/// source = "<name>" $$$ }` and `module "$NAME" { $$$ source =
/// "<name>" $$$ }` matches 0 even when it parses). Module blocks
/// where `source` is NOT the first attribute miss the block-form
/// pattern (the direct-attr pattern still hits them). Empirically
/// 0/all module blocks in qa1 fixtures hit this case — terraform
/// convention puts `source` first. Mirrors the shape-only-precision
/// tradeoff documented in the nix precedent (77624641).
///
/// Wildcards / aliasing / multi-import per stmt: HCL has no wildcard
/// import syntax; the module name IS the alias (bound to `$NAME`);
/// each `module` block declares exactly one source.
/// Phase 6C round-3 (TC-102 refinement): dual-anchor — accept BOTH
/// block-name queries (`imports vpc /tree`) AND source-path queries
/// (`imports './modules/vpc' /tree`). Round-2 dropped source-path
/// outright; round-3 restores it scoped INSIDE `module` blocks (so
/// `required_providers { aws = { source = ... } }` nested-object
/// source attrs still don't match — those aren't module imports).
///
/// Both patterns target the same `block` node. A query that activates
/// both (rare: requires block-name == source-path) emits twice; in
/// practice block names are identifiers and source paths contain
/// `./`/`/`/`.sol`-style separators — natural disjointness preserves
/// "1 hit per module block" without needing post-pattern dedup.
pub struct HclImportGrammar;
impl ImportGrammar for HclImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      // Anchor on block name: `imports vpc` matches
      // `module "vpc" { source = ... }`. Lenient matcher
      // relax-matches trailing attrs.
      format!("module \"{name}\" {{ source = $SRC }}"),
      // Anchor on source path: `imports ./modules/vpc` matches
      // `module "$X" { source = "./modules/vpc" }`.
      format!("module \"$X\" {{ source = \"{name}\" }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// HCL had no inheritance grammar in lang-core — keep the empty
/// shape (matches `NoInheritanceGrammar` catch-all; default methods
/// all return `Vec::new()`).
pub struct HclInheritanceGrammar;
impl InheritanceGrammar for HclInheritanceGrammar {}

// ─────────────────────────── decoration grammar ──────────────────

/// HCL had no decoration grammar in lang-core — keep the empty
/// shape (matches `NoDecorationGrammar` catch-all).
pub struct HclDecorationGrammar;
impl DecorationGrammar for HclDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ──────────────────────

/// HCL had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct HclTraceTemplate;
impl TraceTemplate for HclTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

/// HCL taxonomy — ported from lang-core's `taxonomy!(HclTaxonomy { … })`.
pub struct HclKindTaxonomy;
impl KindTaxonomy for HclKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &[],
      PortableKind::Method => &[],
      PortableKind::Class => &[],
      PortableKind::Struct => &["block"],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &[],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["attribute"],
      PortableKind::Macro => &[],
      PortableKind::Import => &[],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// HCL isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct HclTopLevelKinds;
impl TopLevelKinds for HclTopLevelKinds {}
