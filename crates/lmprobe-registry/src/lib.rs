//! `lmprobe-registry` — per-language dispatch fan-out.
//!
//! Phase 5A foundation crate. Resolves the W2 dispatch-cycle: each
//! per-language crate (`lmprobe-lang-rust`, `lmprobe-lang-python`,
//! …) depends on `lmprobe-lang-core` for the trait definitions; this
//! crate depends on all of them and on `lmprobe-lang-core`, exposing
//! the central `name_extractor_for(SgLang)` etc. dispatch fns the
//! engine + binary call.
//!
//! Each dispatch fn has a `match` whose arms are bracketed by
//! `xtask:per-lang:<fn>:start` / `:end` marker comments. `cargo
//! xtask new-lang <lang>` inserts a per-language `SgLang::<Lang>
//! => &lmprobe_lang_<lang>::<Lang><Trait>` arm into each dispatch
//! fn's marker region; the catch-all `_` arm delegates to
//! lang-core's per-language impls (still hosted there during 5A;
//! migrate progressively in 5B as each language extracts).
//!
//! Cycle-free at every step: lang-core never depends on a per-lang
//! crate; per-lang crates depend only on lang-core + types; this
//! crate aggregates them all.

use lmprobe_lang_core::langs;

// Re-export trait + types from lang-core so consumers only need
// `use lmprobe_registry::{NameExtractor, SgLang, …}`.
pub use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, NameExtractor,
  SgLang, SgNode, TopLevelKinds, TraceTemplate,
};

// ───────────── Dispatch fns (xtask-managed regions) ──────────────
//
// Phase 5B capstone: all 29 per-language crates are now extracted,
// so every SgLang variant has an explicit arm. The `_ =>` catch-all
// arms (delegating to lang-core) are kept for xtask marker structure
// but are unreachable at compile time.

#[inline]
pub fn name_extractor_for(lang: SgLang) -> &'static dyn NameExtractor {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:name_extractor_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxExtractor,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptExtractor,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptExtractor,
    SgLang::Css => &lmprobe_lang_css::CssExtractor,
    SgLang::Html => &lmprobe_lang_html::HtmlExtractor,
    SgLang::Xml => &lmprobe_lang_xml::XmlExtractor,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlExtractor,
    SgLang::Json => &lmprobe_lang_json::JsonExtractor,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityExtractor,
    SgLang::Hcl => &lmprobe_lang_hcl::HclExtractor,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellExtractor,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirExtractor,
    SgLang::Nix => &lmprobe_lang_nix::NixExtractor,
    SgLang::Lua => &lmprobe_lang_lua::LuaExtractor,
    SgLang::Bash => &lmprobe_lang_bash::BashExtractor,
    SgLang::Dart => &lmprobe_lang_dart::DartExtractor,
    SgLang::Php => &lmprobe_lang_php::PhpExtractor,
    SgLang::C => &lmprobe_lang_c::CExtractor,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyExtractor,
    SgLang::Go => &lmprobe_lang_go::GoExtractor,
    SgLang::Cpp => &lmprobe_lang_cpp::CppExtractor,
    SgLang::Swift => &lmprobe_lang_swift::SwiftExtractor,
    SgLang::Scala => &lmprobe_lang_scala::ScalaExtractor,
    SgLang::Python => &lmprobe_lang_python::PythonExtractor,
    SgLang::Java => &lmprobe_lang_java::JavaExtractor,
    SgLang::Toml => &lmprobe_lang_toml::TomlExtractor,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpExtractor,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinExtractor,
    SgLang::Rust => &lmprobe_lang_rust::RustExtractor,
    // xtask:per-lang:name_extractor_for:end
    _ => langs::name_extractor_for(lang),
  }
}

