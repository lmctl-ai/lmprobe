// Stamp the meta package.json (@lmctl-ai/lmprobe) so its version and its
// per-platform optionalDependencies are single-sourced from the Rust
// workspace version in Cargo.toml. Run in CI before packing/publishing; also
// safe to run locally to keep the committed file in sync.
//
//   node npm/scripts/prepare-meta.mjs

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { PLATFORMS, packageVersion, platformPackageName } from './platforms.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const metaPath = join(here, '..', 'package.json');

const version = packageVersion(readFileSync(join(repoRoot, 'Cargo.toml'), 'utf8'));
const meta = JSON.parse(readFileSync(metaPath, 'utf8'));

meta.version = version;
meta.optionalDependencies = Object.fromEntries(
  PLATFORMS.map((p) => [platformPackageName(p), version]),
);

writeFileSync(metaPath, JSON.stringify(meta, null, 2) + '\n');
console.log(`prepared @lmctl-ai/lmprobe meta package at version ${version}`);
console.log(`  optionalDependencies: ${Object.keys(meta.optionalDependencies).join(', ')}`);
