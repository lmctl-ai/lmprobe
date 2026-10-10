# npm distribution — `@lmctl-ai/lmprobe`

lmprobe ships to npm as **`@lmctl-ai/lmprobe`** so users can
`npm install -g @lmctl-ai/lmprobe` / `npx @lmctl-ai/lmprobe` without a Rust
toolchain. This is the public install channel documented at
https://lmctl.com/lmprobe. Building from source (`cargo build -p lmprobe`) is
described in the repository `README.md`.

## Shape (esbuild / @ast-grep/cli pattern)

- **Meta package `@lmctl-ai/lmprobe`** (`npm/package.json`): no binary itself —
  a `bin` launcher shim (`npm/bin/lmprobe.js`) plus one `optionalDependency`
  per platform. The shim resolves `@lmctl-ai/lmprobe-<os>-<cpu>` via its
  `package.json` and execs the binary. It forwards argv/stdio and preserves
  the binary's exit status; launcher infrastructure failures use shell-like
  codes (`127` for missing platform package, `126` for launch failure,
  `128+signal` for signal termination).
- **Five platform packages `@lmctl-ai/lmprobe-<os>-<cpu>`**: the prebuilt
  binary tagged with `os`/`cpu` so npm installs only the matching one. Targets:
  `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, `win32-x64`.

## Source of truth

`npm/scripts/platforms.mjs` — the 5-target matrix (Node os/cpu ↔ Rust triple ↔
binary name) and the `[workspace.package] version` parser. Everything derives
from it:
- `prepare-meta.mjs` stamps the meta `version` + `optionalDependencies` from
  `Cargo.toml` (npm version is single-sourced from the Rust workspace).
- `assemble-platform.mjs <nodeOs> <nodeCpu>` builds one platform package dir
  from `target/release/<binary>` into `npm/dist/<os>-<cpu>/` (gitignored).

## Release

`.github/workflows/release-npm.yml`: native package assembly workflow. In full
production mode it mirrors `release-binaries.yml`, assembles every platform,
packs the meta package, and publishes all six. It is `workflow_dispatch`-only
and defaults to `dry_run=true` (validates, uploads nothing); a real publish is a
manual dispatch with `dry_run` unchecked. The plan job still blocks a real
publish when the latest `v*` release tag is less than 7 days old or when there
are no release-relevant code/package changes since that tag. The counted paths
are `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/`, `npm/`,
`scripts/`, and `xtask/`. There is no `NPM_TOKEN` repository secret. If a real
publish is requested before all six current-version package names exist, the
plan job exits with a notice instead of running the native build matrix.

Publishing uses npm OIDC Trusted Publishing: GitHub Actions presents an OIDC
token and npm matches it against each package's Trusted Publisher record. Do
not replace this with a long-lived npm token or a normal repository secret.
If OIDC fails, fix the npm Trusted Publisher records and rerun the workflow;
manual browser/OTP publishing is only for first bootstrap or emergency recovery.
The publish job strips setup-node's placeholder `_authToken` line from the
generated npmrc before `npm publish`; leaving that placeholder in place makes
npm skip the OIDC exchange and fail with the same E404/ENEEDAUTH shape as a
bad Trusted Publisher record.

Real publishes choose the current `[workspace.package].version` when that npm
version does not exist yet (bootstrap publish), otherwise they publish the next
patch version. The workflow stamps that planned version into `Cargo.toml`,
`Cargo.lock`, and `npm/package.json` before native builds so
`lmprobe --version`, package metadata, and optional dependency versions agree.
After a successful real publish, the publish job commits the stamped files as
`release: <version> [skip ci]` and tags `v<version>`. The tag does not trigger
the native archive workflow; `.github/workflows/release-binaries.yml` is
manual-only.

### Prerelease/internal builds

Prerelease builds are not public npm releases and do not need the
GitHub-hosted cross-platform matrix. Build them locally, then upload payloads
to storage you control.

Portable Linux x64 build (Ubuntu 20.04 container, glibc 2.31 floor):

```bash
docker run --rm \
  -v "$PWD":/work \
  -w /work \
  -e HOST_UID="$(id -u)" \
  -e HOST_GID="$(id -g)" \
  ubuntu:20.04 \
  bash scripts/ci/linux_portable_build.sh build

