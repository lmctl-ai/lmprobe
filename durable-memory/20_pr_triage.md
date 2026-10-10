# PR Triage

`lmprobe pr-triage <github-pr-url>` gathers GitHub pull request status evidence through the GitHub CLI:

```bash
lmprobe pr-triage https://github.com/<owner>/<repo>/pull/<number>
lmprobe --format json pr-triage --since all https://github.com/<owner>/<repo>/pull/<number>
```

This first slice is metadata-only. It parses the PR URL, runs:

```bash
gh pr view <url> --json number,url,title,state,isDraft,mergeable,reviewDecision,author,statusCheckRollup,comments,reviews,reviewThreads
```

If the installed `gh` does not support `reviewThreads`, `pr-triage` retries
with the same field set minus `reviewThreads`, returns the available status,
check, issue-comment, and review evidence, and adds a
`GITHUB_CLI_REVIEW_THREADS_UNSUPPORTED` warning. Inline review-thread comments
are optional evidence because `gh` exposes that field inconsistently across
versions.

and returns an `lmprobe.pr_triage.v1` envelope with:

- `target`: normalized owner, repo, number, and URL.
- `status`: title, author, state, draft state, mergeability, and review decision.
- `check_summary` and `checks`: passing, failing, pending, skipped/neutral check evidence.
- `evidence`: issue comments, PR reviews, and inline review comments.

`--since last-author-comment` is the default and keeps evidence after the PR author's latest issue comment. `--since all` includes all fetched comment/review evidence. `--compact` shortens evidence excerpts for terminal scans.

Action terms are evidence-only string matches such as `changes requested`, `failed`, `DCO`, `CLA`, `nitpick`, `does not verify`, and `would still pass`. The command does not infer correctness, recommend reviewers, or compute regression impact yet.

For tests or controlled environments, set `LMPROBE_GH=/path/to/gh-compatible-wrapper`.
