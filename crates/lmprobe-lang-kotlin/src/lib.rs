//! `lmprobe-lang-kotlin` — D5 capability impls for Kotlin.
//!
//! Phase 5B.1 first-extraction. Mirrors the trait impls that
//! previously lived in `lmprobe-lang-core::langs`. Each impl
//! struct is reachable from the registry's per-language
//! dispatch arms (`SgLang::Kotlin => &lmprobe_lang_kotlin::Kotlin*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `KotlinExtractor` — bespoke: `field("name")` skips Kotlin
//!   declaration keywords (`class`, `interface`, `fun`, …) before
//!   walking children for the actual `simple_identifier` /
//!   `type_identifier`.
//! - `KotlinCallGrammar` — Kotlin's call shapes are not yet
//!   modelled (matches lang-core's pre-extraction default of
//!   `NoCallGrammar`); empty patterns.
//! - `KotlinImportGrammar` — `import a.b.c` (+ `.$$$` glob), plus
//!   `fun` / `class` / `interface` / `object` exports.
//! - `KotlinInheritanceGrammar` — `class C : Super`, `interface I :
//!   J`, `override fun …`.
//! - `KotlinDecorationGrammar` — bare `@Foo`, called `@Foo(…)`, and
//!   the four use-site targets (`@field:`, `@param:`, `@get:`,
//!   `@set:`).
//! - `KotlinTraceTemplate` — none (matches lang-core default).
//! - `KotlinKindTaxonomy` — portable-kind ↔ Kotlin tree-sitter-kind
//!   map; Kotlin reuses `class_declaration` for class / interface /
//!   enum.
//! - `KotlinTopLevelKinds` — empty (Kotlin isn't a data lang).
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

// ──────────────────────────── helpers ─────────────────────────────
//
// `clean` + `first_named_child` are the same trivial helpers
// lang-core uses internally. Replicated here (rather than re-
// exported) because they're 2-line utilities and per-lang crates
// stay self-contained — the alternative would be inflating
// lang-core's public API for every lang's bespoke extractor.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn first_named_child<'a>(node: &SgNode<'a>, kinds: &[&str]) -> Option<SgNode<'a>> {
  node.children().find(|c| kinds.contains(&c.kind().as_ref()))
}

fn is_keyword_kotlin(s: &str) -> bool {
  matches!(
    s,
    "class" | "interface" | "fun" | "object" | "suspend" | "override" | "open" | "data"
  )
}

// ──────────────────────────── name extractor ──────────────────────

/// Kotlin: `class_declaration` / `function_declaration` have a
/// `name` field, but `child(0)` is the keyword (`class`, `interface`,
/// `fun`, `suspend`). Skip the field path if it returns a keyword
/// and walk children for the actual identifier.
pub struct KotlinExtractor;
impl NameExtractor for KotlinExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    if let Some(name) = node.field("name") {
      let s = clean(&name.text());
      if !s.is_empty() && !is_keyword_kotlin(&s) {
        return Some(s);
      }
    }
    first_named_child(node, &["simple_identifier", "type_identifier"]).map(|n| clean(&n.text()))
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Kotlin's call shapes aren't yet modelled in the registry —
/// matches lang-core's pre-extraction `NoCallGrammar` fallback.
/// Empty patterns surface as `NOT_IMPLEMENTED` from the sensor.
pub struct KotlinCallGrammar;
impl CallGrammar for KotlinCallGrammar {
  fn call_patterns(&self, _name: &str) -> Vec<String> {
    Vec::new()
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct KotlinImportGrammar;
impl ImportGrammar for KotlinImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("import {name}"), format!("import {name}.$$$")]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("fun {name}($$$): $RET = $$$"),
      format!("fun {name}($$$) {{ $$$ }}"),
      format!("class {name} {{ $$$ }}"),
      format!("interface {name} {{ $$$ }}"),
      format!("object {name} {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct KotlinInheritanceGrammar;
impl InheritanceGrammar for KotlinInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} : $SUPER {{ $$$ }}"),
      format!("class {name} : $SUPER($$$) {{ $$$ }}"),
      format!("class {name} : $$$IFACES {{ $$$ }}"),
      format!("interface {name} : $$$IFACES {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C : {name} {{ $$$ }}"),
      format!("class $C : {name}($$$) {{ $$$ }}"),
      format!("class $C : {name}, $$$ {{ $$$ }}"),
      format!("interface $I : {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("override fun {name}($$$): $RET = $$$"),
      format!("override fun {name}($$$) {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Kotlin annotations: bare `@Foo`, called `@Foo(…)`, and the four
/// use-site targets (`@field:Foo`, `@param:Foo`, `@get:Foo`,
/// `@set:Foo`).
pub struct KotlinDecorationGrammar;
impl DecorationGrammar for KotlinDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("@{name}"),
      format!("@{name}($$$)"),
      format!("@field:{name}"),
      format!("@param:{name}"),
      format!("@get:{name}"),
      format!("@set:{name}"),
    ]
  }
}

// ─────────────────────────── trace template ───────────────────────

/// Kotlin trace-instrumentation isn't wired (matches lang-core's
/// `NoTraceTemplate` pre-extraction default).
pub struct KotlinTraceTemplate;
impl TraceTemplate for KotlinTraceTemplate {
  fn template_for(&self, _var: &str) -> Option<String> {
    None
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct KotlinKindTaxonomy;
impl KindTaxonomy for KotlinKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["function_declaration"],
      PortableKind::Method => &["function_declaration"],
      PortableKind::Class => &["class_declaration", "object_declaration"],
      PortableKind::Struct => &[],
      // Kotlin reuses `class_declaration` for both classes and
      // interfaces — disambiguation is structural (the `interface`
      // keyword), not at the kind level.
      PortableKind::Interface => &["class_declaration"],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["class_declaration"],
      PortableKind::TypeAlias => &["type_alias"],
      PortableKind::Const => &["property_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_header"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Kotlin isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct KotlinTopLevelKinds;
impl TopLevelKinds for KotlinTopLevelKinds {}
