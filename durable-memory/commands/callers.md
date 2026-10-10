# lmprobe callers

Find calls to a function or method.

## Use cases

| Case | Invocation |
| --- | --- |
| All call anchors | `lmprobe callers handle_request src/` |
| Test-path callers only | `lmprobe callers handle_request --tests-only .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe callers [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--tests-only` | Keep only caller hits from likely test paths |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Callers](../../crates/lmprobe/src/cli.rs) and `Commands::Callers`; [dispatch: Commands::Callers](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::callers](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: CallersResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: callers](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Syntactic call anchors do not prove runtime dispatch. `--tests-only` filters likely test paths.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [callers_callees_regressions](../../crates/lmprobe/tests/callers_callees_regressions.rs)
- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test callers_callees_regressions
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe callers -h` and `lmprobe callers --help` after option changes.
