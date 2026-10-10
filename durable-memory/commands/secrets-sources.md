# lmprobe secrets-sources

List or show credential source query recipes.

## Use cases

| Case | Invocation |
| --- | --- |
| Enumerate sources | `lmprobe secrets-sources list --source env-aws` |
| Inspect generated query | `lmprobe secrets-sources show env-aws/python` |
| Execute selected recipe | `lmprobe query --recipe secrets-source/env-aws/python --var path=src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe secrets-sources [OPTIONS] <COMMAND>
```

| Argument / option | Behavior |
| --- | --- |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

Subcommands: [list](secrets-sources-list.md), [show](secrets-sources-show.md).

## Implementation entry points

- [CLI arguments: Commands::SecretsSources](../../crates/lmprobe/src/cli.rs) and `Commands::SecretsSources`; [dispatch: Commands::SecretsSources](../../crates/lmprobe/src/cli.rs).
- [dispatch_secrets_sources](../../crates/lmprobe/src/cli.rs).
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Run generated recipes with lmprobe query --recipe secrets-source/<KEY>.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [secrets_sources_cli](../../crates/lmprobe/tests/secrets_sources_cli.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test secrets_sources_cli
```

Also check `lmprobe secrets-sources -h` and `lmprobe secrets-sources --help` after option changes.