#[inline]
pub fn call_grammar_for(lang: SgLang) -> &'static dyn CallGrammar {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:call_grammar_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxCallGrammar,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptCallGrammar,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptCallGrammar,
    SgLang::Css => &lmprobe_lang_css::CssCallGrammar,
    SgLang::Html => &lmprobe_lang_html::HtmlCallGrammar,
    SgLang::Xml => &lmprobe_lang_xml::XmlCallGrammar,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlCallGrammar,
    SgLang::Json => &lmprobe_lang_json::JsonCallGrammar,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityCallGrammar,
    SgLang::Hcl => &lmprobe_lang_hcl::HclCallGrammar,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellCallGrammar,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirCallGrammar,
    SgLang::Nix => &lmprobe_lang_nix::NixCallGrammar,
    SgLang::Lua => &lmprobe_lang_lua::LuaCallGrammar,
    SgLang::Bash => &lmprobe_lang_bash::BashCallGrammar,
    SgLang::Dart => &lmprobe_lang_dart::DartCallGrammar,
    SgLang::Php => &lmprobe_lang_php::PhpCallGrammar,
    SgLang::C => &lmprobe_lang_c::CCallGrammar,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyCallGrammar,
    SgLang::Go => &lmprobe_lang_go::GoCallGrammar,
    SgLang::Cpp => &lmprobe_lang_cpp::CppCallGrammar,
    SgLang::Swift => &lmprobe_lang_swift::SwiftCallGrammar,
    SgLang::Scala => &lmprobe_lang_scala::ScalaCallGrammar,
    SgLang::Python => &lmprobe_lang_python::PythonCallGrammar,
    SgLang::Java => &lmprobe_lang_java::JavaCallGrammar,
    SgLang::Toml => &lmprobe_lang_toml::TomlCallGrammar,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpCallGrammar,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinCallGrammar,
    SgLang::Rust => &lmprobe_lang_rust::RustCallGrammar,
    // xtask:per-lang:call_grammar_for:end
    _ => langs::call_grammar_for(lang),
  }
}

#[inline]
pub fn import_grammar_for(lang: SgLang) -> &'static dyn ImportGrammar {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:import_grammar_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxImportGrammar,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptImportGrammar,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptImportGrammar,
    SgLang::Css => &lmprobe_lang_css::CssImportGrammar,
    SgLang::Html => &lmprobe_lang_html::HtmlImportGrammar,
    SgLang::Xml => &lmprobe_lang_xml::XmlImportGrammar,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlImportGrammar,
    SgLang::Json => &lmprobe_lang_json::JsonImportGrammar,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityImportGrammar,
    SgLang::Hcl => &lmprobe_lang_hcl::HclImportGrammar,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellImportGrammar,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirImportGrammar,
    SgLang::Nix => &lmprobe_lang_nix::NixImportGrammar,
    SgLang::Lua => &lmprobe_lang_lua::LuaImportGrammar,
    SgLang::Bash => &lmprobe_lang_bash::BashImportGrammar,
    SgLang::Dart => &lmprobe_lang_dart::DartImportGrammar,
    SgLang::Php => &lmprobe_lang_php::PhpImportGrammar,
    SgLang::C => &lmprobe_lang_c::CImportGrammar,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyImportGrammar,
    SgLang::Go => &lmprobe_lang_go::GoImportGrammar,
    SgLang::Cpp => &lmprobe_lang_cpp::CppImportGrammar,
    SgLang::Swift => &lmprobe_lang_swift::SwiftImportGrammar,
    SgLang::Scala => &lmprobe_lang_scala::ScalaImportGrammar,
    SgLang::Python => &lmprobe_lang_python::PythonImportGrammar,
    SgLang::Java => &lmprobe_lang_java::JavaImportGrammar,
    SgLang::Toml => &lmprobe_lang_toml::TomlImportGrammar,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpImportGrammar,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinImportGrammar,
    SgLang::Rust => &lmprobe_lang_rust::RustImportGrammar,
    // xtask:per-lang:import_grammar_for:end
    _ => langs::import_grammar_for(lang),
  }
}

