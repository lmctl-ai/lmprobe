import { spawnSync } from 'node:child_process';
import { chmodSync, cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';

const repoRoot = path.resolve(import.meta.dirname, '..', '..');
const sourceLauncher = path.join(repoRoot, 'npm', 'bin', 'lmprobe.js');

function makeFixture(t) {
  const root = path.join(tmpdir(), `lmprobe-launcher-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const binDir = path.join(root, 'bin');
  mkdirSync(binDir, { recursive: true });
  cpSync(sourceLauncher, path.join(binDir, 'lmprobe.js'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function writePlatformPackage(root, body, mode = 0o755) {
  const pkgDir = path.join(
    root,
    'node_modules',
    '@lmctl-ai',
    `lmprobe-${process.platform}-${process.arch}`,
  );
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(path.join(pkgDir, 'package.json'), '{"name":"fixture","version":"0.0.0"}\n');
  const binary = path.join(pkgDir, process.platform === 'win32' ? 'lmprobe.exe' : 'lmprobe');
  writeFileSync(binary, body);
  chmodSync(binary, mode);
  return binary;
}

function runLauncher(root) {
  return spawnSync(process.execPath, [path.join(root, 'bin', 'lmprobe.js')], {
    cwd: root,
    encoding: 'utf8',
  });
}

test('missing platform package exits 127', (t) => {
  const root = makeFixture(t);
  const result = runLauncher(root);
  assert.equal(result.status, 127);
  assert.match(result.stderr, /no prebuilt binary/);
});

test('launcher preserves binary exit status', (t) => {
  const root = makeFixture(t);
  writePlatformPackage(root, '#!/usr/bin/env node\nprocess.exit(2);\n');
  const result = runLauncher(root);
  assert.equal(result.status, 2);
});

test('failed binary launch exits 126', { skip: process.platform === 'win32' }, (t) => {
  const root = makeFixture(t);
  writePlatformPackage(root, '#!/usr/bin/env node\nprocess.exit(0);\n', 0o644);
  const result = runLauncher(root);
  assert.equal(result.status, 126);
  assert.match(result.stderr, /failed to launch/);
});

test('signal termination exits as 128 plus signal number', { skip: process.platform === 'win32' }, (t) => {
  const root = makeFixture(t);
  writePlatformPackage(root, '#!/usr/bin/env node\nprocess.kill(process.pid, "SIGTERM");\n');
  const result = runLauncher(root);
  assert.equal(result.status, 128 + 15);
});
