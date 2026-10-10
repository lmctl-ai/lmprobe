# lmprobe static manual

This directory is the public lmprobe manual published at
[https://lmctl.com/lmprobe](https://lmctl.com/lmprobe). It is dependency-free static HTML
and is served under the `/lmprobe/` path prefix (asset URLs are written as `/lmprobe/...`),
not from a top-level root.

- `index.html` - the manual and examples.
- `bugs.html` - sanitized bug hunts found with lmprobe.
- `styles.css` - manual styling.
- `app.js` - copy-button behavior and small UI helpers.
- `assets/` - static assets (`lmprobe-flow.svg`).

Only this directory is deployed (by `scripts/deploy-lmprobe.sh` in the lmctl website repo,
which uploads `site/` plus the repository's `LICENSE` and `ATTRIBUTION.md`). The rest of the
repository is the lmprobe source code and is never uploaded.
