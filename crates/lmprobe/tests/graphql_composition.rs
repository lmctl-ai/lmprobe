use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("lmprobe-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).expect("create tmp dir");
  dir
}

fn gql_string(path: &Path) -> String {
  serde_json::to_string(&path.to_string_lossy().to_string()).expect("json string")
}

fn normalize_path(path: &str) -> String {
  path.replace('\\', "/")
}

fn run_query(query: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "query", query])
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn warning_codes_at<'a>(envelope: &'a serde_json::Value, pointer: &str) -> Vec<&'a str> {
  envelope
    .pointer(pointer)
    .and_then(|v| v.as_array())
    .into_iter()
    .flatten()
    .filter_map(|warning| warning.get("code").and_then(|v| v.as_str()))
    .collect()
}

#[test]
fn graphql_grep_and_find_accept_additional_paths() {
  let dir = fixture_dir("graphql-multi-root");
  let one = dir.join("one");
  let two = dir.join("two");
  fs::create_dir_all(&one).expect("create one");
  fs::create_dir_all(&two).expect("create two");
  fs::write(one.join("a.txt"), "alpha needle\n").expect("write one");
  fs::write(two.join("b.txt"), "beta needle\n").expect("write two");

  let query = format!(
    r#"{{
      grep(pattern: "needle", path: {}, paths: [{}]) {{
        hits {{ filePath lineNumber lineContent }}
        exitStatus
      }}
      find(pattern: "\\.txt$", path: {}, paths: [{}]) {{
        paths
        exitStatus
      }}
    }}"#,
    gql_string(&one),
    gql_string(&two),
    gql_string(&one),
    gql_string(&two),
  );

  let (envelope, stderr, exit) = run_query(&query);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope
      .pointer("/data/grep/hits")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(2),
    "GraphQL grep(paths:) should scan both roots:\n{envelope:#}",
  );
  assert_eq!(
    envelope
      .pointer("/data/find/paths")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(2),
    "GraphQL find(paths:) should scan both roots:\n{envelope:#}",
  );
}

#[test]
fn graphql_grep_defaults_skip_dependency_dirs_with_explicit_opt_in() {
  let dir = fixture_dir("graphql-default-excludes");
  fs::create_dir_all(dir.join("src")).expect("create src");
  fs::create_dir_all(dir.join("node_modules/pkg")).expect("create dependency dir");
  fs::write(dir.join("src/app.js"), "needle source\n").expect("write src");
  fs::write(
    dir.join("node_modules/pkg/generated.js"),
    "needle dependency\n",
  )
  .expect("write dependency");

  let query = format!(
    r#"{{
      defaultScan: grep(pattern: "needle", path: {}) {{
        hits {{ filePath }}
        exitStatus
      }}
      exhaustiveScan: grep(pattern: "needle", path: {}, noDefaultExcludes: true) {{
        hits {{ filePath }}
        exitStatus
      }}
    }}"#,
    gql_string(&dir),
    gql_string(&dir),
  );

  let (envelope, stderr, exit) = run_query(&query);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  let default_paths = envelope
    .pointer("/data/defaultScan/hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected default hits:\n{envelope:#}"))
    .iter()
    .filter_map(|hit| hit.get("filePath").and_then(|path| path.as_str()))
    .map(normalize_path)
    .collect::<Vec<_>>();
  assert!(
    default_paths
      .iter()
      .any(|path| path.ends_with("src/app.js")),
    "default GraphQL scan should keep source hits:\n{default_paths:#?}",
  );
  assert!(
    !default_paths
      .iter()
      .any(|path| path.contains("node_modules")),
    "default GraphQL scan should skip dependencies:\n{default_paths:#?}",
  );

  let exhaustive_paths = envelope
    .pointer("/data/exhaustiveScan/hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected exhaustive hits:\n{envelope:#}"))
    .iter()
    .filter_map(|hit| hit.get("filePath").and_then(|path| path.as_str()))
    .map(normalize_path)
    .collect::<Vec<_>>();
  assert!(
    exhaustive_paths
      .iter()
      .any(|path| path.contains("node_modules")),
    "explicit GraphQL opt-in should scan dependencies:\n{exhaustive_paths:#?}",
  );
}