#[inline]
pub fn inheritance_grammar_for(lang: SgLang) -> &'static dyn InheritanceGrammar {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:inheritance_grammar_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxInheritanceGrammar,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptInheritanceGrammar,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptInheritanceGrammar,
    SgLang::Css => &lmprobe_lang_css::CssInheritanceGrammar,
    SgLang::Html => &lmprobe_lang_html::HtmlInheritanceGrammar,
    SgLang::Xml => &lmprobe_lang_xml::XmlInheritanceGrammar,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlInheritanceGrammar,
    SgLang::Json => &lmprobe_lang_json::JsonInheritanceGrammar,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityInheritanceGrammar,
    SgLang::Hcl => &lmprobe_lang_hcl::HclInheritanceGrammar,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellInheritanceGrammar,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirInheritanceGrammar,
    SgLang::Nix => &lmprobe_lang_nix::NixInheritanceGrammar,
    SgLang::Lua => &lmprobe_lang_lua::LuaInheritanceGrammar,
    SgLang::Bash => &lmprobe_lang_bash::BashInheritanceGrammar,
    SgLang::Dart => &lmprobe_lang_dart::DartInheritanceGrammar,
    SgLang::Php => &lmprobe_lang_php::PhpInheritanceGrammar,
    SgLang::C => &lmprobe_lang_c::CInheritanceGrammar,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyInheritanceGrammar,
    SgLang::Go => &lmprobe_lang_go::GoInheritanceGrammar,
    SgLang::Cpp => &lmprobe_lang_cpp::CppInheritanceGrammar,
    SgLang::Swift => &lmprobe_lang_swift::SwiftInheritanceGrammar,
    SgLang::Scala => &lmprobe_lang_scala::ScalaInheritanceGrammar,
    SgLang::Python => &lmprobe_lang_python::PythonInheritanceGrammar,
    SgLang::Java => &lmprobe_lang_java::JavaInheritanceGrammar,
    SgLang::Toml => &lmprobe_lang_toml::TomlInheritanceGrammar,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpInheritanceGrammar,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinInheritanceGrammar,
    SgLang::Rust => &lmprobe_lang_rust::RustInheritanceGrammar,
    // xtask:per-lang:inheritance_grammar_for:end
    _ => langs::inheritance_grammar_for(lang),
  }
}

#[inline]
pub fn decoration_grammar_for(lang: SgLang) -> &'static dyn DecorationGrammar {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:decoration_grammar_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxDecorationGrammar,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptDecorationGrammar,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptDecorationGrammar,
    SgLang::Css => &lmprobe_lang_css::CssDecorationGrammar,
    SgLang::Html => &lmprobe_lang_html::HtmlDecorationGrammar,
    SgLang::Xml => &lmprobe_lang_xml::XmlDecorationGrammar,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlDecorationGrammar,
    SgLang::Json => &lmprobe_lang_json::JsonDecorationGrammar,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityDecorationGrammar,
    SgLang::Hcl => &lmprobe_lang_hcl::HclDecorationGrammar,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellDecorationGrammar,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirDecorationGrammar,
    SgLang::Nix => &lmprobe_lang_nix::NixDecorationGrammar,
    SgLang::Lua => &lmprobe_lang_lua::LuaDecorationGrammar,
    SgLang::Bash => &lmprobe_lang_bash::BashDecorationGrammar,
    SgLang::Dart => &lmprobe_lang_dart::DartDecorationGrammar,
    SgLang::Php => &lmprobe_lang_php::PhpDecorationGrammar,
    SgLang::C => &lmprobe_lang_c::CDecorationGrammar,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyDecorationGrammar,
    SgLang::Go => &lmprobe_lang_go::GoDecorationGrammar,
    SgLang::Cpp => &lmprobe_lang_cpp::CppDecorationGrammar,
    SgLang::Swift => &lmprobe_lang_swift::SwiftDecorationGrammar,
    SgLang::Scala => &lmprobe_lang_scala::ScalaDecorationGrammar,
    SgLang::Python => &lmprobe_lang_python::PythonDecorationGrammar,
    SgLang::Java => &lmprobe_lang_java::JavaDecorationGrammar,
    SgLang::Toml => &lmprobe_lang_toml::TomlDecorationGrammar,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpDecorationGrammar,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinDecorationGrammar,
    SgLang::Rust => &lmprobe_lang_rust::RustDecorationGrammar,
    // xtask:per-lang:decoration_grammar_for:end
    _ => langs::decoration_grammar_for(lang),
  }
}

