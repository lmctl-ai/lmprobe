//! CC3 perf gate — Phase 0 Reviewer must-fix #3.
//!
//! Measures wall-clock for every stateless sensor against the
//! ast-grep workspace and fails if any exceeds its threshold.
//!
//! Thresholds are set to **measured-on-release + ~20% headroom**
//! (per Lead arbitration). The Phase-0 baselines are pinned here
//! verbatim; any phase that needs to loosen them requires explicit
//! Lead approval (see `crates/lmprobe/ARCHITECTURE.md` § Known
//! Perf Debt).
//!
//! Run: `cargo test -p lmprobe --release --test perf -- --ignored`.
//! Without `--release` the dev binary is much slower; the test
//! intentionally fails because hardcoded thresholds assume release.
//!
//! Override (for noisy CI hosts): `LMPROBE_PERF_RELAX=2.0` doubles
//! every threshold. The default is 1.0 — strict gate.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// Number of warm-cache runs per sensor; we keep the **minimum** as
/// the canonical reading (lowest noise).
const RUNS: usize = 5;

/// Fixture: the ast-grep workspace itself. Its `crates/` tree drives
/// the per-file walker through ~150 `.rs` files which is the
/// representative scale the plan calls out (CC3).
fn workspace_dir() -> PathBuf {
  // CARGO_MANIFEST_DIR points at crates/lmprobe; walk up two for
  // the workspace root.
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop(); // crates/
  p.pop(); // workspace root
  p
}

struct Sensor {
  name: &'static str,
  args: &'static [&'static str],
  threshold_ms: u128,
}

/// Phase-0 baselines (release-build, hot-cache, on the ast-grep
/// workspace). search / def are over the plan's <100ms target — see
/// `## Known Perf Debt` in ARCHITECTURE.md for the Phase-1 plan to
/// tighten via the §1.4 KindMatcher prefilter.
const SENSORS: &[Sensor] = &[
  // Sub-30ms sensors (process-spawn dominated).
  Sensor {
    name: "discover",
    args: &["discover"],
    threshold_ms: 30,
  },
  Sensor {
    name: "find",
    args: &["find", "Cargo"],
    threshold_ms: 30,
  },
  Sensor {
    name: "grep",
    args: &["grep", "^fn"],
    threshold_ms: 30,
  },
  // §1.3 — `ref` switched from textual regex to AST identifier
  // walking; threshold raised from 30→260ms accordingly. Phase 1
  // §1.4 prefilter is the path back to ~100ms.
  //
  // Phase 5A.7 hardware-variance bump: 260→360ms. Pre-existing
  // failure on the 5B development machine (296-321ms across 5
  // runs at 5A.6 / 5B.1, before any routing changes); not
  // introduced by the engine→registry routing fix in 5A.7.
  // Per arbitration: max(observed 321) × 1.10 = 353, rounded
  // to 360. Same hardware-variance pattern as 4E-D's
  // context_fn 250→280 bump.
  Sensor {
    name: "ref",
    args: &["ref", "Pattern"],
    threshold_ms: 360,
  },
  Sensor {
    name: "callers",
    args: &["callers", "main"],
    threshold_ms: 50,
  },
  Sensor {
    name: "imports",
    args: &["imports", "clap"],
    threshold_ms: 50,
  },
  // Sensors with lookahead/decoration walking.
  Sensor {
    name: "decorations",
    args: &["decorations", "derive"],
    threshold_ms: 100,
  },
  // Phase 1A DFS-once kind prefilter brought search/def from 142ms
  // to ~80ms — back under the original Phase 0 budget. Threshold
  // pinned at 100ms (Phase 0 plan target).
  Sensor {
    name: "search",
    args: &["search", "Pattern"],
    threshold_ms: 100,
  },
  Sensor {
    name: "def",
    args: &["def", "Pattern"],
    threshold_ms: 100,
  },
  // Phase 3B §3.4 context bundle. CC3 target is 250 ms warm
  // on Coder's machine; Phase 4A populated `blob_signatures`
  // cache and the bundle's `recent_history` component now
  // hits 187/216 ms warm there.
  //
  // Phase 4E-D RC fix: Reviewer's hardware measured `context_fn`
  // at 243-253 ms across three runs (1/3 over the 250 ms gate).
  // Per Lead arbitration: bump to **280 ms** = max(observed) ×
  // 1.10 = 253 × 1.10 ≈ 278, rounded up. This absorbs hardware
  // variance, not a perf regression — the blob_signatures cache
  // closure from 4A is intact (14-15× speedup on blame stays).
  // `context_struct` shares the same hardware-variance axis;
  // bumped to match for symmetry.
  //
  // Phase 5A.7 follow-up: `context_fn` regressed to 338-395ms
  // on the 5B development machine. Pre-existing — measured
  // identically at 5A.6 / 5B.1 commits before the engine
  // routing fix. Same hardware drift the 4E-D bump tracked.
  // Bump to 440ms = max(observed 395) × 1.10 = 434, rounded.
  // `context_struct` stays at 280 (within threshold across all
  // 5A.7 runs, max observed 203ms).
  Sensor {
    name: "context_struct",
    args: &["context", "Pattern"],
    threshold_ms: 280,
  },
  Sensor {
    name: "context_fn",
    args: &["context", "run"],
    threshold_ms: 440,
  },
];

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn relax_factor() -> f64 {
  std::env::var("LMPROBE_PERF_RELAX")
    .ok()
    .and_then(|s| s.parse().ok())
    .unwrap_or(1.0)
}

fn run_once(args: &[&str], dir: &PathBuf) -> u128 {
  let start = Instant::now();
  let out = Command::new(binary())
    .args(args)
    .current_dir(dir)
    .output()
    .expect("spawn lmprobe");
  let elapsed = start.elapsed().as_millis();
  assert_ne!(
    out.status.code(),
    Some(101),
    "perf: {} panicked (exit 101); cannot measure",
    args.join(" ")
  );
  elapsed
}

#[test]
#[ignore]
fn perf_gate() {
  let dir = workspace_dir();
  let factor = relax_factor();

  // Warm cache once — file system page cache + tree-sitter init.
  let _ = Command::new(binary())
    .args(["find", "Cargo"])
    .current_dir(&dir)
    .output();

  let mut measurements: Vec<(&str, u128, u128)> = Vec::new();
  let mut violations: Vec<String> = Vec::new();

  for s in SENSORS {
    let mut best = u128::MAX;
    for _ in 0..RUNS {
      let ms = run_once(s.args, &dir);
      if ms < best {
        best = ms;
      }
    }
    let effective = (s.threshold_ms as f64 * factor) as u128;
    measurements.push((s.name, best, effective));
    if best > effective {
      violations.push(format!(
        "{}: {}ms > {}ms (relax_factor={})",
        s.name, best, effective, factor
      ));
    }
  }

  // Print the table whether or not we passed — useful in CI logs.
  eprintln!("\nperf_gate (RUNS={}, relax={}):", RUNS, factor);
  eprintln!("{:<14} {:>10} {:>12}", "sensor", "best(ms)", "threshold");
  for (n, b, t) in &measurements {
    let marker = if *b > *t { " ❌" } else { "" };
    eprintln!("{:<14} {:>10} {:>12}{}", n, b, t, marker);
  }

  if !violations.is_empty() {
    panic!(
      "perf gate FAILED — {} sensor(s) over threshold:\n  - {}",
      violations.len(),
      violations.join("\n  - ")
    );
  }
}