#[test]
fn graphql_find_and_grep_support_quick_project_roots() {
  let dir = fixture_dir("graphql-quick-project-roots");
  let app = dir.join("js-app/src");
  let perl = dir.join("perl-lib");
  let sparse_perl = dir.join("sparse-perl");
  let loose = dir.join("loose");
  fs::create_dir_all(&app).expect("create app");
  fs::create_dir_all(&perl).expect("create perl");
  fs::create_dir_all(&sparse_perl).expect("create sparse perl");
  fs::create_dir_all(&loose).expect("create loose");
  fs::write(
    dir.join("js-app/package.json"),
    r#"{"name":"js-app","version":"1.0.0"}"#,
  )
  .expect("write package");
  fs::write(
    app.join("copilot.ts"),
    "export const provider = 'copilot oauth';\n",
  )
  .expect("write app");
  for idx in 0..5 {
    let body = if idx == 2 {
      "package Provider::Auth; # copilot oauth\n"
    } else {
      "package Provider::Other;\n"
    };
    fs::write(perl.join(format!("Provider{idx}.pm")), body).expect("write perl file");
  }
  for idx in 0..4 {
    let body = if idx == 1 {
      "package Sparse::Auth; # copilot oauth sparse\n"
    } else {
      "package Sparse::Other;\n"
    };
    fs::write(sparse_perl.join(format!("Sparse{idx}.pm")), body).expect("write sparse perl file");
  }
  fs::write(loose.join("copilot-notes.txt"), "copilot oauth loose\n").expect("write loose");

  let query = format!(
    r#"{{
      grep(pattern: "copilot oauth", path: {}, quickProjects: true) {{
        hits {{ filePath }}
        exitStatus
      }}
      find(pattern: "copilot", path: {}, quickProjects: true) {{
        paths
        exitStatus
      }}
    }}"#,
    gql_string(&dir),
    gql_string(&dir),
  );

  let (envelope, stderr, exit) = run_query(&query);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  let grep_paths = envelope
    .pointer("/data/grep/hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"))
    .iter()
    .filter_map(|hit| hit.get("filePath").and_then(|path| path.as_str()))
    .map(normalize_path)
    .collect::<Vec<_>>();
  assert!(
    grep_paths
      .iter()
      .any(|path| path.ends_with("js-app/src/copilot.ts")),
    "quick GraphQL grep should include package roots:\n{grep_paths:#?}",
  );
  assert!(
    grep_paths
      .iter()
      .any(|path| path.ends_with("perl-lib/Provider2.pm")),
    "quick GraphQL grep should include dense Perl roots:\n{grep_paths:#?}",
  );
  assert!(
    !grep_paths.iter().any(|path| path.contains("/loose/")),
    "quick GraphQL grep should skip loose matches:\n{grep_paths:#?}",
  );
  assert!(
    !grep_paths.iter().any(|path| path.contains("/sparse-perl/")),
    "quick GraphQL grep should not promote Perl dirs below the density threshold:\n{grep_paths:#?}",
  );

  let find_paths = envelope
    .pointer("/data/find/paths")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected find paths:\n{envelope:#}"))
    .iter()
    .filter_map(|path| path.as_str())
    .map(normalize_path)
    .collect::<Vec<_>>();
  assert!(
    find_paths
      .iter()
      .any(|path| path.ends_with("js-app/src/copilot.ts")),
    "quick GraphQL find should include package-root paths:\n{find_paths:#?}",
  );
  assert!(
    !find_paths.iter().any(|path| path.contains("/loose/")),
    "quick GraphQL find should skip loose paths:\n{find_paths:#?}",
  );
  assert!(
    !find_paths.iter().any(|path| path.contains("/sparse-perl/")),
    "quick GraphQL find should not promote Perl dirs below the density threshold:\n{find_paths:#?}",
  );
}

