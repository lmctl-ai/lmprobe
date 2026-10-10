# lmprobe ref

Find identifier references to a symbol.

## Use cases

| Case | Invocation |
| --- | --- |
| Identifiers only | `lmprobe ref Parser src/` |
| Include comments/doc strings | `lmprobe ref Parser --include-comments --include-docs src/` |
| Data languages | `lmprobe ref database --scope all .` |
| Local roles and line context | `lmprobe ref Parser --role -C 2 --format json src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe ref [OPTIONS] <NAME> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<NAME>` | Required command input; see use cases above. |
| `[PATH]` | [default: .] |
| `--scope <SCOPE>` | Choose programming languages (code) or all supported languages (all) [default: code] [possible values: code, all] |
| `--include-comments` | Include identifier matches inside comment nodes |
| `--include-docs` | Include identifier matches inside doc-string nodes |
| `--role` | Attach a mechanical local role classification to JSON/NDJSON hits |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Ref](../../crates/lmprobe/src/cli.rs) and `Commands::Ref`; [dispatch: Commands::Ref](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::refs](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: RefResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: refs](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Identifier-node matching is not binding resolution. Comment/doc inclusion broadens matching. Role classification is mechanical local evidence.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [role_classified_anchors](../../crates/lmprobe/tests/role_classified_anchors.rs)
- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test role_classified_anchors
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe ref -h` and `lmprobe ref --help` after option changes.
