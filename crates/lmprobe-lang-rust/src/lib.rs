//! `lmprobe-lang-rust` — D5 capability impls for Rust.
//!
//! Phase 5A.3 first-extraction. Mirrors the trait impls that
//! previously lived in `lmprobe-lang-core::langs`. Each impl
//! struct is reachable from the registry's per-language
//! dispatch arms (`SgLang::Rust => &lmprobe_lang_rust::Rust*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `RustExtractor` — delegates to `GenericExtractor` (Rust's
//!   declaration nodes carry a `name` field; the generic
//!   extractor handles them correctly).
//! - `RustCallGrammar` — delegates to `StandardCallGrammar`.
//! - `RustImportGrammar` — Rust's `use` syntax + `pub` exports.
//! - `RustInheritanceGrammar` — trait supertypes + `impl X for Y`.
//! - `RustDecorationGrammar` — `#[…]` / `#[derive(…)]` chains.
//! - `RustTraceTemplate` — `eprintln!("[TRACE] {} = {:?}", …)`.
//! - `RustKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `RustTopLevelKinds` — empty (Rust isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, GenericExtractor, ImportGrammar, InheritanceGrammar,
  KindTaxonomy, NameExtractor, PortableKind, SgNode, StandardCallGrammar, TopLevelKinds,
  TraceTemplate,
};

// ──────────────────────────── name extractor ──────────────────────

/// Rust mostly uses the generic `field("name") → field("key") →
/// child(0)` strategy. `impl Foo { ... }` is the exception: the
/// container target lives under the `type` field, while child(0) is
/// the `impl` keyword.
pub struct RustExtractor;
impl NameExtractor for RustExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if node.kind() == "impl_item" {
      if let Some(type_node) = node.field("type") {
        let name = rust_type_head(&type_node.text());
        if !name.is_empty() {
          return Some(name);
        }
      }
      if let Some(name) = rust_impl_target_name(&node.text()) {
        return Some(name);
      }
    }
    GenericExtractor.extract(node)
  }
}

fn rust_impl_target_name(text: &str) -> Option<String> {
  let mut rest = text.trim().strip_prefix("impl")?.trim_start();
  if rest.starts_with('<') {
    rest = rest.get(skip_balanced_angle(rest)..)?.trim_start();
  }
  if let Some((_, target)) = rest.rsplit_once(" for ") {
    rest = target.trim_start();
  }
  let rest = rest
    .split('{')
    .next()
    .unwrap_or(rest)
    .split(" where ")
    .next()
    .unwrap_or(rest)
    .trim();
  let name = rust_type_head(rest);
  (!name.is_empty()).then_some(name)
}

fn skip_balanced_angle(text: &str) -> usize {
  let mut depth = 0usize;
  for (idx, ch) in text.char_indices() {
    match ch {
      '<' => depth += 1,
      '>' => {
        depth = depth.saturating_sub(1);
        if depth == 0 {
          return idx + ch.len_utf8();
        }
      }
      _ => {}
    }
  }
  0
}

fn rust_type_head(text: &str) -> String {
  let leaf = text
    .trim()
    .rsplit("::")
    .next()
    .unwrap_or("")
    .trim_start_matches('&')
    .trim_start();
  leaf
    .chars()
    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
    .collect()
}

// ──────────────────────────── call grammar ────────────────────────

