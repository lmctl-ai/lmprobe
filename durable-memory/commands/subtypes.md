# lmprobe subtypes

Find subtypes or implementers of a type.

## Use cases

| Case | Invocation |
| --- | --- |
| Child types and implementers | `lmprobe subtypes Widget src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe subtypes [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Subtypes](../../crates/lmprobe/src/cli.rs) and `Commands::Subtypes`; [dispatch: Commands::Subtypes](../../crates/lmprobe/src/cli.rs).
- [Engine: graph_sensors::subtypes](../../crates/lmprobe-engine/src/graph_sensors.rs).
- [Result schema: SubtypesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: subtypes](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Coverage is capability-based; unsupported language behavior must stay explicit through diagnostics.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [relation_ast_diff_regressions](../../crates/lmprobe/tests/relation_ast_diff_regressions.rs)
- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test relation_ast_diff_regressions
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe subtypes -h` and `lmprobe subtypes --help` after option changes.
