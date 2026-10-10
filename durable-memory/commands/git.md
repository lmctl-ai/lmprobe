# lmprobe git

Read-only parsed repository inspection, using the existing libgit2 dependency.

## Common uses

| Case | Invocation |
| --- | --- |
| Working tree and index | `lmprobe git status --repo . --json` |
| Unstaged patch | `lmprobe git diff --json` |
| Staged patch | `lmprobe git diff --staged --json` |
| Revision comparison | `lmprobe git diff HEAD~1 HEAD --json` |
| Recent commits | `lmprobe git log --max-count 10 --json` |
| Historical file | `lmprobe git show HEAD --file README.md --json` |
| Quiet output file | `lmprobe git branch --all --outfile ../branches.json` |

## Transport and repository selection

--repo PATH defaults to . and is accepted before/after the child command.
Discovery finds the enclosing repository; operation paths are repository-root-relative
even when --repo is a subdirectory. Linked worktrees use their own index/HEAD.
Bare repositories support object/branch/remote reads; worktree operations error.
Metadata includes repo (Git admin directory), workdir (null for bare), head object ID,
head_name (branch shorthand or null), unborn, detached, bare, shallow.
--workspace is refused; choose one repo explicitly. This is CLI-only, not GraphQL.

Use --json for one versioned lmprobe.result.v1 result on stdout, including errors;
--outfile writes that same result atomically and keeps stdout quiet. --infile remains
a GraphQL document and cannot be combined with git. --format json remains a raw
lmprobe.git_VERB.v1 sensor envelope; piped auto output is NDJSON (rows + summary).
The commands expose named semantic options, not arbitrary Git argument passthrough.
No Git subprocess, hooks, credential helpers, index refresh/write or network operation
is launched. Existing Git history/AST sensors remain separate and unchanged.

Status/diff/log/branch/remote/ls-files/blame use rows; show/rev-parse use hierarchical
data. Raw envelopes include schema, exit_status, warnings, partial and metadata.
0 success, 1 empty selection, 2 error, 3 partial (e.g. missing upstream graph objects).
Clean status is success, not no_match. --exit-zero-on-no-match overrides exit 1 only.
Traversal/language/cache/glob/context/--limit options do not affect Git operations;
use --repo, Git --path where supported and log --max-count instead. --help and
[common.md](common.md) describe shared flags.

## Byte-preserving fields and safety

Paths, source lines, commit messages and signatures use UTF-8 strings when valid;
otherwise an object {bytes_hex:HEX} preserves the original bytes on Unix. Do not
treat that object as a lossy display string. Timestamps are Unix seconds with
offset_minutes; object IDs are full IDs. Remote URI credentials/query/fragment
are redacted. No changes are made to source/index/refs during inspection.

Outfile is reserved read-only before inspection and written afterward. Git output
refuses destinations under the Git admin/common directory (including linked
worktrees), existing files inside the worktree, and aliases of indexed files or
important metadata. Write outside the repo or to a new working-tree path. Reusing
a generated outfile inside the worktree is refused because it becomes an inspection
input; use an external destination for repeated calls. Parser/help errors preserve
repo metadata too. No files are created during status/diff selection.
If the repository cannot be opened, outfile routing is cancelled and the error
is returned on stdout; safety cannot be validated against unreadable/corrupt metadata.

## Implementation and tests

- crates/lmprobe/src/git_command.rs: clap commands, libgit2 data extraction.
- crates/lmprobe/src/cli.rs: Git dispatch and outfile protection, early parser guard.
- crates/lmprobe/src/program_output.rs: Git schema-specific rows/hierarchy adapters.
- crates/lmprobe/src/output.rs: Git-specific NDJSON rows; legacy sensors unchanged.
- crates/lmprobe/tests/git_json.rs: isolated repository fixtures and regressions.

Run cargo test -p lmprobe --locked --test git_json and binary help tests.
Compare status/diff behavior to official [Git status](https://git-scm.com/docs/git-status)
and [Git diff](https://git-scm.com/docs/git-diff); this wrapper exposes a documented
subset via libgit2, not every Git configuration/alias/extension or flag.

## Commands

- [status](git-status.md): Staged, worktree, untracked, ignored, and conflict states.
- [diff](git-diff.md): Structured file changes, hunks, lines, and counts.
- [log](git-log.md): Bounded commit records and parent/identity metadata.
- [show](git-show.md): Commit metadata and changes, or a historical file/blob.
- [branch](git-branch.md): Local/remote branches and upstream graph state.
- [remote](git-remote.md): Configured remote names, URLs, and refspecs.
- [rev-parse](git-rev-parse.md): Resolved object IDs and object types.
- [ls-files](git-ls-files.md): Tracked index entries including conflict stages.
- [blame](git-blame.md): Committed line attribution with source content.
