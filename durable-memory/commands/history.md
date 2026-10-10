# lmprobe history

Explore Git blame, commits, churn, and symbol history.

## Use cases

| Case | Invocation |
| --- | --- |
| Blame | `lmprobe history blame Parser .` |
| Commit/time queries | `lmprobe history commit --since 30d .` |
| Change hotspots | `lmprobe history churn --top 10 .` |
| Nested command help | `lmprobe history co-change --help` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history [OPTIONS] <COMMAND>
```

| Argument / option | Behavior |
| --- | --- |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

Subcommands: [blame](history-blame.md), [introduced](history-introduced.md), [commit](history-commit.md), [diff](history-diff.md), [churn](history-churn.md), [co-change](history-co-change.md), [todos](history-todos.md), [evolution](history-evolution.md).

## Implementation entry points

- [CLI arguments: Commands::History](../../crates/lmprobe/src/cli.rs) and `Commands::History`; [dispatch: Commands::History](../../crates/lmprobe/src/cli.rs).
- [dispatch_history](../../crates/lmprobe/src/cli.rs).
- [GraphQL resolver: history](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Git is required. See each child file for actual cache and filter support: global flags are parsed for every command but some history paths do not use them.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [history_blame_around](../../crates/lmprobe/tests/history_blame_around.rs)
- [history_co_change_filters](../../crates/lmprobe/tests/history_co_change_filters.rs)
- [history_shallow_diagnostics](../../crates/lmprobe/tests/history_shallow_diagnostics.rs)
- [history_blame_absolute_pathline](../../crates/lmprobe/tests/history_blame_absolute_pathline.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test history_blame_around
cargo test -p lmprobe --locked --test history_co_change_filters
```

Also check `lmprobe history -h` and `lmprobe history --help` after option changes.
