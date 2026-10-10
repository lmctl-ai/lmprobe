//! The hand-maintained lmapi overlay (`crates/lmprobe/lmapi/overlay.toml`): published operation
//! names, access, dangerous ops and per-op parameter exclusions. Everything else about an
//! operation is generated from clap (see `generate.rs`).

use serde::Deserialize;

/// The overlay compiled into the binary.
pub(crate) const OVERLAY_TOML: &str = include_str!("../../lmapi/overlay.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Overlay {
  /// Clap commands (space-separated paths) that are never published.
  #[serde(default)]
  pub excluded: Vec<String>,
  /// Clap arg ids never published for any op.
  #[serde(default)]
  pub exclude_params: Vec<String>,
  /// Clap arg ids published only for the listed ops (e.g. `-A/-B/-C` only where consumed).
  #[serde(default)]
  pub only_for: std::collections::BTreeMap<String, Vec<String>>,
  #[serde(rename = "op", default)]
  pub ops: Vec<OverlayOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OverlayAccess {
  Read,
  Write,
  Dynamic,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StdinParam {
  pub name: String,
  pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OverlayOp {
  /// Published lmapi operation name (snake_case).
  pub name: String,
  /// Clap command path, words separated by spaces (`git status`).
  pub command: String,
  /// Required: a missing access must never default to read-only.
  pub access: OverlayAccess,
  /// Dangerous ops need `--confirm` (lmapi exit 9 without it).
  #[serde(default)]
  pub dangerous: bool,
  /// Replaces the clap `about` text.
  pub description: Option<String>,
  /// Clap arg ids not published for this op.
  #[serde(default)]
  pub exclude_params: Vec<String>,
  /// Legacy arguments always appended (e.g. `--apply`).
  #[serde(default)]
  pub fixed_args: Vec<String>,
  /// A list param fed to the legacy command on stdin, one entry per line.
  pub stdin_param: Option<StdinParam>,
  /// `false` for commands whose dispatch never reads the root common options (`CommonArgs`):
  /// none of them is published for the op.
  #[serde(default = "yes")]
  pub common_options: bool,
}

fn yes() -> bool {
  true
}

/// Parse an overlay document.
pub(crate) fn parse(text: &str) -> Result<Overlay, String> {
  toml::from_str(text).map_err(|error| format!("invalid lmapi overlay: {error}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shipped_overlay_parses() {
    let overlay = parse(OVERLAY_TOML).unwrap();
    assert!(overlay.ops.len() > 60);
    assert_eq!(overlay.excluded, ["serve", "help"]);
  }

  #[test]
  fn an_op_without_access_is_rejected() {
    let err = parse("[[op]]\nname = \"x\"\ncommand = \"x\"\n").unwrap_err();
    assert!(err.contains("access"), "{err}");
  }

  #[test]
  fn unknown_keys_and_bad_access_are_rejected() {
    assert!(parse("[[op]]\nname = \"x\"\ncommand = \"x\"\naccess = \"readonly\"\n").is_err());
    assert!(
      parse("[[op]]\nname = \"x\"\ncommand = \"x\"\naccess = \"read\"\ndangrous = true\n").is_err()
    );
  }
}
