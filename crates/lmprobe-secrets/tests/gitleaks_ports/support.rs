use lmprobe_secrets::{FileScanInput, Layer1Engine, SecretFinding};
use serde_json::Value;
use std::path::Path;
use std::process::Command;

pub const GITLEAKS_CONFIG: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/fixtures/gitleaks-default.toml"
);
pub const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

pub fn scan(config: &str, file_path: &str, content: &str) -> Vec<SecretFinding> {
  let engine = Layer1Engine::from_config_str(config).expect("build layer1 engine");
  engine.scan_file(FileScanInput {
    file_path,
    content,
    source_mode: "gitleaks-port",
  })
}

pub fn simple_akia_config(extra: &str) -> String {
  format!(
    r#"
title = "gitleaks port"

{extra}

[[rules]]
id = "aws-access-token"
description = "AWS access token"
regex = '''\b((?:A3T|AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{{16}})\b'''
entropy = 3
keywords = ["akia"]
"#
  )
}

pub fn simple_akia_config_with_rule_extra(rule_extra: &str) -> String {
  format!(
    r#"
title = "gitleaks port"

[[rules]]
id = "aws-access-token"
description = "AWS access token"
regex = '''\b((?:A3T|AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{{16}})\b'''
entropy = 3
keywords = ["akia"]

{rule_extra}
"#
  )
}

pub fn run_lmprobe_json(args: &[&str]) -> Value {
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args(["run", "-q", "-p", "lmprobe", "--", "--format", "json"])
    .args(args)
    .output()
    .expect("run lmprobe");
  assert!(
    matches!(output.status.code(), Some(0) | Some(1)),
    "unexpected lmprobe exit: status={} stderr={}",
    output.status,
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).expect("parse lmprobe json")
}

pub fn run_git(repo: &Path, args: &[&str]) {
  let status = Command::new("git")
    .current_dir(repo)
    .args(args)
    .status()
    .expect("run git");
  assert!(status.success(), "git {args:?} failed with {status}");
}
