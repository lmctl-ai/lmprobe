# lmprobe git branch

Local/remote branches and upstream graph state.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git branch --all --json` |

## Options and output

--all adds remote-tracking branches; default local only. Rows include name, current, remote, target, upstream, ahead, behind. Missing upstream counts are null. Graph computation failures retain rows plus diagnostics, partial:true and exit 3; never invent zero counts. This is listing only: no branch creation/deletion. Detached HEAD has no current branch.

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
