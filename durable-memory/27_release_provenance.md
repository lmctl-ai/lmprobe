# 27 - Release Provenance

This chapter is the source-of-truth map for how lmprobe's public release
identity is assembled. It exists to keep issue reports, npm packages, release
archives, and `lmprobe --version` aligned.

## Canonical Version Source

The canonical release version is `[workspace.package].version` in the
workspace root `Cargo.toml`.

The `crates/lmprobe` package inherits that value with `version.workspace =
true`, so `env!("CARGO_PKG_VERSION")` in the binary reports the same semantic
version that npm packages and release archives use.

`Cargo.lock` must record the same version for every crate that inherits the
workspace version. Today that set is:

| Package | Role |
| --- | --- |
| `lmprobe` | CLI binary |
| `ast-grep-core` | vendored structural matching core |
| `ast-grep-config` | vendored ast-grep config layer |
| `ast-grep-language` | vendored language registry layer |

## Public npm Packages

The public install channel is the npm meta package:

```bash
npm install -g @lmctl-ai/lmprobe
```

The meta package depends optionally on one platform package per supported host:

| npm package | Host |
| --- | --- |
| `@lmctl-ai/lmprobe-linux-x64` | Linux x64 |
| `@lmctl-ai/lmprobe-linux-arm64` | Linux arm64 |
| `@lmctl-ai/lmprobe-darwin-x64` | macOS Intel |
| `@lmctl-ai/lmprobe-darwin-arm64` | macOS Apple Silicon |
| `@lmctl-ai/lmprobe-win32-x64` | Windows x64 |

All six npm packages must share the same version. The meta package's
`optionalDependencies` must point at that exact version for every platform
package.

## Version Stamping

`npm/scripts/set-version.mjs <version>` is the release-stamping tool. It
updates:

- root `Cargo.toml` `[workspace.package].version`
- `Cargo.lock` entries for workspace-version crates
- `npm/package.json` version
- `npm/package.json` platform `optionalDependencies`

The GitHub Actions npm workflow calls this before native builds. That ordering
matters: the compiled binary's `--version` output comes from Cargo package
metadata, so the workspace version must be stamped before `cargo build`.

The workflow calls the same script again after a successful real publish and
commits the stamped files as `release: <version> [skip ci]`, then tags
`v<version>`.

## Binary Version String

`lmprobe --version` prints:

```text
lmprobe <semver> (commit <sha7>)
```

The semver comes from Cargo package metadata. The short commit SHA is emitted
by `crates/lmprobe/build.rs` through `LMPROBE_GIT_SHA`. Release CI builds from
the checked-out release commit, so the SHA should identify the source tree used
for the native binary.

## Release Workflows

- `.github/workflows/release-binaries.yml` builds native archive artifacts.
- `.github/workflows/release-npm.yml` normally builds the same native binary
  matrix, assembles npm platform packages, publishes all platform packages
  first, then publishes the `@lmctl-ai/lmprobe` meta package.

`release-npm` is workflow_dispatch-only: the matrix includes macOS and Windows
runners, and a real release is a deliberate human action. Manual runs default
to dry-run; real production publishing requires an explicit dispatch with
`dry_run=false`. Ordinary pushes and schedules cannot publish.
The plan job blocks any real publish when the latest `v*` release tag is less
than 7 days old. It
also blocks real publishes when there are no release-relevant code/package
changes since that tag; docs, site, and workflow-only changes do not
spend the hosted runner matrix. The publish job has `id-token: write`, strips
the setup-node placeholder `_authToken` so npm uses OIDC, uses current npm,
runs with `NPM_CONFIG_PROVENANCE=false`, publishes platform
tarballs first, then publishes the meta package in full-matrix mode. The plan
job checks that all six current-version package names exist before allowing a
real publish; missing names mean manual bootstrap is still required and the
native build matrix is skipped. No `NPM_TOKEN` secret is used; if OIDC publishing
fails, the fix is the npm Trusted Publisher configuration, not a token.

The source version in `Cargo.toml` can be ahead of the npm `latest` version.
That is deliberate when fixes have landed but no release has been dispatched
yet. Do not stamp the source back to the published version: that would make
source-built and npm-installed artifacts report the same version while
behaving differently. The next real release publishes the source version.

## Bootstrap Requirements

The first real npm publish creates the six `@lmctl-ai/lmprobe*` package names.
Do that first publish manually from the workflow's `lmprobe-npm-tarballs`
artifact: publish the five platform packages first, then publish
`@lmctl-ai/lmprobe`. After the names exist, configure npm Trusted Publisher on
all six packages for GitHub repo `lmctl-ai/lmprobe`, workflow
`release-npm.yml`, allowed action `npm publish`. No `NPM_TOKEN` secret is used.
The Trusted Publisher `Organization or user` field is the GitHub owner of the
publishing repo (`lmctl-ai`), not the npm package scope (also `@lmctl-ai` here).

## Verification Checklist

Before claiming a release-integrity change is complete:

```bash
cargo metadata --locked --format-version 1 >/tmp/lmprobe-metadata.json
cargo test -p lmprobe --test smoke --locked
cargo build -p lmprobe --release --locked
target/release/lmprobe --version
node --check npm/scripts/platforms.mjs
node --check npm/scripts/prepare-meta.mjs
node --check npm/scripts/assemble-platform.mjs
node --check npm/scripts/set-version.mjs
node npm/scripts/prepare-meta.mjs
```

For npm packaging, additionally pack and install the meta package plus the
current platform package into a temporary project, then run:

```bash
./node_modules/.bin/lmprobe --version
./node_modules/.bin/lmprobe --help
```

The installed binary version must match the npm package version.
