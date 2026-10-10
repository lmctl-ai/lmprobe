use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
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
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-grep-parents-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(
    root.join("nested.py"),
    "class Runner:\n    def run(self):\n        marker = \"needle\"\n        return marker\n\n    def branch(self, flag):\n        if flag:\n            branch_marker = \"branch-token\"\n            return branch_marker\n        return \"miss\"\n",
  )
  .expect("write nested fixture");
  std::fs::write(
    root.join("top_level.py"),
    "outside_marker = \"outside-needle\"\n",
  )
  .expect("write top-level fixture");
  std::fs::write(
    root.join("Dao.java"),
    "public class Dao {\n    public void run() {\n        String outside = \"SELECT * FROM config\";\n        while (hasMoreRows()) {\n            String loopSql = \"SELECT * FROM live_rows\";\n            process(loopSql);\n        }\n        for (int i = 0; i < 10; i++) {\n            String eachRow = \"SELECT * FROM loop_rows\";\n            process(eachRow);\n        }\n        for (Item item : items) {\n            process(item);\n            String enhancedRow = \"SELECT * FROM enhanced_rows\";\n        }\n    }\n}\n",
  )
  .expect("write Java fixture");
  Fixture { root }
}

fn run(root: &Path, args: &[&str]) -> (String, String, i32) {
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
  let (stdout, stderr, exit) = run(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

fn first_hit(envelope: &serde_json::Value) -> &serde_json::Value {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .and_then(|hits| hits.first())
    .unwrap_or_else(|| panic!("expected at least one grep hit:\n{envelope:#}"))
}

#[test]
fn grep_json_parents_adds_enclosing_structural_chain() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--parents",
      "needle",
      "nested.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  let parents = hit
    .get("parents")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected parents:\n{hit:#}"));
  assert_eq!(
    parents
      .iter()
      .filter_map(|parent| parent.get("name").and_then(|v| v.as_str()))
      .collect::<Vec<_>>(),
    vec!["Runner", "run"]
  );
  assert_eq!(
    parents
      .first()
      .and_then(|parent| parent.get("kind"))
      .and_then(|v| v.as_str()),
    Some("class_definition")
  );
  assert_eq!(
    parents
      .get(1)
      .and_then(|parent| parent.get("kind"))
      .and_then(|v| v.as_str()),
    Some("function_definition")
  );
  assert_eq!(
    parents
      .get(1)
      .and_then(|parent| parent.get("line_start"))
      .and_then(|v| v.as_u64()),
    Some(2)
  );
}

#[test]
fn grep_json_omits_parents_without_parents_flag() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "grep", "needle", "nested.py"],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert!(
    hit.get("parents").is_none(),
    "default grep hit should stay compact:\n{hit:#}"
  );
  assert!(
    hit.get("body").is_none(),
    "default grep hit should omit body:\n{hit:#}"
  );
}

#[test]
fn grep_json_body_adds_innermost_structural_body() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "grep", "--body", "needle", "nested.py"],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert!(
    hit.get("parents").is_none(),
    "--body alone should not attach parents:\n{hit:#}"
  );
  assert_eq!(
    hit.pointer("/body/name").and_then(|v| v.as_str()),
    Some("run")
  );
  assert_eq!(
    hit.pointer("/body/kind").and_then(|v| v.as_str()),
    Some("function_definition")
  );
  assert_eq!(
    hit.pointer("/body/line_start").and_then(|v| v.as_u64()),
    Some(2)
  );
  assert!(
    hit
      .pointer("/body/content")
      .and_then(|v| v.as_str())
      .is_some_and(|content| content.contains("return marker")),
    "body should contain the innermost function source:\n{hit:#}"
  );
}

#[test]
fn grep_body_omits_when_hit_is_outside_structural_body() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--body",
      "outside-needle",
      "top_level.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert!(
    hit.get("body").is_none(),
    "top-level hit should omit body rather than fail:\n{hit:#}"
  );
}

