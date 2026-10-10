# Native GitHub search JSON

All five installed `gh search` commands already expose machine-readable JSON.
Use that capability directly for search records; no lmprobe search wrapper was
added solely to parse terminal tables. This keeps GitHub qualifiers and supported
fields under the upstream CLI rather than maintaining a duplicate parser.

Verified locally with gh 2.89.0 on 2026-10-05: repositories, issues, PRs, commits,
and code each returned a JSON array with one result from public cli/cli searches.
Checks used read-only requests and Python json.loads; no repository or GitHub
content was changed. The existing lmprobe `pr-triage` command remains available
for normalized PR/check/review evidence.

## Common calls

```bash
gh search repos widget --owner acme --limit 20 \
  --json fullName,url,description,language,updatedAt

gh search issues --repo cli/cli --state open --limit 20 \
  --json number,title,state,repository,url,labels

gh search prs --repo cli/cli --state open --limit 20 \
  --json number,title,state,repository,url,isDraft

gh search commits fix --repo cli/cli --limit 20 \
  --json sha,commit,repository,url

gh search code GitHub --repo cli/cli --filename README.md --limit 20 \
  --json path,repository,url,textMatches
```

`--json` takes a comma-separated field list, unlike lmprobe's boolean --json.
Available fields differ by command; inspect `gh search KIND --help` / JSON FIELDS.
`--limit` defaults 30. Repository and owner filters may be repeated where the
upstream CLI supports them. Use its existing named filters for state, language,
author, owner, labels and sort rather than interpolating qualifiers yourself.

Use one argv entry for each query word/qualifier or named filters. In the tested
build, a single argument containing `repo:cli/cli is:open` was incorrectly treated
as a repo qualifier containing spaces for issues/PRs; `--repo cli/cli --state open`
worked. Likewise commits used `fix --repo cli/cli`. Do not shell-split a quoted
search phrase: explicit flags avoid conflating phrase search with qualifiers.
For exclusions starting with a hyphen, place query arguments after `--`:

```bash
gh search issues --repo cli/cli --json number,title,url -- '-label:bug'
```

## Program consumption and limits

- On success, stdout is a JSON array of objects using gh field names (camelCase),
  including nested repository/commit/textMatches objects. Parse once with a JSON
  decoder. An empty successful search is `[]`; callers decide whether that means
  no_match. Native gh success is exit 0, including empty results.
- Capture exit code and stderr separately. Native gh auth/API/rate-limit errors
  are not lmprobe JSON envelopes. Do not interpret failed/empty stdout as `[]`.
- Native output has no lmprobe.result.v1 status/metadata/diagnostics wrapper and
  is not a local lmprobe GraphQL sensor. A future adapter should reuse gh --json
  and only add the transport contract/diagnostics; it should not scrape tables.
- `--jq` can reshape JSON, while `--template` can output arbitrary text; avoid
  either when the caller expects an unmodified result array.
- GH authentication/host behavior stays with gh (GH_HOST, GH_TOKEN, etc.). Never
  print, persist, or incorporate tokens in command recipes or error artifacts.
- Search limits are not total-match counts or complete coverage guarantees.
  The array does not expose the search API total_count/incomplete_results flags;
  bounded results must not be presented as proof that no further matches exist.
- Code search uses the upstream legacy API engine, not all browser code-search
  functionality; regex browser syntax is not generally available through it.

## Primary references

- [Search commands](https://cli.github.com/manual/gh_search)
- [Repository search](https://cli.github.com/manual/gh_search_repos)
- [Code search](https://cli.github.com/manual/gh_search_code)
- [Issue search](https://cli.github.com/manual/gh_search_issues)
- [PR search](https://cli.github.com/manual/gh_search_prs)
- [Commit search](https://cli.github.com/manual/gh_search_commits)

This is a native tool recipe, not an lmprobe command entry. Update it if adding
a transport adapter, changing the supported invocation strategy, or observing
different upstream field/exit behavior.
