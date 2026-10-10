# lmprobe git diff

Structured file changes, hunks, lines, and counts.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git diff --staged --json` |

## Options and output

Optional FROM and TO revisions: no revision compares index to worktree; FROM compares its tree to live tracked worktree including staged changes; FROM TO compares committed trees. --staged compares HEAD (or optional FROM) to index, including unborn HEAD; TO conflicts with staged. Repeat --path for libgit2 pathspecs. Rows contain old_path/new_path, status, object IDs, modes, binary, additions/deletions, hunks with lines: origin, old_line, new_line, content. Stats files/insertions/deletions are metadata. Renames are detected at default similarity thresholds. Untracked files are omitted; inspect status. Empty diff is no_match (exit 1).

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
