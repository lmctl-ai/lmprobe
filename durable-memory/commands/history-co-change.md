# lmprobe history co-change

Rank files that often change with a given file.

## Use cases

| Case | Invocation |
| --- | --- |
| Related files | `lmprobe history co-change src/lib.rs .` |
| Rank/score/time filters | `lmprobe history co-change src/lib.rs --top 10 --min-score 0.2 --since 30d .` |
| Exclude paths or filter types | `lmprobe history co-change src/lib.rs --exclude-glob '**/tests/**' --type rust .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history co-change [OPTIONS] <FILE> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<FILE>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--since <SINCE>` | Time window, such as 30d, 12h, or 2w |
| `--top <TOP>` | Keep only the first N co-change rows after score sorting |
| `--min-score <MIN_SCORE>` | Keep only rows whose score is at least this value |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: HistoryCmd::CoChange](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::CoChange`; [dispatch: HistoryCmd::CoChange](../../crates/lmprobe/src/cli.rs).
- [Engine: history_sensors::co_change](../../crates/lmprobe-engine/src/history_sensors.rs).
- [Result schema: HistoryCoChangeResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: co_change](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

The engine caps its walk at 1000 commits and signals HISTORY_TRUNCATED. `filter_co_change_result` applies --top, --min-score, --exclude-glob, and --type after scoring. Uses shared cache even with global --no-cache.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [history_co_change_filters](../../crates/lmprobe/tests/history_co_change_filters.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test history_co_change_filters
```

Also check `lmprobe history co-change -h` and `lmprobe history co-change --help` after option changes.
