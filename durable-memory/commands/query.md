# lmprobe query

Run a GraphQL document or recipe in-process.

## Use cases

| Case | Invocation |
| --- | --- |
| Inline GraphQL | `lmprobe query '{ discover(path: ".") { projects { name } } }'` |
| Advanced expression file | `lmprobe --infile expression.graphql --outfile result.json` |
| Program output | `lmprobe query '{ __typename }' --json` |
| GraphQL file | `lmprobe query @query.graphql` |
| GraphQL stdin | `lmprobe query -` |
| Variables and operation selection | `lmprobe query @query.graphql --vars-file vars.json --var limit=10 --operation-name Inspect` |
| Generated provider/source recipe | `lmprobe query --recipe secrets/aws/python --var path=src/ --all-shapes` |
| Persistent mutations | `lmprobe query @mutation.graphql` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe query [OPTIONS] [QUERY]
```

| Argument / option | Behavior |
| --- | --- |
| `[QUERY]` | GraphQL query source. Use `-` for stdin or `@path` for a file |
| `--recipe <KEY>` | Run a generated recipe, e.g. `secrets/aws/python` or `secrets-source/env-aws/python` |
| `--all-shapes` | With --recipe, execute every shape under the selected key |
| `--var <KEY=VALUE>` | Variable KEY=VALUE (repeatable; JSON values or plain strings) |
| `--vars-file <PATH>` | JSON object of variables; --var entries override matching keys |
| `--operation-name <NAME>` | Operation name for documents containing multiple operations |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Query](../../crates/lmprobe/src/cli.rs) and `Commands::Query`; [dispatch: Commands::Query](../../crates/lmprobe/src/cli.rs).
- [dispatch_query](../../crates/lmprobe/src/cli.rs).
- [QueryResult](../../crates/lmprobe-types/src/schema.rs).
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Global --infile/--intfile accepts full advanced GraphQL documents literally and may imply query. --outfile/--json use the [program JSON contract](../program-json.md), while --format json remains the legacy envelope. Competing inline/recipe/infile sources are rejected.

Variables load from file first, then repeatable --var overrides; valid JSON values keep their type, otherwise become strings. --recipe conflicts with query source; --all-shapes requires --recipe. GraphQL camelCase differs from CLI JSON snake_case. fix/trace/untrace mutations exist in GraphQL; untrace has no top-level CLI command.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [graphql_composition](../../crates/lmprobe/tests/graphql_composition.rs)
- [graphql_surface_equality](../../crates/lmprobe/tests/graphql_surface_equality.rs)
- [graphql_failure_context](../../crates/lmprobe/tests/graphql_failure_context.rs)
- [query_context_exit_status](../../crates/lmprobe/tests/query_context_exit_status.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test graphql_composition
cargo test -p lmprobe --locked --test graphql_surface_equality
```

Also check `lmprobe query -h` and `lmprobe query --help` after option changes.
