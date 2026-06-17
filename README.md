# lmprobe

lmprobe is an AST-aware code-search CLI for developers and AI agents that need
structured evidence from source trees. It finds definitions and references,
maps impact graphs, inspects history, returns machine-readable JSON, and
supports safe code-change workflows.

This public repository contains the lmprobe documentation site. The CLI
implementation is distributed as a packaged binary through npm.

## Install

```bash
npm install -g @lmctl-ai/lmprobe
```

Or run without a global install:

```bash
npx @lmctl-ai/lmprobe --help
```

## Documentation

The public manual is published at:

https://lmctl.com/lmprobe

The static site in this repository is dependency-free and can be opened from
`index.html` or served with any static file server.

## Repository contents

- `index.html` - lmprobe manual and examples.
- `styles.css` - manual styling.
- `app.js` - copy-button behavior and small UI helpers.
- `assets/` - static documentation assets.
- `ATTRIBUTION.md` - upstream attribution for lmprobe's implementation lineage.

No closed-source Rust implementation is included in this public docs repository.

## License

MIT. See `LICENSE` and `ATTRIBUTION.md`.
