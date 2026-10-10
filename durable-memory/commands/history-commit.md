# lmprobe history commit

Find commits by message or code pattern.

## Use cases

| Case | Invocation |
| --- | --- |
| All commits in a window | `lmprobe history commit --since 30d .` |
| Message or code pattern | `lmprobe history commit -p 'fix' .` |
| Author filter | `lmprobe history commit --author 'Alice' --since 2w .` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe history commit [OPTIONS] [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `-p, --pattern <PATTERN>` | Match commit messages or code changes |
| `--since <SINCE>` | Time window, such as 30d, 12h, or 2w |
| `--author <AUTHOR>` | Filter commits by author text |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: HistoryCmd::Commit](../../crates/lmprobe/src/cli.rs) and `HistoryCmd::Commit`; [dispatch: HistoryCmd::Commit](../../crates/lmprobe/src/cli.rs).
- [Engine: history_sensors::history_commit](../../crates/lmprobe-engine/src/history_sensors.rs).
- [Result schema: HistoryCommitResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: commit](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Message matches are case-insensitive substrings. Code fallback uses AST matching for recognized files and substring matching for unrecognized files. No pattern means all commits in the window. Uses shared cache even with global --no-cache.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe history commit -h` and `lmprobe history commit --help` after option changes.
