# lmprobe find

Find paths by regular expression with filesystem metadata, age filtering, and preview-first actions.

## Use cases

| Case | Invocation |
| --- | --- |
| Path regex | `lmprobe find '\.rs$\|\.ts$' src/` |
| Several roots | `lmprobe find 'test' --path src --path tests` |
| Raw paths for shell loops | `lmprobe find '\.rs$' . --paths-only` |
| Safe NUL-delimited paths | `lmprobe find '\.rs$' . --paths-only --null` |
| Age filter before result limit | `lmprobe find '\.lmctl$' . --older-than 1w --limit 10` |
| Display selected columns | `lmprobe find '\.lmctl$' . --display name,size` |
| Preview delete | `lmprobe find '\.lmctl$' . --older-than 1w --action delete` |
| Execute delete after review | `lmprobe find '\.lmctl$' . --older-than 1w --action delete --force` |
| Find by remote repository identity | `lmprobe find '.*' . --filter '{"kind":"directory","git_remote":{"owner":"acme","repo":"widget"}}' --prune '{"has_entry":".git"}'` |
| Narrow project scan | `lmprobe find 'package.json$' --quick-projects ~/repos` |

Recipes use illustrative source paths/symbols. Run from the relevant project;
quote AST metavariables so the shell does not expand `$NAME`.

## Inputs and options

```text
lmprobe find [OPTIONS] <PATTERN> [PATH]
```

| Argument / option | Behavior |
| --- | --- |
| `<PATTERN>` | Required command input; see use cases above. |
| `[PATH]` | Single compatibility path. When omitted, `find` defaults to `.` unless one or more `--path <PATH>` flags are set |
| `--path <PATH>` | Additional scan roots (repeatable); defaults to . when no paths are given |
| `--quick-projects` | Narrow scans to npm roots or directories with at least five Perl files |
| `--paths-only` | Print raw paths for shell pipelines instead of an output envelope |
| `--null` | Separate --paths-only output with NUL bytes for xargs -0 |
| `--display <DISPLAY>` | name,lastmod,size,type (comma-separated); default name,lastmod,size. Human display only. |
| `--older-than <OLDER_THAN>` | Modification age strictly greater than a positive integer duration in s,m,h,d,w. |
| `--action <ACTION>` | display or delete; default display. Delete previews until --force is supplied. Conflicts with --paths-only. |
| `--filter <FILTER>` | JSON filesystem selection predicate: kind, name_regex, path_regex, has_entry, git_remote, all/any/not. |
| `--prune <PRUNE>` | JSON predicate: matched directories remain results but none of their children are traversed. Applies to ignored counts too; conflicts with quick-projects. |
| `--force` | Execute --action delete, after validating the complete selection. |
| `-h, --help` | Print help (see a summary with '-h') |

All inherited flags are catalogued in [common.md](common.md). These flags are
parsed globally; their operational support depends on the dispatch path below.

## Implementation entry points

- [CLI arguments: Commands::Find](../../crates/lmprobe/src/cli.rs) and `Commands::Find`; [dispatch: Commands::Find](../../crates/lmprobe/src/cli.rs).
- [Engine: sensors::find_many_with_predicates](../../crates/lmprobe-engine/src/sensors.rs).
- [Result schema: FindResult](../../crates/lmprobe-types/src/schema.rs).
- [GraphQL resolver: find](../../crates/lmprobe/src/cli.rs). Check CLI/GraphQL input and envelope parity.
- [Output rendering](../../crates/lmprobe/src/output.rs); language-specific behavior belongs in [registry capability dispatch](../../crates/lmprobe-registry/src/lib.rs).

## Boundaries and enhancement notes

Default human output is tab-separated path, UTC last-modification time, and byte size (directories/symlinks show - for size). --display name restores the old path-only display. JSON retains paths and additively includes typed files; NDJSON retains legacy path-string rows and includes typed files in its summary. Regex is matched against path strings, not globs. --paths-only overrides --format and removes the envelope; --null only affects raw path output. Warnings remain on stderr for raw paths. Multiple roots use --path.

Find also lists matching directories/symlinks for backward-compatible paths; the delete action refuses a whole batch containing nonregular entries. Age filtering happens before --limit. Forced deletion refuses incomplete scans and workspace mode. See [delete](delete.md) for input formats, identity checks, auditing, and failure behavior.

Quick-project discovery aborts on I/O errors before producing a selection. The query wrapper now captures status/diagnostics before projection; [program JSON](../program-json.md) exposes them consistently.

See [filesystem predicates](../filesystem-predicates.md) for generic traversal expressions and remote repository identity. Repository names come from Git remotes; name_regex refers only to local basenames. Prune is independent of filter, so even nonmatching checkout roots can stop traversal.

GraphQL find accepts filter/prune typed predicates and olderThan and exposes files metadata alongside paths; it performs no actions.

Preserve versioned envelope meanings; optional fields may be added within v1.
See [schema](../08_envelope_schema.md) and [architecture](../../crates/lmprobe/ARCHITECTURE.md)
for shared contracts. Add a focused fixture regression for changed behavior.

## Verification

- [filesystem_predicates](../../crates/lmprobe/tests/filesystem_predicates.rs)
- [file_actions](../../crates/lmprobe/tests/file_actions.rs)
- [missing_explicit_paths](../../crates/lmprobe/tests/missing_explicit_paths.rs)
- [multiple_positional_paths_diagnostic](../../crates/lmprobe/tests/multiple_positional_paths_diagnostic.rs)
- [find_path_ranking](../../crates/lmprobe/tests/find_path_ranking.rs)

```bash
cargo test -p lmprobe --locked --bin lmprobe help_tests
cargo test -p lmprobe --locked --test missing_explicit_paths
cargo test -p lmprobe --locked --test multiple_positional_paths_diagnostic
```

Also check `lmprobe find -h` and `lmprobe find --help` after option changes.
