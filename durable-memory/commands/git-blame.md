# lmprobe git blame

Committed line attribution with source content.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git blame src/lib.rs --revision HEAD --json` |

## Options and output

FILE is repository-root-relative. --revision defaults HEAD; blame is for that committed tree, not uncommitted live modifications. Rows include line, original_line, commit, original_commit, author signature, path, content, boundary. Binary files error. Empty text returns no_match. Rename/copy handling uses libgit2 default blame behavior; no blame flags are passed through.

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
