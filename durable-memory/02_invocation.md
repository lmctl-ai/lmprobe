# 02 — Invocation

How to install, build, and run lmprobe. Global flags are documented here; per-verb flags live in the per-sensor chapters ([03](03_sensors_search.md), [04](04_sensors_graph.md), [05](05_history.md), [06](06_mutations.md)).

## Build

The `lmprobe` binary lives at `crates/lmprobe/`. As of Phase 5A, the source is split across multiple workspace crates:

```
crates/lmprobe-types/        # D4 envelope shapes + scalars
crates/lmprobe-lang-core/    # D5 trait definitions + SgLang re-export
crates/lmprobe-registry/     # Per-language dispatch fan-out (xtask-managed)
crates/lmprobe-engine/       # Sensors (search, def, callers, history, …)
crates/lmprobe-lang-rust/    # Per-language D5 impls (extracted in 5A.3)
crates/lmprobe/              # Binary: clap CLI + GraphQL Query/Mutation + output
xtask/                       # cargo xtask new-lang / check-lang scaffolding
```

Phase 5B extraction is complete for the remaining language families; all current
language implementations are in dedicated `crates/lmprobe-lang-<name>/` crates. Build commands run from the workspace root:

```bash
# Default: lightweight CLI, no server.
cargo build -p lmprobe --release

# Opt-in GraphQL HTTP daemon.
cargo build -p lmprobe --release --features serve

# Add a new per-language crate (Phase 5B workflow).
cargo xtask new-lang <name>
cargo xtask check-lang [<name>]   # drift detector
```

The release binary lands at `target/release/lmprobe`. There is no install step beyond putting it on `$PATH`. Both feature configurations (default, `serve`) build warning-free.

Walker defaults skip dependency, build, VCS, and cache directories such as
`node_modules`, `.git`, `target`, `dist`, `.next`, `.venv`, and Python cache
folders. This keeps broad old-project and home-directory scans focused on
source code. Use `--no-default-excludes` only when intentionally inspecting
generated or vendored trees. GraphQL discovery-style fields expose the same
escape hatch as `noDefaultExcludes: true`.

For broad exploratory `find`/`grep` scans, `--quick-projects` first narrows
roots to `package.json` directories and directories containing at least five
`.pl`/`.pm` files, then runs the normal sensor over those roots. GraphQL
`find` and `grep` expose the same recipe as `quickProjects: true`. The quick
pre-scan uses the normal walker, so it honors `.gitignore`/`.ignore`,
`--no-ignore`, and default excludes. Quick project discovery is depth-capped so
home-directory scans do not crawl every project internals before narrowing.
Combine `grep --quick-projects` with `--limit` for fast exploratory content
answers; grep stops after collecting the requested result count. `find` still
collects and ranks path hits before applying `--limit`. The Perl rule is
intentionally just a density heuristic; sparse Perl layouts fall back only when
scanned as their own explicit root. A deep marker below the discovery depth can
be missed under a broad root that already produced shallower candidates; scan
that subtree explicitly when the broad quick pass does not find it.

See [12_optional_layers.md](12_optional_layers.md) for the `serve` feature-flag layer and [11_extending.md](11_extending.md) for the per-language crate workflow.

## First invocation

```bash
$ lmprobe --help
```

prints the verb list. Pick one and try it against any git-tracked directory:

```bash
$ cd /path/to/some/repo
$ lmprobe discover .
```

Output is auto-formatted: markdown when stdout is a TTY, NDJSON when piped. See [07_output_formats.md](07_output_formats.md) for every format.

## CLI shape

```
lmprobe [GLOBAL FLAGS] <verb> [verb-specific args]
```

Verbs are listed in [01_overview.md](01_overview.md) § Sensor inventory. Each verb takes its own positional args + flags; most accept an optional `[PATH]` defaulting to `.`.

The `query` verb is special: it accepts a GraphQL document instead of per-verb flags and runs the document against the in-process schema. Use it to compose multiple sensors in one round-trip, parameterize queries with variables, or call mutations (`fix` / `trace` / `untrace`).

```bash
# Inline GraphQL
lmprobe query '{ search(query: "Pattern", path: ".") { hits { filePath lineStart } } }'

# stdin
cat q.graphql | lmprobe query -

# File + variables
lmprobe query @q.graphql --var name='"Pattern"' --var depth=2
```

Full reference: [17_graphql.md](17_graphql.md).

## Global flags

These apply to every verb. Defined in `Cli` and `CommonArgs` in `crates/lmprobe/src/cli.rs`.

### Output / control

| Flag | Default | Purpose |
|------|---------|---------|
| `--format <FORMAT>` | `auto` | One of `auto`, `markdown`, `json`, `ndjson`, `sarif`, `github`, `mermaid`, `dot`. `auto` resolves to markdown on TTY, ndjson when piped. Mermaid/DOT only on graph-shaped verbs. See [07_output_formats.md](07_output_formats.md). |
| `--exit-zero-on-no-match` | off | Treat `no_match` as exit 0 (default exit 1). Useful for shell pipelines. See [14_diagnostics.md](14_diagnostics.md) § Exit codes. |
| `--no-cache` | off | Skip the XDG SQLite cache on this invocation. Forces re-walk for `evolution`, `history *`, `context`'s `recent_history`. See [09_cache.md](09_cache.md). |
| `--workspace <YAML>` | unset | Federated multi-repo mode. YAML config maps `roots:` to per-root paths + optional aliases. Read-only verbs fan out across roots; mutation verbs (`fix --apply`, `trace`) refuse. See [13_workspace.md](13_workspace.md). |
| `-q, --quiet` | off | Suppress stderr diagnostic mirror. Envelope `warnings` field still ships in JSON. |
| `--limit <N>` | unset | Cap the result count in the envelope. |

