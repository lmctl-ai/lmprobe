//! `lmprobe-lang-core` — D5 capability traits and SgLang re-export.
//!
//! Phase 5A foundation crate. Trait definitions for `NameExtractor`,
//! `CallGrammar`, `ImportGrammar`, `InheritanceGrammar`,
//! `DecorationGrammar`, `TraceTemplate`, `KindTaxonomy`,
//! `TopLevelKinds`. Per-language impls live here during 5A; they
//! migrate to per-language crates (`lmprobe-lang-<name>`) in 5B.
//!
//! **Does NOT depend on any concrete `lmprobe-lang-*` crate** — that
//! would create a cycle. Per-language dispatch lives in
//! `lmprobe-registry`, which depends on this crate plus all
//! per-language crates as they're extracted.

pub mod langs;

// Top-level re-exports for convenience. Downstream crates can
// `use lmprobe_lang_core::SgLang` instead of the longer path.
pub use langs::SgLang;
