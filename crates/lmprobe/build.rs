// Phase 6A #2: emit GIT_SHA env var for `--version` rendering.
// Falls back to "unknown" if git isn't available (e.g. crates.io
// build, source tarball without .git/).

use std::process::Command;

fn main() {
  let sha = Command::new("git")
    .args(["rev-parse", "--short=7", "HEAD"])
    .output()
    .ok()
    .and_then(|o| {
      if o.status.success() {
        String::from_utf8(o.stdout)
          .ok()
          .map(|s| s.trim().to_string())
          .filter(|s| !s.is_empty())
      } else {
        None
      }
    })
    .unwrap_or_else(|| "unknown".to_string());
  println!("cargo:rustc-env=LMPROBE_GIT_SHA={}", sha);
  // Re-run when HEAD moves so the SHA stays accurate.
  println!("cargo:rerun-if-changed=../../.git/HEAD");
  println!("cargo:rerun-if-changed=../../.git/refs/heads");

  if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
    && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
  {
    println!("cargo:rustc-link-lib=advapi32");
    println!("cargo:rustc-link-arg=/STACK:8388608");
  }
}
