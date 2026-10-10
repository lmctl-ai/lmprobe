# lmprobe

> One-shot, AST-aware **structural code-search CLI** with schema-first JSON envelopes.

`lmprobe` reads the live filesystem, parses files via tree-sitter (through a fork
of [ast-grep](https://github.com/ast-grep/ast-grep)), and prints versioned JSON
describing what it found. Every invocation parses on demand and exits — no
daemon, no project index in your tree.

## Install

```bash
npm install -g @lmctl-ai/lmprobe
# or run without installing:
npx @lmctl-ai/lmprobe --help
```

This meta-package pulls in the right prebuilt native binary for your platform
(`@lmctl-ai/lmprobe-<os>-<cpu>`). Supported platforms:

| OS | Arch | Package |
|----|------|---------|
| Linux | x64 | `@lmctl-ai/lmprobe-linux-x64` |
| Linux | arm64 | `@lmctl-ai/lmprobe-linux-arm64` |
| macOS | x64 (Intel) | `@lmctl-ai/lmprobe-darwin-x64` |
| macOS | arm64 (Apple Silicon) | `@lmctl-ai/lmprobe-darwin-arm64` |
| Windows | x64 | `@lmctl-ai/lmprobe-win32-x64` |

> Installing with `--no-optional` / `--omit=optional` skips the binary and
> `lmprobe` will refuse to run — reinstall without it.

## First run

```bash
lmprobe --help                 # list commands
lmprobe discover .             # map a repository
lmprobe search -p 'fn $NAME() { $$$ }' -l rust .
```

Output auto-formats: human-readable markdown on a TTY, NDJSON when piped. See
`--format json|ndjson|sarif|github|mermaid|dot` for the rest.

## Docs

Full manual: **https://lmctl.com/lmprobe**

## License

MIT. lmprobe forks the `ast-grep` engine crates — see the bundled `LICENSE` and
`ATTRIBUTION.md` for the dual copyright.
