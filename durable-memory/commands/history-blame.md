# lmprobe history blame

Show logic-changing commits for a symbol or source line.

## Use cases

| Case | Invocation |
| --- | --- |
| Symbol blame | `lmprobe history blame Parser .` |
| Line blame with nearby source | `lmprobe history blame src/lib.rs:42 --around 3 .` |
| Rename hops and semantic categories | `lmprobe history blame Parser --follow --semantic .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history blame [OPTIONS] <TARGET> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<TARGET>` | Symbol name OR `<path>:<line>` form for line blame |
| `[PATH]` | [default: .] |
| `--around <AROUND>` | Include N source lines before and after PATH:LINE targets |
| `--follow` | Track the symbol across file renames |
| `--semantic` | Classify body, parameter, and return-type changes |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: HistoryCmd::Blame](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::Blame`; [dispatch: HistoryCmd::Blame](../../crates/lmprobe/src/cli.rs).
- [Engine: history_sensors::blame_with_follow_semantic_around](../../crates/lmprobe-engine/src/history_sensors.rs).
- [Result schema: BlameResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: blame](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

`--around` applies to PATH:LINE targets. Rename/semantic heuristics do not prove semantic equivalence. Shallow/partial history can emit diagnostics.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [history_blame_around](../../crates/lmprobe/tests/history_blame_around.rs)
- [history_blame_absolute_pathline](../../crates/lmprobe/tests/history_blame_absolute_pathline.rs)
- [history_shallow_diagnostics](../../crates/lmprobe/tests/history_shallow_diagnostics.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test history_blame_around
cargo test -p lmprobe --locked --test history_blame_absolute_pathline
```

Also check `lmprobe history blame -h` and `lmprobe history blame --help` after option changes.
