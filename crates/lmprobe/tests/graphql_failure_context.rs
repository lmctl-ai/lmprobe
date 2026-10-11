use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.root);
  }
}

fn fixture() -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-graphql-failure-context-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("create fixture root");
  std::fs::write(
    root.join("app.py"),
    r#"class Service:
    def handle(self, value):
        result = target(value)
        if result:
            return result
        return 0


def target(value):
    return value + 1


def other():
    return target(3)
"#,
  )
  .expect("write app.py");
  init_git(&root);
  Fixture { root }
}

fn init_git(root: &Path) {
  let git = |args: &[&str]| {
    Command::new("git")
      .args(args)
      .current_dir(root)
      .output()
      .expect("spawn git")
  };
  git(&["init", "-q"]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "config",
    "commit.gpgsign",
    "false",
  ]);
  git(&["add", "."]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "commit",
    "-qm",
    "init",
  ]);
}

fn run_query(root: &Path, query: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "query", query])
    .current_dir(root)
    .output()
    .expect("spawn lmprobe query");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn graphql_exposes_failure_anchor_expansions() {
  let fix = fixture();
  let query = r#"
    {
      lineCtx: context(symbol: "app.py:3", path: ".", around: 1) {
        symbol
        sourceContext { lineNumber lineContent }
        warnings { code message }
        exitStatus
      }
      usageCtx: context(symbol: "target", path: ".", usage: true, around: 1, parents: true, body: true) {
        usages {
          filePath
          lineNumber
          lineContent
          beforeContext { lineNumber lineContent }
          afterContext { lineNumber lineContent }
          parents { kind name lineStart lineEnd }
          body { kind name lineStart lineEnd content }
        }
        exitStatus
      }
      grepExp: grep(pattern: "target", path: ".", around: 1, parents: true, body: true, role: true) {
        hits {
          filePath
          lineNumber
          lineContent
          beforeContext { lineNumber lineContent }
          afterContext { lineNumber lineContent }
          parents { kind name lineStart lineEnd }
          body { kind name lineStart lineEnd content }
          role
        }
        exitStatus
      }
      defExp: def(name: "target", path: ".", around: 1) {
        hits { name filePath lineStart sourceContext { lineNumber lineContent } }
        exitStatus
      }
      refsExp: refs(name: "target", path: ".", role: true) {
        hits { filePath lineNumber lineContent role }
        exitStatus
      }
    }
  "#;

  let (envelope, stderr, exit) = run_query(&fix.root, query);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let data = envelope.get("data").expect("data");

  let source_context = data
    .pointer("/lineCtx/sourceContext")
    .and_then(|value| value.as_array())
    .expect("line context sourceContext");
  assert!(
    source_context.iter().any(
      |line| line["lineNumber"] == 3 && line["lineContent"] == "        result = target(value)"
    ),
    "context path:line should include source context around the supplied anchor:\n{envelope:#}"
  );

  let usages = data
    .pointer("/usageCtx/usages")
    .and_then(|value| value.as_array())
    .expect("usage hits");
  let call_usage = usages
    .iter()
    .find(|hit| hit["lineNumber"] == 3)
    .expect("usage hit on target(value)");
  assert!(
    call_usage["beforeContext"]
      .as_array()
      .is_some_and(|lines| !lines.is_empty())
      && call_usage["afterContext"]
        .as_array()
        .is_some_and(|lines| !lines.is_empty()),
    "usage context should carry around lines:\n{call_usage:#}"
  );
  assert!(
    call_usage["parents"]
      .as_array()
      .is_some_and(|parents| !parents.is_empty()),
    "usage context should carry structural parents:\n{call_usage:#}"
  );
  assert!(
    call_usage.get("body").is_some_and(|body| body["content"]
      .as_str()
      .is_some_and(|content| content.contains("result = target(value)"))),
    "usage context should carry structural body:\n{call_usage:#}"
  );

  let grep_hits = data
    .pointer("/grepExp/hits")
    .and_then(|value| value.as_array())
    .expect("grep hits");
  let grep_call = grep_hits
    .iter()
    .find(|hit| hit["lineNumber"] == 3)
    .expect("grep hit on target(value)");
  assert!(
    grep_call["parents"]
      .as_array()
      .is_some_and(|parents| !parents.is_empty())
      && grep_call.get("body").is_some()
      && grep_call["beforeContext"]
        .as_array()
        .is_some_and(|lines| !lines.is_empty())
      && grep_call["afterContext"]
        .as_array()
        .is_some_and(|lines| !lines.is_empty())
      && grep_call["role"]
        .as_str()
        .is_some_and(|role| !role.is_empty()),
    "grep GraphQL should expose parents/body/around/role:\n{grep_call:#}"
  );

  let def_hit = data
    .pointer("/defExp/hits")
    .and_then(|value| value.as_array())
    .and_then(|hits| hits.first())
    .expect("def hit");
  assert!(
    def_hit["sourceContext"]
      .as_array()
      .is_some_and(|lines| lines
        .iter()
        .any(|line| line["lineContent"] == "def target(value):")),
    "def GraphQL should expose around source_context:\n{def_hit:#}"
  );

  let ref_hits = data
    .pointer("/refsExp/hits")
    .and_then(|value| value.as_array())
    .expect("ref hits");
  assert!(
    ref_hits
      .iter()
      .any(|hit| hit["lineNumber"] == 3 && hit["role"].as_str().is_some()),
    "refs GraphQL should expose role when requested:\n{envelope:#}"
  );
}
