use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn fixture() -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-guard-sites-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(root.join("app")).expect("create fixture dirs");
  fs::write(
    root.join("app/handlers.py"),
    r#"# auth note from TODO, not a decorator
@router.delete("/users/{user_id}")
@requires_auth
def remove_user(user, user_id):
    if not authorize(user, "delete"):
        return "denied"
    delete_user(user_id)

def run_command(command):
    exec(command)
"#,
  )
  .expect("write python fixture");
  Fixture { root }
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
fn guard_sites_reports_local_guard_and_decoration_evidence() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "guard-sites",
      ".",
      "--sensitive",
      "delete_user|exec",
      "--guard",
      "authorize|auth",
      "--around",
      "3",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.guard_sites.v1");
  assert_eq!(envelope["scan_stats"]["files_scanned"], 1);
  assert_eq!(envelope["scan_stats"]["anchors_found"], 2);
  assert!(
    envelope["scan_stats"]["guard_hits"]
      .as_u64()
      .is_some_and(|hits| hits > 0),
    "guard hit count should summarize nearby guard evidence:\n{envelope:#}"
  );
  let candidates = envelope["candidates"].as_array().expect("candidates");
  assert_eq!(candidates.len(), 2, "envelope:\n{envelope:#}");
  assert!(
    !envelope.to_string().contains("vulnerability")
      && !envelope.to_string().contains("missing auth")
      && !envelope.to_string().contains("protected")
      && !envelope.to_string().contains("unprotected"),
    "runtime output should stay evidence-only:\n{envelope:#}"
  );

  let delete_candidate = candidates
    .iter()
    .find(|candidate| candidate["sensitive"]["match_text"] == "delete_user")
    .expect("delete_user candidate");
  assert_eq!(delete_candidate["confidence"], "local-evidence-only");
  assert!(
    delete_candidate["nearby_guards"]
      .as_array()
      .is_some_and(|guards| guards
        .iter()
        .any(|guard| guard["match_text"] == "authorize")),
    "nearby authorize guard missing:\n{delete_candidate:#}"
  );
  assert!(
    delete_candidate["decorations"]
      .as_array()
      .is_some_and(|decorations| decorations.len() >= 2),
    "decorator evidence missing:\n{delete_candidate:#}"
  );
  assert!(
    delete_candidate["decorations"]
      .as_array()
      .is_some_and(|decorations| decorations
        .iter()
        .all(|decoration| !decoration["line_content"]
          .as_str()
          .unwrap_or_default()
          .contains("TODO"))),
    "plain comments should not be decoration evidence:\n{delete_candidate:#}"
  );
  assert!(
    delete_candidate["parents"]
      .as_array()
      .is_some_and(|parents| !parents.is_empty())
      && delete_candidate.get("body").is_some()
      && delete_candidate["source_context"]
        .as_array()
        .is_some_and(|lines| !lines.is_empty()),
    "requested structural/source context missing:\n{delete_candidate:#}"
  );

  let exec_candidate = candidates
    .iter()
    .find(|candidate| candidate["sensitive"]["match_text"] == "exec")
    .expect("exec candidate");
  assert!(
    exec_candidate["nearby_guards"]
      .as_array()
      .is_some_and(|guards| guards.is_empty()),
    "second sensitive anchor should simply have no local guard evidence:\n{exec_candidate:#}"
  );
}

#[test]
fn guard_sites_reports_no_anchor_scan_diagnostic() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "guard-sites",
      ".",
      "--sensitive",
      "send_money",
      "--guard",
      "authorize",
    ],
  );
  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["scan_stats"]["files_scanned"], 1);
  assert_eq!(envelope["scan_stats"]["anchors_found"], 0);
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "NO_ANCHORS_FOUND")),
    "empty scanner result should explain that no sensitive anchors matched:\n{envelope:#}"
  );
}

#[test]
fn guard_sites_reports_anchor_without_guard_diagnostic() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "guard-sites",
      ".",
      "--sensitive",
      "exec",
      "--guard",
      "requires_admin",
      "--around",
      "1",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["scan_stats"]["anchors_found"], 1);
  assert_eq!(envelope["scan_stats"]["guard_hits"], 0);
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "ANCHORS_WITHOUT_GUARD")),
    "matched sensitive anchors without local guard evidence should be explicit:\n{envelope:#}"
  );
}

#[test]
fn guard_sites_reports_missing_path_diagnostic() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "guard-sites",
      "does-not-exist",
      "--sensitive",
      "delete_user",
      "--guard",
      "authorize",
    ],
  );
  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.guard_sites.v1");
  assert_eq!(envelope["warnings"][0]["code"], "PATH_NOT_FOUND");
}

#[test]
fn guard_sites_reports_regex_parse_failures_in_envelope() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "guard-sites",
      ".",
      "--sensitive",
      "[",
      "--guard",
      "authorize",
    ],
  );
  assert_eq!(exit, 2, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.guard_sites.v1");
  assert_eq!(envelope["exit_status"], "error");
  assert_eq!(envelope["warnings"][0]["code"], "REGEX_PARSE_FAILED");
}

#[test]
fn guard_sites_ndjson_and_markdown_render() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "guard-sites",
      ".",
      "--sensitive",
      "delete_user|exec",
      "--guard",
      "authorize|auth",
      "--around",
      "3",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 3, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("candidate item");
  assert!(item.get("sensitive").is_some(), "item:\n{item:#}");
  let summary: serde_json::Value = serde_json::from_str(lines[2]).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("candidates").is_none());

  let (markdown, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "markdown",
      "guard-sites",
      ".",
      "--sensitive",
      "delete_user|exec",
      "--guard",
      "authorize|auth",
      "--around",
      "3",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nmarkdown:\n{markdown}");
  assert!(markdown.contains("# guard-sites"));
  assert!(markdown.contains("sensitive delete_user"));
  assert!(markdown.contains("guard"));
  assert!(markdown.contains("body:"));
  assert!(markdown.contains("delete_user(user_id)"));
}

#[test]
fn guard_sites_graphql_matches_cli_surface() {
  let fix = fixture();
  let query = r#"
    {
      guardSites(path: ".", sensitive: "delete_user|exec", guard: "authorize|auth", around: 3, parents: true, body: true) {
        candidates {
          filePath
          sensitive { matchText lineNumber lineContent }
          nearbyGuards { matchText lineNumber lineContent }
          decorations { matchText lineNumber lineContent }
          parents { kind name lineStart lineEnd }
          body { kind name lineStart lineEnd content }
          sourceContext { lineNumber lineContent }
          confidence
        }
        exitStatus
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let candidates = envelope
    .pointer("/data/guardSites/candidates")
    .and_then(|value| value.as_array())
    .expect("graphql candidates");
  assert!(
    candidates.iter().any(
      |candidate| candidate["sensitive"]["matchText"] == "delete_user"
        && candidate["nearbyGuards"]
          .as_array()
          .is_some_and(|guards| !guards.is_empty())
    ),
    "graphql guardSites should expose local guard evidence:\n{envelope:#}"
  );
}