#[inline]
pub fn trace_template_for(lang: SgLang) -> &'static dyn TraceTemplate {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:trace_template_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxTraceTemplate,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptTraceTemplate,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptTraceTemplate,
    SgLang::Css => &lmprobe_lang_css::CssTraceTemplate,
    SgLang::Html => &lmprobe_lang_html::HtmlTraceTemplate,
    SgLang::Xml => &lmprobe_lang_xml::XmlTraceTemplate,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlTraceTemplate,
    SgLang::Json => &lmprobe_lang_json::JsonTraceTemplate,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityTraceTemplate,
    SgLang::Hcl => &lmprobe_lang_hcl::HclTraceTemplate,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellTraceTemplate,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirTraceTemplate,
    SgLang::Nix => &lmprobe_lang_nix::NixTraceTemplate,
    SgLang::Lua => &lmprobe_lang_lua::LuaTraceTemplate,
    SgLang::Bash => &lmprobe_lang_bash::BashTraceTemplate,
    SgLang::Dart => &lmprobe_lang_dart::DartTraceTemplate,
    SgLang::Php => &lmprobe_lang_php::PhpTraceTemplate,
    SgLang::C => &lmprobe_lang_c::CTraceTemplate,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyTraceTemplate,
    SgLang::Go => &lmprobe_lang_go::GoTraceTemplate,
    SgLang::Cpp => &lmprobe_lang_cpp::CppTraceTemplate,
    SgLang::Swift => &lmprobe_lang_swift::SwiftTraceTemplate,
    SgLang::Scala => &lmprobe_lang_scala::ScalaTraceTemplate,
    SgLang::Python => &lmprobe_lang_python::PythonTraceTemplate,
    SgLang::Java => &lmprobe_lang_java::JavaTraceTemplate,
    SgLang::Toml => &lmprobe_lang_toml::TomlTraceTemplate,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpTraceTemplate,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinTraceTemplate,
    SgLang::Rust => &lmprobe_lang_rust::RustTraceTemplate,
    // xtask:per-lang:trace_template_for:end
    _ => langs::trace_template_for(lang),
  }
}

#[inline]
pub fn taxonomy_for(lang: SgLang) -> &'static dyn KindTaxonomy {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:taxonomy_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxKindTaxonomy,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptKindTaxonomy,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptKindTaxonomy,
    SgLang::Css => &lmprobe_lang_css::CssKindTaxonomy,
    SgLang::Html => &lmprobe_lang_html::HtmlKindTaxonomy,
    SgLang::Xml => &lmprobe_lang_xml::XmlKindTaxonomy,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlKindTaxonomy,
    SgLang::Json => &lmprobe_lang_json::JsonKindTaxonomy,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityKindTaxonomy,
    SgLang::Hcl => &lmprobe_lang_hcl::HclKindTaxonomy,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellKindTaxonomy,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirKindTaxonomy,
    SgLang::Nix => &lmprobe_lang_nix::NixKindTaxonomy,
    SgLang::Lua => &lmprobe_lang_lua::LuaKindTaxonomy,
    SgLang::Bash => &lmprobe_lang_bash::BashKindTaxonomy,
    SgLang::Dart => &lmprobe_lang_dart::DartKindTaxonomy,
    SgLang::Php => &lmprobe_lang_php::PhpKindTaxonomy,
    SgLang::C => &lmprobe_lang_c::CKindTaxonomy,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyKindTaxonomy,
    SgLang::Go => &lmprobe_lang_go::GoKindTaxonomy,
    SgLang::Cpp => &lmprobe_lang_cpp::CppKindTaxonomy,
    SgLang::Swift => &lmprobe_lang_swift::SwiftKindTaxonomy,
    SgLang::Scala => &lmprobe_lang_scala::ScalaKindTaxonomy,
    SgLang::Python => &lmprobe_lang_python::PythonKindTaxonomy,
    SgLang::Java => &lmprobe_lang_java::JavaKindTaxonomy,
    SgLang::Toml => &lmprobe_lang_toml::TomlKindTaxonomy,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpKindTaxonomy,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinKindTaxonomy,
    SgLang::Rust => &lmprobe_lang_rust::RustKindTaxonomy,
    // xtask:per-lang:taxonomy_for:end
    _ => langs::taxonomy_for(lang),
  }
}