#[test]
fn grep_body_selects_innermost_statement_block() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--parents",
      "--body",
      "branch-token",
      "nested.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert_eq!(
    hit.pointer("/body/kind").and_then(|v| v.as_str()),
    Some("if_statement"),
    "body should select the nested if block rather than the enclosing function:\n{hit:#}",
  );
  assert!(
    hit.pointer("/body/name").is_none(),
    "statement block bodies should not invent names:\n{hit:#}"
  );
  assert_eq!(
    hit.pointer("/body/line_start").and_then(|v| v.as_u64()),
    Some(7)
  );
  assert!(
    hit
      .pointer("/body/content")
      .and_then(|v| v.as_str())
      .is_some_and(|content| {
        content.contains("if flag:")
          && content.contains("branch-token")
          && !content.contains("def branch")
      }),
    "body content should be the inner statement block:\n{hit:#}"
  );
  assert_eq!(
    hit.pointer("/parents/1/name").and_then(|v| v.as_str()),
    Some("branch"),
    "parents should remain class/function-like and not include the if statement:\n{hit:#}",
  );
}

#[test]
fn grep_body_composes_with_parents_and_line_context_flags() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "-C",
      "1",
      "grep",
      "--parents",
      "--body",
      "needle",
      "nested.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert!(
    hit.get("parents").and_then(|v| v.as_array()).is_some(),
    "parents should be attached with --parents:\n{hit:#}"
  );
  assert_eq!(
    hit.pointer("/body/name").and_then(|v| v.as_str()),
    Some("run")
  );
  assert_eq!(
    hit
      .pointer("/before_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("    def run(self):")
  );
  assert_eq!(
    hit
      .pointer("/after_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("        return marker")
  );
}

#[test]
fn grep_ndjson_parents_are_attached_to_item_lines() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "grep",
      "--parents",
      "needle",
      "nested.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{stdout}\nstderr:\n{stderr}");
  let item: serde_json::Value = stdout
    .lines()
    .map(|line| serde_json::from_str(line).expect("ndjson line should parse"))
    .find(|value: &serde_json::Value| value.get("kind").is_none())
    .unwrap_or_else(|| panic!("expected an item line:\n{stdout}"));

  assert_eq!(
    item.pointer("/parents/0/name").and_then(|v| v.as_str()),
    Some("Runner")
  );
  assert_eq!(
    item.pointer("/parents/1/name").and_then(|v| v.as_str()),
    Some("run")
  );
}

#[test]
fn grep_ndjson_body_is_attached_to_item_lines() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "grep",
      "--body",
      "needle",
      "nested.py",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{stdout}\nstderr:\n{stderr}");
  let item: serde_json::Value = stdout
    .lines()
    .map(|line| serde_json::from_str(line).expect("ndjson line should parse"))
    .find(|value: &serde_json::Value| value.get("kind").is_none())
    .unwrap_or_else(|| panic!("expected an item line:\n{stdout}"));

  assert_eq!(
    item.pointer("/body/name").and_then(|v| v.as_str()),
    Some("run")
  );
  assert!(
    item
      .pointer("/body/content")
      .and_then(|v| v.as_str())
      .is_some_and(|content| content.contains("marker = \"needle\"")),
    "expected body content in ndjson item:\n{item:#}"
  );
}

#[test]
fn grep_json_inside_kind_filters_to_ast_loop_ancestors() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--inside-kind",
      "for_statement,while_statement,enhanced_for_statement",
      "SELECT",
      "Dao.java",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"));
  let line_contents = hits
    .iter()
    .map(|hit| {
      hit
        .get("line_content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
    })
    .collect::<Vec<_>>();
  assert_eq!(
    line_contents,
    vec![
      "String loopSql = \"SELECT * FROM live_rows\";",
      "String eachRow = \"SELECT * FROM loop_rows\";",
      "String enhancedRow = \"SELECT * FROM enhanced_rows\";",
    ],
    "inside-kind filter should only keep loop hits:\n{envelope:#}"
  );
  for hit in hits {
    let parents = hit
      .get("parents")
      .and_then(|v| v.as_array())
      .unwrap_or_else(|| panic!("inside-kind should attach parents:\n{hit:#}"));
    let has_loop_parent = parents.iter().any(|parent| {
      matches!(
        parent.get("kind").and_then(|v| v.as_str()),
        Some("for_statement" | "while_statement" | "enhanced_for_statement")
      )
    });
    let has_loop_body = hit
      .get("body")
      .and_then(|b| b.get("kind").and_then(|v| v.as_str()))
      .is_some_and(|kind| {
        matches!(
          kind,
          "for_statement" | "while_statement" | "enhanced_for_statement"
        )
      });
    assert!(
      has_loop_parent || has_loop_body,
      "each kept hit should include one of the requested loop kinds:\n{hit:#}"
    );
  }
}

