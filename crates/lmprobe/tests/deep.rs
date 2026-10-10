//! Tier-2 deep regression suite — Phase 0 §0.11 / CC2 + Phase 2
//! verdict tracking (Lead arbitration item 3).
//!
//! ### Verdict criteria
//!
//! For each language we run `def`, `ref`, `search`, `callers`,
//! `imports` against the QA fixture's representative symbol and
//! count how many produce a non-empty `hits` array (= a "correct
//! hit" signal). Verdict by count:
//!
//!   - **OK**:      ≥ 3 of {def, ref, search, callers, imports}
//!                  return ≥ 1 hit on the fixture symbol.
//!   - **PARTIAL**: 1–2 of those return ≥ 1 hit.
//!   - **FAIL**:    zero return any hit on the fixture symbol.
//!
//! `LANG_VERDICTS` records the expected verdict per language at
//! the END of each phase. The deep tier asserts the live counts
//! match the recorded verdict — when a phase improves a lang's
//! score, bump the verdict and the test enforces it.
//!
//! Phase 2A column reflects: §2.1 discover (no per-lang impact on
//! these sensors) + §2.2 ImportGrammar's 12 new langs (which moves
//! several FAIL → PARTIAL via imports alone). Phase 2B/2C bump
//! further.
//!
//! For every QA-tested language we hold one cloned reference repo
//! under `$LMPROBE_QA_CORPUS/<lang>/<repo>/` and run every
//! sensor that applies. The contract is **zero panics** on the
//! corpus (Phase 0 exit criterion #1) plus reasonable exit codes
//! (`matched | no_match | error`, never 101).
//!
//! Run: `cargo test -p lmprobe --test deep -- --ignored`. Target
//! wall-clock <10min. Cases skip cleanly when a fixture path is
//! missing (so this passes on machines without the qa1 corpus).
//!
//! Smoke goldens still live in `tests/smoke.rs` — that's the PR-blocking
//! tier. This is the nightly evidence we point at when Reviewer asks
//! for proof of "zero panics on the QA repo set".

use std::path::PathBuf;
use std::process::Command;

/// Root of the external QA corpus (`<lang>/<repo>` clones), taken from
/// `LMPROBE_QA_CORPUS`. When unset, every corpus-backed case is skipped.
fn fixture_base() -> PathBuf {
  std::env::var_os("LMPROBE_QA_CORPUS")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("/nonexistent/lmprobe-qa-corpus"))
}

/// 29 QA-tested languages with one canonical repo each. Mirrors the
/// language table of the qa1 corpus report (§3).
///
/// **Probe-symbol selection (Phase 2C):** symbols are chosen to be
/// LOCALLY DEFINED in their fixture so `def <symbol>` returns ≥1 hit
/// instead of false-failing on imported-but-not-defined names. For
/// data-tier langs, the symbol is a content fragment (key / element
/// name / attribute) that exists in the fixture so `--has-child`
/// can find it. Where the qa1 fixture itself is too thin (toml,
/// lua), we route the deep test to an in-tree fixture under
/// `crates/lmprobe/tests/fixtures/<lang>/` instead.
const LANGUAGES: &[(&str, &str, &str)] = &[
  // (lang, fixture-relative-path, representative-symbol)
  ("bash", "bash/git-extras", "main"),
  ("c", "c/sds", "sdsnew"),
  ("cpp", "cpp/stb", "stbi_load"),
  ("csharp", "csharp/commandline", "Parser"),
  ("css", "css/normalize.css", "html"),
  ("dart", "dart/dart_style", "DartFormatter"),
  ("elixir", "elixir/plug", "Plug"),
  ("go", "go/cobra", "Execute"),
  ("haskell", "haskell/xmonad", "main"),
  ("hcl", "hcl/terraform-aws-vpc", "vpc"),
  ("html", "html/50projects50days", "div"),
  ("java", "java/junit5-samples", "Calculator"),
  ("javascript", "javascript/JSON-js", "quote"),
  ("json", "json/JSON-Schema-Test-Suite", "type"),
  (
    "kotlin",
    "kotlin/architecture-templates",
    "MyModelRepository",
  ),
  ("lua", "@in-tree/lua", "setup"),
  ("nix", "nix/nixos-generators", "systemBuildIsSubmodule"),
  ("php", "php/PHP-Parser", "Parser"),
  ("python", "python/markupsafe", "escape"),
  ("ruby", "ruby/sinatra", "Application"),
  ("rust", "rust/fd", "TimeFilter"),
  ("scala", "scala/scopt", "OParser"),
  ("solidity", "solidity/solmate", "ERC20"),
  ("swift", "swift/Alamofire", "Session"),
  ("toml", "@in-tree/toml", "package"),
  ("tsx", "tsx/swr", "useSWRHandler"),
  ("typescript", "typescript/zod", "ZodError"),
  ("xml", "xml/dom4j", "Document"),
  ("yaml", "yaml/ansible-examples", "name"),
];

