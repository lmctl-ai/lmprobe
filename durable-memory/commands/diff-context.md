# lmprobe diff-context

Show changed Git lines with source or AST context.

## Use cases

| Case | Invocation |
| --- | --- |
| Changed new-side lines | `lmprobe diff-context --from HEAD~1 .` |
| Source and declaration context | `lmprobe diff-context --from main --to HEAD --around 3 --parents --body src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe diff-context [OPTIONS] --from <FROM> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `--from <FROM>` | Base Git revision |
| `--to <TO>` | Target Git revision [default: HEAD] |
| `--around <AROUND>` | Include N source lines around each changed-line anchor |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::DiffContext](../../crates/lmprobe/src/cli.rs) and `Commands::DiffContext`; [dispatch: Commands::DiffContext](../../crates/lmprobe/src/cli.rs).
- [Engine: diff_context::diff_context](../../crates/lmprobe-engine/src/diff_context.rs).
- [Result schema: DiffContextResult](../../crates/lmprobe-types/src/schema.rs).
- No direct same-name GraphQL resolver; inspect query composition before extending surface parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports changed-line evidence, not semantic conclusions.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [diff_context](../../crates/lmprobe/tests/diff_context.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test diff_context
```

Also check `lmprobe diff-context -h` and `lmprobe diff-context --help` after option changes.
