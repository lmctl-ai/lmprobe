# lmprobe development

High-performance AST-aware structural search, Git inspection, live text editing, and read-only AWS querying CLI.

Start at [durable-memory/commands/index.md](durable-memory/commands/index.md) for command contracts, options, entry points, and focused tests. [durable-memory/index.md](durable-memory/index.md) links cross-command design chapters; implementation takes precedence over stale notes.

## External resources (optional)

| Resource | Where | Why |
|---|---|---|
| qa1 test corpus | `$LMPROBE_QA_CORPUS` (`<lang>/<repo>` clones) | 29-language corpus for `crates/lmprobe/tests/deep.rs`, corpus smoke cases and `history_perf`. Tests skip when unset or absent. |
| gitleaks checkout | `$LMPROBE_GITLEAKS_REPO` (built binary at `<repo>/gitleaks`) | Parity tests in `crates/lmprobe-secrets/tests/gitleaks_parity.rs`; skipped when unset. |
| lmapi | [`lmctl-ai/lmapi`](https://github.com/lmctl-ai/lmapi), via the [`lmrust`](https://github.com/lmctl-ai/lmrust) SDK | The `cli.schema.v1` contract behind lmprobe's lmapi mode. |
| Manual site | `site/` in this repo, published at https://lmctl.com/lmprobe | Deployed by the lmctl website repository; only `site/` plus `LICENSE`/`ATTRIBUTION.md` are uploaded. |

## Invariants and surprises that have bitten people

- **Running tests requires both packages**: `cargo test -p lmprobe` does NOT run `lmprobe-engine` unit tests! Run `cargo test -p lmprobe -p lmprobe-engine --locked`. Deep tests (`crates/lmprobe/tests/deep.rs`) and perf gates are `#[ignore]` by default and need `$LMPROBE_QA_CORPUS`. Ignore the unrelated `cargo test -p xtask` schema generator ENOENT failure; check only `cargo test -p xtask --locked capability_matrix`.
- **Exit code 3 is Partial Coverage, not an error**: Sensors exit 3 when results are truncated by size limits (`FILE_TOO_LARGE`) or read errors. `scan_stats.files_skipped` counts considered-then-skipped files; `scan_stats.files_ignored` (None vs Some(n)) counts `.gitignore` exclusions. `files_skipped: 0` does NOT mean all files were examined if `.gitignore` excluded them.
- **AST mutation and trace isolation**: `fix --apply` and `trace --worktree` create isolated worktrees branched from `HEAD` (ignoring uncommitted working-tree changes). In contrast, in-place `trace`, GraphQL `untrace`, and text `edit` operate directly on the working tree after verifying clean Git status (`--force` overrides). Note `untrace` is currently GraphQL-only (`Mutation::untrace`), not a CLI command. See [06_mutations](durable-memory/06_mutations.md).
- **File deletion is preview-first**: `find --action delete` and piped `delete` preview changes by default and require `--force` to remove regular files. Roots and non-regular files are refused.
- **AWS queries require strict allowlist**: `aws catalog` and `aws query` run via the local AWS CLI but require operations to match an exact reviewed 41-operation read-only allowlist. Passthrough flags or mutation overrides are strictly forbidden. Request files are validated against `durable-memory/schemas/lmprobe.aws.request.v1.schema.json`.
- **Source version vs published npm**: the `Cargo.toml` version can be ahead of npm `latest` (unreleased fixes); do not stamp it back. Never dispatch GitHub Actions release workflows (`release-npm.yml`, `release-binaries.yml`) as a verification step; they are `workflow_dispatch`-only and a release requires explicit maintainer authorization. Keep `ast-grep` path-dependency version requirements aligned with `workspace.package.version`. See [27_release_provenance](durable-memory/27_release_provenance.md).
- **Architecture & Crate cycles**: Language behavior belongs in capability implementations and registry dispatch (`crates/lmprobe-registry`), NOT hardcoded sensor matches in `crates/lmprobe-engine`. `lang-core` must never depend on concrete language crates.
- **Commit discipline**: Never use `git add -A` or `git commit -a`; untracked files, `.lmctl` session teamfiles, and local state exist and must never be committed. Commits follow the [Lore protocol](durable-memory/lore-commits.md) with an intent line, context body, and git-native decision trailers.

Normal checks: `cargo fmt --all -- --check`, `cargo check -p lmprobe --locked`, `cargo clippy -p lmprobe --locked --all-targets`, `cargo test -p lmprobe -p lmprobe-engine --locked`.
When CLI options or help text move: `UPDATE_GOLDENS=1 cargo test -p lmprobe --release --test smoke`.
Use `--features serve` for daemon changes.
