use lmprobe_secrets::{FileScanInput, Layer1Engine};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FALLBACK_GITLEAKS_CONFIG: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/fixtures/gitleaks-default.toml"
);
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const PARITY_THRESHOLD: f64 = 1.0;

/// A built gitleaks checkout (binary at `<repo>/gitleaks`), taken from
/// `LMPROBE_GITLEAKS_REPO`. When unset, the parity tests are skipped.
fn gitleaks_repo() -> PathBuf {
  std::env::var_os("LMPROBE_GITLEAKS_REPO")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("/nonexistent/gitleaks"))
}

fn gitleaks_bin() -> PathBuf {
  gitleaks_repo().join("gitleaks")
}

fn gitleaks_testdata() -> PathBuf {
  gitleaks_repo().join("testdata")
}

fn gitleaks_config_path() -> PathBuf {
  std::env::var("LMPROBE_GITLEAKS_CONFIG")
    .ok()
    .and_then(|path| {
      let config_path = PathBuf::from(path);
      config_path.exists().then_some(config_path)
    })
    .unwrap_or_else(|| PathBuf::from(FALLBACK_GITLEAKS_CONFIG))
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct NormFinding {
  rule_id: String,
  file_path: String,
  line: usize,
  secret: String,
  match_text: String,
}

#[test]
fn gitleaks_testdata_and_indirection_synthetic_parity() {
  if !gitleaks_bin().exists() || !gitleaks_testdata().exists() {
    eprintln!("skipping gitleaks parity test; local gitleaks checkout is unavailable");
    return;
  }
  let temp = tempfile::tempdir().expect("tempdir");
  write_synthetic_cases(temp.path());

  let roots = [gitleaks_testdata(), temp.path().to_path_buf()];
  let gitleaks = run_gitleaks(&roots);
  let ours = run_ours(&roots);

  let matched = gitleaks.intersection(&ours).count();
  let missed: Vec<_> = gitleaks.difference(&ours).cloned().collect();
  let extra: Vec<_> = ours.difference(&gitleaks).cloned().collect();
  let denominator = gitleaks.len().max(1) as f64;
  let parity = matched as f64 / denominator;

  println!(
    "gitleaks={} ours={} matched={} missed={} extra={} parity={:.3}",
    gitleaks.len(),
    ours.len(),
    matched,
    missed.len(),
    extra.len(),
    parity
  );
  if !missed.is_empty() {
    println!("missed={missed:#?}");
  }
  if !extra.is_empty() {
    println!("extra={extra:#?}");
  }

  assert!(
    parity >= PARITY_THRESHOLD,
    "parity {parity:.3} below threshold {PARITY_THRESHOLD:.3}"
  );
  assert!(missed.is_empty(), "missed findings: {missed:#?}");
  assert!(extra.is_empty(), "extra findings: {extra:#?}");
}

#[test]
fn gitleaks_repo_git_config_discovery_uses_source_extension() {
  if !gitleaks_bin().exists() || !gitleaks_repo().exists() {
    eprintln!("skipping gitleaks config-discovery test; local gitleaks checkout is unavailable");
    return;
  }

  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args([
      "run",
      "-q",
      "-p",
      "lmprobe",
      "--",
      "--format",
      "json",
      "secrets",
      "--source",
      "git",
      "--gitleaks-compat",
    ])
    .arg(gitleaks_repo())
    .output()
    .expect("run lmprobe secrets git");
  assert!(
    matches!(output.status.code(), Some(0)),
    "expected no findings with discovered repo extension; status={} stderr={}",
    output.status,
    String::from_utf8_lossy(&output.stderr)
  );
  let value: Value = serde_json::from_slice(&output.stdout).expect("parse lmprobe envelope");
  assert_eq!(value["schema"], "lmprobe.secrets.v1");
  assert_eq!(value["findings"].as_array().expect("findings").len(), 0);
}

#[test]
fn gitleaksignore_does_not_suppress_by_path_and_rule_only() {
  let temp = tempfile::tempdir().expect("tempdir");
  run_git(temp.path(), &["init"]);
  run_git(
    temp.path(),
    &["config", "user.email", "lmprobe@example.test"],
  );
  run_git(temp.path(), &["config", "user.name", "lmprobe"]);
  fs::write(
    temp.path().join("secret.go"),
    "awsToken := \"AKIALALEMEL33243OLIA\"\n",
  )
  .expect("write secret");
  run_git(temp.path(), &["add", "secret.go"]);
  run_git(temp.path(), &["commit", "-m", "add secret"]);
  fs::write(
    temp.path().join(".gitleaksignore"),
    "secret.go:aws-access-token:999\n",
  )
  .expect("write gitleaksignore");

  let value = run_lmprobe_secrets(temp.path(), "git");
  let findings = value["findings"].as_array().expect("findings");
  assert_eq!(
    findings.len(),
    1,
    "same path/rule with a different line must not suppress: {value:#?}"
  );
  assert_eq!(findings[0]["rule_id"], "aws-access-token");
}

#[test]
fn gitleaksignore_suppresses_exact_filesystem_and_git_fingerprints() {
  let fs_temp = tempfile::tempdir().expect("tempdir");
  fs::write(
    fs_temp.path().join("secret.go"),
    "awsToken := \"AKIALALEMEL33243OLIA\"\n",
  )
  .expect("write fs secret");
  let fs_unsuppressed = run_lmprobe_secrets(fs_temp.path(), "filesystem");
  let fs_findings = fs_unsuppressed["findings"].as_array().expect("findings");
  assert_eq!(
    fs_findings.len(),
    1,
    "fs unsuppressed: {fs_unsuppressed:#?}"
  );
  let fs_fingerprint = fs_findings[0]["fingerprint"]
    .as_str()
    .expect("fs fingerprint");
  fs::write(
    fs_temp.path().join(".gitleaksignore"),
    format!("{fs_fingerprint}\n"),
  )
  .expect("write fs gitleaksignore");
  let fs_suppressed = run_lmprobe_secrets(fs_temp.path(), "filesystem");
  assert_eq!(
    fs_suppressed["findings"]
      .as_array()
      .expect("findings")
      .len(),
    0,
    "exact filesystem fingerprint must suppress: {fs_suppressed:#?}"
  );

  let git_temp = tempfile::tempdir().expect("tempdir");
  run_git(git_temp.path(), &["init"]);
  run_git(
    git_temp.path(),
    &["config", "user.email", "lmprobe@example.test"],
  );
  run_git(git_temp.path(), &["config", "user.name", "lmprobe"]);
  fs::write(
    git_temp.path().join("secret.go"),
    "awsToken := \"AKIALALEMEL33243OLIA\"\n",
  )
  .expect("write git secret");
  run_git(git_temp.path(), &["add", "secret.go"]);
  run_git(git_temp.path(), &["commit", "-m", "add secret"]);
  let git_unsuppressed = run_lmprobe_secrets(git_temp.path(), "git");
  let git_findings = git_unsuppressed["findings"].as_array().expect("findings");
  assert_eq!(
    git_findings.len(),
    1,
    "git unsuppressed: {git_unsuppressed:#?}"
  );
  let git_fingerprint = git_findings[0]["fingerprint"]
    .as_str()
    .expect("git fingerprint");
  assert_eq!(
    git_fingerprint.split(':').count(),
    4,
    "git fingerprint should be commit:path:rule:line"
  );
  fs::write(
    git_temp.path().join(".gitleaksignore"),
    format!("{git_fingerprint}\n"),
  )
  .expect("write git gitleaksignore");
  let git_suppressed = run_lmprobe_secrets(git_temp.path(), "git");
  assert_eq!(
    git_suppressed["findings"]
      .as_array()
      .expect("findings")
      .len(),
    0,
    "exact git fingerprint must suppress: {git_suppressed:#?}"
  );
}

fn write_synthetic_cases(root: &Path) {
  fs::write(
    root.join("app.py"),
    "key1 = open(\"key1.txt\").read().strip()\nclient.login(\"alice\", key1)\napi_key = \"S3cr3tV4lue_kJ8shdfLKJ23lkjsdf09sdfkjh23\"\nclient.login(api_key)\n",
  )
  .expect("write app.py");
  fs::write(
    root.join("key1.txt"),
    "AWS_ACCESS_KEY_ID=AKIAZ3M7Q6YB2N4P5R6S\nAWS_SECRET_ACCESS_KEY=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\n",
  )
  .expect("write key1.txt");
}

fn run_lmprobe_secrets(root: &Path, source: &str) -> Value {
  let gitleaks_config = gitleaks_config_path();
  let mut args = vec![
    "run", "-q", "-p", "lmprobe", "--", "--format", "json", "secrets",
  ];
  if source == "git" {
    args.extend(["--source", "git"]);
  }
  args.extend([
    "--gitleaks-compat",
    "--config",
    gitleaks_config.to_str().expect("utf8 config path"),
    root.to_str().expect("utf8 root"),
  ]);
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args(args)
    .output()
    .expect("run lmprobe secrets");
  assert!(
    matches!(output.status.code(), Some(0) | Some(1)),
    "unexpected lmprobe exit: status={} stderr={}",
    output.status,
    String::from_utf8_lossy(&output.stderr)
  );
  serde_json::from_slice(&output.stdout).expect("parse lmprobe envelope")
}

fn run_git(repo: &Path, args: &[&str]) {
  let status = Command::new("git")
    .current_dir(repo)
    .args(args)
    .status()
    .expect("run git");
  assert!(status.success(), "git {args:?} failed with {status}");
}

fn run_gitleaks(roots: &[PathBuf]) -> BTreeSet<NormFinding> {
  let gitleaks_config = gitleaks_config_path();
  roots
    .iter()
    .flat_map(|root| {
      let report = tempfile::NamedTempFile::new().expect("report file");
      let status = Command::new(gitleaks_bin())
        .args([
          "dir",
          root.to_str().expect("utf8 root"),
          "--config",
          gitleaks_config.to_str().expect("utf8 config path"),
          "--report-format",
          "json",
          "--report-path",
          report.path().to_str().expect("utf8 report"),
          "--no-color",
          "--redact=0",
          "--exit-code",
          "1",
        ])
        .status()
        .expect("run gitleaks");
      assert!(
        matches!(status.code(), Some(0) | Some(1)),
        "unexpected gitleaks exit: {status}"
      );
      let json = fs::read_to_string(report.path()).unwrap_or_else(|_| "[]".to_string());
      let values: Vec<Value> = serde_json::from_str(&json).expect("parse gitleaks report");
      values
        .into_iter()
        .map(|value| NormFinding {
          rule_id: value["RuleID"].as_str().unwrap().to_string(),
          file_path: value["File"].as_str().unwrap().to_string(),
          line: value["StartLine"].as_u64().unwrap() as usize,
          secret: value["Secret"].as_str().unwrap().to_string(),
          match_text: value["Match"].as_str().unwrap().to_string(),
        })
        .collect::<Vec<_>>()
    })
    .collect()
}

fn run_ours(roots: &[PathBuf]) -> BTreeSet<NormFinding> {
  let config = fs::read_to_string(gitleaks_config_path()).expect("read gitleaks config");
  let engine = Layer1Engine::from_config_str(&config).expect("build engine");
  roots
    .iter()
    .flat_map(|root| {
      let gitleaks_config = gitleaks_config_path();
      let output = Command::new("cargo")
        .current_dir(WORKSPACE_ROOT)
        .args([
          "run",
          "-q",
          "-p",
          "lmprobe",
          "--",
          "--format",
          "json",
          "secrets",
          "--gitleaks-compat",
          "--config",
          gitleaks_config.to_str().expect("utf8 config path"),
          root.to_str().expect("utf8 root"),
        ])
        .output()
        .expect("run lmprobe secrets");
      assert!(
        matches!(output.status.code(), Some(0) | Some(1)),
        "unexpected lmprobe exit: status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
      );
      let value: Value = serde_json::from_slice(&output.stdout).expect("parse lmprobe envelope");
      assert_eq!(value["schema"], "lmprobe.secrets.v1");
      value["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| hydrate_raw_finding(&engine, finding))
        .collect::<Vec<_>>()
    })
    .collect()
}

fn hydrate_raw_finding(engine: &Layer1Engine, finding: &Value) -> NormFinding {
  let rule_id = finding["rule_id"].as_str().unwrap();
  let file_path = finding["file_path"].as_str().unwrap();
  let line = finding["start_line"].as_u64().unwrap() as usize;
  let start_column = finding["start_column"].as_u64().unwrap() as usize;
  let end_column = finding["end_column"].as_u64().unwrap() as usize;
  let content = fs::read_to_string(file_path).expect("read file from lmprobe finding");
  let raw = engine
    .scan_file(FileScanInput {
      file_path,
      content: &content,
      source_mode: "filesystem-gitleaks-compat",
    })
    .into_iter()
    .find(|candidate| {
      candidate.rule_id == rule_id
        && candidate.start_line == line
        && candidate.start_column == start_column
        && candidate.end_column == end_column
    })
    .unwrap_or_else(|| panic!("hydrate raw secret for {file_path}:{line}:{rule_id}"));

  NormFinding {
    rule_id: raw.rule_id,
    file_path: raw.file_path,
    line: raw.start_line,
    secret: raw.redacted_secret.expose_raw().to_string(),
    match_text: raw.match_text.expose_raw().to_string(),
  }
}
