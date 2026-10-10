#!/usr/bin/env node
'use strict';

// Launcher shim for the @lmctl-ai/lmprobe meta package.
//
// lmprobe ships as a native Rust binary. The meta package carries no binary
// itself; instead it declares one optionalDependency per platform
// (@lmctl-ai/lmprobe-<os>-<cpu>), each shipping the prebuilt binary with `os`
// and `cpu` fields so npm installs ONLY the one matching the host. This shim
// resolves that platform package and execs its binary, forwarding argv, stdio,
// and exit code. (Same pattern as esbuild / @ast-grep/cli.)

const { spawnSync } = require('child_process');
const os = require('os');
const path = require('path');

const EXIT_CANNOT_EXECUTE = 126;
const EXIT_NOT_FOUND = 127;

function resolveBinary() {
  const { platform, arch } = process;
  const pkg = `@lmctl-ai/lmprobe-${platform}-${arch}`;
  const binName = platform === 'win32' ? 'lmprobe.exe' : 'lmprobe';
  try {
    // Resolve via package.json (always resolvable; avoids `exports`/extension
    // edge cases), then join the binary next to it.
    const pkgJson = require.resolve(`${pkg}/package.json`);
    return path.join(path.dirname(pkgJson), binName);
  } catch {
    return null;
  }
}

const binary = resolveBinary();
if (binary === null) {
  process.stderr.write(
    `lmprobe: no prebuilt binary for ${process.platform}-${process.arch}.\n` +
      'Supported platforms: linux-x64, linux-arm64, darwin-x64, darwin-arm64, win32-x64.\n' +
      'If you installed with --no-optional or --omit=optional, reinstall without it.\n',
  );
  process.exit(EXIT_NOT_FOUND);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  process.stderr.write(`lmprobe: failed to launch ${binary}: ${result.error.message}\n`);
  process.exit(EXIT_CANNOT_EXECUTE);
}
// Mirror signal-terminations as the conventional 128+signal exit code.
if (result.signal) {
  const signalNumber = os.constants.signals[result.signal];
  process.exit(typeof signalNumber === 'number' ? 128 + signalNumber : 1);
}
process.exit(result.status === null ? 1 : result.status);
