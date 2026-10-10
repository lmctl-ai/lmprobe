# lmapi mode (LLM-era API)

lmprobe is a native [lmapi](https://github.com/lmctl-ai/lmapi) tool (lmapi protocol section 6.1):
the same binary serves a published `cli.schema.v1` document and operations with lmapi's JSON
results, envelopes and exit codes, while every legacy invocation keeps its own parser, output and
exit codes. The server side is the lmapi Rust SDK
[lmrust](https://github.com/lmctl-ai/lmrust) (`crates/lmprobe/Cargo.toml`, tag `v0.1.1`, with
`default-features = false`: lmrust's default `preserve_order` feature would switch every
`serde_json::Value` lmprobe prints to insertion order and break the legacy sorted-key output and
goldens, because Cargo unifies features across the binary).

## Use cases

| Task | Command |
| --- | --- |
| Publish the contract | `lmprobe --schema` |
| List operations | `lmprobe --list` |
| One operation's schema / help | `lmprobe --explain git_status`, `lmprobe git_status --help` |
| Flags form | `lmprobe git_status --repo .` |
| JSON form | `lmprobe git_status --input-json '{"repo": "."}'` or `--input-file request.json` (a `{"op", "params"}` envelope also names the op) |
| Preview a write | `lmprobe edit_text --pattern a --replacement b src/x.rs --dryrun` |
| Apply a dangerous write | `lmprobe find_delete '\.log$' --force --confirm` |

## Mode detection

`cli::run()` calls `lmapi::maybe_run` before clap parsing (`crates/lmprobe/src/lmapi/mod.rs`).
It uses lmrust's guard (`maybe_run_cli` / `is_lmapi_invocation`): skipping leading lmapi globals,
the argv is lmapi-shaped iff its first remaining token is a published operation name, or (with no
such token) it contains a root lmapi global: `--schema`, `--list`, `--explain`, `--input-json`,
`--input-file`, `--input-graphql`, `--dryrun`, `--readonly`, `--confirm` (any spelling).
`--help`/`-h`, `--version`, empty argv and every legacy command stay legacy. Published names never
equal a legacy command name (`def` is published as `def_symbol`, `grep` as `grep_text`), so
`lmprobe def X` is unchanged. A cheap pre-filter (`could_be_lmapi`: a published op name or a trigger
flag somewhere in argv) skips building the spec for legacy argv; a unit test checks it never rejects
an argv the guard accepts.

In lmapi mode: results are one JSON document on stdout, exit 0 for `ok`, `no_match` and `partial`
(the status stays inside the `lmprobe.result.v1` document), and failures are lmapi envelopes with
lmapi exit codes. The same pattern that exits 1 (`no_match`) in legacy mode exits 0 in lmapi mode.

## The published document

- Generated from clap (`lmapi/generate.rs`): every visible argument of the command path, global
  options included, with long name (camelCase param name), shorts and visible aliases, occurrence
  (`1 ? + *` from required/num_args/action), type (numbers and booleans from the value parser,
  value enums as `enum`), defaults and help text. Hidden arguments are not published.
- Merged with `crates/lmprobe/lmapi/overlay.toml`: published name, `access` (required),
  `dangerous`, description overrides, per-op excluded params, fixed legacy args (`--apply`,
  `--action delete`) and the stdin list param of `delete_paths`.
- Never published: `serve`, `help`, and the output-owning options `--json`, `--outfile`,
  `--infile`, `--format`, `--exit-zero-on-no-match`. When a command has both a single positional
  `path` and a repeatable `--path`, the repeatable option is published as `path`.
- `inputs` is `["flags", "json"]` (lmprobe's own GraphQL `query`/`--infile` dialect is not lmapi's
  graphql form; `--input-graphql` is refused with `FORM_NOT_ALLOWED`). The `format` global is
  dropped (lmprobe owns `--format`). No `product`; `version` is `CARGO_PKG_VERSION`.
- Common options are published only where the command consumes them (overlay): `--quiet` and
  `--verbose` never (they only change stderr, which lmapi mode discards); `-A/-B/-C`
  (`after`/`before`/`context`) only on `grep_text` and `ref_symbol` (the only `ContextLines` users);
  none on git_*, aws_* and the secrets recipe/source listings (`common_options = false`: their
  dispatch ignores `CommonArgs`/`walk_opts`, and `--workspace` is refused). Every other op keeps all
  common options, so nothing an op actually reads is hidden. This took the document from ~311 KB to
  ~196 KB; trimming further would need per-sensor evidence of which `WalkOpts` fields are read.
- Array params with an `enum` publish their default (the enum applies per item; lmrust >= 0.1.1).

Channel files at the repo root: `cli.schema.dev.json` equals `lmprobe --schema` byte for byte
(regenerate: `UPDATE_LMAPI_SCHEMA=1 cargo test -p lmprobe --test lmapi schema_dev_file`);
`cli.schema.latest.json` is the last released contract and must stay an additive subset of dev
(copy dev to latest in a release commit). Both are checked against lmrust's publish check and the
vendored lmapi JSON Schema `crates/lmprobe/lmapi/cli.schema.v1.schema.json` (from lmapi `c7768fb`).

## Operations

| Operation | Legacy command | Access | Dangerous |
| --- | --- | --- | --- |
| `discover_projects` | `discover` | read |  |
| `search_symbols` | `search` | read |  |
| `def_symbol` | `def` | read |  |
| `ref_symbol` | `ref` | read |  |
| `list_kinds` | `list-kinds` | read |  |
| `callers_of` | `callers` | read |  |
| `imports_of` | `imports` | read |  |
| `decorations_of` | `decorations` | read |  |
| `evolution_of` | `evolution` | read |  |
| `callees_of` | `callees` | read |  |
| `supertypes_of` | `supertypes` | read |  |
| `subtypes_of` | `subtypes` | read |  |
| `overrides_of` | `overrides` | read |  |
| `exports_of` | `exports` | read |  |
| `impact_of` | `impact` | read |  |
| `pr_triage` | `pr-triage` | read |  |
| `context_bundle` | `context` | read |  |
| `ast_diff` | `ast-diff` | read |  |
| `diff_context` | `diff-context` | read |  |
| `resource_sites` | `resource-sites` | read |  |
| `async_leaks` | `async-leaks` | read |  |
| `crypto_sites` | `crypto-sites` | read |  |
| `config_keys` | `config-keys` | read |  |
| `secrets_scan` | `secrets` | read |  |
| `secrets_recipes_list` | `secrets-recipes list` | read |  |
| `secrets_recipes_show` | `secrets-recipes show` | read |  |
| `secrets_sources_list` | `secrets-sources list` | read |  |
| `secrets_sources_show` | `secrets-sources show` | read |  |
| `guard_sites` | `guard-sites` | read |  |
| `openapi_facts` | `openapi-facts` | read |  |
| `route_anchors` | `route-anchors` | read |  |
| `http_client_calls` | `http-client-calls` | read |  |
| `py_slice_candidates` | `py-slice-candidates` | read |  |
| `callsite_args` | `callsites` | read |  |
| `grep_text` | `grep` | read |  |
| `find_paths` | `find` | read |  |
| `git_status` | `git status` | read |  |
| `git_diff` | `git diff` | read |  |
| `git_log` | `git log` | read |  |
| `git_show` | `git show` | read |  |
| `git_branch` | `git branch` | read |  |
| `git_remote` | `git remote` | read |  |
| `git_rev_parse` | `git rev-parse` | read |  |
| `git_ls_files` | `git ls-files` | read |  |
| `git_blame` | `git blame` | read |  |
| `history_blame` | `history blame` | read |  |
| `history_introduced` | `history introduced` | read |  |
| `history_commit` | `history commit` | read |  |
| `history_diff` | `history diff` | read |  |
| `history_churn` | `history churn` | read |  |
| `history_co_change` | `history co-change` | read |  |
| `history_todos` | `history todos` | read |  |
| `history_evolution` | `history evolution` | read |  |
| `aws_catalog` | `aws catalog` | read |  |
| `aws_query` | `aws query` | read |  |
| `edit_preview` | `edit` | read |  |
| `edit_text` | `edit --apply` | write | yes |
| `fix_preview` | `fix` | read |  |
| `fix_ast` | `fix --apply` | write | yes |
| `find_delete` | `find --action delete` | write | yes |
| `delete_paths` | `delete` | write | yes |
| `trace_exec` | `trace` | write | yes |
| `query_graphql` | `query` | dynamic |  |

`edit_preview`/`fix_preview` are the legacy default previews; `edit_text`/`fix_ast` add `--apply`.
`find_paths` cannot delete; `find_delete` adds `--action delete` and keeps lmprobe's own `--force`
as an ordinary parameter (without it, deletion still only previews). `delete_paths` takes `paths`
and feeds them to `lmprobe delete` on stdin. `query_graphql` is `dynamic`: a document containing a
`mutation` (inline or `@file`) is treated as a write for `--readonly`/`--dryrun`.

## Policy

- `--readonly` (or `LMPROBE_READONLY=1|true|yes|on`) refuses write ops with `READONLY_VIOLATION`
  (exit 6); a flag cannot loosen the env floor. Reads always run.
- `--dryrun` (or `LMPROBE_DRYRUN`) returns `{success, dryRun, simulated, command}` for a write op
  without running anything; `command` is the legacy argv that would run.
- Dangerous ops (`delete_paths`, `find_delete`, `trace_exec`, `edit_text`, `fix_ast`) need
  `--confirm` (`MISSING_CONFIRMATION`, exit 9).
- lmprobe's own safety checks (dirty tree, workspace read-only, delete refusals) still apply inside
  the legacy run and surface as errors.

## Errors

Legacy error codes become standard lmapi codes; the legacy code is kept in `error.data.tool_code`,
the legacy exit code in `data.legacy_exit_code` and the legacy result in `data.result`:

| Legacy code | lmapi code (exit) |
| --- | --- |
| `ARGUMENT_ERROR`, `INVALID_*` scope/marker/trace mode, format/infile conflicts | `VALIDATION_ERROR` (2) |
| `DIRTY_TREE`, `EDIT_DIRTY_TREE`, `WORKSPACE_READ_ONLY` | `CONFLICT` (5) |
| `NOT_GIT_REPO`, `GIT_REPOSITORY_ERROR`, `*_NOT_FOUND` | `NOT_FOUND` (4) |
| `*_REFUSED` | `FORBIDDEN` (6) |
| `*_TIMEOUT` (AWS query timeout) | `TIMEOUT` (7) |
| `AWS_CLI_UNAVAILABLE` | `COMMAND_NOT_FOUND` (127) |
| `OUTFILE_*`, a panic (exit 101) or a run without a result | `INTERNAL_ERROR` (1) |
| anything else (e.g. `AWS_CLI_ERROR`) | `COMMAND_FAILED` (1) |

## Implementation and limitations

- Phase 1 runs every operation as a child process: `lmapi/child.rs::run_legacy` (the only spawn
  site) executes the current executable with `--outfile <private temp> <legacy argv>` (no shell)
  and reads the `lmprobe.result.v1` document ([program JSON](../program-json.md)). Cost: one extra
  process per operation.
- Phase 2 (in-process) is blocked by the `process::exit` calls on the legacy path: in `cli.rs`
  `run()` (parse/outfile failures and the program-JSON error exit), `render_parse_error`, the
  workspace mutation refusal, and the exit-code exits at the end of `run_invocation` / sensor
  dispatch; `output.rs` `handle_stdout_err` also exits. Replace `run_legacy` once those return.
- The document is still sizeable (~196 KB): sensors repeat the remaining common options.
- `query_graphql` mutation detection is lexical (a `mutation` word outside `#` comments).

## Tests

- `crates/lmprobe/src/lmapi/*`: generator types/occurs/defaults/bindings, overlay rules (missing
  access is an error, every clap command published or excluded, no name shadows a legacy command),
  access table, argv building, error mapping, pre-filter and legacy-stays-legacy tables.
- `crates/lmprobe/tests/lmapi.rs`: channel files, JSON Schema and publish checks, `--list`,
  flags and JSON forms, unknown op, bad params, readonly/env floor/confirm/dryrun, `delete_paths`,
  no-match exit 0 vs legacy exit 1, lmrust's interpreter over the published document.
