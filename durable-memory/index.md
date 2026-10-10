# lmprobe Development Memory Index

Last updated: 2026-10-06

`durable-memory/` is the project development, design, and implementation
memory for fresh portal/LLM sessions and contributors working in this
repository. Load this file first, then load only the chapter relevant to the
task. Each chapter is intended to stand alone once paired with this index.

For command implementation or enhancement, start at
[commands/index.md](commands/index.md). There is a developer file for every CLI
command and nested subcommand, with use cases, all local options, source/schema
entry points, implementation limitations, and relevant tests.
[commands/common.md](commands/common.md) covers inherited flags and verification
pitfalls. Keep the matching command file aligned when behavior or options change.
[filesystem-predicates.md](filesystem-predicates.md) documents composable
filesystem selection/traversal expressions and Git remote identity.
[program-json.md](program-json.md) defines the program result contract,
advanced expression-file input, and atomic outfile behavior.
[commands/lmapi.md](commands/lmapi.md) documents lmapi mode: the published
`cli.schema.v1` document, operation names, mode detection and its limitations.
[github-search.md](github-search.md) records native `gh search --json` recipes,
field selection, query argument behavior, and the boundary with lmprobe envelopes.
[commands/aws.md](commands/aws.md) documents the exact read-only AWS catalog;
[commands/aws-query.md](commands/aws-query.md) and the
[AWS request schema](schemas/lmprobe.aws.request.v1.schema.json) define complex
JSON inputs, pagination and credential/output guards.
Root [AGENTS.md](../AGENTS.md) contains the short project operating contract;
[lore-commits.md](lore-commits.md) records the commit-message convention.

