# lmprobe history todos

Find TODO/FIXME/XXX/HACK markers added in history.

## Use cases

| Case | Invocation |
| --- | --- |
| All added markers | `lmprobe history todos .` |
| Recent markers | `lmprobe history todos --since 30d .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history todos [OPTIONS] [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `--since <SINCE>` | Time window, such as 30d, 12h, or 2w |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: HistoryCmd::Todos](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::Todos`; [dispatch: HistoryCmd::Todos](../../crates/lmprobe/src/cli.rs).
- [Engine: history_sensors::todos](../../crates/lmprobe-engine/src/history_sensors.rs).
- [Result schema: HistoryTodosResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: todos](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Looks for markers added by commits, not all markers currently present. Uses shared cache even with global --no-cache.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe history todos -h` and `lmprobe history todos --help` after option changes.
