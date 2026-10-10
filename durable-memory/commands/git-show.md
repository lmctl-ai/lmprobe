# lmprobe git show

Commit metadata and changes, or a historical file/blob.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git show HEAD --file README.md --json` |

## Options and output

REVISION defaults HEAD. --file is a repository-root-relative path in the selected tree. Hierarchical data contains historical file content (UTF-8 string or bytes_hex object), binary flag, file mode, oid, revision; tree-ish revisions work with --file. A blob object/revision can be shown directly. Commit show contains commit info, files with structured diff hunks, stats, and comparison:first_parent; root commit compares to empty tree. Merge commits compare to the first parent. No combined merge diff or tag-message presentation.

--repo PATH selects a repository (default .), before or after the child command.
--help shows usage. Shared transport flags are in [common.md](common.md); full
Git output, read-only boundaries, byte encoding, and outfile protection are in
[git.md](git.md). --json / --outfile use the existing versioned program envelope.

## Developer entry points

crates/lmprobe/src/git_command.rs: GitCommand and corresponding extraction helper.
crates/lmprobe/src/cli.rs: Commands::Git dispatch and protect_git_output.
crates/lmprobe/src/program_output.rs: schema adapter.
crates/lmprobe/tests/git_json.rs: temp repository checks. No new dependencies.

Run cargo test -p lmprobe --locked --test git_json.
