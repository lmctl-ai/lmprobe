# 23 - Release Binary Builds

This chapter records the first release-binary build strategy for GitHub
Actions. It is intentionally conservative: build host-native binaries on
native hosted runners first, publish workflow artifacts, and only add
automatic GitHub Release publishing after the matrix has passed at least one
manual run.

## Workflow

The release artifact workflow lives at
`.github/workflows/release-binaries.yml`. It runs on manual
`workflow_dispatch` and `repository_dispatch` with type `release-binaries`.
It intentionally does not run on `v*` tags; npm releases already build the full
native matrix, and tag-triggered archive builds would double the hosted-runner
cost for the weekly release window.

Temporary cost mode (2026-07-09): hosted release artifact builds are locked to
Linux x64 only. The manual UI only exposes `linux-x64`, and stale
`repository_dispatch` payloads that still send `target=all` are treated as
Linux x64. Any other target exits before starting a platform runner. Restore
the full list below only when the operator explicitly wants a full production
artifact pass.

Normal full-matrix target set, currently disabled:

- `all`
- `linux-x64`
- `linux-arm64`
- `macos-x64`
- `macos-arm64`
- `windows-x64`

`repository_dispatch` accepts the same value at `client_payload.target` when
full-matrix mode is restored.

The workflow uses a per-ref `concurrency` group so duplicate manual or
repository-dispatch runs do not overlap unexpectedly.

The workflow uploads one archive per native host target plus a `SHA256SUMS`
artifact:

| Artifact | Runner | Rust target | Archive |
|---|---|---|---|
| `lmprobe-x86_64-unknown-linux-gnu` | `ubuntu-24.04` host, `ubuntu:20.04` build container | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `lmprobe-aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` host, `ubuntu:20.04` build container | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| `lmprobe-x86_64-apple-darwin` | `macos-15-intel` | `x86_64-apple-darwin` | `.tar.gz` |
| `lmprobe-aarch64-apple-darwin` | `macos-15` | `aarch64-apple-darwin` | `.tar.gz` |
| `lmprobe-x86_64-pc-windows-msvc` | `windows-2022` | `x86_64-pc-windows-msvc` | `.zip` |

Each archive contains the binary plus `README.md`, `LICENSE`, and
`ATTRIBUTION.md`. Both `.tar.gz` and `.zip` archives preserve the wrapping
`${artifact}/` directory when extracted.

The workflow explicitly installs the pinned stable Rust toolchain with
`dtolnay/rust-toolchain@stable` before building. Do not rely on whatever Rust
version happens to be preloaded on a hosted runner image.

Checkout uses `fetch-depth: 0`. Some smoke goldens and context/history tests
assert exact `recent_history` payloads; shallow clones can change those
payloads and fail the test suite even when the binary behavior is correct.

`CARGO_TARGET_DIR` is pinned to `${{ github.workspace }}/target` so Cargo
passes absolute test-binary paths to integration tests that change
`current_dir` into temporary fixtures. Without this, platform-dependent
relative `CARGO_BIN_EXE_lmprobe` values can break child-process tests.

The workflow sets `RUST_TEST_THREADS=1` to serialize tests inside each
integration-test binary. Several legacy integration tests still use
process-id/timestamp temp paths; serialization keeps release CI focused on
packaging portability instead of platform-specific timestamp granularity.

## Why Native Runners First

`lmprobe` depends on native code through `git2` / `libgit2-sys`,
`openssl-sys`, `libssh2-sys`, bundled `rusqlite` / `libsqlite3-sys`, and many
`tree-sitter-*` grammar crates. Cross-compiling those dependencies is possible
but makes OpenSSL/pkg-config/vcpkg plumbing the highest-risk part of the first
release workflow. Native runners keep the first binary pipeline easier to
debug. The workflow therefore does not pass `cargo build --target`; the Rust
target triple in the matrix is an artifact name and runner expectation, not a
cross-compilation request.

Linux uses `pkg-config` and `libssl-dev`. macOS installs `pkg-config` and
`openssl@3` through Homebrew and exports `OPENSSL_DIR` / `PKG_CONFIG_PATH`.
Windows uses MSVC plus vcpkg packages for OpenSSL, zlib, and libssh2 with
`VCPKGRS_DYNAMIC=0` and `OPENSSL_STATIC=1`.

The `lmprobe` crate has a Windows/MSVC `build.rs` that links `advapi32` and
sets `/STACK:8388608`. `advapi32` resolves `libgit2-sys` Win32 registry,
security, and CryptoAPI symbols. The explicit stack size avoids Windows'
smaller default main-thread stack overflowing in child-process CLI tests and
in the packaged binary.

## Test And Smoke Gate

Each target runs the default package test suite before the release build:

```bash
cargo test -p lmprobe --locked
```

The ignored deep/perf tests remain out of scope for this packaging workflow.
After the release build, each target smoke-tests the produced binary:

```bash
lmprobe --version
lmprobe --help
```

This confirms the binary starts on the target runner before it is archived.

Some integration tests create temporary repositories and spawn the compiled
`lmprobe` binary from those directories. When adding or editing those tests,
prefer `tempfile::TempDir` over process-id/timestamp temp paths. The release
workflow also sets `RUST_TEST_THREADS=1` to serialize Rust test functions in
each integration-test binary; tests that spawn their own internal worker
threads must still manage fixture isolation themselves.

`SHA256SUMS` is generated after `download-artifact` with `merge-multiple: true`
so checksum entries are flat archive filenames. A user can place
`SHA256SUMS` next to a downloaded archive and run `sha256sum -c SHA256SUMS`.

## Known Risks

- Windows dependency packaging is the least proven piece. If vcpkg static
  linkage fails, fall back to dynamic vcpkg packages and include required DLLs
  in the archive, or move Windows to a dedicated packaging iteration.
- macOS runner labels can shift over time. If `macos-15-intel` or `macos-15`
  becomes unavailable, update the matrix to the current Intel/ARM labels.
- Linux GNU binaries are not maximally portable across old distributions.
  Static musl builds are deferred until `git2`/OpenSSL/libssh2 cross-linking is
  proven.
- macOS binaries are smoke-tested on the Homebrew-equipped hosted runners.
  If downloaded artifacts fail on clean macOS hosts because of dynamic OpenSSL
  linkage, try `OPENSSL_STATIC=1` on the macOS build jobs or package the
  required dylibs explicitly.
- The Rust toolchain tracks the `stable` channel. Stable-channel drift can
  break native dependencies on a future tag even when the repo did not change;
  pin a concrete Rust version if release reproducibility becomes more
  important than automatically picking up stable fixes.
- The workflow uploads artifacts only. Adding tag-time GitHub Release asset
  publishing should be a small second step after one successful manual run.
