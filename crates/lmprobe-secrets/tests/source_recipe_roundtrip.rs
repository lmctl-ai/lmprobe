use lmprobe_secrets::{generate_source_graphql_recipe, load_default_source_db, ReadShapeKind};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn run_recipe(recipe: &str, fixture: &str) -> serde_json::Value {
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args([
      "run",
      "-q",
      "-p",
      "lmprobe",
      "--",
      "query",
      recipe,
      "--var",
      &format!("path={fixture}"),
    ])
    .output()
    .expect("run generated source recipe");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(
    output.status.success(),
    "generated source recipe failed\nstderr:\n{stderr}\nstdout:\n{stdout}\nrecipe:\n{recipe}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn run_query(query: &str) -> serde_json::Value {
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args(["run", "-q", "-p", "lmprobe", "--", "query", query])
    .output()
    .expect("run sourceReads query");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(
    output.status.success(),
    "sourceReads query failed\nstderr:\n{stderr}\nstdout:\n{stdout}\nquery:\n{query}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn run_query_allow_no_match(query: &str) -> serde_json::Value {
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args(["run", "-q", "-p", "lmprobe", "--", "query", query])
    .output()
    .expect("run sourceReads query");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(
    matches!(output.status.code(), Some(0) | Some(1)),
    "sourceReads query failed\nstderr:\n{stderr}\nstdout:\n{stdout}\nquery:\n{query}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn run_query_error(query: &str) -> serde_json::Value {
  let output = Command::new("cargo")
    .current_dir(WORKSPACE_ROOT)
    .args(["run", "-q", "-p", "lmprobe", "--", "query", query])
    .output()
    .expect("run sourceReads query");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  assert_ne!(
    output.status.code(),
    Some(0),
    "sourceReads query should fail validation:\n{stdout}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn run_secrets_json(fixture: &str) -> serde_json::Value {
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
      "crates/lmprobe-secrets/tests/fixtures/gitleaks-default.toml",
      fixture,
    ])
    .output()
    .expect("run lmprobe secrets");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(
    matches!(output.status.code(), Some(0) | Some(1)),
    "lmprobe secrets failed\nstderr:\n{stderr}\nstdout:\n{stdout}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-source-recipe-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

#[test]
fn source_reads_rejects_unsupported_around_argument() {
  let root = fixture_root("around-removed");
  fs::write(
    root.join("app.py"),
    "import os\nsecret = os.environ[\"AWS_SECRET_ACCESS_KEY\"]\n",
  )
  .expect("write python fixture");

  let root_s = root.display();
  let query = format!(
    r#"{{
      sourceReads(path: "{root_s}", language: PYTHON, around: 1, filters: [{{ envVarNames: ["AWS_SECRET_ACCESS_KEY"] }}]) {{
        schema
      }}
    }}"#
  );
  let envelope = run_query_error(&query);
  let errors = envelope["errors"].as_array().expect("errors array");
  assert!(
    errors.iter().any(|error| error["message"]
      .as_str()
      .is_some_and(|message| message.contains("around"))),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn source_reads_and_python_scanner_share_source_match_extraction() {
  let root = fixture_root("shared-source-matcher");
  fs::write(
    root.join("app.py"),
    "import os\nimport boto3\nsecret = os.environ[\"AWS_SECRET_ACCESS_KEY\"]\nboto3.client(\"s3\", aws_secret_access_key=secret)\n",
  )
  .expect("write python fixture");

  let root_s = root.display();
  let query = format!(
    r#"{{
      sourceReads(path: "{root_s}", language: PYTHON, filters: [{{ sourceIds: ["env-aws"], envVarNames: ["AWS_SECRET_ACCESS_KEY"], accessPaths: ["os.environ"], importHints: ["os"] }}]) {{
        sourceReads {{ sourceId providerId envVarName lineNumber column }}
      }}
    }}"#
  );
  let graphql = run_query(&query);
  let graphql_hit = graphql["data"]["sourceReads"]["sourceReads"]
    .as_array()
    .and_then(|hits| hits.first())
    .expect("sourceReads hit");

  let root_text = root.to_string_lossy();
  let scan = run_secrets_json(root_text.as_ref());
  let scanner_hit = scan["source_evidence"]
    .as_array()
    .and_then(|hits| hits.first())
    .expect("scanner source_evidence hit");

  for (graphql_field, scanner_field) in [
    ("sourceId", "source_id"),
    ("providerId", "provider_id"),
    ("envVarName", "env_var_name"),
  ] {
    assert_eq!(
      graphql_hit[graphql_field], scanner_hit[scanner_field],
      "sourceReads and scanner diverged for {graphql_field}/{scanner_field}\ngraphql:\n{graphql:#}\nscanner:\n{scan:#}"
    );
  }
  assert_eq!(graphql_hit["lineNumber"], scanner_hit["line"]);
  assert_eq!(graphql_hit["column"], scanner_hit["column"]);
}

#[test]
fn aws_python_env_recipe_roundtrips_through_lmprobe_query() {
  let db = load_default_source_db().expect("default source db");
  let source = db
    .sources
    .iter()
    .find(|source| source.id == "env-aws")
    .expect("env-aws source");
  let language = source
    .languages
    .iter()
    .find(|language| language.language.to_string() == "python" && language.sdk == "stdlib-env")
    .expect("env-aws python language");
  let shape = language
    .read_shapes
    .iter()
    .find(|shape| shape.kind == ReadShapeKind::EnvRead)
    .expect("env-read shape");
  let recipe = generate_source_graphql_recipe(source, language.language, shape);
  assert!(recipe.contains("sourceReads("));
  assert!(recipe.contains("envVarNames:"));
  assert!(recipe.contains("\"AWS_SECRET_ACCESS_KEY\""));
  assert!(recipe.contains("accessPaths:"));
  assert!(recipe.contains("importHints:"));

  let fixture = PathBuf::from(WORKSPACE_ROOT)
    .join("crates/lmprobe-secrets/tests/fixtures/source_baseline/python")
    .to_string_lossy()
    .into_owned();
  let envelope = run_recipe(&recipe, &fixture);
  let result = envelope["data"]
    .as_object()
    .and_then(|data| data.values().next())
    .expect("query result object");
  let hits = result["sourceReads"].as_array().expect("sourceReads array");
  assert!(!hits.is_empty(), "envelope:\n{envelope:#}");
  assert!(
    hits.iter().any(|hit| {
      hit["envVarName"] == "AWS_SECRET_ACCESS_KEY"
        && hit["sourceId"] == "env-aws"
        && hit["providerId"] == "aws"
    }),
    "expected env-aws AWS_SECRET_ACCESS_KEY hit:\n{envelope:#}"
  );
}

#[test]
fn source_reads_ast_traversal_ignores_braces_in_strings_and_comments() {
  let root = fixture_root("brace-regression");
  fs::write(
    root.join("app.py"),
    "import os\nnoise = \"}\"\n# }\nsecret = os.environ[\"AWS_SECRET_ACCESS_KEY\"]\n",
  )
  .expect("write python fixture");

  let root_s = root.display();
  let query = format!(
    r#"{{
      sourceReads(path: "{root_s}", language: PYTHON, filters: [{{ envVarNames: ["AWS_SECRET_ACCESS_KEY"], accessPaths: ["os.environ"], importHints: ["os"] }}]) {{
        argValuesRedacted
        sourceReads {{ sourceId providerId envVarName lineNumber readText }}
      }}
    }}"#
  );
  let envelope = run_query(&query);
  let hits = envelope["data"]["sourceReads"]["sourceReads"]
    .as_array()
    .expect("sourceReads array");
  assert_eq!(hits.len(), 1, "envelope:\n{envelope:#}");
  assert_eq!(hits[0]["sourceId"], "env-aws");
  assert_eq!(hits[0]["providerId"], "aws");
  assert_eq!(hits[0]["envVarName"], "AWS_SECRET_ACCESS_KEY");
  assert_eq!(hits[0]["lineNumber"], 4);
  assert_eq!(envelope["data"]["sourceReads"]["argValuesRedacted"], false);
}

