#![cfg(unix)]
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn mock(root: &Path, body: &str) {
  use std::os::unix::fs::PermissionsExt;
  std::fs::write(root.join("aws-mock"),format!("#!/usr/bin/env python3\nimport sys,json,os,time\nopen(os.environ['CALL_MARKER'],'a').write(json.dumps(sys.argv[1:])+'\\n')\n{body}\n")).unwrap();
  std::fs::set_permissions(
    root.join("aws-mock"),
    std::fs::Permissions::from_mode(0o700),
  )
  .unwrap();
}
fn run(root: &Path, args: &[&str], stdin: Option<&str>) -> Output {
  let mut child = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .arg("--json")
    .arg("aws")
    .args(args)
    .current_dir(root)
    .env("LMPROBE_AWS", root.join("aws-mock"))
    .env("CALL_MARKER", root.join("called"))
    .env("AWS_SHARED_CREDENTIALS_FILE", root.join("credentials"))
    .env("AWS_CONFIG_FILE", root.join("config"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  if let Some(input) = stdin {
    child
      .stdin
      .take()
      .unwrap()
      .write_all(input.as_bytes())
      .unwrap();
  }
  child.wait_with_output().unwrap()
}
fn data(output: &Output) -> Value {
  serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
    panic!(
      "{} {}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    )
  })
}

#[test]
fn catalog_and_mutating_requests_never_execute_aws() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p, "raise RuntimeError('must not run')");
  let output = run(p, &["catalog"], None);
  assert!(output.status.success());
  assert_eq!(data(&output)["payload_format"], "rows");
  assert!(data(&output)["payload"]["rows"].as_array().unwrap().len() > 30);
  for (svc, operation) in [
    ("ec2", "terminate-instances"),
    ("s3api", "delete-bucket"),
    ("lambda", "invoke"),
    ("sts", "assume-role"),
    ("logs", "start-query"),
    ("iam", "attach-role-policy"),
  ] {
    let output = run(p, &["query", svc, operation], None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(data(&output)["status"], "error");
  }
  std::fs::write(
    p.join("request.json"),
    json!({"service":"s3api","operation":"put-object","parameters":{"Bucket":"bad"}}).to_string(),
  )
  .unwrap();
  assert_eq!(
    run(p, &["query", "--request-file", "request.json"], None)
      .status
      .code(),
    Some(2)
  );
  assert!(!p.join("called").exists());
}

#[test]
fn nested_parameters_profiles_and_json_file_stdin_are_single_safe_arguments() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(
    p,
    "print(json.dumps({'Reservations':[{'Instances':[{'InstanceId':'i-example'}]}]}))",
  );
  let request=json!({"schema":"lmprobe.aws.request.v1","service":"ec2","operation":"describe-instances","parameters":{"Filters":[{"Name":"tag:Name","Values":["x ; $(touch bad) file://none"]}]},"region":"us-east-1","profile":"test profile","pagination":{"max_items":1,"page_size":1},"select":"/Reservations/0/Instances"}).to_string();
  let output = run(p, &["query", "--request-file", "-"], Some(&request));
  assert!(output.status.success());
  assert_eq!(
    data(&output)["payload"]["data"][0]["InstanceId"],
    "i-example"
  );
  let args: Vec<String> = serde_json::from_str(
    std::fs::read_to_string(p.join("called"))
      .unwrap()
      .lines()
      .next()
      .unwrap(),
  )
  .unwrap();
  assert!(args.contains(&"--profile=test profile".into()));
  assert!(args.contains(&"--max-items=1".into()));
  assert!(args
    .iter()
    .any(|arg| arg.starts_with("--cli-input-json={") && arg.contains("$(touch bad)")));
  assert!(!p.join("bad").exists());
}

#[test]
fn continuation_and_partial_failures_survive_projection_in_all_formats() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p,"print(json.dumps({'NextToken':'opaque-cli-token','Reservations':[],'failures':[{'reason':'MISSING'}]}))");
  let output = run(
    p,
    &[
      "query",
      "ec2",
      "describe-instances",
      "--select",
      "/Reservations",
    ],
    None,
  );
  assert_eq!(output.status.code(), Some(3));
  let value = data(&output);
  assert_eq!(value["status"], "partial");
  assert_eq!(value["payload"]["data"], json!([]));
  assert_eq!(
    value["metadata"]["page"]["resume_token"],
    "opaque-cli-token"
  );
  assert_eq!(value["diagnostics"].as_array().unwrap().len(), 2);
  let output = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args([
      "aws",
      "query",
      "ec2",
      "describe-instances",
      "--format",
      "json",
    ])
    .current_dir(p)
    .env("LMPROBE_AWS", p.join("aws-mock"))
    .env("CALL_MARKER", p.join("called"))
    .output()
    .unwrap();
  assert_eq!(output.status.code(), Some(3));
  assert_eq!(data(&output)["schema"], "lmprobe.aws_query.v1");
}

