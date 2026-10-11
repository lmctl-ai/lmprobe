# Current project status

Last updated: 2026-10-11

Canonical repository: [lmctl-ai/lmprobe](https://github.com/lmctl-ai/lmprobe).
The source workspace, manual, tests and packaging all live in this public project.
The source/package version is `0.43.0`; the lmapi SDK is pinned to `lmrust 0.1.2`.
Source and registry versions can differ until an explicit release is performed.

## Available capabilities

Current command contracts cover structural/text search, symbol and call relations,
history/cache queries, evidence scanners, preview-first changes, live text editing,
parsed Git inspections, read-only AWS queries and lmapi mode. Start at the
[command index](commands/index.md). AWS remains an exact 41-operation allowlist;
credential/token sources and pagination coverage require their existing guards.

## Workspace migration

The source/test audit found no missing implementation files. The public import
and later public fixes remain authoritative. Reusable technical memory was
consolidated into [trace integration](integration-guidance.md),
[portable builds](31_portable_builds.md),
[publishing diagnostics](30_publishing_diagnostics.md), and
[design lessons](historical/design-evolution.md), with a synthetic trace fixture.
The [migration note](migration.md) explains canonical checkout and installation.

## Verification and release boundaries

Verification from the public checkout on 2026-10-11:

- Locked optimized build and `cargo check -p lmprobe --locked` passed.
- `cargo test -p lmprobe -p lmprobe-engine --locked`: 481 passed, 4 ignored.
  The smoke run used repository fixtures; 37 optional corpus cases skipped because
  the corpus environment variable was unset. No full corpus/performance claim is made.
- Formatting, clippy and whitespace checks passed; clippy retains existing warnings.
- Two context smoke snapshots had stale import commit/time fields; only those
  history values were updated to the current public import, then the suite passed.
- The local PATH installation was linked to the public checkout's release binary.
  It reports `lmprobe 0.43.0 (commit 80f545e)`, parses its lmapi schema, and emits
  the 41-operation read-only AWS catalog as structured JSON.
- New memory links, the synthetic trace fixture and a public privacy scan passed.
  Independent review approved the sanitized documentation.

Corpus-dependent checks require their documented optional environment variables;
a skipped fixture is not evidence of a full corpus pass. Non-Linux builds and the
optional daemon feature were not part of this local verification.

Production npm publication and hosted release workflows were not requested and
were not dispatched. Trusted publisher records must match this public repository
before a future real release. See [release provenance](27_release_provenance.md).
The unrelated xtask schema-generator ENOENT fixture limitation remains documented
in [testing](16_testing.md); the capability-matrix check is independently runnable.