### Walker / filter

| Flag | Default | Purpose |
|------|---------|---------|
| `-l, --lang <LANG>` | unset | Restrict the walker to one language (`rust`, `python`, `tsx`, `ruby`, `kotlin`, `go`, etc. — full list in [10_languages.md](10_languages.md)). Required for `-p / --pattern` directory searches; single-file pattern searches can infer from a known extension. |
| `-G, --globs <GLOB>` | unset | File-glob include/exclude (repeatable, comma-sep). Uses `ignore`-crate semantics. `-G '!**/test/**'` excludes; `-G 'src/**/*.rs'` restricts. |
| `--no-ignore` | off | Disregard `.gitignore` / `.ignore` files. Walks directories git would skip. |
| `-j, --threads <N>` | num_cpus | Parallel walker thread count. Lower for noisy machines; higher rarely helps. |
| `-t, --type <TYPE>` | unset | File-type shortcut at the walker level (`rust`, `py`, `js`, etc.) — same conceptual role as `--lang` but no AST parse, just file-filter. |
| `--max-filesize <BYTES>` | 5 MiB | Skip files larger than this. Skipped files emit `FILE_TOO_LARGE` diagnostic. |
| `-i, --ignore-case` | off | Case-insensitive substring matching (textual filters only). |

### Context lines (markdown rendering only)

| Flag | Default | Purpose |
|------|---------|---------|
| `-A, --after <N>` | 0 | Lines of context after each match. |
| `-B, --before <N>` | 0 | Lines of context before each match. |
| `-C, --context <N>` | 0 | Shorthand for `-A N -B N`. |

These render under `--format markdown`; JSON envelope ships only the matched node's `content` field.

## Exit codes

lmprobe follows three-state exit semantics. See [14_diagnostics.md](14_diagnostics.md) for the full table; summary:

| Code | Meaning |
|------|---------|
| 0    | `matched` — at least one hit. |
| 1    | `no_match` — verb ran cleanly, found nothing. |
| 2    | `error` — refusal (dirty tree under `trace`/`fix --apply`, malformed YAML rule, missing `--lang` for pattern, `WORKSPACE_READ_ONLY` mutation refusal, etc.). |
| 101  | (panic) reserved; lmprobe should never exit 101. The deep tier asserts zero panics across 261 sensor invocations on 29-language qa1 fixtures. If you see 101, file a bug. |

`--exit-zero-on-no-match` collapses `1 → 0` for shell-script use.

## Examples

```bash
# Markdown output on a TTY.
lmprobe search --name Pattern .

# Pipe to jq.
lmprobe --format json search --name Pattern --path crates/core | jq '.hits | length'

# Restrict to one language.
lmprobe search --kind class -l python .

# Workspace fan-out.
cat > ws.yaml <<'EOF'
roots:
  - path: /home/me/api
    alias: api
  - path: /home/me/web
    alias: web
EOF
lmprobe --workspace ws.yaml --format json search --name UserID --limit 50

# Cache-bypass (forces re-walk for history-family verbs).
lmprobe --no-cache history blame Pattern .

# Suppress no-match exit-1 in a shell pipeline.
lmprobe --exit-zero-on-no-match search Foo . || echo "search failed"
```

## Where the binary's state lives

| Path | Purpose |
|------|---------|
| `$XDG_CACHE_HOME/lmprobe/history.db` | SQLite cache for evolution + history-family verbs. Defaults to `~/.cache/lmprobe/`. See [09_cache.md](09_cache.md). |
| `$XDG_CACHE_HOME/lmprobe/trace-<ts>.log` | Audit log written by `trace --force` invocations (and every `trace --worktree` run). |
| `$XDG_CACHE_HOME/lmprobe/patches/trace-<ts>.patch` | Optional patch capture from `trace --keep-patch`. |
| `$XDG_CACHE_HOME/lmprobe/worktrees/trace-<ts>/` | Ephemeral worktree provisioned by `trace --worktree`. Removed on success; preserved on exec failure with `WORKTREE_PRESERVED` diagnostic. |
| `$XDG_CACHE_HOME/lmprobe/fix-<ts>.log` | Audit log for `fix --apply`. |
| `$XDG_CACHE_HOME/lmprobe/patches/fix-<ts>.patch` | Patch captured for `fix --apply`. |
| `$XDG_CACHE_HOME/lmprobe/worktrees/fix-<ts>/` | Ephemeral worktree for `fix --apply` (removed on success). |

The legacy path `<root>/.lmprobe/history.db` is read for back-compat; new writes always go to XDG. Reading the legacy path emits `DEPRECATED_CACHE_PATH`.

## Read next

- [03_sensors_search.md](03_sensors_search.md) for the search/def/ref/find/grep/discover verbs.
- [07_output_formats.md](07_output_formats.md) for choosing an output format.
- [13_workspace.md](13_workspace.md) for multi-repo setup.