#[test]
fn paginator_models_distinguish_s3_echoes_native_tokens_and_cli_tokens() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(
    p,
    "print(json.dumps({'Contents':[],'ContinuationToken':'echo','IsTruncated':False}))",
  );
  let output = run(
    p,
    &["query", "s3api", "list-objects-v2", "--first-page"],
    None,
  );
  assert!(output.status.success());
  assert_eq!(data(&output)["metadata"]["complete"], true);
  mock(
    p,
    "print(json.dumps({'Contents':[],'NextContinuationToken':'native-next','IsTruncated':True}))",
  );
  let output = run(
    p,
    &["query", "s3api", "list-objects-v2", "--first-page"],
    None,
  );
  assert_eq!(output.status.code(), Some(3));
  assert!(data(&output)["metadata"]["page"]["resume_token"].is_null());
  assert_eq!(
    data(&output)["metadata"]["page"]["tokens"]["NextContinuationToken"],
    "native-next"
  );
  mock(
    p,
    "print(json.dumps({'Buckets':[],'ContinuationToken':'buckets-next'}))",
  );
  let output = run(p, &["query", "s3api", "list-buckets", "--first-page"], None);
  assert_eq!(output.status.code(), Some(3));
  assert_eq!(
    data(&output)["metadata"]["page"]["tokens"]["ContinuationToken"],
    "buckets-next"
  );
  mock(
    p,
    "print(json.dumps({'Reservations':[],'NextToken':'some-token'}))",
  );
  let output = run(
    p,
    &["query", "ec2", "describe-instances", "--first-page"],
    None,
  );
  assert_eq!(output.status.code(), Some(3));
  assert!(data(&output)["metadata"]["page"]["resume_token"].is_null());
  let output = run(p, &["query", "ec2", "describe-instances"], None);
  assert_eq!(output.status.code(), Some(3));
  assert_eq!(
    data(&output)["metadata"]["page"]["resume_token"],
    "some-token"
  );
}

#[test]
fn missing_selection_keeps_all_upstream_diagnostics_and_redacted_failure_detail() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p,"print(json.dumps({'NextToken':'next','failures':[{'reason':'MISSING'}],'Environment':{'TOKEN':'must-never-appear'}}))");
  let output = run(
    p,
    &["query", "ec2", "describe-instances", "--select", "/missing"],
    None,
  );
  assert_eq!(output.status.code(), Some(2));
  let value = data(&output);
  let codes: Vec<_> = value["diagnostics"]
    .as_array()
    .unwrap()
    .iter()
    .map(|d| d["code"].as_str().unwrap())
    .collect();
  assert_eq!(
    codes,
    [
      "AWS_PAGE_INCOMPLETE",
      "AWS_PARTIAL_FAILURE",
      "AWS_VALUES_REDACTED",
      "AWS_SELECT_NOT_FOUND"
    ]
  );
  assert_eq!(value["metadata"]["partial"], true);
  assert_eq!(value["metadata"]["redactions"], 1);
  assert_eq!(value["metadata"]["aws_failures"][0]["reason"], "MISSING");
  assert!(!String::from_utf8(output.stdout)
    .unwrap()
    .contains("must-never-appear"));
}

#[test]
fn token_inputs_expanded_profiles_and_cache_outfiles_are_refused() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p, "raise RuntimeError('must not run')");
  std::fs::create_dir_all(p.join(".aws/sso/cache")).unwrap();
  std::fs::write(
    p.join(".aws/config"),
    "[profile test]\nweb_identity_token_file = token.txt\n",
  )
  .unwrap();
  std::fs::write(p.join("token.txt"), "preserve token").unwrap();
  std::fs::hard_link(p.join("token.txt"), p.join("alias.txt")).unwrap();
  let cases = [
    ("AWS_WEB_IDENTITY_TOKEN_FILE", "token.txt", "token.txt"),
    (
      "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
      "token.txt",
      "alias.txt",
    ),
    ("AWS_CONFIG_FILE", "~/.aws/config", ".aws/config"),
    (
      "AWS_CONFIG_FILE",
      "${AWS_FIXTURE_HOME}/.aws/config",
      "alias.txt",
    ),
    (
      "AWS_SHARED_CREDENTIALS_FILE",
      "$AWS_FIXTURE_HOME/token.txt",
      "token.txt",
    ),
  ];
  for (env, value, outfile) in cases {
    let profile = if env == "AWS_CONFIG_FILE" && value.starts_with("${") {
      "[profile test]\nweb_identity_token_file = token.txt\n"
    } else {
      "[profile test]\nregion = us-east-1\n"
    };
    std::fs::write(p.join(".aws/config"), profile).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
      .args(["aws", "catalog", "--json", "--outfile", outfile])
      .current_dir(p)
      .env("HOME", p)
      .env("AWS_FIXTURE_HOME", p)
      .env("LMPROBE_AWS", p.join("aws-mock"))
      .env("CALL_MARKER", p.join("called"))
      .env("AWS_CONFIG_FILE", p.join(".aws/config"))
      .env(env, value)
      .output()
      .unwrap();
    assert_eq!(
      output.status.code(),
      Some(2),
      "{env} {value}: {}",
      String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(data(&output)["status"], "error");
  }
  let output = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args([
      "aws",
      "query",
      "--help",
      "--json",
      "--outfile",
      ".aws/sso/cache/new.json",
    ])
    .current_dir(p)
    .env("HOME", p)
    .env("AWS_CONFIG_FILE", p.join(".aws/config"))
    .output()
    .unwrap();
  assert_eq!(output.status.code(), Some(2));
  assert!(!p.join(".aws/sso/cache/new.json").exists());
  assert_eq!(
    std::fs::read_to_string(p.join("token.txt")).unwrap(),
    "preserve token"
  );
  assert!(!p.join("called").exists());
}

