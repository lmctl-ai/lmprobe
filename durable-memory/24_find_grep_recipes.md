# 24 - Find/Grep Recipes For LLM Agents

This chapter answers the practical question: when an agent would normally
chain `find`/`fd` and `grep`/`rg`, what is the lmprobe equivalent?

Use `find` for path regexes, `grep` for line-oriented content regexes, and
GraphQL when several roots or evidence streams should be collected in one
process. Use `search`/`def`/`ref` instead of `grep` when the question is about
symbols rather than raw text.

## Capability Summary

| Need | lmprobe today |
|------|---------------|
| File path match | `lmprobe find '<path-regex>' <path>` |
| Raw content match | `lmprobe grep '<content-regex>' <path>` |
| OR over terms | Rust regex alternation: `foo|bar` |
| Multiple roots | CLI repeated `--path`; GraphQL `paths: [...]` on `find` and `grep` |
| Content plus context | `grep -C N`, `grep --parents`, `grep --body`, `grep --role` |
| Dirs with marker AND content | GraphQL `grepDirsWith(markerPattern, pattern, path, paths)` |
| Project map | `discover(path: ..., topLevelOnly: true)` or `lmprobe discover --top-level-only` |
| Symbol definition/reference | `def`, `ref`, `search --name`, not raw `grep` |

The interface is intentionally close to common tools:

- `find` ~= `fd`/`find` over path strings.
- `grep` ~= `rg`/`git grep` over text lines, using Rust regex syntax.
- `ref` ~= identifier-aware grep.
- `search` ~= AST structural grep.

The main difference from POSIX grep is regex flavor: alternation is bare `|`.
`TODO|FIXME` is OR; `TODO\|FIXME` searches for a literal pipe and emits
`REGEX_LITERAL_PIPE_NO_MATCH` when it likely came from POSIX muscle memory.

## CLI Recipes

Find package markers:

```bash
lmprobe --format json find '(^|[\\/])package\.json$' .
lmprobe --format json find '(^|[\\/])(Cargo\.toml|pyproject\.toml|go\.mod)$' .
```

Find source files by extension:

```bash
lmprobe --format json find '\.(rs|ts|tsx)$' src
lmprobe --format json find '(^|/)route(s)?\.(ts|tsx|py)$' .
```

Search raw content:

```bash
lmprobe --format json grep 'login|signin|authenticate' .
lmprobe --format json grep -i 'todo|fixme' .
lmprobe --format json grep 'process\.env\.[A-Z0-9_]+' --path apps/web --path packages/shared
```

Search content with local context:

```bash
lmprobe --format json grep -C 3 'login' apps/web
lmprobe --format json grep --parents --body 'login' apps/web
lmprobe --format json grep --role 'base_config_key' src
```

Restrict by file type or glob:

```bash
lmprobe --format json grep 'login' . --type typescript
lmprobe --format json grep 'login' . -G 'src/**/*.ts' --exclude-glob '**/*.test.ts'
```

Prefer symbol-aware verbs for code identifiers:

```bash
lmprobe --format json def LoginService .
lmprobe --format json ref LoginService .
lmprobe --format json search --name Login .
```

## GraphQL Recipes

Batch two raw searches in one process:

```graphql
{
  login: grep(pattern: "login|signin", path: "apps/web") {
    hits { filePath lineNumber lineContent }
    exitStatus
  }
  routes: find(pattern: "(^|/)route(s)?\\.(ts|tsx|py)$", path: ".") {
    paths
    exitStatus
  }
}
```

Scan multiple explicit roots without shell orchestration:

```graphql
{
  grep(pattern: "process\\.env\\.[A-Z0-9_]+", path: "apps/web", paths: ["packages/shared"]) {
    hits { filePath lineNumber lineContent }
    warnings { code message hint }
    exitStatus
  }
}
```

Find files under directories that contain `package.json` and whose contents
contain `login`:

```graphql
{
  grepDirsWith(
    markerPattern: "(^|[\\\\/])package\\.json$",
    pattern: "login",
    path: ".",
    around: 2,
    parents: true
  ) {
    hits {
      filePath
      lineNumber
      lineContent
      beforeContext { lineNumber lineContent }
      afterContext { lineNumber lineContent }
      parents { kind name lineStart lineEnd }
    }
    warnings { code message hint }
    exitStatus
  }
}
```

`grepDirsWith` is the first in-process AND composition primitive for raw
filesystem scans. It performs:

1. `find(markerPattern, path)` over file paths.
2. Parent-directory extraction and ancestor pruning.
3. `grep(pattern, <marker dirs>)`.

It returns the normal `GrepResult` envelope, so callers consume the same hit
shape as `grep`. It accepts optional `paths: [...]` for additional marker roots
when the packages/services of interest live in separate top-level directories.

Use aliases for OR across different evidence streams:

```graphql
{
  env: grep(pattern: "process\\.env", path: ".") { hits { filePath lineNumber lineContent } }
  config: configKeys(path: ".", grepCode: true) { keys { filePath keyPath codeAnchors { filePath lineNumber lineContent } } }
  routes: routeAnchors(path: ".", around: 2) { routes { filePath lineNumber lineContent method path } }
}
```

## Interface Assessment

The primitive CLI is clear and concise for common `find`/`grep` replacement:
one verb, one regex, one path, with repeated `--path` for multiple roots.
The flags map to established conventions (`-i`, `-G`, `-A/-B/-C`) while adding
agent-specific context (`--parents`, `--body`, `--role`).

The GraphQL surface is stronger for agents because it eliminates shell pipes,
lets the caller select only needed fields, and returns structured diagnostics
in one envelope. The new `paths` args on `find`/`grep` close the multi-root
gap between CLI and GraphQL. `grepDirsWith` covers the most common internal
pipe: path-marker scope AND content match.

Remaining enhancement candidates:

- Add CLI sugar for the new composition, for example
  `lmprobe grep --under-marker '(^|[\\/])package\.json$' 'login' .`. The GraphQL
  resolver exists first because agents can use it immediately without adding
  more clap surface.
- Add a first-class reusable `ScopeInput` for read resolvers if more
  composition primitives appear. Mutations already use `ScopeInput`; read-side
  search could eventually share `paths`, `markerPattern`, `globs`, and `type`
  in one input object.
- Add named GraphQL recipes for high-frequency patterns once usage proves
  stable. Avoid a broad arbitrary pipeline language until there are several
  repeated, well-understood compositions.
