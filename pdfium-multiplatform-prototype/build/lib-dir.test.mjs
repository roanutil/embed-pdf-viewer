// The fetcher and cargo (crates/build-support) both honour EPDF_SCAFFOLD_LIB_DIR.
// The staging scripts must read the library from the same place, or a redirected
// fetch is followed by a copy out of build/libpdfium that was never populated.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { test } from 'node:test';

const FAKE_DYLIB = 'fake libembedpdf.dylib staged from EPDF_SCAFFOLD_LIB_DIR\n';
const FAKE_ARCHIVE = 'fake libembedpdf.a staged from EPDF_SCAFFOLD_LIB_DIR\n';

function scaffold(t) {
  const tmp = mkdtempSync(join(tmpdir(), 'epdf-lib-dir-'));
  t.after(() => rmSync(tmp, { recursive: true, force: true }));
  const root = join(tmp, 'root');
  const libDir = join(tmp, 'elsewhere', 'libs');
  const bin = join(tmp, 'bin');
  const log = join(tmp, 'calls.log');
  for (const dir of [
    'build', 'packages/swift/EpdfOps', 'packages/kotlin/epdf-ops',
    'target/aarch64-apple-darwin/release', 'target/wasm32-unknown-emscripten/release',
  ]) mkdirSync(join(root, dir), { recursive: true });
  mkdirSync(bin, { recursive: true });
  mkdirSync(libDir, { recursive: true });
  writeFileSync(log, '');

  for (const name of ['fetch-libpdfium', 'build-swift', 'build-kotlin', 'link-wasm']) {
    copyFileSync(new URL(`./${name}.sh`, import.meta.url), join(root, `build/${name}.sh`));
  }
  writeFileSync(join(root, 'target/aarch64-apple-darwin/release/libepdf_uniffi.dylib'), 'fake rust dylib');
  writeFileSync(join(root, 'target/wasm32-unknown-emscripten/release/libepdf_wasm.a'), 'fake rust staticlib');

  // A real archive for the real fetcher, served over file://.
  const stage = join(tmp, 'stage');
  mkdirSync(join(stage, 'lib'), { recursive: true });
  mkdirSync(join(stage, 'include'), { recursive: true });
  writeFileSync(join(stage, 'lib/libembedpdf.dylib'), FAKE_DYLIB);
  writeFileSync(join(stage, 'lib/libembedpdf.a'), FAKE_ARCHIVE);
  writeFileSync(join(stage, 'include/fpdfview.h'), '// fake header\n');
  const archive = join(tmp, 'libembedpdf.tar.gz');
  execFileSync('tar', ['-czf', archive, '-C', stage, 'lib', 'include']);
  const sha256 = createHash('sha256').update(readFileSync(archive)).digest('hex');
  const entry = { url: pathToFileURL(archive).href, sha256 };
  const pinFile = join(tmp, 'pin.json');
  writeFileSync(pinFile, JSON.stringify({ artifacts: { 'darwin-arm64': entry, 'wasm32-eh': entry } }));

  const stub = (name, body) => writeFileSync(join(bin, name), `#!/bin/bash\n${body}\n`, { mode: 0o755 });
  stub('rustup', 'exit 0');
  stub('install_name_tool', 'exit 0');
  stub('otool', 'exit 0');
  // `cargo run ... generate --out-dir X` must leave the files build-swift.sh moves.
  stub('cargo', [
    'if [[ "$1" == run ]]; then',
    '  out=""; while [[ $# -gt 0 ]]; do [[ "$1" == --out-dir ]] && out="$2"; shift; done',
    '  mkdir -p "$out" && touch "$out/epdf_uniffiFFI.h" "$out/epdf_uniffiFFI.modulemap"',
    'fi',
    'exit 0',
  ].join('\n'));
  stub('em-config', `echo "${join(tmp, 'emcache')}"`);
  stub('wasm-opt', 'echo "wasm-opt version 123"');
  stub('em++', [
    `printf '%s\\n' "$@" >> "${log}"`,
    'out=""; while [[ $# -gt 0 ]]; do [[ "$1" == -o ]] && out="$2"; shift; done',
    'touch "$out" "${out%.mjs}.wasm"',
  ].join('\n'));

  const run = (script, ...args) => spawnSync('bash', [join(root, 'build', script), ...args], {
    encoding: 'utf8',
    env: {
      ...process.env,
      PATH: `${bin}:${process.env.PATH}`,
      EPDF_SCAFFOLD_PIN_FILE: pinFile,
      EPDF_SCAFFOLD_LIB_DIR: libDir,
    },
  });
  return { root, libDir, log, run };
}

test('build-swift.sh stages libembedpdf from EPDF_SCAFFOLD_LIB_DIR', (t) => {
  const { root, run } = scaffold(t);
  const result = run('build-swift.sh', 'macos-arm64');
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const staged = join(root, 'packages/swift/EpdfOps/Frameworks/macos-arm64/libembedpdf.dylib');
  assert.equal(readFileSync(staged, 'utf8'), FAKE_DYLIB);
  assert.ok(!existsSync(join(root, 'build/libpdfium')), 'the default library root must stay untouched');
});

test('build-kotlin.sh stages libembedpdf from EPDF_SCAFFOLD_LIB_DIR', (t) => {
  const { root, run } = scaffold(t);
  const result = run('build-kotlin.sh', 'host');
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const staged = join(root, 'packages/kotlin/epdf-ops/libs/darwin-aarch64/libembedpdf.dylib');
  assert.equal(readFileSync(staged, 'utf8'), FAKE_DYLIB);
  assert.ok(!existsSync(join(root, 'build/libpdfium')), 'the default library root must stay untouched');
});

test('link-wasm.sh links libembedpdf.a and headers from EPDF_SCAFFOLD_LIB_DIR', (t) => {
  const { root, libDir, log, run } = scaffold(t);
  const result = run('link-wasm.sh');
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const args = readFileSync(log, 'utf8').split('\n');
  assert.ok(args.includes(join(libDir, 'wasm32-eh/lib/libembedpdf.a')), `em++ args:\n${args.join('\n')}`);
  assert.ok(args.includes(`-I${join(libDir, 'wasm32-eh/include')}`), `em++ args:\n${args.join('\n')}`);
  assert.ok(!existsSync(join(root, 'build/libpdfium')), 'the default library root must stay untouched');
});
