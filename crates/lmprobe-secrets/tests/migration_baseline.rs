use serde_json::{Map, Number, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const FIXTURE_ROOT: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/fixtures/migration_baseline"
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
}

const CASES: &[FixtureCase] = &[
  FixtureCase {
    name: "python",
    dir: "python",
    expected_findings: 2,
  },
  FixtureCase {
    name: "javascript",
    dir: "javascript",
    expected_findings: 2,
  },
  FixtureCase {
    name: "go",
    dir: "go",
    expected_findings: 1,
  },
  FixtureCase {
    name: "java",
    dir: "java",
    expected_findings: 1,
  },
];

#[test]
fn migration_baseline_snapshots_are_byte_equal_after_normalization() {
  // B2 non-lossy migration proof: this preserves the current hand-rolled
  // scanner/semantic output before P2/P3 provider-DB migration. Future
  // migrations must keep these normalized envelopes byte-equal unless a
  // reviewer explicitly accepts changed behavior and regenerates snapshots.
  // Failures on JS `require("stripe")(key)` after P2/P3 are planned recall
  // improvements; see fixtures/migration_baseline/README.md "Known gaps the
  // baseline currently encodes" before treating them as regressions.
  for case in CASES {
    let fixture_arg = format!(
      "crates/lmprobe-secrets/tests/fixtures/migration_baseline/{}",
      case.dir
    );
    let fixture_dir = Path::new(FIXTURE_ROOT).join(case.dir);
    let expected_path = fixture_dir.join("expected.json");
    let expected = normalize_json(&fs::read(&expected_path).expect("read expected snapshot"));
    assert_redacted(&expected, case.name);

    // Fixtures keep a `REMOVEME` mark inside Stripe-shaped keys so GitHub secret
    // scanning does not reject pushes; scan an unmarked copy at the same
    // workspace-relative path so reported paths match the snapshots.
    let scan_root = unmarked_copy(&fixture_arg, case.name);
    let output = Command::new("cargo")
      .current_dir(&scan_root)
      .args([
        "run",
        "-q",
        "--manifest-path",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"),
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
      .expect("run lmprobe secrets baseline");
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
      actual_value["findings"].as_array().expect("findings").len(),
      case.expected_findings,
      "{} finding count",
      case.name
    );
    assert_eq!(actual, expected, "{} baseline snapshot drift", case.name);
  }
}

fn unmarked_copy(rel_dir: &str, name: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("lmprobe-migration-{}-{name}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  copy_unmarked(
    &Path::new(WORKSPACE_ROOT).join(rel_dir),
    &root.join(rel_dir),
  );
  root
}

fn copy_unmarked(from: &Path, to: &Path) {
  fs::create_dir_all(to).expect("create scan dir");
  for entry in fs::read_dir(from).expect("read fixture dir") {
    let entry = entry.expect("fixture entry");
    let target = to.join(entry.file_name());
    if entry.file_type().expect("file type").is_dir() {
      copy_unmarked(&entry.path(), &target);
    } else {
      let bytes = fs::read(entry.path()).expect("read fixture");
      match String::from_utf8(bytes) {
        Ok(text) => fs::write(&target, text.replace("REMOVEME", "")),
        Err(err) => fs::write(&target, err.into_bytes()),
      }
      .expect("write fixture copy");
    }
  }
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
