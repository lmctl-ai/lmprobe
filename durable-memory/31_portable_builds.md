# Portable local builds

Local verification builds and distributable builds serve different purposes.
A host-native release build is useful for testing the current source; it does
not establish a portable Linux ABI floor.

## Linux portability

From a checkout with Docker available:

```bash
docker run --rm \
  -v "$PWD":/work -w /work \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  ubuntu:20.04 bash scripts/ci/linux_portable_build.sh build
```

The helper installs container build prerequisites and uses
`--features linux-portable-vendored` to avoid inheriting host OpenSSL/libgit2
requirements. The helper's default GLIBC floor is 2.31; inspect its current
configuration before treating a different compatibility target as established.
Verify the actual artifact, not just the compiler/container version:

```bash
target/release/lmprobe --version
target/release/lmprobe --help
scripts/audit_glibc_floor.sh target/release/lmprobe 2.31
```

Record source revision, package version, build target/features, artifact checksum
and ABI audit result alongside a distributed artifact. No historical timing or
ABI result proves compatibility for a newly compiled binary. Audit each platform
package independently; validate the npm launcher from a temporary installation.

## Troubleshooting and publication boundary

- An unavailable Docker daemon or socket permission is a local environment
  problem. Verify daemon access before debugging Cargo or changing source.
- Container dependency/network failures do not imply a sensor correctness failure.
- Host-native builds do not become portable by stripping the binary.
- Preserve ownership of mounted build outputs using the helper's UID/GID options.
- Local builds do not require hosted workflow dispatch, tags, or npm publication.
  A production release remains a separately authorized operation.

See [release provenance](27_release_provenance.md),
[npm packaging](npm_distribution.md), and [testing](16_testing.md).
