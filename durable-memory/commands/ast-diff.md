# lmprobe ast-diff

Compare declarations in a file between Git revisions.

## Use cases

| Case | Invocation |
| --- | --- |
| Parent revision vs HEAD | `lmprobe ast-diff src/lib.rs --from HEAD~1` |
| Explicit revision pair | `lmprobe ast-diff src/lib.rs --from main --to feature` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe ast-diff [OPTIONS] --from <FROM> <FILE>
```

| Argument / option | Behavior |
| --- | --- |
| `<FILE>` | Required command input; see use cases above. |
| `--from <FROM>` | Base Git revision |
| `--to <TO>` | Target Git revision [default: HEAD] |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::AstDiff](../../crates/lmprobe/src/cli.rs) and `Commands::AstDiff`; [dispatch: Commands::AstDiff](../../crates/lmprobe/src/cli.rs).
- [Engine: ast_diff_sensor::ast_diff](../../crates/lmprobe-engine/src/ast_diff_sensor.rs).
- [Result schema: AstDiffResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: ast_diff](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Coverage is capability-based; unsupported language behavior must stay explicit through diagnostics.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [relation_ast_diff_regressions](../../crates/lmprobe/tests/relation_ast_diff_regressions.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test relation_ast_diff_regressions
```

Also check `lmprobe ast-diff -h` and `lmprobe ast-diff --help` after option changes.