#[test]
fn source_reads_rejects_lookalike_env_accessors() {
  let root = fixture_root("lookalike-accessors");
  fs::write(
    root.join("app.py"),
    "secret = pretend_os_environ_get_workaround(\"AWS_SECRET_ACCESS_KEY\")\n",
  )
  .expect("write python fixture");
  fs::write(
    root.join("app.go"),
    "package main\nfunc main() { secret := pretend.os.Getenv(\"AWS_SECRET_ACCESS_KEY\"); _ = secret }\n",
  )
  .expect("write go fixture");

  let root_s = root.display();
  let py_query = format!(
    r#"{{
      sourceReads(path: "{root_s}", language: PYTHON, filters: [{{ envVarNames: ["AWS_SECRET_ACCESS_KEY"], accessPaths: ["os.environ.get"] }}]) {{
        sourceReads {{ sourceId envVarName accessPath readText }}
      }}
    }}"#
  );
  let py_envelope = run_query_allow_no_match(&py_query);
  assert_eq!(
    py_envelope["data"]["sourceReads"]["sourceReads"]
      .as_array()
      .expect("python sourceReads array")
      .len(),
    0,
    "python lookalike must not match os.environ.get:\n{py_envelope:#}"
  );

  let go_query = format!(
    r#"{{
      sourceReads(path: "{root_s}", language: GO, filters: [{{ envVarNames: ["AWS_SECRET_ACCESS_KEY"], accessPaths: ["os.Getenv"] }}]) {{
        sourceReads {{ sourceId envVarName accessPath readText }}
      }}
    }}"#
  );
  let go_envelope = run_query_allow_no_match(&go_query);
  assert_eq!(
    go_envelope["data"]["sourceReads"]["sourceReads"]
      .as_array()
      .expect("go sourceReads array")
      .len(),
    0,
    "go lookalike must not match os.Getenv:\n{go_envelope:#}"
  );
}

