use lmprobe_secrets::{generate_graphql_recipe, load_default_provider_db};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-secrets-recipes-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn run_ok(args: &[String]) -> String {
  let out = Command::new(binary())
    .args(args)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  assert_eq!(
    out.status.code(),
    Some(0),
    "stderr:\n{stderr}\nstdout:\n{stdout}"
  );
  stdout
}

fn run_error(args: &[String]) -> String {
  let out = Command::new(binary())
    .args(args)
    .output()
    .expect("spawn lmprobe");
  assert_ne!(out.status.code(), Some(0));
  String::from_utf8_lossy(&out.stderr).into_owned()
}

fn run_json(args: &[String]) -> serde_json::Value {
  let stdout = run_ok(args);
  serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"))
}

#[test]
fn secrets_recipes_list_enumerates_provider_language_shapes() {
  let db = load_default_provider_db().expect("default provider db");
  let expected_providers = db.providers.len();
  let expected_rows = db
    .providers
    .iter()
    .flat_map(|provider| &provider.languages)
    .map(|language| language.call_shapes.len())
    .sum::<usize>();

  let envelope = run_json(&[
    "secrets-recipes".into(),
    "list".into(),
    "--format".into(),
    "json".into(),
  ]);
  let recipes = envelope["recipes"].as_array().expect("recipes array");
  assert_eq!(recipes.len(), expected_rows);

  let providers = recipes
    .iter()
    .filter_map(|row| row["provider_id"].as_str())
    .collect::<HashSet<_>>();
  assert_eq!(providers.len(), expected_providers);

  let languages = recipes
    .iter()
    .filter_map(|row| row["language"].as_str())
    .collect::<HashSet<_>>();
  assert!(languages.contains("python"));
  assert!(languages.contains("javascript"));
  assert!(languages.contains("go"));
  assert!(languages.contains("java"));
}

#[test]
fn secrets_recipes_list_filters_provider_and_language() {
  let envelope = run_json(&[
    "secrets-recipes".into(),
    "list".into(),
    "--language".into(),
    "python".into(),
    "--provider".into(),
    "aws".into(),
    "--format".into(),
    "json".into(),
  ]);
  let recipes = envelope["recipes"].as_array().expect("recipes array");
  assert!(!recipes.is_empty());
  assert!(recipes.iter().all(|row| row["provider_id"] == "aws"));
  assert!(recipes.iter().all(|row| row["language"] == "python"));
}

#[test]
fn secrets_recipes_show_matches_generated_graphql() {
  let db = load_default_provider_db().expect("default provider db");
  let provider = db
    .providers
    .iter()
    .find(|provider| provider.id == "aws")
    .unwrap();
  let language = provider
    .languages
    .iter()
    .find(|language| language.language.to_string() == "python")
    .unwrap();
  let expected = generate_graphql_recipe(provider, language, &language.call_shapes[0]);

  let stdout = run_ok(&["secrets-recipes".into(), "show".into(), "aws/python".into()]);
  assert_eq!(stdout, format!("{expected}\n"));
  assert!(stdout.contains("callsites"));
  assert!(stdout.contains("argNames:"));
  assert!(stdout.contains("boto3.client"));
}

#[test]
fn secrets_recipes_show_requires_sdk_when_ambiguous() {
  let out = Command::new(binary())
    .args(["secrets-recipes", "show", "aws/java"])
    .output()
    .expect("spawn lmprobe");
  assert_ne!(out.status.code(), Some(0));
  let stderr = String::from_utf8_lossy(&out.stderr);
  assert!(stderr.contains("multiple SDKs match `aws/java`"));
  assert!(stderr.contains("aws-sdk-java-v1"));
  assert!(stderr.contains("aws-sdk-java-v2"));

  let stdout = run_ok(&[
    "secrets-recipes".into(),
    "show".into(),
    "aws/java/aws-sdk-java-v2".into(),
  ]);
  assert!(stdout.contains("AwsBasicCredentials.create"));
}

fn write_all_shapes_provider_fixture(root: &Path) {
  fs::write(
    root.join("app.py"),
    r#"import boto3
boto3.client("s3", aws_secret_access_key=client_secret)
boto3.Session(aws_secret_access_key=session_secret)
"#,
  )
  .expect("write provider fixture");
}

#[test]
fn secrets_recipes_run_is_removed_and_query_recipe_all_shapes_executes_sweep() {
  let fixture = fixture_root("all-shapes");
  write_all_shapes_provider_fixture(&fixture);
  let fixture_s = fixture.display().to_string();
  let stderr = run_error(&[
    "secrets-recipes".into(),
    "run".into(),
    "aws/python".into(),
    fixture_s.clone(),
  ]);
  assert!(
    stderr.contains("run") && stderr.contains("Usage:"),
    "stderr:\n{stderr}"
  );

  let single = run_json(&[
    "--format".into(),
    "json".into(),
    "query".into(),
    "--recipe".into(),
    "secrets/aws/python".into(),
    "--var".into(),
    format!("path={fixture_s}"),
  ]);
  let all = run_json(&[
    "--format".into(),
    "json".into(),
    "query".into(),
    "--recipe".into(),
    "secrets/aws/python".into(),
    "--all-shapes".into(),
    "--var".into(),
    format!("path={fixture_s}"),
  ]);

  let single_sites = single["callsites"].as_array().expect("single callsites");
  let all_sites = all["callsites"].as_array().expect("all callsites");
  assert_eq!(single_sites.len(), 1, "single envelope:\n{single:#}");
  assert_eq!(all_sites.len(), 2, "all-shapes envelope:\n{all:#}");
  assert_eq!(all["schema"], "lmprobe.callsites.v1");
  assert_eq!(all["arg_values_redacted"], true);

  let callees = all_sites
    .iter()
    .filter_map(|site| site["callee_text"].as_str())
    .collect::<HashSet<_>>();
  assert!(callees.contains("boto3.client"), "all envelope:\n{all:#}");
  assert!(callees.contains("boto3.Session"), "all envelope:\n{all:#}");

  let query_has_secret_kwarg = all_sites.iter().any(|site| {
    site["structured_args"].as_array().is_some_and(|args| {
      args
        .iter()
        .any(|arg| arg["name"] == "aws_secret_access_key" && arg["text"] == "REDACTED")
    })
  });
  assert!(query_has_secret_kwarg, "all envelope:\n{all:#}");
}