/// Resolve a fixture-relative path. `@in-tree/<lang>` reroutes to
/// `crates/lmprobe/tests/fixtures/<lang>/`; everything else is
/// joined under the qa1 corpus root.
fn resolve_fixture(rel: &str) -> PathBuf {
  if let Some(lang) = rel.strip_prefix("@in-tree/") {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures")
      .join(lang)
  } else {
    fixture_base().join(rel)
  }
}

/// Verdict record per language, one row per phase. Updated after
/// each Phase-N chunk lands. Deep tier's `lang_verdicts_match`
/// test asserts the LIVE measurement matches `post_phase_2a`
/// (the most recent column).
///
/// Verdict criteria — see file-level docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
  Ok,
  Partial,
  Fail,
}
impl Verdict {
  fn from_score(score: usize) -> Self {
    match score {
      0 => Verdict::Fail,
      1 | 2 => Verdict::Partial,
      _ => Verdict::Ok,
    }
  }
}

/// (lang, post_phase_0, post_phase_1, post_phase_2a, post_phase_2b,
/// post_phase_2c) — bump the rightmost column when a chunk improves
/// a lang's score. The post_phase_2c column is the LIVE measurement
/// after Chunk 2C; earlier columns are historical reconstructions.
/// Rerun `lang_verdicts_match` and reconcile after each chunk.
///
/// Phase 2C changes the criteria significantly:
///   • Data-tier languages (json/yaml/toml/xml/html/css) now score
///     over `{search --name, search --kind class, search --kind
///     class --has-child <X>, search --kind class --outline}` (4
///     probes); ≥3 hits = Ok.
///   • Code-tier probes unchanged: {def, ref, search --name,
///     callers, imports} (5 probes); ≥3 hits = Ok.
///   • Probe symbols moved to LOCALLY-DEFINED names (was: imported-
///     but-not-defined names like `WalkParallel`/`Test`/`JSON`).
///   • Where qa1 fixtures are too thin (toml, lua), the deep test
///     reroutes to `crates/lmprobe/tests/fixtures/<lang>/`.
const LANG_VERDICTS: &[(&str, Verdict, Verdict, Verdict, Verdict, Verdict)] = &[
  (
    "bash",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "c",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "cpp",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "csharp",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "css",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "dart",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "elixir",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "go",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "haskell",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
  ),
  (
    "hcl",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "html",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "java",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "javascript",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "json",
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "kotlin",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "lua",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "nix",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "php",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "python",
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "ruby",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "rust",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "scala",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "solidity",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "swift",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "toml",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Fail,
    Verdict::Fail,
    Verdict::Ok,
  ),
  (
    "tsx",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "typescript",
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
  (
    "xml",
    Verdict::Fail,
    Verdict::Partial,
    Verdict::Ok,
    Verdict::Ok,
    Verdict::Ok,
  ),
  (
    "yaml",
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Partial,
    Verdict::Ok,
  ),
];

/// Sensors invoked per language. Each closure returns the args
/// after the binary path.
fn invocations(symbol: &str) -> Vec<Vec<String>> {
  vec![
    vec!["discover".into(), ".".into()],
    vec!["find".into(), r"\.[a-z]+$".into(), ".".into()],
    vec!["grep".into(), symbol.into(), ".".into()],
    vec!["search".into(), symbol.into(), ".".into()],
    vec!["def".into(), symbol.into(), ".".into()],
    vec!["ref".into(), symbol.into(), ".".into()],
    vec!["callers".into(), symbol.into(), ".".into()],
    vec!["imports".into(), symbol.into(), ".".into()],
    vec!["decorations".into(), symbol.into(), ".".into()],
  ]
}

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

#[derive(Debug)]
#[allow(dead_code)] // fields read via Debug only
struct Outcome {
  lang: &'static str,
  sensor: String,
  exit: i32,
}

#[test]
#[ignore]
fn deep_29_languages() {
  let mut outcomes: Vec<Outcome> = Vec::new();
  let mut panics: Vec<(String, String, i32)> = Vec::new();
  let mut skipped_langs: Vec<&str> = Vec::new();
  let started = std::time::Instant::now();

  for (lang, rel_fixture, symbol) in LANGUAGES {
    let fixture = resolve_fixture(rel_fixture);
    if !fixture.exists() {
      skipped_langs.push(lang);
      continue;
    }
    for args in invocations(symbol) {
      let mut cmd = Command::new(binary());
      // Always pass --no-cache so evolution doesn't mutate state.
      cmd.arg("--no-cache");
      cmd.args(&args);
      cmd.current_dir(&fixture);
      let out = cmd.output().expect("spawn lmprobe");
      let exit = out.status.code().unwrap_or(-1);
      let sensor = args[0].clone();
      // 101 = unhandled panic, the verdict we MUST never see.
      if exit == 101 {
        panics.push((lang.to_string(), sensor.clone(), exit));
      }
      outcomes.push(Outcome { lang, sensor, exit });
    }
  }

  let elapsed = started.elapsed();
  let total = outcomes.len();
  let exit_codes: std::collections::BTreeMap<i32, usize> =
    outcomes
      .iter()
      .fold(std::collections::BTreeMap::new(), |mut m, o| {
        *m.entry(o.exit).or_insert(0) += 1;
        m
      });

  eprintln!(
    "deep: {} invocations across {} languages in {:.1}s",
    total,
    LANGUAGES.len() - skipped_langs.len(),
    elapsed.as_secs_f32()
  );
  eprintln!("deep: exit-code histogram = {:?}", exit_codes);
  if !skipped_langs.is_empty() {
    eprintln!(
      "deep: skipped {} languages (fixture missing): {:?}",
      skipped_langs.len(),
      skipped_langs
    );
  }
  if !panics.is_empty() {
    let preview: Vec<_> = panics.iter().take(10).collect();
    panic!(
      "deep: {} panic(s) detected (≠101 expected, never 101). first 10:\n{:#?}",
      panics.len(),
      preview
    );
  }
}

/// §Phase-2 verdict-tracking — for each language, run the
/// tier-appropriate probes ("did this find anything?") and compare
/// the live score to `LANG_VERDICTS`'s post_phase_2c column.
///
/// **Code tier** probes: {def, ref, search --name, callers, imports}
/// — five probes; ≥3 hits = Ok.
///
/// **Data tier** probes: {search --name, search --kind class,
/// search --has-child <symbol>, search --outline} — four probes;
/// ≥3 hits = Ok. Data languages don't have call/declaration/import
/// semantics, so the code probes never apply; the data tier scores
/// over the structural probes that *do* apply.
///
/// `Verdict::from_score` is unchanged (0=Fail, 1-2=Partial, ≥3=Ok),
/// the *probe set* is what changes per tier.
#[test]
#[ignore]
fn lang_verdicts_match() {
  use std::process::Stdio;
  let mut deltas: Vec<String> = Vec::new();
  let mut skipped: Vec<&str> = Vec::new();

  for (lang, rel_fixture, symbol) in LANGUAGES {
    let fixture = resolve_fixture(rel_fixture);
    if !fixture.exists() {
      skipped.push(lang);
      continue;
    }
    // Tier dispatch — data langs use a structural probe set.
    let data_tier = matches!(*lang, "json" | "yaml" | "toml" | "xml" | "html" | "css");
    let probes: Vec<Vec<&str>> = if data_tier {
      vec![
        vec!["search", "--name", symbol],
        vec!["search", "--kind", "class"],
        vec!["search", "--kind", "class", "--has-child", symbol],
        vec!["search", "--kind", "class", "--outline"],
      ]
    } else {
      vec![
        vec!["def", symbol],
        vec!["ref", symbol],
        vec!["search", "--name", symbol],
        vec!["callers", symbol],
        vec!["imports", symbol],
      ]
    };
    let mut hits_count = 0usize;
    for probe in &probes {
      let mut cmd = Command::new(binary());
      cmd.arg("--no-cache");
      cmd.arg("--format").arg("json");
      cmd.args(probe);
      // Don't append `.` — for `search`, the positional `.`
      // would bind to the `query` slot (as a substring filter),
      // not the `path`. All sensors default `path` to `.` so
      // omission is fine.
      cmd.current_dir(&fixture);
      cmd.stderr(Stdio::null());
      let out = cmd.output().expect("spawn lmprobe");
      let stdout = String::from_utf8_lossy(&out.stdout);
      if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
        let items = v
          .get("hits")
          .or_else(|| v.get("projects"))
          .or_else(|| v.get("paths"));
        let len = items
          .and_then(|x| x.as_array().map(|a| a.len()))
          .unwrap_or(0);
        if len > 0 {
          hits_count += 1;
        }
      }
    }
    let live = Verdict::from_score(hits_count);
    let expected = LANG_VERDICTS
      .iter()
      .find(|(l, _, _, _, _, _)| l == lang)
      .map(|(_, _, _, _, _, v)| *v)
      .expect("every language present in LANG_VERDICTS");
    if live != expected {
      let probe_count = probes.len();
      deltas.push(format!(
        "{}: expected {:?}, got {:?} ({}/{} probes hit, tier={})",
        lang,
        expected,
        live,
        hits_count,
        probe_count,
        if data_tier { "Data" } else { "Code" }
      ));
    }
  }

  if !skipped.is_empty() {
    eprintln!(
      "lang_verdicts_match: skipped {} (fixtures missing): {:?}",
      skipped.len(),
      skipped
    );
  }
  if !deltas.is_empty() {
    panic!(
      "verdict mismatches ({} of {}):\n  - {}",
      deltas.len(),
      LANG_VERDICTS.len(),
      deltas.join("\n  - ")
    );
  }
}
