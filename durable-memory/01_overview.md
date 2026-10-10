# 01 — Overview

`lmprobe` is a structural code-search CLI. It reads the live filesystem, parses files via tree-sitter through `ast-grep`, and emits versioned JSON envelopes describing what it found.

## What lmprobe is, in one paragraph

A one-shot CLI with a generated command surface over a single binary. Every invocation parses the working tree on demand (or reads an XDG SQLite cache for git-history queries), prints a JSON envelope with `schema`, `hits`/`projects`/`paths`/etc., `warnings`, and `exit_status`, then exits. There is no daemon, no project index file in the working tree, no background process. The default build also includes the in-process GraphQL surface (`lmprobe query '<gql>'`); the long-running HTTP daemon (`serve`) is the only opt-in feature-gated layer (`--features serve`).

## Doctrine

These three rules constrain every feature decision:

1. **Lightweight one-shot CLI is the always-default.** Long-running surfaces (server, daemon, REPL) are opt-in layers, not the default. A user who runs `lmprobe search Foo .` should never accidentally start a daemon.
2. **Live filesystem is truth.** lmprobe never persists symbol indices to the working tree. The XDG SQLite cache (`$XDG_CACHE_HOME/lmprobe/history.db`) memoises **immutable git blobs only** (history, evolution, blame). Cache invariants: content-addressable, only-grows, no invalidation.
3. **Schema-first.** Every sensor emits a `serde::Serialize` envelope declared in `crates/lmprobe-types/src/schema.rs`. CLI markdown / JSON / GraphQL all render from the same shape. New fields are v1-additive (`#[serde(default)]`); breaking changes bump the schema string from `lmprobe.<verb>.v1` to `.v2` (and require explicit Lead arbitration).

If a feature proposal violates one of these, it's either wrong or it's an opt-in layer. See [12_optional_layers.md](12_optional_layers.md).

## Relationship to ast-grep

`lmprobe` lives in the `ast-grep` workspace, split across multiple crates as of Phase 5A:
- `crates/lmprobe/` — binary (clap CLI + GraphQL Query/Mutation + output rendering).
- `crates/lmprobe-types/` — D4 envelope shapes + typed scalars.
- `crates/lmprobe-lang-core/` — D5 capability trait definitions + `SgLang` re-export.
- `crates/lmprobe-registry/` — per-language dispatch fan-out (xtask-managed).
- `crates/lmprobe-engine/` — sensors (search, def, callers, history, …).
- `crates/lmprobe-lang-rust/` — Rust D5 trait impls (5A.3 first extraction).
- `xtask/` — `cargo xtask new-lang` / `check-lang` scaffolding tool.

Phase 5B is complete: all remaining languages are now in dedicated
`lmprobe-lang-<name>/` crates. See [18_phase5b_extraction_playbook.md](18_phase5b_extraction_playbook.md) for
the ongoing extension template.

It depends on the upstream `ast-grep` workspace:
- `ast_grep_core` — the pattern engine (`Pattern::try_new`, `replace_all`, `dfs`).
- `ast_grep_language` — the `SupportLang` enum and per-language tree-sitter grammars.
- `ast_grep_config` — relational rules (`has`, `inside`, `precedes`, `follows`).

lmprobe adds, on top of those:
- Full command inventory in the generated matrix (result-producing, interface, and GraphQL aliases) in this chapter, with safety/read/write classification.
- Evidence scanner contracts in [19_scanner_recipes.md](19_scanner_recipes.md).
- Schema-first envelopes ([08_envelope_schema.md](08_envelope_schema.md)).
- An XDG-located SQLite cache for git-history operations ([09_cache.md](09_cache.md)).
- Multi-language D5 capability traits ([11_extending.md](11_extending.md)).
- Multi-repo `--workspace` mode ([13_workspace.md](13_workspace.md)).
- Mutation-safety contracts for `trace` and `fix --apply` ([06_mutations.md](06_mutations.md)).

The upstream `ast-grep` CLI (`sg`) is not part of this workspace (see `ATTRIBUTION.md`); lmprobe's CLI surface is independent.

## Release-history context (high level)

lmprobe reached the current release line after the five core development phases plus follow-up work. This summary is here so a fresh reader can place a feature historically; the canonical record is the git log + `crates/lmprobe/ARCHITECTURE.md`.