The root `README.md` stays concise. The user manual is the static site under
`site/` (published at https://lmctl.com/lmprobe); issues are tracked at
[`lmctl-ai/lmprobe`](https://github.com/lmctl-ai/lmprobe/issues). This directory
is the engineering context for contributors and coding agents.

The canonical code-level architecture spec is
`crates/lmprobe/ARCHITECTURE.md` (D1-D7 module split, sensor inventory, cache
schema, perf budgets, known debt). When this memory and the spec disagree, the
spec wins for code-level detail and the implementation wins over either.

## Project Snapshot

This repository is a Rust/Cargo workspace for `lmprobe`, a one-shot, AST-aware
structural code-search CLI built on a fork of ast-grep and tree-sitter
grammars. The lightweight CLI is the core product; the GraphQL query surface
is built into the default binary through `lmprobe query`, while the
long-running HTTP daemon is opt-in through the `serve` feature.

Current trace watch mode supports structured runtime evidence for Python,
Bash, and line-based R. R support is intentionally watch-only and not
AST-backed: `.R`/`.r` files are instrumented only when the trimmed query equals
a standalone trimmed source line.

## Workspace Shape

- `crates/lmprobe`: binary crate with clap CLI dispatch, GraphQL schema/query
  handling, output rendering, and workspace fan-out.
- `crates/lmprobe-engine`: language-agnostic sensors, history/cache,
  graph/context/impact/fix/trace/secrets-facing engine modules.
- `crates/lmprobe-types`: versioned result envelopes, diagnostics, GraphQL
  scalars, and shared data types.
- `crates/lmprobe-lang-core`: D5 capability trait definitions and generic
  language helpers.
- `crates/lmprobe-registry`: per-language dispatch fan-out into extracted
  language crates.
- `crates/lmprobe-lang-*`: per-language capability implementations for
  supported languages.
- `crates/lmprobe-secrets`: gitleaks-compatible rules, regex compatibility,
  redaction-safe secret types, provider/source DBs.
- `crates/core`, `crates/config`, `crates/language`: ast-grep fork foundation
  crates.
- `xtask`: workspace tooling, including language scaffolding and checks.

The dependency direction is intended to stay acyclic:
binary -> engine -> registry -> language crates/core traits -> types. Sensors
should call registry dispatch helpers for language-specific behavior rather
than matching on languages directly.

## Build And Test Workflow

Toolchain is pinned by `rust-toolchain.toml` to `stable` with `rustfmt` and
`clippy` components. `Cargo.toml` declares Rust 1.79 as the workspace package
minimum. Local setup may require system OpenSSL development headers and
`pkg-config` because `git2`/`libgit2-sys` pull OpenSSL through native
dependencies.

Normal commands from the workspace root:

```bash
cargo build -p lmprobe
cargo test -p lmprobe -p lmprobe-engine --locked
```

Release/default CLI build:

```bash
cargo build -p lmprobe --release
```

Optional daemon build:

```bash
cargo build -p lmprobe --release --features serve
```

Deeper gates are separate from the default test command:

```bash
cargo test -p lmprobe --test deep -- --ignored
cargo test -p lmprobe --test perf -- --ignored
cargo test -p lmprobe --test history_perf -- --ignored
cargo test -p lmprobe --features serve --test auth
```

## Chapters

- [01_overview.md](01_overview.md) - what lmprobe is; the lightweight
  one-shot CLI doctrine; release-history context at a high level; and the
  generated capability matrix that should be treated as the command-surface source
  of truth.
- [02_invocation.md](02_invocation.md) - installation, build matrix, first
  invocation, global flags (`--format` / `--no-cache` / `--workspace` / `-l` /
  `-G` / `--limit` / etc.), exit-code semantics.
- [03_sensors_search.md](03_sensors_search.md) - name/structural search
  verbs: `search`, `def`, `ref`, `find`, `grep`, `discover`, `list-kinds`.
  Per verb: purpose, syntax, example, schema namespace, NOT_IMPLEMENTED
  behavior.
- [04_sensors_graph.md](04_sensors_graph.md) - relational verbs: `callers`,
  `callees`, `imports`, `exports`, `decorations`, `supertypes`, `subtypes`,
  `overrides`, `impact`, `context`.
- [05_history.md](05_history.md) - `lmprobe history <subcommand>` family
  (8 verbs including evolution alias) plus `ast-diff`; `--follow`
  cross-rename; `--semantic` change-type taxonomy.
- [06_mutations.md](06_mutations.md) - `trace` (with `--worktree`,
  `--keep-patch`); `fix` (`--dry-run`, `--apply`); D7 mutation-safety
  contract; audit logs.
- [07_output_formats.md](07_output_formats.md) - markdown / json / ndjson /
  sarif / github / mermaid / dot. When to pick each. FORMAT_NOT_APPLICABLE
  behavior on non-graph verbs.
- [08_envelope_schema.md](08_envelope_schema.md) - D4 schema-first principle;
  envelope shape; `Diagnostic`; `ExitStatus`; `lmprobe.<verb>.v1`
  namespaces; v1-additive policy.
- [09_cache.md](09_cache.md) - XDG cache; tables (`symbol_milestones`,
  `repo_commits`, `repo_head`, `blob_signatures`); cache-key tuples;
  only-grows / content-addressable invariants; `--no-cache`; legacy-path
  back-compat.
- [10_languages.md](10_languages.md) - 29-language matrix, code vs data tier,
  verdict criteria, per-language quirks, KindTaxonomy, in-tree vs qa1
  fixtures.
- [11_extending.md](11_extending.md) - Workspace layout (Phase 5A); D5
  capability traits (NameExtractor, KindTaxonomy, CallGrammar, ImportGrammar,
  DecorationGrammar, TraceTemplate, TopLevelKinds, InheritanceGrammar); how to
  add a language via `cargo xtask new-lang <name>`; how to add a sensor;
  v1-additive schema rules.
- [12_optional_layers.md](12_optional_layers.md) - `--features serve`
  (GraphQL HTTP daemon), bearer-token auth, and opt-in layering doctrine.
- [13_workspace.md](13_workspace.md) - multi-repo `--workspace ws.yaml`,
  per-root cache namespacing, `origin_root` tagging, mutation refusal under
  workspace.
- [14_diagnostics.md](14_diagnostics.md) - diagnostic-code reference table;
  exit-code semantics; `--exit-zero-on-no-match`.
- [15_perf.md](15_perf.md) - three-tier budget (hot-cache <100ms /
  warm-cache <250ms / cold-cache <2s); the perf gates (`tests/perf.rs`,
  `tests/history_perf.rs`); known debt; CC3 perf-CI policy.
- [16_testing.md](16_testing.md) - test tiers
  (smoke/deep/perf/history_perf/blob_cache/auth); LANG_VERDICTS table;
  verdict-criteria definitions; how to add a smoke case.
- [17_graphql.md](17_graphql.md) - GraphQL surface (`lmprobe query '<gql>'`):
  composition / nesting / aliases / variables / introspection; typed scalars;
  `SymbolHit` child resolvers; `fix` / `trace` / `untrace` mutations + D7
  contract; surface equality vs CLI; exit-code semantics.
- [18_phase5b_extraction_playbook.md](18_phase5b_extraction_playbook.md) -
  Phase 5B per-language extraction playbook: deep-dive workflow for
  per-language Coders, simple-vs-bespoke trait patterns, Step 6 dead-impl
  deletion, gate sweep, troubleshooting, and STOP-signal heuristic.
- [19_scanner_recipes.md](19_scanner_recipes.md) - first-class evidence scanner
  contract: command family, safety classes, read/write matrix, and GraphQL alias
  templates for resource pairing, config/code references, permission guards, and
  API surface evidence without adding a premature `scan` resolver.
- [20_pr_triage.md](20_pr_triage.md) - `pr-triage` first slice: GitHub PR
  status/check/review evidence via `gh pr view`.
- [21_secrets.md](21_secrets.md) - `secrets` scanner: gitleaks-compatible
  rule layer, AST/symbol semantic evidence, output schema, modes,
  cache-safety, and v1 limits.
- [23_release_binaries.md](23_release_binaries.md) - GitHub Actions strategy
  for native release-binary builds across Linux, macOS, and Windows.
- [24_find_grep_recipes.md](24_find_grep_recipes.md) - common AI-agent
  `find`/`grep` recipes, GraphQL internal-pipe composition, and CLI interface
  improvement notes.
- [25_static_manual_site.md](25_static_manual_site.md) - static manual site
  structure, content contract, and verification steps for `site/`.
- [29_scanner_namespace_decision.md](29_scanner_namespace_decision.md) -
  scanner namespace decision for the current 0.x surface: keep direct evidence
  commands canonical, document no-rename migration state, and preserve
  evidence-vs-verdict boundaries.

Plus:

- [llmuser_guide.md](llmuser_guide.md) - condensed agent-first operating
  guide: which verb to reach for, JSON envelope handling, diagnostics to
  inspect, GraphQL composition, and common pitfalls.
- [test_cases.md](test_cases.md) - categorized test cases for AI-agent manual
  end-to-end testing. Each case is reproducible against a clean checkout with
  shell access.
- [npm_distribution.md](npm_distribution.md) - npm package layout,
  OIDC/trusted-publisher release path, manual bootstrap checklist, and the
  Linux glibc floor.

## Suggested Reading Orders

| Goal | Read |
|------|------|
| Change or enhance a command | `commands/index.md` -> `commands/<command>.md` -> linked implementation/tests |
| Use as an end user (CLI) | `01_overview.md` -> `02_invocation.md` -> the relevant sensor chapter (`03`/`04`/`05`/`06`) -> `07_output_formats.md` -> `17_graphql.md` |
| Use as an autonomous coding agent | `01_overview.md` -> `llmuser_guide.md` -> the relevant sensor chapter (`03`/`04`/`05`/`06`) -> `17_graphql.md` |
| Reason about evidence safety classes | `01_overview.md` -> `08_envelope_schema.md` -> `19_scanner_recipes.md` |
| Use the GraphQL surface | `01_overview.md` -> `17_graphql.md` -> `03_sensors_search.md` / `04_sensors_graph.md` -> `06_mutations.md` |
| Run AI scanner recipes | `01_overview.md` -> `llmuser_guide.md` -> `19_scanner_recipes.md` -> `17_graphql.md` |
| Translate find/grep/rg/fd recipes | `01_overview.md` -> `llmuser_guide.md` -> `24_find_grep_recipes.md` -> `17_graphql.md` |
| Triage GitHub PR status | `01_overview.md` -> `20_pr_triage.md` -> `07_output_formats.md` |
| Scan for secrets | `01_overview.md` -> `21_secrets.md` -> `07_output_formats.md` -> `14_diagnostics.md` |
| Build release binaries | `23_release_binaries.md` -> `.github/workflows/release-binaries.yml` |
| Maintain release provenance | `27_release_provenance.md` -> `npm_distribution.md` -> `.github/workflows/release-npm.yml` |
| Maintain static manual site | `25_static_manual_site.md` -> `README.md` -> `02_invocation.md` -> `24_find_grep_recipes.md` |
| Maintain evidence scanner command namespace | `19_scanner_recipes.md` -> `29_scanner_namespace_decision.md` -> `08_envelope_schema.md` |
| Understand the architecture | `01_overview.md` -> `08_envelope_schema.md` -> `09_cache.md` -> `crates/lmprobe/ARCHITECTURE.md` |
| Add a language | `01_overview.md` -> `10_languages.md` -> `11_extending.md` |
| Extract a Phase-5B language | `11_extending.md` -> `18_phase5b_extraction_playbook.md` |
| Add a sensor | `01_overview.md` -> `08_envelope_schema.md` -> `11_extending.md` |
| Debug a perf regression | `15_perf.md` -> `09_cache.md` -> `16_testing.md` |
| Run as a server | `12_optional_layers.md` -> `17_graphql.md` -> `02_invocation.md` |
| Multi-repo setup | `13_workspace.md` -> `02_invocation.md` |
| Triage a failing test | `14_diagnostics.md` -> `16_testing.md` -> `test_cases.md` |

## Distribution

- [npm_distribution.md](npm_distribution.md) - how lmprobe ships to npm as
  `@lmctl-ai/lmprobe` (meta package + 5 per-platform binary packages, the
  `npm/` scaffolding, manual-only `release-npm.yml`, and local prerelease
  builds).
- [27_release_provenance.md](27_release_provenance.md) - one-version-source
  release identity map for Cargo, `lmprobe --version`, native archives, npm
  platform packages, and the npm meta package.
