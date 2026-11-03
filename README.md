# lmprobe

lmprobe is a one-shot, AST-aware structural code-search CLI for developers and
AI agents that need structured evidence from source trees, built on a fork of
[ast-grep](https://github.com/ast-grep/ast-grep). It finds definitions and
references, maps call and impact graphs, inspects Git history, scans for secrets,
returns machine-readable JSON, and supports preview-first code-change workflows.

Manual and examples: https://lmctl.com/lmprobe

## Install

```bash
npm install -g @lmctl-ai/lmprobe
```

The npm package installs a prebuilt native binary for Linux (x64, arm64), macOS
(x64, arm64) and Windows (x64). For repeated agent loops, prefer the global
install above. `npx @lmctl-ai/lmprobe` is useful for one-off runs, but it
re-resolves the package each time and can add overhead.

## Program invocation

```bash
lmprobe find '\.py$' . --json
lmprobe --infile expression.graphql --outfile result.json
```

`--infile` accepts full GraphQL expressions, including advanced composition.
The [program JSON contract](durable-memory/program-json.md) defines the versioned
result shape; `--format json` retains the legacy sensor format.

Text editing uses a preview-first command:

```bash
lmprobe edit notes.txt -p oldword -r newword --word --json
lmprobe edit notes.txt -p oldword -r newword --word --apply --force --json
lmprobe edit notes.txt --delete-lines 2:4
```

The [edit command reference](durable-memory/commands/edit.md) covers regex
captures, line insertion/replacement, prefix/suffix edits, and sequential JSON
scripts. `--apply` writes; `--force` permits dirty Git trees or non-Git files.
Recovery copies are recorded before writes. Use `fix` for AST-based replacement.

Common Git inspections return parsed records:

```bash
lmprobe git status --json
lmprobe git diff --staged --json
lmprobe git log --max-count 10 --json
lmprobe git --repo ../another-repo show HEAD --file README.md --json
```

See the [Git command reference](durable-memory/commands/git.md) for branches,
remotes, revision resolution, tracked files and blame. These operations are read-only.

Read-only AWS queries use the installed AWS CLI and the same program envelope:

```bash
lmprobe aws catalog --json
lmprobe aws query sts get-caller-identity --json
lmprobe aws query --request-file request.json --outfile result.json
```

The [AWS query reference](durable-memory/commands/aws-query.md) defines complex
JSON parameters, pagination, output selection, and the supported operation catalog.

## lmcode host

lmprobe exposes its 63 operation library as an lmcode host. Inspect the host
contract with `lmprobe --lmcode-schema --json` or print its function signatures
with `lmprobe --lmcode-signatures`. Programs invoke `probe_<operation>` functions
with one JSON string argument; use `json_parse` to turn the returned operation
JSON into a structured lmcode value. The parameter catalog is
[`crates/lmprobe/lmcode/operations.json`](crates/lmprobe/lmcode/operations.json).

Run compiled programs with `lmprobe --lmcode program.bytecode`. Each invocation
writes a terminal `LMCODE_RETURN` envelope. Invocation dry mode is handled by the
lmcode runtime before operation callbacks, so dry runs do not launch the legacy
CLI or touch its resources. Existing CLI commands retain their output and exit
codes. See [lmcode host notes](crates/lmprobe/lmcode/README.md).

## Build from source

Requires a stable Rust toolchain (pinned by `rust-toolchain.toml`; minimum Rust
1.79), plus `pkg-config` and OpenSSL development headers for `git2`.

```bash
cargo build -p lmprobe --release            # default (lightweight CLI) -> target/release/lmprobe
cargo build -p lmprobe --features serve     # + GraphQL HTTP daemon
```

Tests:

```bash
cargo test -p lmprobe -p lmprobe-engine --locked
cargo test -p lmprobe --release --test deep -- --ignored   # needs LMPROBE_QA_CORPUS
cargo test -p lmprobe --release --test perf -- --ignored
```

Tests that use the external qa1 corpus (`LMPROBE_QA_CORPUS`) or a gitleaks
checkout (`LMPROBE_GITLEAKS_REPO`) skip when those variables are not set.

## Repository layout

- `crates/lmprobe` - the CLI binary; `crates/lmprobe-*` - engine, types,
  registry, secrets scanner and per-language crates.
- `crates/{core,config,language}` - forks of the upstream ast-grep crates.
- `xtask/` - workspace tooling (language scaffolding, coverage checks).
- `npm/` - npm packaging for `@lmctl-ai/lmprobe` (see `npm/README.md`).
- `site/` - the static manual published at https://lmctl.com/lmprobe (see `site/README.md`).
- `durable-memory/` - design and implementation notes, command references and
  AI-agent test cases; start at `durable-memory/index.md`.

## Contributing

Read [AGENTS.md](AGENTS.md) first: it lists the invariants, the checks to run
before sending a change, and the commit-message convention. Command contracts
live in `durable-memory/commands/`; the architecture spec is
`crates/lmprobe/ARCHITECTURE.md`; release history is `crates/lmprobe/CHANGELOG.md`.
Report bugs at https://github.com/lmctl-ai/lmprobe/issues.

## License + attribution

MIT, with dual copyright; see `LICENSE` and `ATTRIBUTION.md`. The
`crates/{core,config,language}` crates are forks of upstream ast-grep components
of the same name. The `crates/lmprobe` and `crates/lmprobe-*` crates are
lmprobe-original work, copyright Lattice Systems LLC.