- **Phase 0** — module split (D1), schemas (D4), language registry foundations (D5), XDG cache (D6), trace mutation-safety baseline (D7), per-language NameExtractor + CallGrammar.
- **Phase 1** — 8 module split fully wired; `KindTaxonomy` portable kinds; `--has`/`--inside`/`--precedes`/`--follows` relational filters; ImportGrammar; DFS-once kind prefilter perf win.
- **Phase 2** — 12 new project-marker discoverers; ImportGrammar broadened to 17 langs; DecorationGrammar (5 langs) + TraceTemplate (5 langs); LangTier (Code vs Data); TopLevelKinds + `--has-child` + `--outline` for data languages; tier-aware verdict scoring; declaration-kind audits for C/C++/CSS/HCL/Solidity.
- **Phase 3** — `lmprobe history <verb>` family (blame/introduced/commit/diff/churn/co-change/todos + evolution alias) backed by SQLite incremental commit-cache; symmetric graph primitives (callees/supertypes/subtypes/overrides/exports) on a new `InheritanceGrammar` trait; `impact <symbol>` BFS with depth+confidence DAG; `context <symbol>` composite bundle.
- **Phase 4** — `blob_signatures` cache populated (closing CC3 AST-aware perf debt); `trace --worktree` + `--keep-patch`; `fix --dry-run` + `fix --apply` with worktree isolation; cross-file rename `--follow`; `ast-diff <file> --from --to`; Mermaid + DOT graph rendering; `history blame --semantic` change-type taxonomy; multi-repo `--workspace`; faithful `enclosing_decl` extraction.

For per-phase test verdicts see [16_testing.md](16_testing.md) (`LANG_VERDICTS`).

## Post-battle-test usability hardening

Real-repo triage after the initial rollout tightened the agent-facing surface without changing the one-shot doctrine:

- `grep` / `find` accept repeatable path scopes, and `grep` warns on the POSIX `\|` regex foot-gun via `REGEX_LITERAL_PIPE_NO_MATCH`.
- `def` supports file-scoped lookup (`--this-file`) and full declaration bodies (`--body`) when skeleton output is too terse.
- `context` supports import-aware scoring (`--import-aware`) and `PATH:LINE` anchors, resolving the nearest containing declaration before building the context bundle.
- `discover` supports `--top-level-only` for compact monorepo maps.
- `search --strictness signature` rejects unsupported ellipsis patterns with `PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED` instead of panicking.
- GraphQL query exit classification is resolver-state based, so no-hit resolvers still report `no_match` even when the selection set omits `hits`.

## Sensor inventory at a glance

Result-producing commands, interface surfaces, and command aliases are generated and grouped below:

<!-- capability-matrix:start -->
Generated from CLI dispatch, GraphQL schema, and envelope schema declarations.

