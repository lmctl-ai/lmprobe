# lmprobe history introduced

Find the first AST appearance of a symbol.

## Use cases

| Case | Invocation |
| --- | --- |
| First appearance | `lmprobe history introduced Parser .` |
| Track file renames | `lmprobe history introduced Parser --follow .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history introduced [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `-l, --lang <LANG>` | Accepted for compatibility; currently does not filter introduction history |
| `--follow` | Track the symbol across file renames |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: HistoryCmd::Introduced](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::Introduced`; [dispatch: HistoryCmd::Introduced](../../crates/lmprobe/src/cli.rs).
- [Engine: history_sensors::introduced_with_follow](../../crates/lmprobe-engine/src/history_sensors.rs).
- [Result schema: IntroducedResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: introduced](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Important implementation limit: CLI parses `-l/--lang` but discards it (`lang: _`); the engine is called with no language filter. `--no-cache` is forwarded. Rename following is file-based; symbol names remain exact.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe history introduced -h` and `lmprobe history introduced --help` after option changes.
