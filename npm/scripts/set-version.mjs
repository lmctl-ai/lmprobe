// Stamp the npm release version into Cargo.toml, Cargo.lock, and npm/package.json.
//
// Usage:
//   node npm/scripts/set-version.mjs <version>
//
// The publish workflow calls this before native builds so CARGO_PKG_VERSION and
// npm package metadata agree, then calls it again after a successful real
// publish so main records the version that went to npm.

import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { PLATFORMS, platformPackageName } from './platforms.mjs';

const version = process.argv[2]?.trim();
if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  console.error('usage: set-version.mjs <semver>');
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..');
const cargoPath = join(repoRoot, 'Cargo.toml');
const lockPath = join(repoRoot, 'Cargo.lock');
const metaPath = join(here, '..', 'package.json');

function readTextLf(path) {
  return readFileSync(path, 'utf8').replace(/\r\n/g, '\n');
}

function workspaceVersionPackageNames(root) {
  const cratesDir = join(root, 'crates');
  const names = [];
  for (const entry of readdirSync(cratesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const manifest = readTextLf(join(cratesDir, entry.name, 'Cargo.toml'));
    if (!/^\s*version\.workspace\s*=\s*true\s*$/m.test(manifest)) {
      continue;
    }
    const name = manifest.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1];
    if (name) {
      names.push(name);
    }
  }
  return new Set(names);
}

const cargo = readTextLf(cargoPath);
let inWorkspacePackage = false;
let changedVersion = false;
const nextCargo = cargo
  .split('\n')
  .map((line) => {
    if (/^\[/.test(line)) {
      inWorkspacePackage = line === '[workspace.package]';
    }
    if (inWorkspacePackage && /^\s*version\s*=/.test(line) && !changedVersion) {
      changedVersion = true;
      return `version = "${version}"`;
    }
    return line;
  })
  .join('\n');

if (!changedVersion) {
  throw new Error('could not find [workspace.package] version in Cargo.toml');
}
writeFileSync(cargoPath, nextCargo);

const workspaceVersionPackages = workspaceVersionPackageNames(repoRoot);
const changedLockPackages = new Set();
let currentLockPackage = null;
const lock = readTextLf(lockPath);
const nextLock = lock
  .split('\n')
  .map((line) => {
    if (line === '[[package]]') {
      currentLockPackage = null;
      return line;
    }
    const name = line.match(/^name = "([^"]+)"$/)?.[1];
    if (name) {
      currentLockPackage = name;
      return line;
    }
    if (currentLockPackage && workspaceVersionPackages.has(currentLockPackage) && /^version = /.test(line)) {
      changedLockPackages.add(currentLockPackage);
      return `version = "${version}"`;
    }
    return line;
  })
  .join('\n');

const missingLockPackages = [...workspaceVersionPackages].filter((name) => !changedLockPackages.has(name));
if (missingLockPackages.length > 0) {
  throw new Error(`could not find package versions in Cargo.lock: ${missingLockPackages.join(', ')}`);
}
writeFileSync(lockPath, nextLock);

const meta = JSON.parse(readFileSync(metaPath, 'utf8'));
meta.version = version;
meta.optionalDependencies = Object.fromEntries(
  PLATFORMS.map((p) => [platformPackageName(p), version]),
);
writeFileSync(metaPath, JSON.stringify(meta, null, 2) + '\n');

console.log(`stamped lmprobe npm version ${version}`);
