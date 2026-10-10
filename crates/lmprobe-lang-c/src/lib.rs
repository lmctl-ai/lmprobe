//! `lmprobe-lang-c` — D5 capability impls for C.
//!
//! Phase 5B.11 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::C => &lmprobe_lang_c::C*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `CExtractor` — bespoke: tries `field("declarator")` first
//!   (function_definition / declaration), walking through nested
//!   function_declarator / pointer_declarator to the leaf identifier;
//!   then falls back to `field("name")` for struct/union/enum.
//! - `CCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)` / `Type::name(args)`).
//! - `CImportGrammar` — `#include <hdr>` + `#include "hdr"` shapes.
//!   Phase 6C #1: net-new wire-up (lang-core's `NoImportGrammar`
//!   catch-all previously emitted NOT_IMPLEMENTED for C).
//! - `CInheritanceGrammar` — empty (C has no OOP inheritance).
//! - `CDecorationGrammar` — empty (C has no decorator/attribute
//!   syntax wired in lang-core).
//! - `CTraceTemplate` — `None` (lang-core's `NoTraceTemplate`
//!   catch-all applied; preserved).
//! - `CKindTaxonomy` — portable-kind -> C tree-sitter-kind map.
//! - `CTopLevelKinds` — empty (C isn't a data lang).
//!
//! Trait API constraint (W4): structs implement only the
//! `lmprobe-lang-core` traits + `ast-grep-core` types. No
//! `lmprobe-types` envelope refs — keeps async-graphql out of
//! per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, NameExtractor,
  PortableKind, SgNode, StandardCallGrammar, TopLevelKinds, TraceTemplate,
};

// ──────────────────────────── helpers ─────────────────────────────
//
// `clean` + `walk_to_identifier` are replicated from lang-core
// rather than re-exported. After this extraction, the lang-core
// copy is deleted (zero remaining callers). Per-lang crates stay
// self-contained — the alternative would be inflating lang-core's
// public API for every bespoke extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Walk a (possibly nested) declarator down to the leaf
/// `identifier` / `field_identifier`. C wraps the actual
/// function name behind `function_declarator` / `pointer_declarator` /
/// `array_declarator` / `parenthesized_declarator` layers.
fn walk_to_identifier<'a>(node: &SgNode<'a>) -> Option<String> {
  let kind = node.kind();
  match kind.as_ref() {
    "identifier" | "field_identifier" | "type_identifier" => Some(clean(&node.text())),
    _ => {
      if let Some(d) = node.field("declarator") {
        if let Some(found) = walk_to_identifier(&d) {
          return Some(found);
        }
      }
      for c in node.children() {
        if c.is_named() {
          if let Some(found) = walk_to_identifier(&c) {
            return Some(found);
          }
        }
      }
      None
    }
  }
}

// ──────────────────────────── name extractor ──────────────────────

/// C: `function_definition` has a `declarator` field that points at
/// `function_declarator` -> `identifier`. The pre-phase-0 fallback to
/// child(0) returned the type specifier (`void`, `int`).
pub struct CExtractor;
impl NameExtractor for CExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    // function_definition / declaration -> declarator -> identifier.
    if let Some(decl) = node.field("declarator") {
      if let Some(name) = walk_to_identifier(&decl) {
        return Some(name);
      }
    }
    // struct/union/enum carry `name` directly.
    if let Some(name) = node.field("name") {
      let s = clean(&name.text());
      if !s.is_empty() {
        return Some(s);
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// C uses standard `name(args)` / `obj.name(args)` shapes;
/// defer to `StandardCallGrammar`.
pub struct CCallGrammar;
impl CallGrammar for CCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// C `#include` has two shapes: angle-bracket (system header) and
/// double-quote (local header). `name` is the full header path the
/// user passes (e.g. `stdio.h`, `foo/bar.h`). Both forms are emitted
/// so a single `imports stdio.h` query catches either site. C has
/// no aliasing/wildcards, so one hit per `#include` line.
pub struct CImportGrammar;
impl ImportGrammar for CImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("#include <{name}>"), format!("#include \"{name}\"")]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

/// C has no OOP inheritance — empty impl preserves the
/// `NoInheritanceGrammar` catch-all shape for byte-equality.
pub struct CInheritanceGrammar;
impl InheritanceGrammar for CInheritanceGrammar {}

// ─────────────────────────── decoration grammar ───────────────────

/// C has no decoration/attribute grammar wired in lang-core.
/// Preserve the empty `NoDecorationGrammar` shape for byte-equality.
pub struct CDecorationGrammar;
impl DecorationGrammar for CDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

/// C had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all).
pub struct CTraceTemplate;
impl TraceTemplate for CTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct CKindTaxonomy;
impl KindTaxonomy for CKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &[],
      PortableKind::Class => &[],
      PortableKind::Struct => &["struct_specifier"],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["enum_specifier"],
      PortableKind::TypeAlias => &["type_definition"],
      PortableKind::Const => &["declaration"],
      PortableKind::Macro => &["preproc_def", "preproc_function_def"],
      PortableKind::Import => &["preproc_include"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// C isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct CTopLevelKinds;
impl TopLevelKinds for CTopLevelKinds {}
