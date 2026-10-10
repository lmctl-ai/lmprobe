# lmprobe git remote

Configured remote names, URLs, and refspecs.

| Case | Invocation |
| --- | --- |
| Common use | `lmprobe git remote --json` |

## Options and output

No local flags. Rows include name, effective libgit2 fetch_url and push_url (fallback to fetch URL), configured_fetch_urls and configured_push_urls arrays preserving every configured value, fetch_refspecs, push_refspecs. Empty configured push arrays mean Git uses fetch URLs; singular effective fields may differ from configuration because of URL rewriting/selection. URI userinfo and query/fragment are removed at the byte boundary before UTF-8/hex encoding. Non-UTF-8 URLs and refspecs retain bytes_hex objects. Local/SCP-style URLs retain literal paths and usernames. No network connection, credential helper, remote add/remove or fetch occurs. Unrepresentable remote names produce partial diagnostics rather than silent skipping.

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
