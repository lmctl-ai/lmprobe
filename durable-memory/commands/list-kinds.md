# lmprobe list-kinds

List portable and parser-specific kinds for a language.

## Use cases

| Case | Invocation |
| --- | --- |
| Portable-to-tree-sitter mappings | `lmprobe list-kinds rust` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe list-kinds [OPTIONS] [LANG]
```

| Argument / option | Behavior |
| --- | --- |
| `[LANG]` | Parser language (rust, python, etc.) |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::ListKinds](../../crates/lmprobe/src/cli.rs) and `Commands::ListKinds`; [dispatch: Commands::ListKinds](../../crates/lmprobe/src/cli.rs).
- [taxonomy_for](../../crates/lmprobe/src/cli.rs).
- [Language taxonomy dispatch](../../crates/lmprobe-registry/src/lib.rs).
- [GraphQL resolver: list_kinds](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Uses registry taxonomy dispatch. Use the positional language. This command shadows the global lang argument; `list-kinds -l python` is rejected. Invalid/missing selection produces an error.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe list-kinds -h` and `lmprobe list-kinds --help` after option changes.
