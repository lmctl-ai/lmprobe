# Filesystem selection and traversal predicates

`find` separates **filter** (which entries become results) from **prune**
(which directories become terminal traversal boundaries). Pruned directories
remain eligible results. Pruning is evaluated independently of result matching:
a directory that fails filter still prevents descent when it matches prune.
The same prune predicate applies to both ignored-path counting walks.

## Repository identity example

Repository name means the configured Git remote URL's repository component,
not the local checkout folder name. This finds a renamed checkout too:

```bash
lmprobe find '.*' ~/repos \
  --filter '{"kind":"directory","git_remote":{"host":"github.com","owner":"acme","repo":"widget"}}' \
  --prune '{"has_entry":".git"}' --display name
```

Equivalent GraphQL (path is a filesystem path; `~` inside a query is not
expanded by the shell):

```graphql
{
  find(
    pattern: ".*"
    path: "/home/me/repos"
    filter: {
      kind: DIRECTORY
      gitRemote: { host: "github.com", owner: "acme", repo: "widget" }
    }
    prune: { hasEntry: ".git" }
  ) {
    paths
    partial
    warnings { code message }
    exitStatus
  }
}
```

The `gitRemote` clause requires a direct `.git` marker and matches any configured
remote unless `remote` selects a remote name (origin/upstream/etc.). Worktree
`.git` files are supported. A checkout folder merely called widget is not enough.
Git metadata is read locally without network access or walking Git object files.
Configured URL identity supports HTTPS/SSH/git URLs, SCP-style SSH URLs, and
local paths. It does not resolve SSH host aliases or contact a Git hosting API.

## Predicate clauses

CLI flags take JSON (snake_case); GraphQL inputs use camelCase. Clauses in a
single object combine with AND. Unknown JSON fields, invalid regexes, or invalid
child names are errors before scanning.

| CLI JSON | GraphQL | Meaning |
| --- | --- | --- |
| `kind` | `kind` | file, directory, symlink, other; GraphQL enum values uppercase. |
| `name_regex` | `nameRegex` | Regex on the local entry basename, **not** repository name. |
| `path_regex` | `pathRegex` | Regex on the walked path. |
| `has_entry` | `hasEntry` | Directory has this direct child entry; accepts a filename such as .git, not a relative path. |
| `git_remote` | `gitRemote` | Directory's Git metadata has a matching configured remote. |
| `all` | `all` | Every nested predicate matches. |
| `any` | `any` | At least one nested predicate matches. |
| `not` | `not` | Negate a nested predicate. |

Git remote clauses `host`, `owner`, `repo`, and `remote` combine with AND within
one remote. Host/owner/repo comparisons ignore ASCII case; the remote name is
exact. URL usernames, credentials, ports, URI query strings/fragments, and .git suffixes are
not part of the repository name. Literal #/? characters in local and SCP paths
are preserved, and local paths with colons are not mistaken for SCP URLs. These identities are not emitted in file
snapshots; only matching filesystem paths are returned.

An empty predicate `{}` matches everything. `all: []` is true; `any: []` is
false. Regexes use Rust syntax and are case-sensitive unless the regex itself
uses `(?i)`. The existing -i option affects the primary find path pattern.
Predicates allow up to 32 nested levels and 256 nodes.

## Other traversal examples

```bash
# List project roots; do not enter projects marked by either marker.
lmprobe find '.*' ~/repos \
  --filter '{"kind":"directory","any":[{"has_entry":".git"},{"has_entry":"package.json"}]}' \
  --prune '{"any":[{"has_entry":".git"},{"has_entry":"package.json"}]}'

# Stop at directories named vendor or generated, regardless of result pattern.
lmprobe find '\.rs$' . --prune '{"name_regex":"^(vendor|generated)$"}'
```

Prune acts on directories only, including an explicitly supplied directory root.
Basename rules resolve special . and .. root spellings to the actual entry name
while returned paths and path_regex keep their walked spelling.
It suppresses all children, not just the marker: .git and sibling source/cache
entries within the checkout are not traversed. Siblings of the checkout remain
searchable. Symlink directories are not followed by the walker. Marker matching
uses entry existence (including a symlink marker), not its file contents.

Prune cannot combine with quick-projects: that separate prescan rewrites the
search roots. Filter may combine with quick-projects. No predicate changes the
existing ignore/glob/default-exclusion rules. Use --no-ignore when ignored
checkout directories themselves need to be discoverable.

Predicate I/O errors become FILE_METADATA_FAILED and partial=true. An unknown
prune decision conservatively stops that directory. Forced file actions still
refuse incomplete selections. GraphQL invocation status/diagnostics are captured before projection; program
JSON exposes them consistently. See [program-json.md](program-json.md).

## Implementation and verification

- [FilePredicateInput, GitRemotePredicateInput, FileKind](../crates/lmprobe-types/src/schema.rs): pure shared input types.
- [FilePredicate](../crates/lmprobe-engine/src/file_predicate.rs): bounded compilation, Boolean matching, direct entry probes, remote identity.
- [find_many_with_predicates, find_paths, count_gitignore_excluded_with_prune](../crates/lmprobe-engine/src/sensors.rs): compile once; directory Skip occurs after considering the directory itself.
- [Commands::Find, parse_file_predicate, Query::find](../crates/lmprobe/src/cli.rs): CLI JSON and GraphQL shared input.
- [filesystem_predicates tests](../crates/lmprobe/tests/filesystem_predicates.rs): renamed checkout, wrong-folder-name trap, nonmatching checkout pruning, worktrees, generic markers/Booleans, invalid input, and unreadable pruned children.

```bash
cargo test -p lmprobe --locked --test filesystem_predicates
cargo test -p lmprobe --locked --test file_actions --test graphql_surface_equality
```
