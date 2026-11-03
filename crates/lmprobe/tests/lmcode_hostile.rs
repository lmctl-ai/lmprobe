mod lmprobe_support;

use lmprobe_support::{fixture, lmcode, lmcode_raw, Opts};
use serde_json::json;

#[test]
fn unknown_and_prototype_looking_params_are_rejected_before_dispatch() {
  let root = fixture();
  for key in ["notAParameter", "__proto__", "constructor", "prototype"] {
    let mut params = json!({"repo":"."});
    params[key] = json!("unexpected");
    let out = lmcode("git_status", &params, root.path(), Opts::default());
    let frame = out.json();
    assert_eq!(frame["ok"], false, "{key}: {frame}");
    assert_eq!(frame["code"], "CLI_USAGE", "{key}: {frame}");
  }
}

#[test]
fn wrong_parameter_types_and_non_object_inputs_are_rejected() {
  let root = fixture();
  for params in [
    json!([]),
    json!(null),
    json!({"repo":1}),
    json!({"pattern":true}),
  ] {
    let op = if params.get("pattern").is_some() {
      "grep_text"
    } else {
      "git_status"
    };
    let out = lmcode(op, &params, root.path(), Opts::default());
    let frame = out.json();
    assert_eq!(frame["ok"], false, "{params}: {frame}");
    assert_eq!(frame["code"], "CLI_USAGE", "{params}: {frame}");
  }
}

#[test]
fn malformed_parameter_json_is_a_structured_host_failure() {
  let root = fixture();
  let out = lmcode_raw("git_status", "{", root.path(), Opts::default());
  let frame = out.json();
  assert_eq!(frame["ok"], false, "{frame}");
  assert_eq!(frame["code"], "CLI_USAGE", "{frame}");
}

#[test]
fn nul_in_a_path_is_an_input_failure_not_a_process_panic() {
  let root = fixture();
  let out = lmcode(
    "git_status",
    &json!({"repo":"\u{0}"}),
    root.path(),
    Opts::default(),
  );
  let frame = out.json();
  assert_eq!(frame["ok"], false, "{frame}");
  assert!(
    ["CLI_USAGE", "APP_FAIL"].contains(&frame["code"].as_str().unwrap_or("")),
    "{frame}"
  );
  assert!(!out.err_text().contains("panicked"));
}

#[test]
fn large_arguments_fail_through_the_process_boundary_without_echoing_input() {
  let root = fixture();
  let secret_like = "x".repeat(1 << 20);
  let out = lmcode(
    "git_status",
    &json!({"repo":secret_like}),
    root.path(),
    Opts::default(),
  );
  let frame = out.json();
  assert_eq!(frame["ok"], false, "{frame}");
  assert!(!frame.to_string().contains(&secret_like));
}
