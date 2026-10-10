//! v1.2 P0.5 source-flow baseline snapshots.
//!
//! These fixtures preserve current v1.1+P0 behavior before source DB scanner
//! integration. They intentionally contain explicit credential-source reads
//! flowing into provider sinks. Python was refreshed in P2 and JS/Go/Java
//! were refreshed in P3 with explicit reviewer-signoff protocol to include
//! `source_evidence[]` and `confirmed_flows[]`.
//!
//! Fixtures:
//! - Python: AWS env read to `boto3.client`, Stripe env read to `stripe.api_key`.
//! - JavaScript: Stripe env read to `require("stripe")`, GitHub env subscript
//!   to `new Octokit({ auth })`.
//! - Go: AWS env read to AWS SDK credentials, Stripe env read to `stripe.Key`.
//! - Java: `System.getenv` and `System.getProperty` into
//!   `BasicAWSCredentials`.
//!
//! See `fixtures/source_baseline/README.md` for known gaps and the
//! reviewer-signoff snapshot-change protocol.

use serde_json::{Map, Number, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const FIXTURE_ROOT: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/fixtures/source_baseline"
);
const CONFIG: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/fixtures/gitleaks-default.toml"
);

#[derive(Debug, Clone, Copy)]
struct FixtureCase {
  name: &'static str,
  dir: &'static str,
  expected_findings: usize,
  expected_semantic_findings: usize,
  expected_source_evidence: usize,
  expected_confirmed_flows: usize,
  expected_canonical_findings: usize,
}

const CASES: &[FixtureCase] = &[
  FixtureCase {
    name: "python",
    dir: "python",
    expected_findings: 2,
    expected_semantic_findings: 3,
    expected_source_evidence: 2,
    expected_confirmed_flows: 2,
    expected_canonical_findings: 6,
  },
  FixtureCase {
    name: "javascript",
    dir: "javascript",
    expected_findings: 1,
    expected_semantic_findings: 1,
    expected_source_evidence: 2,
    expected_confirmed_flows: 2,
    expected_canonical_findings: 3,
  },
  FixtureCase {
    name: "go",
    dir: "go",
    expected_findings: 2,
    expected_semantic_findings: 2,
    expected_source_evidence: 3,
    expected_confirmed_flows: 4,
    expected_canonical_findings: 6,
  },
  FixtureCase {
    name: "java",
    dir: "java",
    expected_findings: 2,
    expected_semantic_findings: 1,
    expected_source_evidence: 2,
    expected_confirmed_flows: 3,
    expected_canonical_findings: 5,
  },
];

#[test]
fn source_baseline_snapshots_are_byte_equal_after_normalization() {
  for case in CASES {
    let fixture_arg = format!(
      "crates/lmprobe-secrets/tests/fixtures/source_baseline/{}",
      case.dir
    );
    let fixture_dir = Path::new(FIXTURE_ROOT).join(case.dir);
    let expected_path = fixture_dir.join("expected.json");
    let expected = normalize_json(&fs::read(&expected_path).expect("read expected snapshot"));
    assert_redacted(&expected, case.name);

    let output = Command::new("cargo")
      .current_dir(WORKSPACE_ROOT)
      .args([
        "run",
        "-q",
        "-p",
        "lmprobe",
        "--",
        "secrets",
        "--format",
        "json",
        "--gitleaks-compat",
        "--config",
        CONFIG,
        &fixture_arg,
      ])
      .output()
      .expect("run lmprobe secrets source baseline");
    assert!(
      matches!(output.status.code(), Some(0) | Some(1)),
      "{}: unexpected exit status={} stderr={}",
      case.name,
      output.status,
      String::from_utf8_lossy(&output.stderr)
    );

    let actual = normalize_json(&output.stdout);
    assert_redacted(&actual, case.name);
    let actual_value: Value = serde_json::from_str(&actual).expect("actual json");
    assert_eq!(
      array_len(&actual_value, "findings"),
      case.expected_findings,
      "{} layer1 finding count",
      case.name
    );
    assert_eq!(
      array_len(&actual_value, "semantic_findings"),
      case.expected_semantic_findings,
      "{} semantic finding count",
      case.name
    );
    assert_eq!(
      array_len(&actual_value, "canonical_findings"),
      case.expected_canonical_findings,
      "{} canonical finding count",
      case.name
    );
    assert_eq!(
      array_len(&actual_value, "source_evidence"),
      case.expected_source_evidence,
      "{} source_evidence count",
      case.name
    );
    assert_eq!(
      array_len(&actual_value, "confirmed_flows"),
      case.expected_confirmed_flows,
      "{} confirmed_flows count",
      case.name
    );
    assert_eq!(
      actual, expected,
      "{} source baseline snapshot drift",
      case.name
    );
  }
}

fn array_len(value: &Value, key: &str) -> usize {
  value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn normalize_json(bytes: &[u8]) -> String {
  let value: Value = serde_json::from_slice(bytes).expect("parse lmprobe json");
  let sorted = sort_json(value);
  let mut out = serde_json::to_string_pretty(&sorted).expect("serialize normalized json");
  out.push('\n');
  out
}

fn sort_json(value: Value) -> Value {
  match value {
    Value::Array(items) => Value::Array(items.into_iter().map(sort_json).collect()),
    Value::Object(object) => {
      let mut sorted = Map::new();
      let mut entries = object.into_iter().collect::<Vec<_>>();
      entries.sort_by(|left, right| left.0.cmp(&right.0));
      for (key, value) in entries {
        sorted.insert(key.clone(), normalize_field(&key, value));
      }
      Value::Object(sorted)
    }
    other => other,
  }
}

fn normalize_field(key: &str, value: Value) -> Value {
  if key == "entropy" {
    if let Some(entropy) = value.as_f64() {
      let rounded = (entropy * 1_000_000_000_000.0).round() / 1_000_000_000_000.0;
      if let Some(number) = Number::from_f64(rounded) {
        return Value::Number(number);
      }
    }
  }
  sort_json(value)
}

fn assert_redacted(json: &str, name: &str) {
  for marker in ["wJalr", "sk_live", "ghp_", "AKIA", "FAKE"] {
    assert!(
      !json.contains(marker),
      "{name}: snapshot leaked raw secret marker {marker}"
    );
  }
}
