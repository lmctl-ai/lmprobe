//! `lmprobe-engine` — language-agnostic + dispatch-driven sensors.
//!
//! Phase 5A foundation crate. Hosts `search`, `def`, `ref`, `find`,
//! `grep`, `discover`, `callers`, `callees`, `imports`, `decorations`,
//! `supertypes`, `subtypes`, `overrides`, `exports`, `impact`,
//! `context`, `history.*`, `evolution`, `ast-diff`, `fix`, `trace`,
//! `untrace`. Plus the cache layer (history.rs) and walk helpers.
//!
//! Sensors that need per-language behavior call into
//! `lmprobe-registry` for dispatch — never `match lang {…}` in
//! sensor code.

pub mod api_facts;
pub mod ast_diff_sensor;
pub mod async_leaks;
pub mod config_keys;
pub mod context_bundle;
pub mod crypto_sites;
pub mod diff_context;
pub mod file_actions;
pub mod file_predicate;
pub mod fix_sensor;
pub mod graph_sensors;
pub mod guard_sites;
pub mod history;
pub mod history_sensors;
pub mod impact;
pub mod index;
pub mod pr_triage;
pub mod resource_sites;
pub mod rules;
pub mod secrets;
pub mod sensors;