./scripts/audit_glibc_floor.sh target/release/lmprobe 2.34
install -d /tmp/lmprobe-internal
tar -C target/release -czf /tmp/lmprobe-internal/lmprobe-linux-x64.tgz lmprobe
```

Upload command depends on the chosen storage target, for example:

```bash
aws s3 cp /tmp/lmprobe-internal/lmprobe-linux-x64.tgz \
  "s3://<operator-internal-bucket>/lmprobe/<version-or-commit>/"
```

For macOS, Windows, or ARM prerelease payloads, build on those platforms
directly. Do not push/tag just to make GitHub-hosted runners build prerelease
artifacts.

### Linux ABI verification (GLIBC floor)

Before publishing Linux npm tarballs, run the repository helper against the
built Linux binaries to enforce the negotiated floor:

```bash
./scripts/audit_glibc_floor.sh target/release/lmprobe 2.34
./scripts/audit_glibc_floor.sh npm/dist/linux-arm64/lmprobe 2.34
```

Use the packaged binary path for each variant you are about to publish. This helper
defaults to `2.31`, matching the `ubuntu:20.04` CI build container, and accepts
a higher explicit floor such as `2.34` for compatibility audits.

**Bootstrap (one-time, operator + agent):** each npm package must exist before
a Trusted Publisher can be attached. Run the workflow in dry-run mode, download
the `lmprobe-npm-tarballs` artifact, then publish the five platform tarballs
first and the meta tarball last with local browser/OTP auth.

### Manual bootstrap checklist

1. **Agent:** verify npm identity and 2FA mode. Do not print tokens.

   ```bash
   npm whoami
   npm profile get --json | jq '{name,tfa}'
   ```

2. **Agent:** verify tarballs and registry absence before publishing.

   ```bash
   cd /tmp/lmprobe-npm-publish
   sha256sum *.tgz
   for p in \
     @lmctl-ai/lmprobe-linux-x64 \
     @lmctl-ai/lmprobe-linux-arm64 \
     @lmctl-ai/lmprobe-darwin-x64 \
     @lmctl-ai/lmprobe-darwin-arm64 \
     @lmctl-ai/lmprobe-win32-x64 \
     @lmctl-ai/lmprobe
   do
     printf '%s ' "$p"
     npm view "$p" version 2>/dev/null || echo E404
   done
   ```

3. **Agent:** publish one tarball at a time in this order. Always use a
   filesystem path with a leading `./`; otherwise npm can parse a bare tarball
   filename as a package/git spec.

```bash
npm publish ./lmctl-ai-lmprobe-linux-x64-<version>.tgz --access public --tag latest
npm publish ./lmctl-ai-lmprobe-linux-arm64-<version>.tgz --access public --tag latest
npm publish ./lmctl-ai-lmprobe-darwin-x64-<version>.tgz --access public --tag latest
npm publish ./lmctl-ai-lmprobe-darwin-arm64-<version>.tgz --access public --tag latest
npm publish ./lmctl-ai-lmprobe-win32-x64-<version>.tgz --access public --tag latest
npm publish ./lmctl-ai-lmprobe-<version>.tgz --access public --tag latest
```

4. **Agent:** if npm returns `EOTP` in a non-interactive shell and redacts the
   browser URL as `https://www.npmjs.com/auth/cli/***`, rerun the failed
   publish in a real terminal/PTY. In Codex tooling this means `tty=true`; in a
   normal shell, run the same command directly. A PTY shows the full URL:

   Non-PTY failure shape:

   ```text
   npm notice Publishing to https://registry.npmjs.org/ with tag latest and public access
   npm error code EOTP
   npm error This operation requires a one-time password.
   npm error Open this URL in your browser to authenticate:
   npm error   https://www.npmjs.com/auth/cli/***
   npm error
   npm error After authenticating, your token can be retrieved from:
   npm error   https://registry.npmjs.org/-/v1/done?authId=***
   ```

   PTY/browser-auth shape (safe sample; UUID is an example):

   ```text
   npm notice Publishing to https://registry.npmjs.org/ with tag latest and public access
   Authenticate your account at:
   https://www.npmjs.com/auth/cli/00000000-0000-4000-8000-000000000000
   Press ENTER to open in the browser...
   ```

   npm's publish-time browser flow does not expose a separate device code in
   the terminal; the auth UUID is embedded in the URL. If npm ever prints a
   browser page code, the operator should compare/type that code in the browser
   page, then tell the agent when the page confirms auth.

