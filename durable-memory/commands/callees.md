# lmprobe callees

Find functions or methods called by a symbol.

## Use cases

| Case | Invocation |
| --- | --- |
| Calls inside a declaration | `lmprobe callees handle_request src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe callees [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Callees](../../crates/lmprobe/src/cli.rs) and `Commands::Callees`; [dispatch: Commands::Callees](../../crates/lmprobe/src/cli.rs).
- [Engine: graph_sensors::callees](../../crates/lmprobe-engine/src/graph_sensors.rs).
- [Result schema: CalleesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: callees](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Only call sites within the selected declaration body are considered; same-named declarations in unrelated scopes must not leak into results.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [callers_callees_regressions](../../crates/lmprobe/tests/callers_callees_regressions.rs)
- [context_callees_scope](../../crates/lmprobe/tests/context_callees_scope.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test callers_callees_regressions
cargo test -p lmprobe --locked --test context_callees_scope
```

Also check `lmprobe callees -h` and `lmprobe callees --help` after option changes.
