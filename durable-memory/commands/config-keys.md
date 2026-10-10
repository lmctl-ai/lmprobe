# lmprobe config-keys

Extract config keys and optional code references.

## Use cases

| Case | Invocation |
| --- | --- |
| Several explicit configs | `lmprobe config-keys package.json pyproject.toml` |
| Recursive config collection | `lmprobe config-keys .` |
| Exact dotted key | `lmprobe config-keys pyproject.toml --key tool.pytest.ini_options.addopts` |
| Lexical code anchors | `lmprobe config-keys . --grep-code --around 2 --parents --role` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe config-keys [OPTIONS] <PATHS>...
```

| Argument / option | Behavior |
| --- | --- |
| `<PATHS>...` | Scan root. |
| `--key <KEY>` | Keep only an exact config key path, for example tool.pytest.ini_options.addopts |
| `--grep-code` | Search code-tier files for escaped leaf-key text and attach matching anchors |
| `--around <AROUND>` | Include N source lines around matching code anchors |
| `--parents` | Attach enclosing structural declaration chain to code anchors |
| `--role` | Attach mechanical local role classification to code anchors |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::ConfigKeys](../../crates/lmprobe/src/cli.rs) and `Commands::ConfigKeys`; [dispatch: Commands::ConfigKeys](../../crates/lmprobe/src/cli.rs).
- [Engine: config_keys::config_keys](../../crates/lmprobe-engine/src/config_keys.rs).
- [Result schema: ConfigKeysResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: config_keys](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports local syntactic/lexical evidence, not ownership, control flow, authorization, compliance, or data-flow correctness. Preserve source anchors and diagnostics when extending coverage.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [config_keys](../../crates/lmprobe/tests/config_keys.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test config_keys
```

Also check `lmprobe config-keys -h` and `lmprobe config-keys --help` after option changes.
