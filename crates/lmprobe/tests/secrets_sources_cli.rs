use lmprobe_secrets::{generate_source_graphql_recipe, load_default_source_db};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// The fixture keeps a `REMOVEME` mark inside Stripe-shaped keys so GitHub
// secret scanning does not reject pushes; scan an unmarked temp copy.
fn source_python_fixture() -> PathBuf {
  let from = workspace_root().join("crates/lmprobe-secrets/tests/fixtures/source_baseline/python");
  let to = std::env::temp_dir().join(format!("lmprobe-sources-cli-{}", std::process::id()));
  let _ = fs::remove_dir_all(&to);
  fs::create_dir_all(&to).expect("create fixture copy dir");
  for entry in fs::read_dir(&from).expect("read fixture dir") {
    let entry = entry.expect("fixture entry");
    let text = fs::read_to_string(entry.path()).expect("read fixture");
    fs::write(to.join(entry.file_name()), text.replace("REMOVEME", "")).expect("write copy");
  }
  to
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
fn query_help_mentions_provider_and_source_recipe_namespaces() {
  let stdout = run_ok(&["query".into(), "--help".into()]);
  assert!(stdout.contains("secrets/aws/python"));
  assert!(stdout.contains("secrets-source/env-aws/python"));
  assert!(stdout.contains("--all-shapes"));
}

#[test]
fn secrets_sources_list_enumerates_source_language_shapes() {
  let db = load_default_source_db().expect("default source db");
  let expected_sources = db.sources.len();
  let expected_rows = db
    .sources
    .iter()
    .flat_map(|source| &source.languages)
    .map(|language| language.read_shapes.len())
    .sum::<usize>();

  let envelope = run_json(&[
    "secrets-sources".into(),
    "list".into(),
    "--format".into(),
    "json".into(),
  ]);
  let recipes = envelope["recipes"].as_array().expect("recipes array");
  assert_eq!(recipes.len(), expected_rows);

  let sources = recipes
    .iter()
    .filter_map(|row| row["source_id"].as_str())
    .collect::<HashSet<_>>();
  assert_eq!(sources.len(), expected_sources);

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
fn secrets_sources_list_filters_source_language_category_and_provider() {
  let envelope = run_json(&[
    "secrets-sources".into(),
    "list".into(),
    "--language".into(),
    "python".into(),
    "--source".into(),
    "env-aws".into(),
    "--category".into(),
    "env".into(),
    "--provider".into(),
    "aws".into(),
    "--format".into(),
    "json".into(),
  ]);
  let recipes = envelope["recipes"].as_array().expect("recipes array");
  assert!(!recipes.is_empty());
  assert!(recipes.iter().all(|row| row["source_id"] == "env-aws"));
  assert!(recipes.iter().all(|row| row["language"] == "python"));
  assert!(recipes.iter().all(|row| row["source_category"] == "env"));
  assert!(recipes.iter().all(|row| row["provider_id"] == "aws"));
}

#[test]
fn secrets_sources_show_matches_generated_graphql() {
  let db = load_default_source_db().expect("default source db");
  let source = db
    .sources
    .iter()
    .find(|source| source.id == "env-aws")
    .unwrap();
  let language = source
    .languages
    .iter()
    .find(|language| language.language.to_string() == "python")
    .unwrap();
  let expected =
    generate_source_graphql_recipe(source, language.language, &language.read_shapes[0]);

  let stdout = run_ok(&[
    "secrets-sources".into(),
    "show".into(),
    "env-aws/python".into(),
  ]);
  assert_eq!(stdout, format!("{expected}\n"));
  assert!(stdout.contains("sourceReads"));
  assert!(stdout.contains("envVarNames:"));
  assert!(stdout.contains("AWS_SECRET_ACCESS_KEY"));
}

#[test]
fn secrets_sources_run_is_removed_and_query_recipe_all_shapes_executes() {
  let fixture = source_python_fixture();
  let fixture_s = fixture.display().to_string();
  let stderr = run_error(&[
    "secrets-sources".into(),
    "run".into(),
    "env-aws/python".into(),
    fixture_s.clone(),
  ]);
  assert!(
    stderr.contains("run") && stderr.contains("Usage:"),
    "stderr:\n{stderr}"
  );

  let envelope = run_json(&[
    "--format".into(),
    "json".into(),
    "query".into(),
    "--recipe".into(),
    "secrets-source/env-aws/python".into(),
    "--all-shapes".into(),
    "--var".into(),
    format!("path={fixture_s}"),
  ]);
  let reads = envelope["source_reads"].as_array().expect("source reads");
  assert!(!reads.is_empty());
  assert_eq!(envelope["schema"], "lmprobe.source_reads.v1");
  assert!(reads.iter().any(|hit| {
    hit["source_id"] == "env-aws"
      && hit["provider_id"] == "aws"
      && hit["env_var_name"] == "AWS_SECRET_ACCESS_KEY"
  }));
}

#[test]
fn secrets_sources_run_redacts_sensitive_source_payloads_by_default() {
  let dir = tempfile::tempdir().expect("tempdir");
  let fixture = dir.path().join("vault.py");
  fs::write(
    &fixture,
    r#"
import hvac
secret = client.secrets.kv.v2.read_secret_version(path="secret/data/prod-stripe-key")
"#,
  )
  .expect("write fixture");

  let envelope = run_json(&[
    "--format".into(),
    "json".into(),
    "query".into(),
    "--recipe".into(),
    "secrets-source/hashicorp-vault-kv/python".into(),
    "--var".into(),
    format!("path={}", dir.path().display()),
  ]);
  assert_eq!(envelope["arg_values_redacted"], true);
  let reads = envelope["source_reads"].as_array().expect("source reads");
  assert!(reads
    .iter()
    .any(|hit| { hit["source_id"] == "hashicorp-vault-kv" && hit["read_text"] == "REDACTED" }));
}
