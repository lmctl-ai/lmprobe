use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
  gh: PathBuf,
  payload: PathBuf,
  args_log: PathBuf,
  fail_review_threads_once: bool,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn fixture(payload: &str) -> Fixture {
  fixture_with_review_threads_failure(payload, false)
}

fn fixture_with_review_threads_failure(payload: &str, fail: bool) -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root =
    std::env::temp_dir().join(format!("lmprobe-pr-triage-{}-{suffix}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture dir");
  let gh = root.join(if cfg!(windows) { "gh.cmd" } else { "gh" });
  let payload_path = root.join("payload.json");
  let args_log = root.join("gh.args");
  fs::write(&payload_path, payload).expect("write payload");
  let shim = if cfg!(windows) {
    r#"@echo off
setlocal enabledelayedexpansion
set "saw_review_threads="
:args
if "%~1"=="" goto done
>> "%LMPROBE_GH_ARGS%" echo %~1
echo %~1 | findstr /c:"reviewThreads" >nul
if not errorlevel 1 set "saw_review_threads=1"
shift
goto args
:done
if defined LMPROBE_GH_FAIL_REVIEW_THREADS if defined saw_review_threads (
  >&2 echo Unknown JSON field: "reviewThreads"
  exit /b 1
)
type "%LMPROBE_GH_PAYLOAD%"
"#
  } else {
    r#"#!/bin/sh
printf '%s\n' "$@" >> "$LMPROBE_GH_ARGS"
if [ -n "$LMPROBE_GH_FAIL_REVIEW_THREADS" ] && printf '%s\n' "$@" | grep -q "reviewThreads"; then
  echo 'Unknown JSON field: "reviewThreads"' >&2
  exit 1
fi
cat "$LMPROBE_GH_PAYLOAD"
"#
  };
  fs::write(&gh, shim).expect("write fake gh");
  #[cfg(unix)]
  {
    let mut perms = fs::metadata(&gh).expect("fake gh metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).expect("chmod fake gh");
  }
  Fixture {
    root,
    gh,
    payload: payload_path,
    args_log,
    fail_review_threads_once: fail,
  }
}

fn run_json(fix: &Fixture, args: &[&str]) -> (serde_json::Value, String, i32) {
  let mut command = Command::new(binary());
  command
    .args(args)
    .env("LMPROBE_GH", &fix.gh)
    .env("LMPROBE_GH_PAYLOAD", &fix.payload)
    .env("LMPROBE_GH_ARGS", &fix.args_log)
    .current_dir(&fix.root);
  if fix.fail_review_threads_once {
    command.env("LMPROBE_GH_FAIL_REVIEW_THREADS", "1");
  }
  let out = command.output().expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn sample_payload() -> &'static str {
  r#"{
  "number": 42,
  "url": "https://github.com/example/project/pull/42",
  "title": "Fix parser panic",
  "state": "OPEN",
  "isDraft": false,
  "mergeable": "MERGEABLE",
  "reviewDecision": "CHANGES_REQUESTED",
  "author": { "login": "alice" },
  "statusCheckRollup": [
    { "name": "ci", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://ci.example/fail" },
    { "context": "lint", "state": "SUCCESS", "targetUrl": "https://ci.example/lint" },
    { "name": "integration", "status": "IN_PROGRESS" },
    { "name": "docs", "status": "COMPLETED", "conclusion": "SKIPPED" }
  ],
  "comments": [
    {
      "author": { "login": "alice" },
      "authorAssociation": "MEMBER",
      "createdAt": "2026-05-07T10:00:00Z",
      "body": "Pushed an update.",
      "url": "https://github.com/example/project/pull/42#issuecomment-1"
    },
    {
      "author": { "login": "bob" },
      "authorAssociation": "MEMBER",
      "createdAt": "2026-05-07T11:00:00Z",
      "body": "DCO failed; this still has an error.",
      "url": "https://github.com/example/project/pull/42#issuecomment-2"
    }
  ],
  "reviews": [
    {
      "author": { "login": "coderabbitai" },
      "authorAssociation": "NONE",
      "submittedAt": "2026-05-07T12:00:00Z",
      "state": "COMMENTED",
      "body": "nitpick: this would still pass but is a quick win",
      "url": "https://github.com/example/project/pull/42#pullrequestreview-1"
    }
  ],
  "reviewThreads": [
    {
      "comments": [
        {
          "author": { "login": "copilot-pull-request-reviewer" },
          "authorAssociation": "NONE",
          "createdAt": "2026-05-07T12:30:00Z",
          "body": "This does not verify the error path.",
          "path": "src/lib.rs",
          "line": 12,
          "url": "https://github.com/example/project/pull/42#discussion_r1"
        }
      ]
    }
  ]
}"#
}

#[test]
fn pr_triage_json_uses_fake_gh_and_filters_since_last_author_comment() {
  let fix = fixture(sample_payload());
  let (env, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "pr-triage",
      "https://github.com/example/project/pull/42?plain=1",
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{env:#}");
  assert_eq!(env["schema"], "lmprobe.pr_triage.v1");
  assert_eq!(env["target"]["owner"], "example");
  assert_eq!(env["target"]["repo"], "project");
  assert_eq!(env["target"]["number"], 42);
  assert_eq!(
    env["target"]["url"],
    "https://github.com/example/project/pull/42"
  );
  assert_eq!(env["status"]["review_decision"], "CHANGES_REQUESTED");
  assert_eq!(env["check_summary"]["total"], 4);
  assert_eq!(env["check_summary"]["passing"], 1);
  assert_eq!(env["check_summary"]["failing"], 1);
  assert_eq!(env["check_summary"]["pending"], 1);
  assert_eq!(env["check_summary"]["skipped_neutral"], 1);
  let evidence = env["evidence"].as_array().expect("evidence array");
  assert_eq!(
    evidence.len(),
    3,
    "author cutoff comment should be excluded:\n{env:#}"
  );
  assert!(
    evidence
      .iter()
      .any(|event| event["kind"] == "review_comment"
        && event["path"] == "src/lib.rs"
        && event["line"] == 12
        && event["action_terms"]
          .as_array()
          .is_some_and(|terms| terms.iter().any(|term| term == "does not verify"))),
    "inline bot review comment should be preserved:\n{env:#}",
  );
  assert!(
    evidence.iter().any(|event| event["kind"] == "comment"
      && event["action_terms"]
        .as_array()
        .is_some_and(|terms| terms.iter().any(|term| term == "dco")
          && terms.iter().any(|term| term == "failed"))),
    "action-like issue comment should expose evidence terms:\n{env:#}",
  );
  let gh_args = fs::read_to_string(&fix.args_log)
    .expect("fake gh args")
    .replace("\r\n", "\n");
  assert!(gh_args.contains("pr\nview\n"));
  assert!(gh_args.contains("https://github.com/example/project/pull/42\n"));
  assert!(gh_args.contains("--json\n"));
  assert!(gh_args.contains("statusCheckRollup"));
}

#[test]
fn pr_triage_since_all_keeps_author_comment_and_compacts_excerpts() {
  let fix = fixture(sample_payload());
  let (env, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "pr-triage",
      "--since",
      "all",
      "--compact",
      "https://github.com/example/project/pull/42",
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{env:#}");
  assert_eq!(env["since"], "all");
  assert_eq!(env["compact"], true);
  let evidence = env["evidence"].as_array().expect("evidence array");
  assert_eq!(
    evidence.len(),
    4,
    "--since all should keep the author comment:\n{env:#}"
  );
  assert!(
    evidence
      .iter()
      .any(|event| event["author"] == "alice" && event["body_excerpt"] == "Pushed an update."),
    "author comment should be present with --since all:\n{env:#}",
  );
}

#[test]
fn pr_triage_json_retries_without_review_threads_on_unsupported_field() {
  let fix = fixture_with_review_threads_failure(sample_payload(), true);
  let (env, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "pr-triage",
      "https://github.com/example/project/pull/42",
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{env:#}");
  assert_eq!(env["schema"], "lmprobe.pr_triage.v1");
  let warnings = env["warnings"].as_array().expect("warnings array");
  let warning = warnings
    .iter()
    .find(|warn| warn["code"] == "GITHUB_CLI_REVIEW_THREADS_UNSUPPORTED")
    .expect("reviewThreads warning");
  assert_eq!(
    warning["message"], "GitHub CLI does not support reviewThreads",
    "warning should explain missing reviewThreads support: {warning:#}"
  );

  let evidence = env["evidence"].as_array().expect("evidence array");
  assert_eq!(
    evidence.len(),
    2,
    "fallback payload without reviewThreads should omit thread comments:\n{env:#}"
  );
  assert!(
    !evidence
      .iter()
      .any(|event| event["kind"] == "review_comment"),
    "review_comment evidence should be skipped when fallback mode is used:\n{env:#}"
  );

  let gh_args = fs::read_to_string(&fix.args_log)
    .expect("fake gh args")
    .replace("\r\n", "\n");
  assert!(gh_args.matches("reviewThreads").count() == 1);
  assert!(gh_args.contains("statusCheckRollup"));
}
