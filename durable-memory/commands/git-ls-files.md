# lmprobe git ls-files

Tracked index entries including conflict stages.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git ls-files --path src --json` |

## Options and output

Repeat --path for libgit2 pathspec matching; omit for all tracked index entries. Rows include path, oid, numeric Git file mode, stage (0 normal; 1 ancestor, 2 ours, 3 theirs). Paths with whitespace/newlines or non-UTF-8 bytes retain exact identity. Lists the live index, not a committed revision. Untracked paths are in status. Empty result is no_match.

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
