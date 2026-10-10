# 13 — Multi-repo workspace

`lmprobe --workspace <yaml>` runs read-only verbs across multiple roots in one invocation. Each hit is tagged with its `origin_root` so consumers can see which root produced it.

Mutation verbs (`fix --apply`, `trace`) refuse `--workspace` mode and exit 2 with `WORKSPACE_READ_ONLY`. The doctrine rationale: cross-root mutation in a one-shot invocation is too dangerous; staged per-root rewrite would need its own design.

## Workspace YAML

```yaml
roots:
  - path: /home/me/api
    alias: api          # optional; falls back to basename
  - path: /home/me/web
    alias: web
  - path: /home/me/shared/utils
```

Loaded by `WorkspaceCtx::load()` in `crates/lmprobe/src/workspace.rs`. Constraints:

- `roots:` must have ≥1 entry.
- Each `path:` is taken verbatim (canonicalised at fan-out time).
- `alias:` is optional. Missing alias → label is the path's basename.
- Duplicate labels (post-fallback) error out at load time: "duplicate workspace root label `<x>` — set distinct aliases".

## Invocation

```bash
lmprobe --workspace ws.yaml search --name UserID --limit 200
lmprobe --workspace ws.yaml --format json def Pattern | jq '.hits | group_by(.origin_root)'
```

Most read verbs work under `--workspace`. In the current tree, `search` is wired through the fan-out helper as the canonical demo path; other read verbs (`def`, `ref`, `callers`, etc.) currently walk the **first root only** when `--workspace` is set. Extending fan-out to the remaining read verbs is a follow-up item.

History / impact / context verbs do **not** fan out — their per-root semantics need design (cross-root DAG edges, per-root rename chains, per-root recent_history). Use them per-root by passing the root path directly.

## What fan-out does

When `--workspace` is set:

1. CLI dispatcher loads `WorkspaceCtx` once at start of `run()`.
2. For verbs wired through `fan_out_symbol_hits(ws, op)`:
   - Run `op(root_path)` for each root, returning the per-root `(Vec<SymbolHit>, Vec<Diagnostic>)`.
   - Tag each hit's `origin_root` with the root's label.
   - **Cross-root dedup** by canonical absolute path — if the same file appears under two roots (rare; user-error case), only the first is kept.
   - Merge into one `Vec<SymbolHit>` + accumulated diagnostics.
3. The merged envelope ships through the standard `render_by_format` dispatcher.

The dedup hash uses `std::fs::canonicalize(file_path)` so symlinks and `../` paths resolve to the same key.

## origin_root tagging

Every `SymbolHit` and `GrepHit` has an optional `origin_root: Option<String>` field (v1-additive — see [08_envelope_schema.md](08_envelope_schema.md)). Outside `--workspace` it's `None` and `#[serde(skip_serializing_if = "Option::is_none")]` keeps it out of the JSON. Under `--workspace` it's `Some(<label>)`.

```bash
$ lmprobe --workspace ws.yaml --format json search --name UserID --limit 50 \
  | jq '.hits | group_by(.origin_root) | map({root: .[0].origin_root, count: length})'

[
  {"root": "api", "count": 32},
  {"root": "web", "count": 18}
]
```

## Per-root cache namespacing

The `repo_commits` and `repo_head` tables in the SQLite cache key on `(repo_root, schema_version)` — Phase 3A's design already isolated per-root commit caches. Multi-repo invocations don't conflict; each root maintains its own cached HEAD + commit list.

`blob_signatures` rows (Phase 4A) are content-addressable by blob_sha — shared across roots when the same blob exists in two repos. No interference.

See [09_cache.md](09_cache.md) for the cache schema.

## Mutation refusal

```bash
$ lmprobe --workspace ws.yaml fix -p '...' -r '...' --apply
lmprobe: fix [WORKSPACE_READ_ONLY]: mutation verbs are not allowed in --workspace mode
$ echo $?
2

$ lmprobe --workspace ws.yaml trace 'fn $X' --exec 'true'
lmprobe: trace [WORKSPACE_READ_ONLY]: mutation verbs are not allowed in --workspace mode
$ echo $?
2
```

The dispatch arm calls `refuse_mutation_under_workspace(ws, verb)` before any file I/O. Bypass: drop `--workspace` and run the mutation per root.

## DUPLICATE_FILE_ACROSS_ROOTS

Reserved diagnostic for when two roots both contain the same canonical file. The current dedup logic silently keeps the first root's hit and drops the second; future work may emit this code as a warning. The diagnostic is documented in this tree but not yet emitted.

## Examples

### Two-root project (api + web)

```yaml
# ws.yaml
roots:
  - path: ~/work/api
    alias: api
  - path: ~/work/web
    alias: web
```

```bash
# Find every component named UserCard across both projects.
lmprobe --workspace ws.yaml --format json search --name UserCard \
  | jq -r '.hits[] | "\(.origin_root): \(.file_path):\(.line_start)"'

# Count pkg.json deps across both.
lmprobe --workspace ws.yaml --format json discover \
  | jq '[.projects[] | {root: .origin_root // "?", deps: .dependencies_count}]'
```

### Federated codebase audit

```yaml
# Federated audit of three microservices.
roots:
  - path: ~/work/payments-svc
    alias: payments
  - path: ~/work/orders-svc
    alias: orders
  - path: ~/work/users-svc
    alias: users
```

```bash
# Which services use react-query?
lmprobe --workspace ws.yaml --format json imports react-query \
  | jq '[.hits[] | .origin_root] | unique'

# Find every TODO across the federation in the last 90 days.
# (history verbs don't fan out today; loop in shell.)
for root in payments orders users; do
  cd ~/work/${root}-svc
  lmprobe history todos --since 90d --format ndjson | grep -v summary
done
```

## Read next

- [02_invocation.md](02_invocation.md) for the global `--workspace` flag.
- [09_cache.md](09_cache.md) for per-root cache namespacing.
- [06_mutations.md](06_mutations.md) for the mutation-safety contract.