/// Rust uses standard `name(args)` / `target.name(args)` /
/// `Type::name(args)` shapes; defer to `StandardCallGrammar`.
pub struct RustCallGrammar;
impl CallGrammar for RustCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// Rust `use` directives, five shapes:
/// - `use {name};`                — plain (single segment OR full
///                                  multi-segment path; e.g.
///                                  `use std;`, `use std::io::Write;`).
/// - `use {name}::$$$;`           — single-symbol continuation
///                                  (`{name}` as prefix; final segment
///                                  is one identifier — `$$$` does
///                                  NOT span `::`-separated segments
///                                  or match groups/globs at this
///                                  position, so glob and group need
///                                  their own patterns).
/// - `use {name} as $ALIAS;`      — alias form. Captures the imported
///                                  target; alias name is metadata
///                                  per doctrine.
/// - `use {name}::{$$$};`         — group import
///                                  (`use std::io::{Read, Write};`).
///                                  One hit per statement; the
///                                  `{name}` prefix anchors the module.
/// - `use {name}::*;`             — glob import. Module = anchor.
///
/// Doctrine: capture the imported module/symbol; alias is metadata;
/// re-exports (`pub use`) are OUT of scope (they belong to the
/// `exports` sensor — and tree-sitter-rust treats `pub use` as a
/// structurally distinct node, so the bare `use {name};` pattern
/// does not leak into pub use).
///
/// Phase 6C #4 broadens the pre-extraction pattern set (which was
/// just `use {name};` + `use {name}::$$$;`) to cover group, glob,
/// and alias forms, all per the PM-set doctrine in the dispatch.
pub struct RustImportGrammar;
impl ImportGrammar for RustImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("use {name};"),
      format!("use {name}::$$$;"),
      format!("use {name} as $ALIAS;"),
      format!("use {name}::{{$$$}};"),
      format!("use {name}::*;"),
    ]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("pub use {name};"),
      format!("pub use {name}::$$$;"),
      format!("pub use $PATH::{name};"),
      format!("pub fn {name}($$$) {{ $$$ }}"),
      format!("pub struct {name} {{ $$$ }}"),
      format!("pub struct {name};"),
      format!("pub enum {name} {{ $$$ }}"),
      format!("pub trait {name} {{ $$$ }}"),
      format!("pub mod {name};"),
      format!("pub mod {name} {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct RustInheritanceGrammar;
impl InheritanceGrammar for RustInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("trait {name}: $SUPER {{ $$$ }}"),
      format!("trait {name}: $SUPER + $$$ {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("impl {name} for $T {{ $$$ }}"),
      format!("impl<$$$GENS> {name} for $T {{ $$$ }}"),
      format!("impl<$$$GENS> {name}<$$$ARGS> for $T {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("fn {name}($$$) {{ $$$ }}"),
      format!("fn {name}($$$) -> $RET {{ $$$ }}"),
      format!("fn {name}(&self, $$$) {{ $$$ }}"),
      format!("fn {name}(&self, $$$) -> $RET {{ $$$ }}"),
      format!("fn {name}(&mut self, $$$) {{ $$$ }}"),
      format!("fn {name}(&mut self, $$$) -> $RET {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

pub struct RustDecorationGrammar;
impl DecorationGrammar for RustDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("#[{name}]"),
      format!("#[{name}($$$)]"),
      format!("#[derive($$$, {name}, $$$)]"),
      format!("#[derive({name}, $$$)]"),
      format!("#[derive($$$, {name})]"),
      format!("#[derive({name})]"),
      format!("#![{name}]"),
      format!("#![{name}($$$)]"),
    ]
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct RustTraceTemplate;
impl TraceTemplate for RustTraceTemplate {
  fn template_for(&self, var: &str) -> Option<String> {
    Some(format!(
      "eprintln!(\"[TRACE] {{}} = {{:?}}\", stringify!(${var}), ${var});"
    ))
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct RustKindTaxonomy;
impl KindTaxonomy for RustKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_item"],
      PortableKind::Method => &["function_item"],
      PortableKind::Class => &[],
      PortableKind::Struct => &["struct_item"],
      PortableKind::Interface => &[],
      PortableKind::Trait => &["trait_item"],
      PortableKind::Enum => &["enum_item"],
      PortableKind::TypeAlias => &["type_item"],
      PortableKind::Const => &["const_item", "static_item"],
      PortableKind::Macro => &["macro_definition", "macro_invocation"],
      PortableKind::Import => &["use_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Rust isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct RustTopLevelKinds;
impl TopLevelKinds for RustTopLevelKinds {}
