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
    .prefix("lmprobe-api-facts-")
    .tempdir()
    .expect("create fixture root");
  let root = tmp.path().to_path_buf();
  fs::create_dir_all(root.join("src")).expect("create fixture dirs");
  fs::write(
    root.join("openapi.json"),
    r##"{
  "openapi": "3.0.0",
  "paths": {
    "/users/{user_id}": {
      "parameters": [
        { "$ref": "#/components/parameters/AccountId" }
      ],
      "get": {
        "operationId": "getUser",
        "parameters": [
          { "name": "user_id", "in": "path", "required": true },
          { "name": "verbose", "in": "query", "required": false }
        ],
        "responses": {
          "200": {
            "content": {
              "application/json": {
                "schema": { "$ref": "#/components/schemas/User" }
              }
            }
          }
        }
      }
    }
  },
  "components": {
    "parameters": {
      "AccountId": {
        "name": "account_id",
        "in": "header",
        "required": true
      }
    }
  }
}
"##,
  )
  .expect("write json spec");
  fs::write(
    root.join("openapi.yaml"),
    r##"openapi: 3.0.0
paths:
  /orders:
    post:
      operationId: createOrder
      parameters:
        - name: request_id
          in: header
          required: true
      responses:
        "201":
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/Order"
"##,
  )
  .expect("write yaml spec");
  fs::write(root.join("bad-openapi.yaml"), "openapi: [").expect("write bad spec");
  fs::write(
    root.join("src/server.py"),
    r#"from fastapi import APIRouter
import requests
import httpx

router = APIRouter()

@router.get("/users/{user_id}")
def get_user(user_id: str):
    return {"id": user_id}

def call_users():
    requests.get("/users/123")
    # requests.get("/comment-only")
    text = 'fetch("/not-a-call")'
    httpx.post("/orders", json={})
"#,
  )
  .expect("write python server/client");
  fs::write(
    root.join("src/client.ts"),
    r#"export async function loadUser() {
  await fetch("/users/123", { method: "GET" });
  return axios.post("/orders", {});
}
"#,
  )
  .expect("write ts client");
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
fn openapi_facts_extracts_json_and_yaml_operations() {
  let fix = fixture();
  let (json_env, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "openapi-facts", "openapi.json"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{json_env:#}");
  assert_eq!(json_env["schema"], "lmprobe.openapi_facts.v1");
  let json_ops = json_env["operations"].as_array().expect("operations");
  assert!(
    json_ops.iter().any(|op| op["method"] == "GET"
      && op["path"] == "/users/{user_id}"
      && op["operation_id"] == "getUser"
      && op["required_params"]
        .as_array()
        .is_some_and(|params| params.iter().any(|param| param == "user_id")
          && params.iter().any(|param| param == "account_id"))
      && op["response_schemas"]
        .as_array()
        .is_some_and(|schemas| schemas.iter().any(|schema| schema == "User"))),
    "json OpenAPI operation missing:\n{json_env:#}"
  );

  let (yaml_env, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "openapi-facts", "openapi.yaml"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{yaml_env:#}");
  assert!(
    yaml_env["operations"]
      .as_array()
      .expect("yaml operations")
      .iter()
      .any(|op| op["method"] == "POST"
        && op["path"] == "/orders"
        && op["operation_id"] == "createOrder"
        && op["response_schemas"]
          .as_array()
          .is_some_and(|schemas| schemas.iter().any(|schema| schema == "Order"))),
    "yaml OpenAPI operation missing:\n{yaml_env:#}"
  );
}

#[test]
fn api_fact_commands_report_missing_path_diagnostics() {
  let fix = fixture();
  for command in ["openapi-facts", "route-anchors", "http-client-calls"] {
    let (envelope, stderr, exit) =
      run_json(&fix.root, &["--format", "json", command, "does-not-exist"]);
    assert_eq!(
      exit, 1,
      "{command} should return diagnostic no-match:\nstderr:\n{stderr}\nenvelope:\n{envelope:#}"
    );
    assert!(
      envelope["warnings"]
        .as_array()
        .is_some_and(|warnings| warnings
          .iter()
          .any(|warning| warning["code"] == "PATH_NOT_FOUND")),
      "{command} missing path diagnostic:\n{envelope:#}"
    );
  }
}

#[test]
fn openapi_facts_reports_malformed_specs_as_non_fatal_warning() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "openapi-facts",
      ".",
      "--globs",
      "openapi.json,bad-openapi.yaml",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert!(envelope["operations"]
    .as_array()
    .is_some_and(|ops| !ops.is_empty()));
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "OPENAPI_PARSE_FAILED")),
    "parse warning missing:\n{envelope:#}"
  );
}

