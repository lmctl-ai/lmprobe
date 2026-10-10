//! Phase 4D §4.7 — multi-repo workspace config.
//!
//! `lmprobe.workspace.yaml`:
//! ```yaml
//! roots:
//!   - path: /home/me/repo1
//!     alias: api          # optional; falls back to basename
//!   - path: /home/me/repo2
//!     alias: web
//! ```
//!
//! Loaded once at CLI dispatch when `--workspace <path>` is set.
//! The dispatcher fans out across roots, runs each read-only verb,
//! tags every hit's `origin_root`, and merges results into one
//! envelope. Mutation verbs (`fix --apply`, `trace`) refuse
//! workspace mode and emit `WORKSPACE_READ_ONLY`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{anyhow, Context, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRootRaw {
  pub path: String,
  #[serde(default)]
  pub alias: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceConfigRaw {
  pub roots: Vec<WorkspaceRootRaw>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
  pub path: PathBuf,
  pub label: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceCtx {
  pub roots: Vec<WorkspaceRoot>,
}

impl WorkspaceCtx {
  pub fn load(yaml_path: &Path) -> Result<Self> {
    let content = std::fs::read_to_string(yaml_path)
      .with_context(|| format!("could not read workspace YAML at {}", yaml_path.display()))?;
    let raw: WorkspaceConfigRaw = serde_yaml::from_str(&content)
      .with_context(|| format!("could not parse workspace YAML at {}", yaml_path.display()))?;
    if raw.roots.is_empty() {
      return Err(anyhow!("workspace YAML has no `roots` entries"));
    }
    let mut roots = Vec::with_capacity(raw.roots.len());
    let mut seen_labels: HashSet<String> = HashSet::new();
    for r in raw.roots {
      let path = PathBuf::from(&r.path);
      let label = r.alias.unwrap_or_else(|| {
        path
          .file_name()
          .and_then(|n| n.to_str())
          .map(|s| s.to_string())
          .unwrap_or_else(|| r.path.clone())
      });
      if !seen_labels.insert(label.clone()) {
        return Err(anyhow!(
          "duplicate workspace root label `{}` — set distinct `alias`es",
          label
        ));
      }
      roots.push(WorkspaceRoot { path, label });
    }
    Ok(WorkspaceCtx { roots })
  }
}
