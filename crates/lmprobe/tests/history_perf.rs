//! Phase 3A history-family perf gate.
//!
//! Measures cold-cache + warm-cache wall-clock for each
//! `lmprobe history <verb>` against a small fixture (qa1 cobra,
//! ~500 commits). Cold-cache uses an isolated tmp `XDG_CACHE_HOME`
//! per test so prior runs don't bleed in.
//!
//! Targets per CC3 / Lead arbitration item 4:
//!   • Cold-cache `<2s` for first-time history operations.
//!   • Warm-cache `<200ms` for **trivial blob-keyed lookups**
//!     (commit / churn / co-change / todos — all O(commits-in-window)).
//!   • Warm-cache `<1500ms` for **per-symbol AST-aware verbs**
//!     (blame / introduced — cost scales with file-history depth and
//!     the symbol's containing-file commit-touches; blob_signatures
//!     caching is a Phase 4 follow-up).
//!
//! Run: `cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture`.
//! Skipped automatically if the fixture isn't present.
//!
//! `LMPROBE_PERF_RELAX=2.0` doubles every threshold.

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

/// The qa1 `go/cobra` clone under `LMPROBE_QA_CORPUS`; skipped when unset.
fn fixture() -> Option<PathBuf> {
  std::env::var_os("LMPROBE_QA_CORPUS").map(|root| PathBuf::from(root).join("go/cobra"))
}

struct HistorySensor {
  name: &'static str,
  args: &'static [&'static str],
  cold_ms: u128,
  warm_ms: u128,
}

const SENSORS: &[HistorySensor] = &[
  // Phase 4A populated `blob_signatures` cache; warm-cache reads
  // dropped 1319 → 81 ms (~16× speedup). Cold also improved
  // (1381 → 947 ms) because all cache writes batch into a single
  // SQLite transaction. Thresholds tightened from 1500 → 300 ms warm.
  // De-concession pass at 30 runs confirmed these targets are stable at 2000ms.
  HistorySensor {
    name: "history_blame",
    // 2000ms cold target reaffirmed by fixed-helper 30-run READY gate.
    args: &["history", "blame", "Execute"],
    cold_ms: 2000,
    warm_ms: 300,
  },
  HistorySensor {
    name: "history_introduced",
    args: &["history", "introduced", "Execute"],
    cold_ms: 2000,
    warm_ms: 300,
  },
  HistorySensor {
    name: "history_commit",
    args: &["history", "commit", "--since", "180d"],
    cold_ms: 2000,
    warm_ms: 200,
  },
  HistorySensor {
    name: "history_churn",
    args: &["history", "churn", "--top", "10", "--since", "180d"],
    cold_ms: 2000,
    warm_ms: 200,
  },
  HistorySensor {
    name: "history_co_change",
    args: &["history", "co-change", "cobra.go", "--since", "180d"],
    cold_ms: 2000,
    warm_ms: 200,
  },
  HistorySensor {
    name: "history_todos",
    args: &["history", "todos", "--since", "180d"],
    cold_ms: 2000,
    warm_ms: 200,
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

fn run_once(args: &[&str], dir: &PathBuf, xdg: &PathBuf) -> u128 {
  let start = Instant::now();
  let out = Command::new(binary())
    .args(args)
    .env("XDG_CACHE_HOME", xdg)
    .current_dir(dir)
    .output()
    .expect("spawn lmprobe");
  let elapsed = start.elapsed().as_millis();
  if let Some(101) = out.status.code() {
    panic!(
      "lmprobe panicked: args={:?}\n--- STDOUT ---\n{}\n--- STDERR ---\n{}",
      args,
      String::from_utf8_lossy(&out.stdout),
      String::from_utf8_lossy(&out.stderr)
    );
  }
  elapsed
}

#[test]
#[ignore]
fn history_perf_gate() {
  let Some(fixture) = fixture().filter(|f| f.exists()) else {
    eprintln!("history_perf_gate: skipping — set LMPROBE_QA_CORPUS to a qa1 corpus with go/cobra");
    return;
  };
  let relax = relax_factor();

  println!("\nhistory_perf_gate (cold + warm, RELAX={:.1}):", relax);
  println!(
    "{:24} {:>10} {:>10} {:>14} {:>14}",
    "sensor", "cold(ms)", "warm(ms)", "cold-thresh", "warm-thresh"
  );

  let mut failures: Vec<(String, u128, u128, u128, u128)> = Vec::new();
  for s in SENSORS {
    // Fresh XDG cache per sensor — guarantees cold-cache.
    let xdg = std::env::temp_dir().join(format!(
      "lmprobe-history-perf-{}-{}",
      s.name,
      std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&xdg);
    std::fs::create_dir_all(&xdg).expect("create xdg tmp");

    let cold = run_once(s.args, &fixture, &xdg);
    // Warm: same XDG, second invocation reads cache.
    let warm = run_once(s.args, &fixture, &xdg);

    let cold_thresh = (s.cold_ms as f64 * relax) as u128;
    let warm_thresh = (s.warm_ms as f64 * relax) as u128;
    println!(
      "{:24} {:>10} {:>10} {:>14} {:>14}",
      s.name, cold, warm, cold_thresh, warm_thresh
    );
    if cold > cold_thresh || warm > warm_thresh {
      failures.push((s.name.to_string(), cold, warm, cold_thresh, warm_thresh));
    }
    let _ = std::fs::remove_dir_all(&xdg);
  }

  if !failures.is_empty() {
    let lines: Vec<String> = failures
      .iter()
      .map(|(n, c, w, ct, wt)| {
        let mut why = Vec::new();
        if c > ct {
          why.push(format!("cold {}>{}ms", c, ct));
        }
        if w > wt {
          why.push(format!("warm {}>{}ms", w, wt));
        }
        format!("  - {}: {}", n, why.join(", "))
      })
      .collect();
    panic!(
      "history_perf_gate: {} sensor(s) over threshold:\n{}",
      failures.len(),
      lines.join("\n")
    );
  }
}
