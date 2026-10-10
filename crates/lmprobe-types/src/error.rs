//! Crate-level error / result aliases.
//!
//! Phase 0 keeps the existing `anyhow` dependency. Phase 4+ may
//! introduce a structured error enum here as needed.

pub use anyhow::{anyhow, Context, Result};
