# lmprobe async-leaks

Find JS/TS teardown calls and await/return evidence.

## Use cases

| Case | Invocation |
| --- | --- |
| Single teardown name | `lmprobe async-leaks src/ --call close` |
| Several names with context | `lmprobe async-leaks src/ --call close --call dispose --around 2 --parents --body` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe async-leaks [OPTIONS] --call <CALL> <PATH>
```

| Argument / option | Behavior |
| --- | --- |
| `<PATH>` | Scan root. |
| `--call <CALL>` | Call name to anchor, e.g. `stop`; repeat for multiple names |
| `--around <AROUND>` | Include N source lines around call anchors |
| `--parents` | Attach enclosing structural declaration chain |
| `--body` | Attach innermost enclosing structural body |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::AsyncLeaks](../../crates/lmprobe/src/cli.rs) and `Commands::AsyncLeaks`; [dispatch: Commands::AsyncLeaks](../../crates/lmprobe/src/cli.rs).
- [Engine: async_leaks::async_leaks](../../crates/lmprobe-engine/src/async_leaks.rs).
- [Result schema: AsyncLeaksResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: async_leaks](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Reports local syntactic/lexical evidence, not ownership, control flow, authorization, compliance, or data-flow correctness. Preserve source anchors and diagnostics when extending coverage.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [async_leaks](../../crates/lmprobe/tests/async_leaks.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test async_leaks
```

Also check `lmprobe async-leaks -h` and `lmprobe async-leaks --help` after option changes.