5. **Agent:** send only the full `https://www.npmjs.com/auth/cli/<uuid>` URL
   to the operator. Keep the npm process waiting.

6. **Operator:** open that URL in the local browser. In WSL2, copy the URL into
   the Windows browser. Complete npm's browser auth page.

7. **Operator:** tell the agent `done` after the browser page confirms auth.

8. **Agent:** press Enter in the waiting npm process and confirm the publish
   prints `+ @lmctl-ai/<package>@<version>`. If npm asks again on the next
   package, repeat steps 4-8 from that package onward.

   Success shape:

   ```text
   + @lmctl-ai/lmprobe-linux-x64@0.42.1
   ```

9. **Agent/operator alternative:** if the operator prefers TOTP instead of
   browser auth, the operator supplies a current six-digit authenticator code
   out-of-band. The agent runs the same package order with `--otp "$OTP"` and
   refreshes `OTP` if it expires mid-sequence. Do not print the code.

   TOTP command shape:

   ```bash
   OTP=<current-6-digit-code>
   npm publish ./lmctl-ai-lmprobe-linux-x64-<version>.tgz --access public --tag latest --otp "$OTP"
   ```

10. **Agent:** if a publish succeeds, do not republish just because `npm view`
    still returns `E404`. Registry reads can lag. Check ownership first:

```bash
npm access list packages lmctl-ai --json | jq 'to_entries | map(select(.key | test("^@lmctl-ai/lmprobe")))'
```

    Access-list success shape:

    ```json
    [
      {
        "key": "@lmctl-ai/lmprobe",
        "value": "read-write"
      },
      {
        "key": "@lmctl-ai/lmprobe-linux-x64",
        "value": "read-write"
      }
    ]
    ```

11. **Agent:** if all six names appear in access output, wait and poll public
    registry reads until all return the version:

    ```bash
    for p in \
      @lmctl-ai/lmprobe-linux-x64 \
      @lmctl-ai/lmprobe-linux-arm64 \
      @lmctl-ai/lmprobe-darwin-x64 \
      @lmctl-ai/lmprobe-darwin-arm64 \
      @lmctl-ai/lmprobe-win32-x64 \
      @lmctl-ai/lmprobe
    do
      printf '%-36s ' "$p"
      npm view "$p" version
    done
    ```

12. **Operator:** after all six public reads work, configure **Trusted
    Publisher** on npmjs.com for each package using the values below.

Then configure **Trusted Publisher** on npmjs.com for all six packages:

| Field | Value |
| --- | --- |
| Organization or user | `lmctl-ai` |
| Repository | `lmprobe` |
| Workflow filename | `release-npm.yml` |
| Allowed actions | `npm publish` |

The `Organization or user` field is the GitHub owner of the repository that
runs `.github/workflows/release-npm.yml`, not the npm package scope (both are
`lmctl-ai` for lmprobe).

## Known follow-ups

- **Linux runtime compatibility:** Linux binaries are built in an
  `ubuntu:20.04` container through `scripts/ci/linux_portable_build.sh`, with
  vendored libgit2/OpenSSL via the `linux-portable-vendored` feature, and CI
  fails if the produced binary references GLIBC symbols above `2.31`. Builds
  made on a newer host (for example Ubuntu 24.04) require that host's glibc
  and fail on older distributions with ``version `GLIBC_2.39' not found``.
- **Binary size ~60 MB/platform** (no symbol strip). Consider
  `strip = "symbols"` on `[profile.release]` or stripping in CI to shrink the
  published tarballs.
- After OIDC is verified for the six packages, set npm package publishing access
  to disallow traditional tokens and revoke any leftover publish tokens.