#[test]
fn source_reads_redacts_secret_paths_by_default_with_opt_out() {
  let root = fixture_root("redaction");
  fs::write(
    root.join("app.py"),
    "import hvac\nclient.secrets.kv.v2.read_secret_version(path=\"secret/data/prod-stripe-key\")\n",
  )
  .expect("write python fixture");

  let root_s = root.display();
  let query = format!(
    r#"{{
      redacted: sourceReads(path: "{root_s}", language: PYTHON, filters: [{{ accessPaths: ["secrets.kv.v2.read_secret_version"], importHints: ["hvac"] }}]) {{
        argValuesRedacted
        sourceReads {{ sourceId keyName accessPath readText }}
      }}
      raw: sourceReads(path: "{root_s}", language: PYTHON, redactArgValues: false, filters: [{{ accessPaths: ["secrets.kv.v2.read_secret_version"], importHints: ["hvac"] }}]) {{
        argValuesRedacted
        sourceReads {{ sourceId keyName accessPath readText }}
      }}
    }}"#
  );
  let envelope = run_query(&query);
  let redacted = &envelope["data"]["redacted"];
  let raw = &envelope["data"]["raw"];
  assert_eq!(
    redacted["argValuesRedacted"], true,
    "envelope:\n{envelope:#}"
  );
  assert_eq!(redacted["sourceReads"][0]["sourceId"], "hashicorp-vault-kv");
  assert_eq!(redacted["sourceReads"][0]["readText"], "REDACTED");
  assert_eq!(raw["argValuesRedacted"], false);
  assert!(
    raw["sourceReads"][0]["readText"]
      .as_str()
      .is_some_and(|text| text.contains("secret/data/prod-stripe-key")),
    "envelope:\n{envelope:#}"
  );
}
