use std::fs;
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
    let _ = fs::remove_dir_all(&self.root);
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
    "lmprobe-context-svelte-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture dir");
  fs::write(
    root.join("PreviewRender.svelte"),
    r#"<script context="module">
  const moduleValue = 1;
</script>

<script lang="ts">
  export let story: unknown;
  let element;
  $effect(() => {});
</script>

{#if story}
  {#each [story] as item}
    <Widget bind:this={element}>{item}</Widget>
  {/each}
{/if}
"#,
  )
  .expect("write svelte fixture");
  Fixture { root }
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn context_svelte_file_stem_returns_synthetic_component_anchor() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "context",
      "PreviewRender",
      "PreviewRender.svelte",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["exit_status"], "matched");
  assert_eq!(envelope["definition"]["name"], "PreviewRender");
  assert_eq!(envelope["definition"]["kind"], "svelte_component");
  assert_eq!(envelope["definition"]["lang"], "Svelte");
  assert!(
    envelope["source_context"]
      .as_array()
      .is_some_and(|lines| !lines.is_empty()),
    "synthetic component anchor should still provide requested source context:\n{envelope:#}",
  );
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "SVELTE_SYNTHETIC_CONTEXT")),
    "synthetic fallback should be explicit:\n{envelope:#}",
  );
  assert!(
    !envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "SYMBOL_NOT_FOUND")),
    "synthetic component anchor should not report symbol not found:\n{envelope:#}",
  );
  assert!(envelope["callers"].as_array().is_some_and(Vec::is_empty));
  assert!(envelope["callees"].as_array().is_some_and(Vec::is_empty));
  let outline = envelope["file_outline"].as_array().expect("file outline");
  assert!(
    outline
      .iter()
      .any(|hit| hit["kind"] == "svelte_module_script"),
    "module script block should be exposed as a synthetic outline anchor:\n{envelope:#}",
  );
  assert!(
    outline.iter().any(|hit| hit["kind"] == "svelte_script"
      && hit["content"]
        .as_str()
        .is_some_and(|content| content.contains("$effect"))),
    "script block outline should include body content when --body is requested:\n{envelope:#}",
  );
  assert!(
    outline
      .iter()
      .any(|hit| hit["kind"] == "svelte_script_declaration"
        && hit["name"] == "element"
        && hit["line_start"] == 7),
    "script declarations should be exposed as synthetic outline anchors:\n{envelope:#}",
  );
  assert!(
    outline
      .iter()
      .any(|hit| hit["kind"] == "svelte_rune_call" && hit["name"] == "$effect"),
    "rune calls should be exposed as synthetic outline anchors:\n{envelope:#}",
  );
  assert!(
    outline.iter().any(|hit| hit["kind"] == "svelte_if_block"),
    "template if block should be exposed as a synthetic outline anchor:\n{envelope:#}",
  );
  assert!(
    outline.iter().any(|hit| hit["kind"] == "svelte_each_block"),
    "template each block should be exposed as a synthetic outline anchor:\n{envelope:#}",
  );
  assert!(
    outline.iter().any(|hit| hit["kind"] == "svelte_bind_this"),
    "bind:this should be exposed as a synthetic outline anchor:\n{envelope:#}",
  );
  assert!(
    outline
      .iter()
      .any(|hit| hit["kind"] == "svelte_component_usage"
        && hit["name"] == "Widget"
        && hit["line_start"] == 13),
    "component usage should be exposed as a synthetic outline anchor:\n{envelope:#}",
  );
}

#[test]
fn context_svelte_fallback_requires_file_stem_match() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "context",
      "OtherComponent",
      "PreviewRender.svelte",
    ],
  );

  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["exit_status"], "no_match");
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "SYMBOL_NOT_FOUND")),
    "non-stem symbols should keep normal context miss behavior:\n{envelope:#}",
  );
  assert!(
    !envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "SVELTE_SYNTHETIC_CONTEXT")),
    "synthetic fallback must stay narrow:\n{envelope:#}",
  );
}
