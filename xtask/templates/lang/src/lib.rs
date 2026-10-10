//! `lmprobe-lang-{{lang}}` — D5 capability impls for {{lang}}.
//!
//! Phase 5B per-language crate. Implements the trait surface
//! defined in `lmprobe-lang-core`:
//!
//! - `NameExtractor`     → `{{Lang}}Extractor`
//! - `CallGrammar`       → `{{Lang}}CallGrammar`
//! - `ImportGrammar`     → `{{Lang}}ImportGrammar`
//! - `InheritanceGrammar`→ `{{Lang}}InheritanceGrammar`
//! - `DecorationGrammar` → `{{Lang}}DecorationGrammar`
//! - `TraceTemplate`     → `{{Lang}}TraceTemplate`
//! - `KindTaxonomy`      → `{{Lang}}KindTaxonomy`
//! - `TopLevelKinds`     → `{{Lang}}TopLevelKinds`
//!
//! Each impl block returns the empty / `None` shape ("not
//! implemented for this language") so the crate compiles as a
//! stub. Per-language Coders override what {{lang}} actually
//! supports as they fill in the implementation. The
//! `#[allow(dead_code)]` lets a stub crate compile cleanly
//! while a Coder ramps up.
//!
//! Trait API constraint (W4): methods take/return only
//! lang-core types + ast-grep-core types + primitives. Never
//! `lmprobe-types` envelope types — keeps this crate from
//! transitively pulling async-graphql into per-language builds.

#![allow(dead_code)]

use lmprobe_lang_core::langs::{
    CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy,
    NameExtractor, PortableKind, SgNode, TopLevelKinds, TraceTemplate,
};

pub struct {{Lang}}Extractor;
impl NameExtractor for {{Lang}}Extractor {
    fn extract<'a>(&self, _node: &SgNode<'a>) -> Option<String> {
        // Override with per-language declaration-name extraction
        // when the GenericExtractor doesn't fit.
        None
    }
}

pub struct {{Lang}}CallGrammar;
impl CallGrammar for {{Lang}}CallGrammar {
    fn call_patterns(&self, _name: &str) -> Vec<String> {
        // Stub — override when {{lang}}'s call shape is wired.
        Vec::new()
    }
}

pub struct {{Lang}}ImportGrammar;
impl ImportGrammar for {{Lang}}ImportGrammar {
    fn import_patterns(&self, _name: &str) -> Vec<String> {
        Vec::new()
    }
}

pub struct {{Lang}}InheritanceGrammar;
impl InheritanceGrammar for {{Lang}}InheritanceGrammar {}

pub struct {{Lang}}DecorationGrammar;
impl DecorationGrammar for {{Lang}}DecorationGrammar {
    fn decoration_patterns(&self, _name: &str) -> Vec<String> {
        Vec::new()
    }
}

pub struct {{Lang}}TraceTemplate;
impl TraceTemplate for {{Lang}}TraceTemplate {
    fn template_for(&self, _var: &str) -> Option<String> {
        None
    }
}

pub struct {{Lang}}KindTaxonomy;
impl KindTaxonomy for {{Lang}}KindTaxonomy {
    fn kind_for(&self, _portable: PortableKind) -> &'static [&'static str] {
        &[]
    }
}

pub struct {{Lang}}TopLevelKinds;
impl TopLevelKinds for {{Lang}}TopLevelKinds {}
