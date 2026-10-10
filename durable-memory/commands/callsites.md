# lmprobe callsites

Extract Python call arguments for a callee.

## Use cases

| Case | Invocation |
| --- | --- |
| Positional and keyword argument text | `lmprobe callsites foo --args src/` |
| Nearby callsite source | `lmprobe callsites foo --args --around 2 src/app.py` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe callsites [OPTIONS] --args <CALLEE> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<CALLEE>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--args` | Enable argument extraction. Required for this first command slice |
| `--around <AROUND>` | Include N source lines around each callsite |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Callsites](../../crates/lmprobe/src/cli.rs) and `Commands::Callsites`; [dispatch: Commands::Callsites](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::callsites_args](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: CallsitesResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: callsites](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

--args is required for this CLI slice. Python AST argument text is syntactic, not overload/binding resolution. GraphQL has additional filters/redaction behavior; check both surfaces before claiming parity.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [graphql_callsites_filters](../../crates/lmprobe/tests/graphql_callsites_filters.rs)
- [callsites_args](../../crates/lmprobe/tests/callsites_args.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test graphql_callsites_filters
cargo test -p lmprobe --locked --test callsites_args
```

Also check `lmprobe callsites -h` and `lmprobe callsites --help` after option changes.
