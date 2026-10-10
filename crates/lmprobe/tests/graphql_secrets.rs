use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_root() -> tempfile::TempDir {
  tempfile::Builder::new()
    .prefix("lmprobe-graphql-secrets-")
    .tempdir()
    .expect("create fixture root")
}

fn graphql_path_literal(path: &Path) -> String {
  serde_json::to_string(&path.to_string_lossy()).expect("GraphQL string literal")
}

fn run_json(root: &Path, args: &[String]) -> serde_json::Value {
  let out = Command::new(binary())
    .args(args)
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

#[test]
fn graphql_secrets_returns_cli_finding_shape() {
  let root = fixture_root();
  let root_path = root.path();
  std::fs::write(
    root_path.join("secret.py"),
    "key = \"AKIAZ3M7Q6YB2N4P5R6S\"\n",
  )
  .expect("write fixture");
  let root_s = root_path.to_string_lossy().into_owned();

  let cli = run_json(
    root_path,
    &[
      "--format".into(),
      "json".into(),
      "secrets".into(),
      root_s.clone(),
    ],
  );
  let root_literal = graphql_path_literal(root_path);
  let query = format!(
    "{{ secrets(path: {root_literal}) {{ schema findings {{ ruleId filePath lineNumber }} exitStatus }} }}"
  );
  let gql = run_json(root_path, &["query".into(), query]);

  let cli_finding = &cli["findings"][0];
  let gql_finding = &gql["data"]["secrets"]["findings"][0];
  assert_eq!(gql["data"]["secrets"]["schema"], "lmprobe.secrets.v1");
  assert_eq!(gql_finding["ruleId"], cli_finding["rule_id"]);
  assert_eq!(gql_finding["filePath"], cli_finding["file_path"]);
  assert_eq!(gql_finding["lineNumber"], cli_finding["line_number"]);
  assert_eq!(gql["data"]["secrets"]["exitStatus"], "MATCHED");
}
