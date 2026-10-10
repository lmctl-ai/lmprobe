# lmprobe delete

Built-in regular-file deletion action for a selected file list. The default is
**preview only**; deletion requires `--force`. This is separate from AST `fix`.

## Use cases

| Case | Invocation |
| --- | --- |
| Preview selected stdin files | `lmprobe delete` |
| Execute selected stdin files | `lmprobe delete --force` |
| NUL-separated raw paths | `lmprobe delete --null --force` |
| Choose display columns | `lmprobe delete --display name,size` |

```bash
# Search/filter, preview the action, then explicitly execute it.
lmprobe find '\.lmctl$' . --older-than 1w | lmprobe delete
lmprobe find '\.lmctl$' . --older-than 1w | lmprobe delete --force

# Save a typed snapshot for review. Modified/replaced files are refused later.
lmprobe find '\.log$' . --older-than 1w --format json > /tmp/selected-files.json
lmprobe delete < /tmp/selected-files.json
lmprobe delete --force < /tmp/selected-files.json

# Raw shell-compatible lists, including filenames with newlines.
lmprobe find '\.log$' . --paths-only --null | lmprobe delete --null --force
fd -e lmctl -0 | lmprobe delete --null
```

## Inputs and options

`lmprobe delete [OPTIONS]` reads stdin to completion before any deletion.

| Option | Behavior |
| --- | --- |
| `--force` | Execute deletion; otherwise preview without filesystem writes. |
| `--null` | Interpret stdin as NUL-separated raw paths. |
| `--display <DISPLAY>` | Comma-separated name,lastmod,size,type; default name,lastmod,size. Presentation only. |
| `-h, --help` | Show usage and example. |

[Common options](common.md) also parse here; walk filters and --limit do not
re-select piped input. Apply them upstream to find. Forced actions reject
`--workspace`. JSON/NDJSON output uses `lmprobe.delete.v1` with mode, per-file
statuses, deleted_count, audit_log_path, warnings, exit_status, and partial.

## Implementation entry points

- [CLI](../../crates/lmprobe/src/cli.rs): Commands::Delete; stdin decoding, force/workspace gate, rendering.
- [Engine](../../crates/lmprobe-engine/src/file_actions.rs): read_selection, snapshot, validate, delete.
- [Schemas](../../crates/lmprobe-types/src/schema.rs): FileEntry, FileDeleteEntry, FileDeleteResult.
- [Rendering](../../crates/lmprobe/src/output.rs): render_delete_markdown, render_file, FileColumn.
- No GraphQL mutation is added for deletion; GraphQL find remains read-only.

## Boundaries and enhancement notes

Accepts complete find JSON envelopes, NDJSON path rows plus typed snapshots in the final summary, or framed object rows. Legacy JSON
path strings require a completion envelope/summary, newline raw paths, or --null raw paths. A JSON envelope must have a recognized completion status. NDJSON must end with exactly one valid summary, and its rows must match the summary snapshots. Truncated streams, malformed input, and
upstream error/partial envelopes are rejected before any deletion. Raw paths
are snapshotted at input time; use typed find JSON to retain earlier identities.
Absolute snapshot paths allow the consumer to run in a different directory.

Force validates the whole selection before the first delete and rechecks each
file just before unlinking. It refuses directories, special files, symlinks,
symlink parents (including parents resolved during the initial snapshot), and changed size/time/identity. Unix identity uses device/inode
and ctime; other platforms use creation time when available. Metadata checks
are best-effort checks, not filesystem locking against concurrent renames.
Quick-project prescan errors abort selection before any action. Duplicate absolute targets are acted on once. Deletion is nonrecursive and
irreversible; it intentionally needs neither Git nor a clean tree.

An XDG audit records the selection and pre-delete attempts before mutation.
Audit creation failure prevents deletion. A later delete failure is reported
per file; prior successful deletes stay deleted. Mixed outcomes return partial
exit 3; all failed/refused inputs return error exit 2. Empty selection exits 1.

## Verification

[file_actions tests](../../crates/lmprobe/tests/file_actions.rs) use only temporary
directories and isolated XDG cache paths. Cover age filtering, preview/force,
metadata, JSON/NDJSON/NUL pipelines, changed files, duplicates, invalid input,
symlink/directory refusal, and display. Engine unit tests cover exact age bounds.

```bash
cargo test -p lmprobe --locked --test file_actions
cargo test -p lmprobe-engine --locked file_actions::tests
```
