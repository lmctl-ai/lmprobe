# lmprobe context

Gather a symbol's definition, calls, imports, and history.

## Use cases

| Case | Invocation |
| --- | --- |
| Definition-based bundle | `lmprobe context handle_request src/` |
| Usage anchors and body | `lmprobe context handle_request --usage --around 3 --parents --body src/` |
| Follow one relative import | `lmprobe context handler --import-aware src/page.ts` |
| Enclosing declaration at a line | `lmprobe context src/app.py:42 --around 3 .` |
| Recent commit cap | `lmprobe context Parser --recent-history-limit 5 .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe context [OPTIONS] <SYMBOL> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<SYMBOL>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--def` | Anchor on a definition. This is the default behavior |
| `--usage` | Anchor on usages/references instead of definitions |
| `--depth <DEPTH>` | Requested graph depth (currently returns one-hop evidence) [default: 1] |
| `--import-aware` | Follow one explicit relative import/re-export hop when the symbol is not declared under PATH |
| `--around <AROUND>` | Include N source lines before and after the resolved context anchor |
| `--parents` | Attach enclosing structural declaration chain metadata to usage anchors |
| `--body` | Attach innermost enclosing structural body metadata/content to usage anchors |
| `--recent-history-limit <RECENT_HISTORY_LIMIT>` | Maximum recent-history entries (default: 3; must be at least 1) |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Context](../../crates/lmprobe/src/cli.rs) and `Commands::Context`; [dispatch: Commands::Context](../../crates/lmprobe/src/cli.rs).
- [Engine: context_bundle::context_with_options](../../crates/lmprobe-engine/src/context_bundle.rs).
- [Result schema: ContextResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: context](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Default definition bundles for regular code query recent history and require a Git repository. Usage mode and synthetic Svelte anchors do not take that history path. Depth is recorded in the envelope but traversal currently remains one hop. `--limit` does not cap composite lists; `--recent-history-limit` caps only recent history and rejects 0. Usage-mode parents/body expansions differ from definition-mode bodies. `--import-aware` follows one explicit relative import/re-export hop. Svelte may use synthetic anchors and omit AST/call/history components.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [context_usage_mode](../../crates/lmprobe/tests/context_usage_mode.rs)
- [context_callees_scope](../../crates/lmprobe/tests/context_callees_scope.rs)
- [context_location_resolution](../../crates/lmprobe/tests/context_location_resolution.rs)
- [query_context_exit_status](../../crates/lmprobe/tests/query_context_exit_status.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test context_usage_mode
cargo test -p lmprobe --locked --test context_callees_scope
```

Also check `lmprobe context -h` and `lmprobe context --help` after option changes.
