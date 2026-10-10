use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn graphql_path_literal(path: &Path) -> String {
  serde_json::to_string(&path.to_string_lossy()).expect("GraphQL string literal")
}

#[test]
fn graphql_callsites_redacts_argument_values_by_default_with_opt_out() {
  let root = fixture_root("redaction");
  fs::write(
    root.join("app.py"),
    "import boto3\nboto3.client(\"s3\", aws_secret_access_key=\"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\")\n",
  )
  .expect("write python");

  let root_s = graphql_path_literal(&root);
  let query = format!(
    r#"{{
      redacted: callsites(path: {root_s}, callee: "boto3.client", language: PYTHON, filters: [{{ argNames: ["aws_secret_access_key"], importHints: ["boto3"] }}]) {{
        argValuesRedacted
        callsites {{ arguments(name: "aws_secret_access_key") {{ name text }} }}
      }}
      raw: callsites(path: {root_s}, callee: "boto3.client", language: PYTHON, redactArgValues: false, filters: [{{ argNames: ["aws_secret_access_key"], importHints: ["boto3"] }}]) {{
        argValuesRedacted
        callsites {{ arguments(name: "aws_secret_access_key") {{ name text }} }}
      }}
    }}"#
  );

  let envelope = run_query(&root, &query);
  let data = &envelope["data"];
  assert_eq!(data["redacted"]["argValuesRedacted"], true);
  assert_eq!(
    data["redacted"]["callsites"][0]["arguments"][0]["text"],
    "REDACTED"
  );
  assert_eq!(data["raw"]["argValuesRedacted"], false);
  assert_eq!(
    data["raw"]["callsites"][0]["arguments"][0]["text"],
    "\"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\""
  );
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-graphql-callsites-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn run_query(root: &Path, query: &str) -> serde_json::Value {
  let out = Command::new(binary())
    .args(["query", query])
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  assert_eq!(
    out.status.code(),
    Some(0),
    "stderr:\n{stderr}\nstdout:\n{stdout}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn run_query_error(root: &Path, query: &str) -> serde_json::Value {
  let out = Command::new(binary())
    .args(["query", query])
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  assert_ne!(
    out.status.code(),
    Some(0),
    "query should fail validation:\n{stdout}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

#[test]
fn graphql_callsites_filters_or_across_import_hints() {
  let root = fixture_root("filter-or-import-hints");
  fs::write(
    root.join("app.py"),
    "import boto3\nboto3.client(\"s3\", aws_secret_access_key=secret)\n",
  )
  .expect("write python");

  let root_s = graphql_path_literal(&root);
  let query = format!(
    r#"{{
      callsites(path: {root_s}, callee: "boto3.client", language: PYTHON, filters: [
        {{ argNames: ["aws_secret_access_key"], importHints: ["not_boto3"] }},
        {{ argNames: ["aws_secret_access_key"], importHints: ["boto3"] }}
      ]) {{
        callsites {{ importEvidence arguments(name: "aws_secret_access_key") {{ name text }} }}
      }}
    }}"#
  );

  let envelope = run_query(&root, &query);
  let callsites = envelope["data"]["callsites"]["callsites"]
    .as_array()
    .expect("callsites array");
  assert_eq!(callsites.len(), 1, "envelope:\n{envelope:#}");
  assert_eq!(
    callsites[0]["arguments"][0]["name"],
    "aws_secret_access_key"
  );
}

#[test]
fn graphql_callsites_requires_language_argument() {
  let root = fixture_root("requires-language");
  fs::write(root.join("app.py"), "import boto3\n").expect("write python");

  let root_s = graphql_path_literal(&root);
  let query = format!(r#"{{ callsites(path: {root_s}) {{ schema }} }}"#);
  let envelope = run_query_error(&root, &query);
  let errors = envelope["errors"].as_array().expect("errors array");
  assert!(
    errors.iter().any(|error| error["message"]
      .as_str()
      .is_some_and(|message| message.contains("language"))),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn graphql_callsites_filters_match_call_new_struct_and_assignment_shapes() {
  let root = fixture_root("filters");
  fs::write(
    root.join("app.py"),
    "import boto3\nboto3.client(\"s3\", aws_secret_access_key=secret)\nstripe.api_key = key\n",
  )
  .expect("write python");
  fs::write(
    root.join("app.js"),
    "import AWS from 'aws-sdk';\nconst creds = new AWS.Credentials(id, secret);\n",
  )
  .expect("write js");
  fs::write(
    root.join("app.go"),
    "package main\nimport \"github.com/aws/aws-sdk-go-v2/aws\"\nvar c = aws.Credentials{SecretAccessKey: secret}\n",
  )
  .expect("write go");

  let root_s = graphql_path_literal(&root);
  let query = format!(
    r#"{{
      call: callsites(path: {root_s}, callee: "boto3.client", language: PYTHON, filters: [{{ argNames: ["aws_secret_access_key"], importHints: ["boto3"] }}]) {{
        callsites {{ importEvidence arguments(name: "aws_secret_access_key") {{ name role text }} }}
      }}
      newSite: callsites(path: {root_s}, callee: "AWS.Credentials", language: JAVASCRIPT, filters: [{{ positionalIndices: [1], importHints: ["AWS"] }}]) {{
        callsites {{ sinkKind importEvidence arguments(position: 1) {{ position role text }} }}
      }}
      structSite: callsites(path: {root_s}, language: GO, filters: [{{ structFields: ["SecretAccessKey"], importHints: ["aws."] }}]) {{
        callsites {{ sinkKind importEvidence arguments(role: STRUCT_FIELD) {{ name role text }} }}
      }}
      assignment: callsites(path: {root_s}, language: PYTHON, filters: [{{ assignmentTargets: ["stripe.api_key"], importHints: ["stripe"] }}]) {{
        callsites {{ sinkKind importEvidence arguments(role: ASSIGNMENT_VALUE) {{ name role text }} }}
      }}
    }}"#
  );

  let envelope = run_query(&root, &query);
  let data = &envelope["data"];
  assert_eq!(
    data["call"]["callsites"][0]["arguments"][0]["name"],
    "aws_secret_access_key"
  );
  assert_eq!(data["newSite"]["callsites"][0]["sinkKind"], "NEW");
  assert_eq!(
    data["newSite"]["callsites"][0]["arguments"][0]["text"],
    "secret"
  );
  assert_eq!(data["structSite"]["callsites"][0]["sinkKind"], "STRUCT");
  assert_eq!(
    data["structSite"]["callsites"][0]["arguments"][0]["name"],
    "SecretAccessKey"
  );
  assert_eq!(data["assignment"]["callsites"][0]["sinkKind"], "ASSIGNMENT");
  assert_eq!(
    data["assignment"]["callsites"][0]["arguments"][0]["name"],
    "stripe.api_key"
  );
}

#[test]
fn graphql_callsites_ast_dfs_ignores_braces_comments_and_lookalike_callees() {
  let root = fixture_root("ast-regressions");
  fs::write(
    root.join("app.py"),
    r#"import boto3
text = "}"
# boto3.client("s3", aws_secret_access_key=commented)
pretend_boto3_client_workaround("s3", aws_secret_access_key=wrong)
boto3.client(
    "s3",
    aws_secret_access_key=secret,
)
"#,
  )
  .expect("write python");
  fs::write(
    root.join("app.go"),
    r#"package main
import "github.com/aws/aws-sdk-go-v2/credentials"
func run(id string, secret string) {
  pretend.credentials.NewStaticCredentialsProvider(id, "wrong", "")
  credentials.NewStaticCredentialsProvider(id, secret, "")
}
"#,
  )
  .expect("write go");

  let root_s = graphql_path_literal(&root);
  let query = format!(
    r#"{{
      py: callsites(path: {root_s}, callee: "boto3.client", language: PYTHON, redactArgValues: false, filters: [{{ argNames: ["aws_secret_access_key"], importHints: ["boto3"] }}]) {{
        callsites {{ lineNumber calleeText arguments(name: "aws_secret_access_key") {{ text }} }}
      }}
      go: callsites(path: {root_s}, callee: "credentials.NewStaticCredentialsProvider", language: GO, redactArgValues: false, filters: [{{ positionalIndices: [1], importHints: ["credentials"] }}]) {{
        callsites {{ lineNumber calleeText arguments(position: 1) {{ text }} }}
      }}
    }}"#
  );

  let envelope = run_query(&root, &query);
  let py_hits = envelope["data"]["py"]["callsites"]
    .as_array()
    .expect("python callsites");
  assert_eq!(py_hits.len(), 1, "envelope:\n{envelope:#}");
  assert_eq!(py_hits[0]["calleeText"], "boto3.client");
  assert_eq!(py_hits[0]["arguments"][0]["text"], "secret");

  let go_hits = envelope["data"]["go"]["callsites"]
    .as_array()
    .expect("go callsites");
  assert_eq!(go_hits.len(), 1, "envelope:\n{envelope:#}");
  assert_eq!(
    go_hits[0]["calleeText"],
    "credentials.NewStaticCredentialsProvider"
  );
  assert_eq!(go_hits[0]["arguments"][0]["text"], "secret");
}
