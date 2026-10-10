# Attribution — fork of ast-grep

This repository (`lmctl-ai/lmprobe`) is a hard fork of [ast-grep](https://github.com/ast-grep/ast-grep), forked on 2026-05-02 at upstream commit `b61e95f4`.

## What was kept from upstream

The following crates are **forks** of upstream ast-grep crates of the same name:

| Crate (this repo) | Upstream origin | Purpose |
|---|---|---|
| `crates/core` | [ast-grep/ast-grep — `crates/core`](https://github.com/ast-grep/ast-grep/tree/main/crates/core) | AST pattern engine |
| `crates/config` | [ast-grep/ast-grep — `crates/config`](https://github.com/ast-grep/ast-grep/tree/main/crates/config) | Rule / scan configuration types |
| `crates/language` | [ast-grep/ast-grep — `crates/language`](https://github.com/ast-grep/ast-grep/tree/main/crates/language) | Tree-sitter language registry |

These keep their original `ast-grep-{core,config,language}` package names. They are **not** the published crates of the same name on crates.io; they are local-path workspace dependencies of `crates/lmprobe`. Do not attempt to publish them under those names.

Original ast-grep authors retain copyright on pre-fork code in those three crates. See `LICENSE` for the dual copyright lines (Herrington Darkholme — ast-grep upstream; Lattice Systems LLC — lmprobe).

## What is lmprobe-original

The `crates/lmprobe` crate and the other `crates/lmprobe-*` crates are the **lmprobe-original work**, copyright 2026 Lattice Systems LLC. This public repository starts from a source snapshot; the earlier development history is not included. `crates/lmprobe/CHANGELOG.md` documents the implementation phases.

`durable-memory/` is the project design and implementation memory plus AI-agent test cases.

## What was dropped from upstream

The following ast-grep crates / directories were not carried over:

- `crates/cli` (the `sg`/`ast-grep` CLI binary — lmprobe ships its own CLI in `crates/lmprobe/src/main.rs`)
- `crates/dynamic`, `crates/lsp`, `crates/napi`, `crates/pyo3`, `crates/wasm`, `crates/tools`
- `npm/`, `xtask/`, `benches/`, `website/`, `schemas/`, `fixtures/`
- Top-level CHANGELOG (preserved at `crates/lmprobe/CHANGELOG.md`)
- `.github/` workflows (lmprobe has its own workflows under `.github/workflows/`)

## Hard fork posture

There is **no plan to merge upstream changes** from ast-grep into this repository. The forked crates evolve under lmprobe's release cadence; if upstream ast-grep ships a fix relevant to lmprobe, it will be applied as a patch with explicit attribution rather than via an automated merge.

If you are building a competing tool on top of upstream ast-grep, use the published `ast-grep-core` etc. from crates.io directly — not this repo's forks.

## Third-party data

`crates/lmprobe-secrets/tests/fixtures/gitleaks-default.toml` is a vendored copy of the default rule set of [gitleaks](https://github.com/gitleaks/gitleaks), used as a parity fixture for lmprobe's secret scanner. gitleaks is licensed under the MIT License, Copyright (c) 2019 Zachary Rice; its license text is at https://github.com/gitleaks/gitleaks/blob/master/LICENSE.
