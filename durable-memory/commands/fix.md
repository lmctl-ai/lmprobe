# lmprobe fix

Preview AST replacements; use --apply to write changes.

## Use cases

| Case | Invocation |
| --- | --- |
| Read-only preview | `lmprobe fix -p 'console.log($X)' -r 'logger.info($X)' -l javascript src/` |
| Audited apply in a clean repo | `lmprobe fix -p 'console.log($X)' -r 'logger.info($X)' -l javascript . --apply` |
| Cross-language pattern | `lmprobe fix -p 'console.log($X)' -r 'logger.info($X)' src/` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe fix [OPTIONS] --pattern <PATTERN> --replacement <REPLACEMENT> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `[PATH]` | [default: .] |
| `-p, --pattern <PATTERN>` | Source pattern (ast-grep metavars allowed: `$X`, `$$$`) |
| `-r, --replacement <REPLACEMENT>` | Replacement template referencing the pattern's metavars |
| `-l, --lang <LANG>` | Parser language (rust, python, etc.) |
| `--apply` | Write replacements via an audited worktree patch (default: dry run) |
| `--force` | Bypass the dirty-tree or non-Git safety check |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Fix](../../crates/lmprobe/src/cli.rs) and `Commands::Fix`; [dispatch: Commands::Fix](../../crates/lmprobe/src/cli.rs).
- [Engine: fix_sensor::fix_dry_run](../../crates/lmprobe-engine/src/fix_sensor.rs).
- [Result schema: FixResult](../../crates/lmprobe-types/src/schema.rs).
- [Write path: fix_apply](../../crates/lmprobe-engine/src/fix_sensor.rs).
- [GraphQL resolver: fix](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

For literal/regex replacement across arbitrary UTF-8 files, including comments
and strings, use [edit](edit.md). `fix` remains AST-based and uses HEAD worktrees;
`edit` supports explicit live-file edits and sequential text/line scripts.

Preview is read-only. --apply provisions a detached HEAD worktree, writes an audit/patch, then applies the patch to the user tree; --force bypasses dirty-tree refusal but does not make the worktree include uncommitted source. --workspace refuses apply.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [smoke](../../crates/lmprobe/tests/smoke.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test smoke
```

Also check `lmprobe fix -h` and `lmprobe fix --help` after option changes.
