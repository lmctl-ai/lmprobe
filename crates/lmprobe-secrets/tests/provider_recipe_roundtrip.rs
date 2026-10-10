use lmprobe_secrets::{generate_graphql_recipe, load_default_provider_db};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn fixture_root() -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-provider-recipe-roundtrip-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn run_recipe(recipe: &str, fixture: &Path) -> serde_json::Value {
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
      &format!("path={}", fixture.display()),
    ])
    .output()
    .expect("run generated provider recipe");
  let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
  assert!(
    output.status.success(),
    "generated recipe failed\nstderr:\n{stderr}\nstdout:\n{stdout}\nrecipe:\n{recipe}"
  );
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

#[test]
fn aws_python_boto3_recipe_roundtrips_through_lmprobe_query() {
  let db = load_default_provider_db().expect("default provider db");
  let provider = db
    .providers
    .iter()
    .find(|provider| provider.id == "aws")
    .expect("aws provider");
  let language = provider
    .languages
    .iter()
    .find(|language| language.language.to_string() == "python" && language.sdk == "boto3-v1")
    .expect("aws python boto3 language");
  let shape = language
    .call_shapes
    .iter()
    .find(|shape| {
      shape.callee.as_deref() == Some("boto3.client")
        && shape
          .kwargs
          .iter()
          .any(|name| name == "aws_secret_access_key")
    })
    .expect("boto3.client aws secret kwarg shape");
  let recipe = generate_graphql_recipe(provider, language, shape);
  assert!(recipe.contains("argNames:"));
  assert!(recipe.contains("\"aws_secret_access_key\""));
  assert!(recipe.contains("importHints:"));
  assert!(recipe.contains("\"boto3\""));

  let fixture = fixture_root();
  fs::write(
    fixture.join("app.py"),
    "import boto3\nsecret = \"synthetic\"\nboto3.client(\"s3\", aws_secret_access_key=secret)\n",
  )
  .expect("write fixture");

  let envelope = run_recipe(&recipe, &fixture);
  let callsites = envelope["data"]
    .as_object()
    .and_then(|data| data.values().next())
    .and_then(|result| result["callsites"].as_array())
    .expect("callsites array");
  let result = envelope["data"]
    .as_object()
    .unwrap()
    .values()
    .next()
    .unwrap();
  assert_eq!(result["argValuesRedacted"], true);
  assert!(!callsites.is_empty(), "envelope:\n{envelope:#}");
  let hit = &callsites[0];
  assert!(
    hit["importEvidence"]
      .as_array()
      .is_some_and(|items| items.iter().any(|item| item == "boto3")),
    "importEvidence should contain boto3:\n{envelope:#}"
  );
  assert!(
    hit["arguments"].as_array().is_some_and(|args| args
      .iter()
      .any(|arg| arg["name"] == "aws_secret_access_key" && arg["text"] == "REDACTED")),
    "arguments should include aws_secret_access_key:\n{envelope:#}"
  );
}
