# Canonical public workspace

The canonical project is [lmctl-ai/lmprobe](https://github.com/lmctl-ai/lmprobe).
Its repository contains source, tests, contributor memory, packaging and the
manual under `site/`. Development and builds should use this checkout.

## Completeness and documentation

The public import `37120b5` contains the complete source and test snapshot.
Comparison with the complete development snapshot found no missing `crates/`
or `xtask/` paths. Public work through `80f545e` adds lmapi SDK 0.1.2 support,
additional invariance tests, portable fixture handling and fix-apply corrections.
Retain those improvements; no further source merge was required by the audit.

Contributor memory preserves reusable technical knowledge. Integration guidance,
portable builds, publishing diagnostics and design lessons were consolidated from
earlier notes. Session conversations, organization-specific deployments, personal
paths, credentials, incident transcripts, obsolete task instructions and unverified
historical success claims are not contributor contracts. Do not restore them while
refreshing documentation or use superseded design proposals as current behavior.

Local data and author-time tooling remain outside the published manual. Evidence
examples under `fixtures/` are synthetic. The public privacy check applies to both
new documentation and the existing memory tree.

## Local build and installation

From the public checkout:

```bash
cargo build -p lmprobe --release --locked
install -d "$HOME/.local/bin"
ln -sfn "$PWD/target/release/lmprobe" "$HOME/.local/bin/lmprobe"
lmprobe --version
lmprobe --schema
```

Ensure `$HOME/.local/bin` is on PATH, and verify `command -v lmprobe` and the
resolved link after moving a checkout. A symlink to a removed build directory
cannot provide a working installation. Build artifacts and local state are not
repository content; hosted releases require a separate explicit request.

See [current status](current_status.md) and [memory index](index.md).
