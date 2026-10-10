# 25 - Static Manual Site

`site/` is the dependency-free static manual published at
https://lmctl.com/lmprobe. It is intentionally separate from `durable-memory/`,
which is the project's design and development memory.

## Shape

- `site/index.html` - single-page, book-like manual with chapter navigation.
- `site/bugs.html` - sanitized bug hunts found with lmprobe.
- `site/styles.css` - responsive visual design.
- `site/app.js` - active navigation, copy buttons, and recipe filtering.
- `site/assets/lmprobe-flow.svg` - local workflow diagram.
- `site/README.md` - layout and deployment notes.

The site has no build step and no package manager dependency. Pages are served
under the `/lmprobe/` path prefix (asset URLs are written as `/lmprobe/...`).
Only `site/` is deployed, plus the repository's `LICENSE` and `ATTRIBUTION.md`;
the deploy script in the lmctl website repository refuses to sync a directory
that looks like the source root. To preview locally:

```bash
python3 -m http.server 8080 --directory site
```

## Content Contract

The site should stay practical and command-focused:

- installation and first run;
- search, `find`, and `grep` workflows;
- graph/context commands;
- history evidence;
- safe `trace` and `fix` usage;
- output formats;
- one-shot GraphQL composition;
- secrets/provider evidence;
- scenario recipes.

Use public/agent-facing material from `README.md` and the durable-memory usage
chapters, but keep implementation process, coordination notes and CI status
out of the manual.

## Verification

When editing the site, run a static serving smoke check:

```bash
python3 -m http.server 8765 --bind 127.0.0.1 --directory site
curl -I http://127.0.0.1:8765/index.html
curl -I http://127.0.0.1:8765/styles.css
curl -I http://127.0.0.1:8765/app.js
curl -I http://127.0.0.1:8765/assets/lmprobe-flow.svg
```

Also scan for accidental internal-only references and non-ASCII content:

```bash
rg -n "durable-memory|rollout|internal" site
LC_ALL=C rg -n "[^\x00-\x7F]" site
```