#[test]
fn shared_credentials_profile_token_paths_are_protected_with_both_ini_delimiters() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p, "raise RuntimeError('must not run')");
  std::fs::write(p.join("token.txt"), "preserve").unwrap();
  std::fs::hard_link(p.join("token.txt"), p.join("alias.txt")).unwrap();
  for delimiter in ["=", ":"] {
    std::fs::write(
      p.join("credentials"),
      format!("[test]\nweb_identity_token_file {delimiter} token.txt\n"),
    )
    .unwrap();
    for args in [
      vec!["catalog", "--outfile", "alias.txt"],
      vec!["query", "--help", "--outfile", "alias.txt"],
      vec![
        "query",
        "sts",
        "get-caller-identity",
        "--profile",
        "test",
        "--outfile",
        "alias.txt",
      ],
    ] {
      let output = run(p, &args, None);
      assert_eq!(output.status.code(), Some(2));
      assert_eq!(data(&output)["status"], "error");
    }
  }
  assert_eq!(
    std::fs::read_to_string(p.join("token.txt")).unwrap(),
    "preserve"
  );
  assert!(!p.join("called").exists());
}

#[test]
fn process_errors_bad_json_timeout_and_missing_cli_are_structured() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  for (body, code) in [
    (
      "sys.stderr.write('AccessDenied: no access');sys.exit(254)",
      "AWS_CLI_ERROR",
    ),
    ("print('not JSON')", "AWS_JSON_INVALID"),
    ("time.sleep(30)", "AWS_QUERY_TIMEOUT"),
  ] {
    mock(p, body);
    let output = run(
      p,
      &[
        "query",
        "sts",
        "get-caller-identity",
        "--timeout-seconds",
        "1",
      ],
      None,
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(data(&output)["diagnostics"][0]["code"], code);
  }
  std::fs::remove_file(p.join("aws-mock")).unwrap();
  let output = run(p, &["query", "sts", "get-caller-identity"], None);
  assert_eq!(
    data(&output)["diagnostics"][0]["code"],
    "AWS_CLI_UNAVAILABLE"
  );
}

#[test]
fn inherited_pipe_from_descendant_still_obeys_deadline() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p,"import subprocess\nsubprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])\nprint('{}')");
  let before = std::time::Instant::now();
  let output = run(
    p,
    &[
      "query",
      "sts",
      "get-caller-identity",
      "--timeout-seconds",
      "1",
    ],
    None,
  );
  assert_eq!(data(&output)["diagnostics"][0]["code"], "AWS_QUERY_TIMEOUT");
  assert!(before.elapsed() < std::time::Duration::from_secs(8));
}

#[test]
fn request_and_credentials_outfile_aliases_are_preserved_before_errors() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p, "print('{}')");
  for name in ["request.json", "credentials", "config"] {
    std::fs::write(p.join(name), "preserve").unwrap();
  }
  for args in [
    vec![
      "query",
      "--request-file",
      "request.json",
      "--outfile",
      "request.json",
    ],
    vec![
      "query",
      "sts",
      "get-caller-identity",
      "--outfile",
      "credentials",
    ],
    vec!["query", "--help", "--outfile", "config"],
    vec!["query", "--bad", "--outfile", "credentials"],
  ] {
    let output = run(p, &args, None);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(data(&output)["status"], "error");
  }
  for name in ["request.json", "credentials", "config"] {
    assert_eq!(std::fs::read_to_string(p.join(name)).unwrap(), "preserve");
  }
  assert!(!p.join("called").exists());
}

#[test]
fn quiet_outfile_redaction_and_empty_projection_have_defined_outcomes() {
  let root = tempfile::tempdir().unwrap();
  let p = root.path();
  mock(p,"print(json.dumps({'Functions':[{'FunctionName':'example','Environment':{'Variables':{'TOKEN':'never-write-this'}}}]}))");
  let output = run(
    p,
    &[
      "query",
      "lambda",
      "list-functions",
      "--outfile",
      "result.json",
    ],
    None,
  );
  assert!(output.status.success());
  assert!(output.stdout.is_empty());
  let text = std::fs::read_to_string(p.join("result.json")).unwrap();
  assert!(!text.contains("never-write-this"));
  let value: Value = serde_json::from_str(&text).unwrap();
  assert_eq!(value["metadata"]["redactions"], 1);
  mock(p, "print(json.dumps({'Functions':[]}))");
  let output = run(
    p,
    &[
      "query",
      "lambda",
      "list-functions",
      "--select",
      "/Functions",
    ],
    None,
  );
  assert_eq!(output.status.code(), Some(1));
  assert_eq!(data(&output)["status"], "no_match");
}
