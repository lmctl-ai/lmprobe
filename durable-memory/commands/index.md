# Command developer map

Start with the command you are changing. Each file contains use cases, every
local option/positional, implementation/schema/resolver links, behavior limits,
and focused verification entry points. [Common options](common.md) are shared
once. The actual clap definitions and implementation take precedence if notes drift.

Update the matching file whenever command behavior/options change. The help
coverage tests check that every command and option stays represented here.

[lmapi mode](lmapi.md) documents the lmapi (LLM-era API) surface: `lmprobe --schema`, the
published snake_case operations, mode detection, policy flags and error mapping.

| Command | Purpose |
| --- | --- |
| [aws](aws.md) | Cataloged read-only AWS queries with JSON request files and structured results. |
| [git](git.md) | Parsed read-only status, diff, log, show, branch, remote, revision, index, and blame. |
| [edit](edit.md) | Preview and apply literal/regex replacements and line edits. |
| [discover](discover.md) | Find project roots and nested projects. |
| [search](search.md) | Find declarations by name, kind, or AST pattern. |
| [def](def.md) | Find a symbol definition by exact name. |
| [ref](ref.md) | Find identifier references to a symbol. |
| [list-kinds](list-kinds.md) | List portable and parser-specific kinds for a language. |
| [callers](callers.md) | Find calls to a function or method. |
| [imports](imports.md) | Find imports of a module. |
| [decorations](decorations.md) | Find uses of an annotation, decorator, or attribute. |
| [evolution](evolution.md) | Show a symbol's changes through Git history. |
| [callees](callees.md) | Find functions or methods called by a symbol. |
| [supertypes](supertypes.md) | Find a type's parents or implemented interfaces. |
| [subtypes](subtypes.md) | Find subtypes or implementers of a type. |
| [overrides](overrides.md) | Find declarations marked as overriding a method. |
| [exports](exports.md) | Find exports of a symbol. |
| [impact](impact.md) | Walk callers or callees to estimate change impact. |
| [pr-triage](pr-triage.md) | Show GitHub pull request checks, reviews, and comments. |
| [context](context.md) | Gather a symbol's definition, calls, imports, and history. |
| [history](history.md) | Explore Git blame, commits, churn, and symbol history. |
| [trace](trace.md) | Inject logs, run a command, then restore source files. |
| [ast-diff](ast-diff.md) | Compare declarations in a file between Git revisions. |
| [diff-context](diff-context.md) | Show changed Git lines with source or AST context. |
| [resource-sites](resource-sites.md) | Find acquire/release anchors and local context. |
| [async-leaks](async-leaks.md) | Find JS/TS teardown calls and await/return evidence. |
| [crypto-sites](crypto-sites.md) | Find Go crypto algorithm uses and nearby FIPS evidence. |
| [config-keys](config-keys.md) | Extract config keys and optional code references. |
| [secrets](secrets.md) | Scan files or Git history for potential secrets. |
| [secrets-recipes](secrets-recipes.md) | List or show credential sink query recipes. |
| [secrets-sources](secrets-sources.md) | List or show credential source query recipes. |
| [guard-sites](guard-sites.md) | Find sensitive actions and nearby guard anchors. |
| [openapi-facts](openapi-facts.md) | Extract HTTP methods and paths from OpenAPI files. |
| [route-anchors](route-anchors.md) | Find route declarations and literal HTTP paths. |
| [http-client-calls](http-client-calls.md) | Find HTTP client calls with literal URLs. |
| [fix](fix.md) | Preview AST replacements; use --apply to write changes. |
| [grep](grep.md) | Search file contents with a regular expression. |
| [delete](delete.md) | Preview or execute regular-file deletion from stdin. |
| [find](find.md) | Find paths with metadata, age filters, and preview-first actions. |
| [py-slice-candidates](py-slice-candidates.md) | Find Python slice/unsliced-variable forwarding suspects. |
| [callsites](callsites.md) | Extract Python call arguments for a callee. |
| [serve](serve.md) | Start the optional GraphQL HTTP server. |
| [query](query.md) | Run a GraphQL document or recipe in-process. |
| [history blame](history-blame.md) | Show logic-changing commits for a symbol or source line. |
| [history introduced](history-introduced.md) | Find the first AST appearance of a symbol. |
| [history commit](history-commit.md) | Find commits by message or code pattern. |
| [history diff](history-diff.md) | Find historical code changes matching an AST pattern. |
| [history churn](history-churn.md) | Rank files by commit count in a time window. |
| [history co-change](history-co-change.md) | Rank files that often change with a given file. |
| [history todos](history-todos.md) | Find TODO/FIXME/XXX/HACK markers added in history. |
| [history evolution](history-evolution.md) | Show a symbol's changes through Git history. |
| [secrets-recipes list](secrets-recipes-list.md) | List credential sink recipes. |
| [secrets-recipes show](secrets-recipes-show.md) | Show a generated credential sink GraphQL recipe. |
| [secrets-sources list](secrets-sources-list.md) | List credential source recipes. |
| [secrets-sources show](secrets-sources-show.md) | Show a generated credential source GraphQL recipe. |
| [help](help.md) | Display root or nested command help. |