#[test]
fn grep_json_inside_kind_filters_to_enhanced_for_statement() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--inside-kind",
      "enhanced_for_statement",
      "SELECT",
      "Dao.java",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"));
  let line_contents = hits
    .iter()
    .map(|hit| {
      hit
        .get("line_content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
    })
    .collect::<Vec<_>>();
  assert_eq!(
    line_contents,
    vec!["String enhancedRow = \"SELECT * FROM enhanced_rows\";"],
    "enhanced-for filter should keep only enhanced loop hits:\n{envelope:#}"
  );
  assert_eq!(
    hits
      .first()
      .and_then(|hit| hit.get("body").and_then(|body| body.get("kind")))
      .and_then(|v| v.as_str()),
    Some("enhanced_for_statement"),
    "enhanced_for_statement hits should include enhanced body metadata:\n{envelope:#}"
  );
}

#[test]
fn grep_json_inside_kind_filters_to_block_ancestor() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--inside-kind",
      "block",
      "SELECT",
      "Dao.java",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"));
  let line_contents = hits
    .iter()
    .map(|hit| {
      hit
        .get("line_content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
    })
    .collect::<Vec<_>>();
  assert_eq!(
    line_contents,
    vec![
      "String outside = \"SELECT * FROM config\";",
      "String loopSql = \"SELECT * FROM live_rows\";",
      "String eachRow = \"SELECT * FROM loop_rows\";",
      "String enhancedRow = \"SELECT * FROM enhanced_rows\";",
    ],
    "inside-kind block should include all hits in block-ancestor regions:\n{envelope:#}"
  );
}

#[test]
fn grep_inside_kind_match_then_empty_has_no_match_status() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--inside-kind",
      "while_statement,for_statement,enhanced_for_statement",
      "config",
      "Dao.java",
    ],
  );

  assert_eq!(
    exit, 1,
    "inside-kind filter should return no_match exit code:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match"),
    "exit_status should become no_match after filtering:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("hits")
      .and_then(|hits| hits.as_array())
      .is_some_and(|hits| hits.is_empty()),
    "expected zero filtered hits:\n{envelope:#}"
  );
}

#[test]
fn grep_json_warns_when_a_scanned_file_cannot_be_read_completely() {
  let fix = fixture();
  std::fs::write(
    fix.root.join("invalid_utf8.txt"),
    b"first line is valid\n\xFFoutside-needle after invalid byte\n",
  )
  .expect("write invalid utf8 fixture");

  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "grep", "outside-needle", "."],
  );

  assert_eq!(
    exit, 3,
    "grep should return the partial-coverage exit code while preserving readable hits:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched"),
    "partial coverage should preserve the stable exit_status enum:\n{envelope:#}"
  );
  assert_eq!(
    envelope.get("partial").and_then(|v| v.as_bool()),
    Some(true),
    "partial coverage should be unmissable in a top-level envelope field:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .is_some_and(|hits| hits.iter().any(|hit| {
        hit
          .get("file_path")
          .and_then(|v| v.as_str())
          .is_some_and(|path| path.ends_with("top_level.py"))
      })),
    "grep should keep readable matches:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("FILE_READ_FAILED")
          && warning
            .get("message")
            .and_then(|v| v.as_str())
            .is_some_and(|message| message.contains("invalid_utf8.txt"))
      })),
    "grep should disclose incomplete file reads:\n{envelope:#}"
  );
  assert!(
    stderr.contains("FILE_READ_FAILED"),
    "partial grep diagnostics should mirror to stderr in JSON mode:\n{stderr}"
  );
}

#[test]
fn list_kinds_uses_global_lang_option() {
  let fix = fixture();
  let (envelope, stderr, exit) =
    run_json(&fix.root, &["-l", "java", "--format", "json", "list-kinds"]);
  assert_eq!(
    exit, 0,
    "list-kinds failed:\n{envelope:#}\nstderr:\n{stderr}"
  );
  let rows = envelope
    .get("rows")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected list-kinds rows:\n{envelope:#}"));
  let has_method = rows.iter().any(|row| {
    row.get("portable").and_then(|v| v.as_str()) == Some("method")
      && row
        .get("kinds")
        .and_then(|v| v.as_array())
        .map_or(false, |kinds| {
          kinds.iter().any(|kind| kind == "method_declaration")
        })
  });
  let has_route = rows
    .iter()
    .any(|row| row.get("portable").and_then(|v| v.as_str()) == Some("route"));
  assert!(
    has_method,
    "list-kinds using -l should include method row with kinds for java:\n{envelope:#}"
  );
  assert!(
    has_route,
    "list-kinds output should include route row:\n{envelope:#}"
  );
}
