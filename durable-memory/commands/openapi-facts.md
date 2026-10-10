# lmprobe openapi-facts

Extract HTTP methods and paths from OpenAPI files.

## Use cases

| Case | Invocation |
| --- | --- |
| One specification | `lmprobe openapi-facts openapi.yaml` |
| Directory of JSON/YAML specs | `lmprobe openapi-facts api/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe openapi-facts [OPTIONS] <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::OpenapiFacts](../../crates/lmprobe/src/cli.rs) and `Commands::OpenapiFacts`; [dispatch: Commands::OpenapiFacts](../../crates/lmprobe/src/cli.rs).
- [Engine: api_facts::openapi_facts](../../crates/lmprobe-engine/src/api_facts.rs).
- [Result schema: OpenApiFactsResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: openapi_facts](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Extracts paths/methods only, no schema validation or $ref resolution. Parse failures produce OPENAPI_PARSE_FAILED.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [api_facts](../../crates/lmprobe/tests/api_facts.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test api_facts
```

Also check `lmprobe openapi-facts -h` and `lmprobe openapi-facts --help` after option changes.
