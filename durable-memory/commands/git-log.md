# lmprobe git log

Bounded commit records and parent/identity metadata.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git log --max-count 10 --json` |

## Options and output

REVISION defaults HEAD and accepts commit/tag expressions or A..B ranges. --max-count defaults 20 and must be positive. Commits ordered topologically then by time. Rows include id, short_id (display only), tree, parents, summary, message, author and committer (name, email, time Unix seconds, offset_minutes). Author/committer timestamps are distinct. Metadata truncated indicates additional commits beyond the cap. Unborn HEAD returns no_match; invalid revisions error. Global --limit is not used.

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
