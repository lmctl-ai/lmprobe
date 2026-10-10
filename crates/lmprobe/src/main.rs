//! `lmprobe` binary entry point.
//!
//! Phase 5A workspace split — module layout:
//! - `lmprobe-types` (external crate) — D4 envelopes, scalars, error types.
//! - `lmprobe-lang-core` (external) — D5 traits + per-language impls.
//! - `lmprobe-registry` (external) — per-language dispatch.
//! - `lmprobe-engine` (external) — sensors.
//! - This crate (binary) — CLI dispatch, GraphQL Query/Mutation, output rendering, workspace fan-out.
//!
//! Within this crate:
//! - [`cli`]       — clap CLI + dispatcher + GraphQL surface (`query` / `serve`).
//! - [`output`]    — render functions (schema → markdown / JSON).
//! - [`workspace`] — multi-repo `--workspace` config loader.
//!
//! Until 5A.1.5 + 5A.1.6 land, the binary still hosts the old
//! per-file modules (langs/sensors/history/etc.); they migrate to
//! lmprobe-engine + lmprobe-lang-core in subsequent commits.

// 5A.1: foundation crates own these modules now. Bridge the
// existing `use crate::xxx` style references via re-export so
// in-binary code (cli.rs, output.rs) compiles unchanged.
pub use lmprobe_engine::api_facts;
pub use lmprobe_engine::ast_diff_sensor;
pub use lmprobe_engine::async_leaks;
pub use lmprobe_engine::config_keys;
pub use lmprobe_engine::context_bundle;
pub use lmprobe_engine::crypto_sites;
pub use lmprobe_engine::diff_context;
pub use lmprobe_engine::fix_sensor;
pub use lmprobe_engine::graph_sensors;
pub use lmprobe_engine::guard_sites;
pub use lmprobe_engine::history;
pub use lmprobe_engine::history_sensors;
pub use lmprobe_engine::impact;
pub use lmprobe_engine::index;
pub use lmprobe_engine::pr_triage;
pub use lmprobe_engine::resource_sites;
pub use lmprobe_engine::rules;
pub use lmprobe_engine::secrets;
pub use lmprobe_engine::sensors;
pub use lmprobe_lang_core::langs;
pub use lmprobe_types::error;
pub use lmprobe_types::gql_scalars;
pub use lmprobe_types::schema;

mod aws_command;
mod aws_policy;
mod cli;
mod edit_command;
mod git_command;
mod lmapi;
mod output;
mod program_output;
mod text_edit;
mod workspace;

fn main() -> error::Result<()> {
  cli::run()
}