| Command | GraphQL field | Envelope schema | Output formats | Language scope | Read/Write |
| --- | --- | --- | --- | --- | --- |
| lmprobe discover | discover | lmprobe.discover.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe search | search | lmprobe.search.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe def | def | lmprobe.def.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe ref | — | lmprobe.ref.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe list-kinds | listKinds | lmprobe.list_kinds.v1 | Markdown, JSON, NDJSON, Sarif, Github | portable-kind taxonomy | Read |
| lmprobe callers | callers | lmprobe.callers.v1 | Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot | multi-language source / AST scans | Read |
| lmprobe imports | imports | lmprobe.imports.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe decorations | decorations | lmprobe.decorations.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe callees | callees | lmprobe.callees.v1 | Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot | multi-language source / AST scans | Read |
| lmprobe supertypes | supertypes | lmprobe.supertypes.v1 | Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot | multi-language source / AST scans | Read |
| lmprobe subtypes | subtypes | lmprobe.subtypes.v1 | Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot | multi-language source / AST scans | Read |
| lmprobe overrides | overrides | lmprobe.overrides.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe exports | exports | lmprobe.exports.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe impact | impact | lmprobe.impact.v1 | Markdown, JSON, NDJSON, Sarif, Github, Mermaid, Dot | multi-language source / AST scans | Read |
| lmprobe pr-triage | — | lmprobe.pr_triage.v1 | Markdown, JSON, NDJSON, Sarif, Github | GitHub API | Read |
| lmprobe context | context | lmprobe.context.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe evolution | evolution | lmprobe.evolution.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe trace | trace | lmprobe.trace.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git working tree (source mutation) | Write |
| lmprobe ast-diff | astDiff | lmprobe.ast_diff.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe diff-context | — | lmprobe.diff_context.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe resource-sites | resourceSites | lmprobe.resource_sites.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe async-leaks | asyncLeaks | lmprobe.async_leaks.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe crypto-sites | cryptoSites | lmprobe.crypto_sites.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe config-keys | configKeys | lmprobe.config_keys.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe secrets | secrets | lmprobe.secrets.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe secrets-recipes | — | lmprobe.secrets_recipes.v1<br>lmprobe.secrets_recipes.show.v1 | JSON | secret provider recipe catalog | Read |
| lmprobe secrets-sources | — | lmprobe.secrets_sources.v1<br>lmprobe.secrets_sources.show.v1 | JSON | secret provider recipe catalog | Read |
| lmprobe guard-sites | guardSites | lmprobe.guard_sites.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe openapi-facts | openapiFacts | lmprobe.openapi_facts.v1 | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| lmprobe route-anchors | routeAnchors | lmprobe.route_anchors.v1 | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| lmprobe http-client-calls | httpClientCalls | lmprobe.http_client_calls.v1 | Markdown, JSON, NDJSON, Sarif, Github | source code API facts | Read |
| lmprobe aws | — | lmprobe.aws_catalog.v1<br>lmprobe.aws_query.v1 | Markdown, JSON, NDJSON, Sarif, Github | Read-only AWS APIs | Read |
| lmprobe git | — | lmprobe.git_status.v1<br>lmprobe.git_diff.v1<br>lmprobe.git_log.v1<br>lmprobe.git_show.v1<br>lmprobe.git_branch.v1<br>lmprobe.git_remote.v1<br>lmprobe.git_rev_parse.v1<br>lmprobe.git_ls_files.v1<br>lmprobe.git_blame.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git repository records | Read |
| lmprobe edit | — | lmprobe.edit.v1 | Markdown, JSON, NDJSON, Sarif, Github | UTF-8 text files | Write |
| lmprobe fix | fix | lmprobe.fix.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git working tree (source mutation) | Write |
| lmprobe grep | grep | lmprobe.grep.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe find | find | lmprobe.find.v1 | Markdown, JSON, NDJSON, Sarif, Github | filesystem paths and metadata | Read / Write (--action delete --force) |
| lmprobe delete | — | lmprobe.delete.v1 | Markdown, JSON, NDJSON, Sarif, Github | filesystem paths and metadata | Write |
| lmprobe py-slice-candidates | — | lmprobe.py_slice_candidates.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe callsites | callsites | lmprobe.callsites.v1 | Markdown, JSON, NDJSON, Sarif, Github | multi-language source / AST scans | Read |
| lmprobe serve | N/A | N/A | N/A (GraphQL HTTP service) | GraphQL service | Interface |
| lmprobe query | custom query | lmprobe.query.v1 | JSON, NDJSON | in-process GraphQL | Read |
| lmprobe history | history | lmprobe.evolution.v1<br>lmprobe.history_blame.v1<br>lmprobe.history_churn.v1<br>lmprobe.history_co_change.v1<br>lmprobe.history_commit.v1<br>lmprobe.history_diff.v1<br>lmprobe.history_introduced.v1<br>lmprobe.history_todos.v1 | JSON | Git history | Read |
| lmprobe history blame | history.blame | lmprobe.history_blame.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history introduced | history.introduced | lmprobe.history_introduced.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history commit | history.commit | lmprobe.history_commit.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history diff | history.diff | lmprobe.history_diff.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history churn | history.churn | lmprobe.history_churn.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history co-change | history.coChange | lmprobe.history_co_change.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history todos | history.todos | lmprobe.history_todos.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |
| lmprobe history evolution | history.evolution | lmprobe.evolution.v1 | Markdown, JSON, NDJSON, Sarif, Github | Git history | Read |

<!-- capability-matrix:end -->

Each command’s envelope is `lmprobe.<noun>.v1` — see [08_envelope_schema.md](08_envelope_schema.md).

## Read next

If you're new, go to [02_invocation.md](02_invocation.md) for installation and the first invocation. If you came here looking for a specific feature, the [index](index.md) lists every chapter with a one-line description.
