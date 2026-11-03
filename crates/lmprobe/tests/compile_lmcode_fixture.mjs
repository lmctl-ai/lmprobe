// Maintainer test helper: compile an lmcode source file to the exact bytecode
// consumed by the host, using the package paired in LMSCRIPT_BIN.
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [input, output] = process.argv.slice(2);
if (!input || !output || !process.env.LMSCRIPT_BIN) {
  throw new Error('usage: node compile_lmcode_fixture.mjs INPUT OUTPUT with LMSCRIPT_BIN set');
}
const cli = fs.realpathSync(process.env.LMSCRIPT_BIN);
const dist = path.resolve(path.dirname(cli), '../dist');
const { CompiledDsl } = await import(pathToFileURL(path.join(dist, 'dsl.js')).href);
const { LMScript } = await import(pathToFileURL(path.join(dist, 'lmscript.js')).href);
let bytes;
const engine = new LMScript();
engine.registerFunction('lmprobe_test_capture', (program) => {
  bytes = new Uint8Array(CompiledDsl.bytesOf(program));
  return null;
}, 1, { isDefault: true });
const sourcePath = path.resolve(input);
const source = fs.readFileSync(sourcePath, 'utf8');
const result = await engine.runAsync(
  `${source}\nlmprobe_test_capture(program);\n`,
  { file: sourcePath, platform: { cwd: path.dirname(sourcePath), env: process.env } },
);
if (!result.success) {
  for (const diagnostic of result.diagnostics) {
    console.error(`${sourcePath}:${diagnostic.line}:${diagnostic.column}: ${diagnostic.message}`);
  }
  process.exitCode = result.exitCode ?? 2;
} else {
  if (!bytes) throw new Error('test program did not construct bytecode');
  fs.writeFileSync(output, bytes);
}
