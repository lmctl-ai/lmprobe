// Single source of truth for the lmprobe npm release matrix.
//
// Each entry maps a Node platform/arch (process.platform / process.arch) to the
// Rust target triple the binary is built for and the on-disk binary name. The
// `release-npm.yml` workflow, the per-platform package assembler, and the meta
// package's optionalDependencies are all derived from this list so the five
// platform packages never drift apart.

export const SCOPE = '@lmctl-ai';
export const PACKAGE = 'lmprobe';

export const PLATFORMS = [
  { nodeOs: 'linux', nodeCpu: 'x64', rustTarget: 'x86_64-unknown-linux-gnu', binary: 'lmprobe' },
  { nodeOs: 'linux', nodeCpu: 'arm64', rustTarget: 'aarch64-unknown-linux-gnu', binary: 'lmprobe' },
  { nodeOs: 'darwin', nodeCpu: 'x64', rustTarget: 'x86_64-apple-darwin', binary: 'lmprobe' },
  { nodeOs: 'darwin', nodeCpu: 'arm64', rustTarget: 'aarch64-apple-darwin', binary: 'lmprobe' },
  { nodeOs: 'win32', nodeCpu: 'x64', rustTarget: 'x86_64-pc-windows-msvc', binary: 'lmprobe.exe' },
];

export function platformPackageName(p) {
  return `${SCOPE}/${PACKAGE}-${p.nodeOs}-${p.nodeCpu}`;
}

export function findPlatform(nodeOs, nodeCpu) {
  const match = PLATFORMS.find((p) => p.nodeOs === nodeOs && p.nodeCpu === nodeCpu);
  if (match === undefined) {
    throw new Error(`unsupported platform ${nodeOs}-${nodeCpu}; known: ${PLATFORMS.map((p) => `${p.nodeOs}-${p.nodeCpu}`).join(', ')}`);
  }
  return match;
}

// Parse `[workspace.package] version = "x.y.z"` out of the root Cargo.toml so
// the npm version is single-sourced from the Rust workspace.
export function workspaceVersion(cargoToml) {
  const normalized = cargoToml.replace(/\r\n/g, '\n');
  const section = normalized.split(/^\[/m).find((s) => s.startsWith('workspace.package]'));
  const source = section ?? normalized;
  const m = source.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (m === null) {
    throw new Error('could not find [workspace.package] version in Cargo.toml');
  }
  return m[1];
}

export function packageVersion(cargoToml) {
  const override = process.env.LMPROBE_NPM_VERSION?.trim();
  if (override) {
    if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(override)) {
      throw new Error(`invalid LMPROBE_NPM_VERSION: ${override}`);
    }
    return override;
  }
  return workspaceVersion(cargoToml);
}
