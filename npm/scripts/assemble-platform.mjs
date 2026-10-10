// Assemble one per-platform npm package (@lmctl-ai/lmprobe-<os>-<cpu>) from a
// freshly built lmprobe binary. Run on each build runner after
// `cargo build -p lmprobe --release`.
//
//   node npm/scripts/assemble-platform.mjs <nodeOs> <nodeCpu> [--binary <path>]
//
// Produces npm/dist/<nodeOs>-<nodeCpu>/ containing the binary, a generated
// package.json (with os/cpu so npm installs it only on the matching host), and
// the license/attribution files. The dir is ready for `npm pack` / `npm publish`.

import { readFileSync, writeFileSync, mkdirSync, copyFileSync, chmodSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { PLATFORMS, findPlatform, packageVersion, platformPackageName } from './platforms.mjs';

const [nodeOs, nodeCpu, ...rest] = process.argv.slice(2);
if (!nodeOs || !nodeCpu) {
  console.error('usage: assemble-platform.mjs <nodeOs> <nodeCpu> [--binary <path>]');
  process.exit(2);
}

const binFlagIdx = rest.indexOf('--binary');
const binaryOverride = binFlagIdx !== -1 ? rest[binFlagIdx + 1] : undefined;

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const platform = findPlatform(nodeOs, nodeCpu);
const version = packageVersion(readFileSync(join(repoRoot, 'Cargo.toml'), 'utf8'));

// Native runners build into target/release; a --target build lands in
// target/<triple>/release. Accept an explicit override for either layout.
const builtBinary =
  binaryOverride ?? join(repoRoot, 'target', 'release', platform.binary);

const outDir = join(here, '..', 'dist', `${nodeOs}-${nodeCpu}`);
rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const pkg = {
  name: platformPackageName(platform),
  version,
  description: `Prebuilt lmprobe binary for ${nodeOs}-${nodeCpu}.`,
  keywords: ['lmprobe', 'ast-grep', 'code-search', 'cli'],
  homepage: 'https://lmctl.com/lmprobe',
  repository: { type: 'git', url: 'git+https://github.com/lmctl-ai/lmprobe.git' },
  license: 'MIT',
  os: [nodeOs],
  cpu: [nodeCpu],
  files: [platform.binary, 'README.md', 'LICENSE', 'ATTRIBUTION.md'],
};

writeFileSync(join(outDir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');

copyFileSync(builtBinary, join(outDir, platform.binary));
if (nodeOs !== 'win32') {
  chmodSync(join(outDir, platform.binary), 0o755);
}

for (const f of ['LICENSE', 'ATTRIBUTION.md']) {
  copyFileSync(join(repoRoot, f), join(outDir, f));
}
writeFileSync(
  join(outDir, 'README.md'),
  `# ${pkg.name}\n\nPrebuilt lmprobe binary for ${nodeOs}-${nodeCpu}. ` +
    `This package is installed automatically as a platform-specific dependency of ` +
    `[\`@lmctl-ai/lmprobe\`](https://www.npmjs.com/package/@lmctl-ai/lmprobe); ` +
    `do not depend on it directly.\n\nDocs: https://lmctl.com/lmprobe\n`,
);

console.log(`assembled ${pkg.name}@${version} -> ${outDir}`);
console.log(`  binary: ${builtBinary} (${platform.binary})`);
console.log(`  os=${JSON.stringify(pkg.os)} cpu=${JSON.stringify(pkg.cpu)}`);
console.log(`KNOWN_PLATFORMS=${PLATFORMS.map((p) => `${p.nodeOs}-${p.nodeCpu}`).join(',')}`);