#[test]
fn graphql_find_grep_and_grep_dirs_with_report_missing_paths() {
  let dir = fixture_dir("graphql-missing-path");
  let missing = dir.join("missing");
  let query = format!(
    r#"{{
      find(pattern: "needle", path: {}) {{
        warnings {{ code message hint }}
        exitStatus
      }}
      grep(pattern: "needle", path: {}) {{
        warnings {{ code message hint }}
        exitStatus
      }}
      grepDirsWith(markerPattern: "package\\.json$", pattern: "needle", path: {}) {{
        warnings {{ code message hint }}
        exitStatus
      }}
    }}"#,
    gql_string(&missing),
    gql_string(&missing),
    gql_string(&missing),
  );

  let (envelope, stderr, exit) = run_query(&query);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 2, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  for (status_pointer, warnings_pointer) in [
    ("/data/find/exitStatus", "/data/find/warnings"),
    ("/data/grep/exitStatus", "/data/grep/warnings"),
    (
      "/data/grepDirsWith/exitStatus",
      "/data/grepDirsWith/warnings",
    ),
  ] {
    assert_eq!(
      envelope.pointer(status_pointer).and_then(|v| v.as_str()),
      Some("ERROR"),
      "missing path should be an error-status resolver:\n{envelope:#}",
    );
    assert!(
      warning_codes_at(&envelope, warnings_pointer).contains(&"PATH_NOT_FOUND"),
      "missing path should carry PATH_NOT_FOUND at {warnings_pointer}:\n{envelope:#}",
    );
  }
}

#[test]
fn graphql_grep_dirs_with_pipes_marker_dirs_into_grep() {
  let dir = fixture_dir("graphql-grep-dirs-with");
  let app = dir.join("apps/web");
  let api = dir.join("apps/api");
  let other = dir.join("other/services/auth");
  let loose = dir.join("loose");
  fs::create_dir_all(app.join("src")).expect("create web");
  fs::create_dir_all(api.join("src")).expect("create api");
  fs::create_dir_all(other.join("src")).expect("create other");
  fs::create_dir_all(&loose).expect("create loose");
  fs::write(
    app.join("package.json"),
    r#"{"name":"web","version":"1.0.0"}"#,
  )
  .expect("write web package");
  fs::write(
    api.join("package.json"),
    r#"{"name":"api","version":"1.0.0"}"#,
  )
  .expect("write api package");
  fs::write(
    other.join("package.json"),
    r#"{"name":"auth","version":"1.0.0"}"#,
  )
  .expect("write other package");
  fs::write(
    app.join("src/login.ts"),
    "export const login = () => true;\n",
  )
  .expect("write login");
  fs::write(
    api.join("src/health.ts"),
    "export const health = () => true;\n",
  )
  .expect("write health");
  fs::write(
    other.join("src/login.ts"),
    "export const login = () => 'auth';\n",
  )
  .expect("write other login");
  fs::write(loose.join("login.txt"), "login outside a package root\n").expect("write loose");

  let query = format!(
    r#"{{
      grepDirsWith(
        markerPattern: "(^|[\\\\/])package\\.json$",
        pattern: "login",
        path: {},
        paths: [{}]
      ) {{
        hits {{ filePath lineNumber lineContent }}
        exitStatus
      }}
    }}"#,
    gql_string(&dir.join("apps")),
    gql_string(&dir.join("other")),
  );

  let (envelope, stderr, exit) = run_query(&query);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  let hits = envelope
    .pointer("/data/grepDirsWith/hits")
    .and_then(|v| v.as_array())
    .expect("hits");
  assert_eq!(
    hits.len(),
    2,
    "only package-root hits from both marker roots should match:\n{envelope:#}"
  );
  let mut file_paths = hits
    .iter()
    .map(|hit| hit["filePath"].as_str().expect("filePath"))
    .map(normalize_path)
    .collect::<Vec<_>>();
  file_paths.sort();
  assert!(
    file_paths
      .iter()
      .any(|path| path.ends_with("apps/web/src/login.ts")),
    "one hit should come from the first marker root:\n{envelope:#}",
  );
  assert!(
    file_paths
      .iter()
      .any(|path| path.ends_with("other/services/auth/src/login.ts")),
    "one hit should come from the paths: marker root:\n{envelope:#}",
  );
  assert!(
    file_paths
      .iter()
      .all(|path| !path.ends_with("loose/login.txt")),
    "loose file outside marker roots should not match:\n{envelope:#}",
  );
}
