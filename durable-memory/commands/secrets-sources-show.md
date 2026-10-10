# lmprobe secrets-sources show

Show a generated credential source GraphQL recipe.

## Use cases

| Case | Invocation |
| --- | --- |
| Source/language recipe | `lmprobe secrets-sources show env-aws/python` |
| Optional SDK key | `lmprobe secrets-sources show env-aws/python/stdlib-env` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe secrets-sources show [OPTIONS] <KEY>
```

| Argument / option | Behavior |
| --- | --- |
| `<KEY>` | Recipe key: <source>/<language>[/<sdk>] |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: SecretsSourcesCmd::Show](../../crates/lmprobe/src/cli.rs) and `SecretsSourcesCmd::Show`; [dispatch: SecretsSourcesCmd::Show](../../crates/lmprobe/src/cli.rs).
- [dispatch_secrets_sources](../../crates/lmprobe/src/cli.rs).
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Coverage is capability-based; unsupported language behavior must stay explicit through diagnostics.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe secrets-sources show -h` and `lmprobe secrets-sources show --help` after option changes.
