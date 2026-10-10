# lmprobe git status

Staged, worktree, untracked, ignored, and conflict states.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git status --repo . --json` |

## Options and output

--ignored includes ignored files. Rows include path, index_status, worktree_status, index_old_path/index_new_path, worktree_old_path/worktree_new_path, untracked, ignored, conflicted, flags. Metadata clean ignores ignored-only entries. Clean status succeeds with empty rows; it is not no_match.

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