#[inline]
pub fn top_level_kinds_for(lang: SgLang) -> &'static dyn TopLevelKinds {
  #[allow(unreachable_patterns)]
  match lang {
    // xtask:per-lang:top_level_kinds_for:start
    SgLang::Tsx => &lmprobe_lang_tsx::TsxTopLevelKinds,
    SgLang::TypeScript => &lmprobe_lang_typescript::TypeScriptTopLevelKinds,
    SgLang::JavaScript => &lmprobe_lang_javascript::JavaScriptTopLevelKinds,
    SgLang::Css => &lmprobe_lang_css::CssTopLevelKinds,
    SgLang::Html => &lmprobe_lang_html::HtmlTopLevelKinds,
    SgLang::Xml => &lmprobe_lang_xml::XmlTopLevelKinds,
    SgLang::Yaml => &lmprobe_lang_yaml::YamlTopLevelKinds,
    SgLang::Json => &lmprobe_lang_json::JsonTopLevelKinds,
    SgLang::Solidity => &lmprobe_lang_solidity::SolidityTopLevelKinds,
    SgLang::Hcl => &lmprobe_lang_hcl::HclTopLevelKinds,
    SgLang::Haskell => &lmprobe_lang_haskell::HaskellTopLevelKinds,
    SgLang::Elixir => &lmprobe_lang_elixir::ElixirTopLevelKinds,
    SgLang::Nix => &lmprobe_lang_nix::NixTopLevelKinds,
    SgLang::Lua => &lmprobe_lang_lua::LuaTopLevelKinds,
    SgLang::Bash => &lmprobe_lang_bash::BashTopLevelKinds,
    SgLang::Dart => &lmprobe_lang_dart::DartTopLevelKinds,
    SgLang::Php => &lmprobe_lang_php::PhpTopLevelKinds,
    SgLang::C => &lmprobe_lang_c::CTopLevelKinds,
    SgLang::Ruby => &lmprobe_lang_ruby::RubyTopLevelKinds,
    SgLang::Go => &lmprobe_lang_go::GoTopLevelKinds,
    SgLang::Cpp => &lmprobe_lang_cpp::CppTopLevelKinds,
    SgLang::Swift => &lmprobe_lang_swift::SwiftTopLevelKinds,
    SgLang::Scala => &lmprobe_lang_scala::ScalaTopLevelKinds,
    SgLang::Python => &lmprobe_lang_python::PythonTopLevelKinds,
    SgLang::Java => &lmprobe_lang_java::JavaTopLevelKinds,
    SgLang::Toml => &lmprobe_lang_toml::TomlTopLevelKinds,
    SgLang::CSharp => &lmprobe_lang_csharp::CSharpTopLevelKinds,
    SgLang::Kotlin => &lmprobe_lang_kotlin::KotlinTopLevelKinds,
    SgLang::Rust => &lmprobe_lang_rust::RustTopLevelKinds,
    // xtask:per-lang:top_level_kinds_for:end
    _ => langs::top_level_kinds_for(lang),
  }
}
