//! `lmprobe-types` — D4 envelope schemas and typed scalars.
//!
//! Phase 5A foundation crate. Pure data shapes; no parser deps,
//! no language registry, no sensor logic. Consumers:
//! `lmprobe-lang-core`, `lmprobe-registry`, `lmprobe-engine`,
//! `lmprobe` (binary).

pub mod error;
pub mod gql_scalars;
pub mod schema;

// Re-export the most-used items at crate root for convenience.
pub use error::{anyhow, Context, Result};
pub use schema::{Diagnostic, ExitStatus};
