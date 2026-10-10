# lmprobe git rev-parse

Resolved object IDs and object types.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git rev-parse HEAD~1 --json` |

## Options and output

REVISION defaults HEAD. Hierarchical data contains revision, oid (full object ID), kind, and commit (peeled commit ID or null for non-commit objects). Works for annotated tags and Git revision suffixes. Root/admin/worktree details are shared metadata on every command; there are no passthrough --show-toplevel or shell argument switches.

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
