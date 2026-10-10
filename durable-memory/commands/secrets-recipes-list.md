# lmprobe secrets-recipes list

List credential sink recipes.

## Use cases

| Case | Invocation |
| --- | --- |
| All recipes | `lmprobe secrets-recipes list` |
| Provider/language filters | `lmprobe secrets-recipes list --provider aws --language python` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe secrets-recipes list [OPTIONS]
```

| Argument / option | Behavior |
| --- | --- |
| `--provider <PROVIDER>` | Limit rows to a provider id such as `aws` or `stripe` |
| `--language <LANGUAGE>` | Limit rows to a language such as `python`, `javascript`, `go`, or `java` |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: SecretsRecipesCmd::List](../../crates/lmprobe/src/cli.rs) and `SecretsRecipesCmd::List`; [dispatch: SecretsRecipesCmd::List](../../crates/lmprobe/src/cli.rs).
- [dispatch_secrets_recipes](../../crates/lmprobe/src/cli.rs).
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

Also check `lmprobe secrets-recipes list -h` and `lmprobe secrets-recipes list --help` after option changes.
