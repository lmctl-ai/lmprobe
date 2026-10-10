//! `lmprobe-lang-cpp` — D5 capability impls for C++.
//!
//! Phase 5B.8 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Cpp => &lmprobe_lang_cpp::Cpp*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `CppExtractor` — bespoke: tries `field("name")` first
//!   (class/struct/enum), then walks `declarator` down to a leaf
//!   `identifier` / `field_identifier` / `type_identifier`
//!   (function_declarator / pointer_declarator nesting).
//! - `CppCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)` / `Type::name(args)`).
//! - `CppImportGrammar` — `#include <hdr>` + `#include "hdr"` shapes.
//!   Phase 6C #1: net-new wire-up (lang-core's `NoImportGrammar`
//!   catch-all previously emitted NOT_IMPLEMENTED for C++). Mirrors
//!   the C grammar — same preprocessor; C++20 `import std;` and
//!   header units are deferred to a future phase.
//! - `CppInheritanceGrammar` — `class C : public Super { … }` (with
//!   and without trailing bases), `struct` variants, `override` methods.
//! - `CppDecorationGrammar` — empty (lang-core fell through to
//!   `NoDecorationGrammar`; preserved).
//! - `CppTraceTemplate` — `None` (lang-core's `NoTraceTemplate`
//!   catch-all applied; preserved).
//! - `CppKindTaxonomy` — portable-kind ↔ C++ tree-sitter-kind map.
//!   Notably reuses `function_definition` for both Function + Method
//!   (the tree-sitter grammar doesn't distinguish), and includes
//!   `alias_declaration` (C++11 `using`) alongside `type_definition`.
//! - `CppTopLevelKinds` — empty (C++ isn't a data lang).
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
// rather than re-exported. C still uses `walk_to_identifier` in
// lang-core (CExtractor); when CoderC extracts `c`, that copy goes
// with it. Per-lang crates stay self-contained — the alternative
// would be inflating lang-core's public API for every bespoke
// extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Walk a (possibly nested) declarator down to the leaf
/// `identifier` / `field_identifier`. C and C++ wrap the actual
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

/// C++: same shape as C plus class_specifier with `name`.
pub struct CppExtractor;
impl NameExtractor for CppExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if let Some(name) = node.field("name") {
      let s = clean(&name.text());
      if !s.is_empty() {
        return Some(s);
      }
    }
    if let Some(decl) = node.field("declarator") {
      if let Some(name) = walk_to_identifier(&decl) {
        return Some(name);
      }
    }
    None
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// C++ uses standard `name(args)` / `obj.name(args)` /
/// `Type::name(args)` shapes; defer to `StandardCallGrammar`.
pub struct CppCallGrammar;
impl CallGrammar for CppCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

/// C++ `#include` has two shapes: angle-bracket (system header) and
/// double-quote (local header). `name` is the full header path the
/// user passes (e.g. `iostream`, `vector`, `myproj/foo.hpp`). Both
/// forms are emitted so a single `imports iostream` query catches
/// either site. Same grammar as C — C++20 `import std;` and header
/// units are deferred to a future phase.
pub struct CppImportGrammar;
impl ImportGrammar for CppImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("#include <{name}>"), format!("#include \"{name}\"")]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct CppInheritanceGrammar;
impl InheritanceGrammar for CppInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} : public $SUPER {{ $$$ }}"),
      format!("class {name} : public $SUPER, $$$ {{ $$$ }}"),
      format!("struct {name} : public $SUPER {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C : public {name} {{ $$$ }}"),
      format!("class $C : public {name}, $$$ {{ $$$ }}"),
      format!("struct $S : public {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("$RET {name}($$$) override {{ $$$ }}"),
      format!("virtual $RET {name}($$$) override {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// C++ has no decoration/attribute grammar wired in lang-core
/// (C++11 `[[attr]]` syntax isn't modelled). Preserve the empty
/// `NoDecorationGrammar` shape for byte-equality.
pub struct CppDecorationGrammar;
impl DecorationGrammar for CppDecorationGrammar {
  fn decoration_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ─────────────────────────── trace template ───────────────────────

/// C++ had no trace template in lang-core — keep `None` (matches
/// `NoTraceTemplate` catch-all). The sensor surfaces NO_TRACE_TEMPLATE
/// so callers know to pass `--template`.
pub struct CppTraceTemplate;
impl TraceTemplate for CppTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct CppKindTaxonomy;
impl KindTaxonomy for CppKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_definition"],
      PortableKind::Method => &["function_definition"],
      PortableKind::Class => &["class_specifier"],
      PortableKind::Struct => &["struct_specifier"],
      PortableKind::Interface => &[],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["enum_specifier"],
      // C++11 `using Foo = Bar;` plus classic `typedef`.
      PortableKind::TypeAlias => &["alias_declaration", "type_definition"],
      PortableKind::Const => &["declaration"],
      PortableKind::Macro => &["preproc_def", "preproc_function_def"],
      PortableKind::Import => &["preproc_include"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// C++ isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct CppTopLevelKinds;
impl TopLevelKinds for CppTopLevelKinds {}
