//! `lmprobe-lang-typescript` — D5 capability impls for TypeScript.
//!
//! Phase 5B.27 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs` as the shared `Js*` structs
//! (shared across JavaScript / TypeScript / Tsx). Each impl struct
//! is reachable from the registry's per-language dispatch arms
//! (`SgLang::TypeScript => &lmprobe_lang_typescript::TypeScript*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `TypeScriptExtractor` — bespoke: walks `lexical_declaration` /
//!   `variable_declaration` to the inner `variable_declarator`'s
//!   `name` field (captures `export const Foo = <arrow>` patterns
//!   that the generic field("name") fallback misses); falls through
//!   to field("name") → child(0) for fn/class declarations.
//! - `TypeScriptCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)`).
//! - `TypeScriptImportGrammar` — ES module imports + CommonJS
//!   `require()`, ES module exports + `module.exports`.
//! - `TypeScriptInheritanceGrammar` — `class X extends Y` patterns,
//!   TS `override` method patterns.
//! - `TypeScriptDecorationGrammar` — `@name` / `@name($$$)`.
//! - `TypeScriptTraceTemplate` — `console.error("[TRACE] var =", $var);`.
//! - `TypeScriptKindTaxonomy` — portable-kind ↔ TS tree-sitter-kind map.
//! - `TypeScriptTopLevelKinds` — empty (TS isn't a data lang).
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
// `clean` is replicated from lang-core rather than re-exported.
// JavaScript and Tsx each get their own copy when extracted.
// Per-lang crates stay self-contained.

fn clean(s: &str) -> String {
  s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

// ──────────────────────────── name extractor ──────────────────────

/// TS: `lexical_declaration` and `variable_declaration` wrap a
/// `variable_declarator` whose `name` field holds the binder. Modern
/// TS codebases ship most named exports as `export const Foo =
/// <arrow>` so the generic field("name") fallback (which sees the
/// `let`/`const` keyword via child(0)) misses them. Walk to the
/// declarator's `name` field; for fn/class declarations, fall
/// through to the generic path.
pub struct TypeScriptExtractor;
impl NameExtractor for TypeScriptExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    let k = node.kind();
    if k == "lexical_declaration" || k == "variable_declaration" {
      for c in node.children() {
        if c.kind() == "variable_declarator" {
          if let Some(name) = c.field("name") {
            let s = clean(&name.text());
            if !s.is_empty() {
              return Some(s);
            }
          }
        }
      }
    }
    if let Some(name) = node.field("name") {
      let s = clean(&name.text());
      if !s.is_empty() {
        return Some(s);
      }
    }
    node
      .child(0)
      .map(|n| clean(&n.text()))
      .filter(|s| !s.is_empty())
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// TypeScript uses standard `name(args)` / `obj.name(args)` shapes;
/// defer to `StandardCallGrammar`.
pub struct TypeScriptCallGrammar;
impl CallGrammar for TypeScriptCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("function {name}($$$) {{ $$$ }}"),
      format!("function {name}($$$): $RET {{ $$$ }}"),
      format!("async function {name}($$$) {{ $$$ }}"),
      format!("async function {name}($$$): $RET {{ $$$ }}"),
      format!("export function {name}($$$) {{ $$$ }}"),
      format!("export function {name}($$$): $RET {{ $$$ }}"),
      format!("export async function {name}($$$) {{ $$$ }}"),
      format!("export async function {name}($$$): $RET {{ $$$ }}"),
      format!("const {name} = ($$$) => {{ $$$ }}"),
      format!("const {name} = async ($$$) => {{ $$$ }}"),
      format!("export const {name} = ($$$) => {{ $$$ }}"),
      format!("export const {name} = async ($$$) => {{ $$$ }}"),
      format!("{name}($$$) {{ $$$ }}"),
      format!("async {name}($$$) {{ $$$ }}"),
    ]
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct TypeScriptImportGrammar;
impl ImportGrammar for TypeScriptImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    // QA TC-019: TS/JS imports use either '"' or '\''. Emit both
    // quote styles; ast-grep is quote-literal in pattern matching.
    vec![
      format!("import $$$ from \"{name}\""),
      format!("import $$$ from '{name}'"),
      format!("import \"{name}\""),
      format!("import '{name}'"),
      format!("require(\"{name}\")"),
      format!("require('{name}')"),
    ]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    // Phase 6A #7: re-export forms — tree-sitter-typescript
    // requires the specifier list between `export` and `from`
    // to be `*` or `{...}`, not a free `$$$`. Use shape-specific
    // patterns so each parses cleanly.
    vec![
      format!("export const {name} = $RHS"),
      format!("export let {name} = $RHS"),
      format!("export function {name}($$$) {{ $$$ }}"),
      format!("export class {name} {{ $$$ }}"),
      format!("export default {name}"),
      format!("export {{ {name} }}"),
      format!("export {{ {name} as $ALIAS }}"),
      format!("export * from \"{name}\""),
      format!("export * from '{name}'"),
      format!("export {{ $$$ }} from \"{name}\""),
      format!("export {{ $$$ }} from '{name}'"),
      format!("module.exports.{name} = $RHS"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct TypeScriptInheritanceGrammar;
impl InheritanceGrammar for TypeScriptInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} extends $SUPER {{ $$$ }}"),
      format!("export class {name} extends $SUPER {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C extends {name} {{ $$$ }}"),
      format!("export class $C extends {name} {{ $$$ }}"),
      format!("interface $I extends {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    let _ = name;
    Vec::new()
  }
}

// ─────────────────────────── decoration grammar ──────────────────

pub struct TypeScriptDecorationGrammar;
impl DecorationGrammar for TypeScriptDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("@{name}"), format!("@{name}($$$)")]
  }
}

// ─────────────────────────── trace template ──────────────────────

pub struct TypeScriptTraceTemplate;
impl TraceTemplate for TypeScriptTraceTemplate {
  fn template_for(&self, var: &str) -> Option<String> {
    Some(format!("console.error(\"[TRACE] {var} =\", ${var});"))
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct TypeScriptKindTaxonomy;
impl KindTaxonomy for TypeScriptKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &[
        "function_declaration",
        "function_expression",
        "arrow_function",
      ],
      PortableKind::Method => &["method_definition"],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &["interface_declaration"],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &["type_alias_declaration"],
      PortableKind::Const => &["lexical_declaration", "variable_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_statement"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// TypeScript isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct TypeScriptTopLevelKinds;
impl TopLevelKinds for TypeScriptTopLevelKinds {}
