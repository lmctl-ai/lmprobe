use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  _tmp: tempfile::TempDir,
  root: PathBuf,
}

fn fixture() -> Fixture {
  let tmp = tempfile::Builder::new()
    .prefix("lmprobe-crypto-sites-")
    .tempdir()
    .expect("create fixture root");
  let root = tmp.path().to_path_buf();
  fs::create_dir_all(root.join("pkg")).expect("create fixture dirs");
  fs::write(
    root.join("pkg/crypto.go"),
    r#"//go:build boringcrypto

package pkg

import (
	"crypto/md5"
	"crypto/sha1"
)

// sha1.New() in a comment is not an anchor.
/*
md5.Sum(nil) in a block comment is not an anchor.
*/
func guardedHash(data []byte) []byte {
	if gofipsMode() || boringcryptoEnabled() {
		return data
	}
	h := sha1.New()
	return h.Sum(data)
}

func unguardedEvidence(data []byte) [16]byte {
	return md5.Sum(data)
}
"#,
  )
  .expect("write go fixture");
  Fixture { _tmp: tmp, root }
}

fn run_raw(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let (stdout, stderr, exit) = run_raw(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

#[test]
fn crypto_sites_reports_go_algorithm_and_fips_evidence() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "crypto-sites",
      ".",
      "--algorithm",
      "sha1",
      "--algorithm",
      "md5",
      "--fips-check",
      "--around",
      "4",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.crypto_sites.v1");
  let sites = envelope["sites"].as_array().expect("sites");
  assert!(
    sites.len() >= 2,
    "expected import and/or call anchors:\n{envelope:#}"
  );
  assert!(
    sites
      .iter()
      .all(|site| site["confidence"] == "syntax-evidence-only"),
    "runtime output should stay evidence-only:\n{envelope:#}"
  );
  let rendered = envelope.to_string().to_ascii_lowercase();
  assert!(
    !rendered.contains("vulnerability")
      && !rendered.contains("non-compliance")
      && !rendered.contains("noncompliance"),
    "crypto-sites should not emit verdict language:\n{envelope:#}"
  );

  let sha1_call = sites
    .iter()
    .find(|site| site["line_content"] == "\th := sha1.New()")
    .expect("sha1.New call site");
  assert_eq!(sha1_call["algorithm"], "sha1");
  assert!(
    sha1_call["import_paths"]
      .as_array()
      .is_some_and(|imports| imports.iter().any(|path| path == "crypto/sha1")),
    "sha1 import evidence missing:\n{sha1_call:#}"
  );
  assert!(
    sha1_call["nearby_guards"]
      .as_array()
      .is_some_and(|guards| guards.iter().any(|guard| guard["match_text"]
        .as_str()
        .unwrap_or_default()
        .eq_ignore_ascii_case("gofips")
        || guard["match_text"]
          .as_str()
          .unwrap_or_default()
          .eq_ignore_ascii_case("boringcrypto"))),
    "nearby fips/boringcrypto evidence missing:\n{sha1_call:#}"
  );
  assert!(
    sha1_call["parents"]
      .as_array()
      .is_some_and(|parents| !parents.is_empty())
      && sha1_call.get("body").is_some()
      && sha1_call["source_context"]
        .as_array()
        .is_some_and(|context| !context.is_empty()),
    "requested context/parents/body should be populated:\n{sha1_call:#}"
  );

  assert!(
    sites.iter().all(|site| {
      let line = site["line_content"].as_str().unwrap_or_default();
      !line.contains("comment is not an anchor") && !line.contains("block comment")
    }),
    "comment-only matches should be skipped:\n{envelope:#}"
  );
}

#[test]
fn crypto_sites_ndjson_markdown_missing_path_and_graphql() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "crypto-sites",
      ".",
      "--algorithm",
      "sha1",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert!(lines.len() >= 2, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("site item");
  assert!(item.get("algorithm").is_some(), "item:\n{item:#}");
  let summary: serde_json::Value =
    serde_json::from_str(lines.last().expect("summary line")).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("sites").is_none());

  let (markdown, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "markdown",
      "crypto-sites",
      ".",
      "--algorithm",
      "sha1",
      "--fips-check",
      "--around",
      "2",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nmarkdown:\n{markdown}");
  assert!(markdown.contains("# crypto-sites"));
  assert!(markdown.contains("sha1"));
  assert!(markdown.contains("imports: crypto/sha1"));

  let (missing, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "crypto-sites",
      "does-not-exist",
      "--algorithm",
      "sha1",
    ],
  );
  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{missing:#}");
  assert_eq!(missing["warnings"][0]["code"], "PATH_NOT_FOUND");

  let query = r#"
    {
      cryptoSites(path: ".", algorithms: ["sha1"], fipsCheck: true, around: 2, parents: true, body: true) {
        sites {
          filePath
          lineNumber
          lineContent
          algorithm
          matchText
          importPaths
          nearbyGuards { matchText lineNumber lineContent }
          sourceContext { lineNumber lineContent }
          parents { kind name lineStart lineEnd }
          body { kind name lineStart lineEnd content }
          confidence
        }
        exitStatus
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope
    .pointer("/data/cryptoSites/sites")
    .and_then(|value| value.as_array())
    .expect("graphql cryptoSites sites");
  assert!(
    sites
      .iter()
      .any(|site| site["lineContent"] == "\th := sha1.New()"),
    "graphql cryptoSites should expose Go anchors:\n{envelope:#}"
  );
}