#[test]
fn route_anchors_extracts_fastapi_decorators_with_expansions() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "route-anchors",
      ".",
      "--framework",
      "fastapi",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.route_anchors.v1");
  let route = envelope["routes"]
    .as_array()
    .expect("routes")
    .iter()
    .find(|route| route["path"] == "/users/{user_id}")
    .expect("route");
  assert_eq!(route["method"], "GET");
  assert_eq!(route["handler"], "get_user");
  assert_eq!(route["confidence"], "literal-evidence-only");
  assert!(route["source_context"]
    .as_array()
    .is_some_and(|lines| !lines.is_empty()));
  assert!(route["parents"]
    .as_array()
    .is_some_and(|parents| !parents.is_empty()));
  assert!(route.get("body").is_some());
}

#[test]
fn route_anchors_generic_does_not_report_http_client_calls() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "route-anchors", "."]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let routes = envelope["routes"].as_array().expect("routes");
  assert!(
    routes
      .iter()
      .all(|route| route["path"] != "/users/123" && route["path"] != "/orders"),
    "HTTP client calls should not be route anchors:\n{envelope:#}"
  );
}

#[test]
fn http_client_calls_extracts_python_and_js_literals() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "http-client-calls",
      ".",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.http_client_calls.v1");
  let calls = envelope["calls"].as_array().expect("calls");
  assert!(
    calls.iter().any(|call| call["client"] == "requests"
      && call["method"] == "GET"
      && call["url"] == "/users/123"),
    "requests call missing:\n{envelope:#}"
  );
  assert!(
    calls.iter().any(|call| call["client"] == "httpx"
      && call["method"] == "POST"
      && call["url"] == "/orders"),
    "httpx call missing:\n{envelope:#}"
  );
  assert!(
    calls.iter().any(|call| call["client"] == "fetch"
      && call["method"] == "GET"
      && call["url"] == "/users/123"),
    "fetch call missing:\n{envelope:#}"
  );
  assert!(
    calls.iter().any(|call| call["client"] == "axios"
      && call["method"] == "POST"
      && call["url"] == "/orders"),
    "axios call missing:\n{envelope:#}"
  );
  assert!(
    calls
      .iter()
      .all(|call| call["url"] != "/comment-only" && call["url"] != "/not-a-call"),
    "comments and string literals should not be client call anchors:\n{envelope:#}"
  );
  assert!(
    calls.iter().any(|call| call["source_context"]
      .as_array()
      .is_some_and(|lines| !lines.is_empty())
      && call["parents"]
        .as_array()
        .is_some_and(|parents| !parents.is_empty())
      && call.get("body").is_some()),
    "requested call expansions missing:\n{envelope:#}"
  );
  assert!(
    !envelope.to_string().contains("drift")
      && !envelope.to_string().contains("mismatch")
      && !envelope.to_string().contains("missing")
      && !envelope.to_string().contains("compatibility"),
    "runtime output should stay evidence-only:\n{envelope:#}"
  );
}

#[test]
fn api_fact_envelopes_itemize_ndjson_and_graphql() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "route-anchors",
      ".",
      "--framework",
      "fastapi",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 2, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("route item");
  assert!(item.get("path").is_some(), "item:\n{item:#}");
  let summary: serde_json::Value = serde_json::from_str(lines[1]).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("routes").is_none());

  let (markdown, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "markdown",
      "http-client-calls",
      ".",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nmarkdown:\n{markdown}");
  assert!(markdown.contains("# http-client-calls"));
  assert!(markdown.contains("body:"));
  assert!(markdown.contains("requests.get"));

  let query = r#"
    {
      spec: openapiFacts(path: "openapi.json") {
        operations { method path operationId requiredParams responseSchemas }
      }
      routes: routeAnchors(path: ".", framework: "fastapi", around: 1, parents: true, body: true) {
        routes { method path handler parents { kind name lineStart lineEnd } body { kind name lineStart lineEnd content } }
      }
      clients: httpClientCalls(path: ".", around: 1, parents: true, body: true) {
        calls { method url client sourceContext { lineNumber lineContent } parents { kind name lineStart lineEnd } body { kind name lineStart lineEnd content } }
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert!(envelope
    .pointer("/data/spec/operations")
    .and_then(|value| value.as_array())
    .is_some_and(|ops| !ops.is_empty()));
  assert!(envelope
    .pointer("/data/routes/routes")
    .and_then(|value| value.as_array())
    .is_some_and(|routes| !routes.is_empty()));
  assert!(envelope
    .pointer("/data/clients/calls")
    .and_then(|value| value.as_array())
    .is_some_and(|calls| !calls.is_empty()));
}
