//! `lmprobe-lang-java` — D5 capability impls for Java.
//!
//! Phase 5B.4 extraction. Mirrors the trait impls that previously
//! lived in `lmprobe-lang-core::langs`. Each impl struct is reachable
//! from the registry's per-language dispatch arms
//! (`SgLang::Java => &lmprobe_lang_java::Java*`).
//!
//! Structs exported (per the registry's expected naming):
//! - `JavaExtractor` — delegates to `GenericExtractor` (Java's
//!   declaration nodes carry a `name` field).
//! - `JavaCallGrammar` — delegates to `StandardCallGrammar`
//!   (`name(args)` / `obj.name(args)` / `Type::name(args)`).
//! - `JavaImportGrammar` — `import X;` / `import X.$$$;` and
//!   `public class/interface/enum X { $$$ }` export forms.
//! - `JavaInheritanceGrammar` — `class/interface` extends/implements
//!   plus `@Override` method patterns.
//! - `JavaDecorationGrammar` — `@Foo` / `@Foo($$$)` annotations.
//! - `JavaTraceTemplate` — `System.err.println("[TRACE] var = " + $var);`.
//! - `JavaKindTaxonomy` — portable-kind ↔ tree-sitter-kind map.
//! - `JavaTopLevelKinds` — empty (Java isn't a data lang).
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

/// Java uses the generic `field("name") → field("key") → child(0)`
/// strategy — `class Foo`, `void bar()`, `interface Baz`, etc. all
/// expose the identifier under the `name` field.
pub struct JavaExtractor;
impl NameExtractor for JavaExtractor {
  fn extract<'a>(&self, node: &SgNode<'a>) -> Option<String> {
    GenericExtractor.extract(node)
  }
}

// ──────────────────────────── call grammar ────────────────────────

/// Java uses standard `name(args)` / `target.name(args)` /
/// `Type::name(args)` shapes; defer to `StandardCallGrammar`.
pub struct JavaCallGrammar;
impl CallGrammar for JavaCallGrammar {
  fn call_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.call_patterns(name)
  }
  fn callee_decl_patterns(&self, name: &str) -> Vec<String> {
    StandardCallGrammar.callee_decl_patterns(name)
  }
}

// ──────────────────────────── import grammar ──────────────────────

pub struct JavaImportGrammar;
impl ImportGrammar for JavaImportGrammar {
  fn import_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("import {name};"), format!("import {name}.$$$;")]
  }
  fn export_patterns(&self, name: &str) -> Vec<String> {
    // Java: `public class/interface/enum X` — explicit module
    // exports landed in JDK 9 (`module-info.java`), keep it
    // simple here.
    vec![
      format!("public class {name} {{ $$$ }}"),
      format!("public interface {name} {{ $$$ }}"),
      format!("public enum {name} {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── inheritance grammar ──────────────────

pub struct JavaInheritanceGrammar;
impl InheritanceGrammar for JavaInheritanceGrammar {
  fn supertype_decl_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class {name} extends $SUPER {{ $$$ }}"),
      format!("class {name} extends $SUPER implements $$$ {{ $$$ }}"),
      format!("class {name} implements $$$IFACES {{ $$$ }}"),
      format!("interface {name} extends $$$IFACES {{ $$$ }}"),
    ]
  }
  fn subtype_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("class $C extends {name} {{ $$$ }}"),
      format!("class $C extends {name} implements $$$ {{ $$$ }}"),
      format!("class $C implements $$$, {name}, $$$ {{ $$$ }}"),
      format!("class $C implements {name} {{ $$$ }}"),
      format!("interface $I extends $$$, {name}, $$$ {{ $$$ }}"),
      format!("interface $I extends {name} {{ $$$ }}"),
    ]
  }
  fn override_patterns(&self, name: &str) -> Vec<String> {
    vec![
      format!("@Override\n$RET {name}($$$) {{ $$$ }}"),
      format!("@Override $RET {name}($$$) {{ $$$ }}"),
      format!("@Override public $RET {name}($$$) {{ $$$ }}"),
      format!("@Override protected $RET {name}($$$) {{ $$$ }}"),
    ]
  }
}

// ─────────────────────────── decoration grammar ───────────────────

/// Java annotations: `@Foo`, `@Foo(...)`, plus the `@Foo.Bar` dotted
/// form occasionally produced by nested annotation classes.
pub struct JavaDecorationGrammar;
impl DecorationGrammar for JavaDecorationGrammar {
  fn decoration_patterns(&self, name: &str) -> Vec<String> {
    vec![format!("@{name}"), format!("@{name}($$$)")]
  }
}

// ─────────────────────────── trace template ───────────────────────

pub struct JavaTraceTemplate;
impl TraceTemplate for JavaTraceTemplate {
  fn template_for(&self, var: &str) -> Option<String> {
    Some(format!(
      "System.err.println(\"[TRACE] {var} = \" + ${var});"
    ))
  }
}

// ─────────────────────────── kind taxonomy ────────────────────────

pub struct JavaKindTaxonomy;
impl KindTaxonomy for JavaKindTaxonomy {
  fn kind_for(&self, p: PortableKind) -> &'static [&'static str] {
    match p {
      PortableKind::Function => &["method_declaration"],
      PortableKind::Method => &["method_declaration"],
      PortableKind::Class => &["class_declaration"],
      PortableKind::Struct => &[],
      PortableKind::Interface => &["interface_declaration"],
      PortableKind::Trait => &[],
      PortableKind::Enum => &["enum_declaration"],
      PortableKind::TypeAlias => &[],
      PortableKind::Const => &["field_declaration"],
      PortableKind::Macro => &[],
      PortableKind::Import => &["import_declaration"],
      PortableKind::Route => &[],
    }
  }
}

// ─────────────────────────── top-level kinds ──────────────────────

/// Java isn't a data language — empty impl. The trait defaults
/// fire (no child kinds, no top-level enumeration).
pub struct JavaTopLevelKinds;
impl TopLevelKinds for JavaTopLevelKinds {}
