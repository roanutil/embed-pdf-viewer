#!/usr/bin/env node
/**
 * cargo -> wasm-bindgen -> packages/geometry/wasm/
 *
 * Deliberately not wasm-pack. Doing the two steps by hand is what the real
 * repo already does for its PDFium runtime, and it keeps the wasm-bindgen
 * version pin visible in one place instead of hidden inside another tool.
 */
import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, rmSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const crate = 'poc-core';
const artifact = 'poc_core';
const outDir = resolve(root, 'packages/core/wasm');

const run = (cmd, args) => {
  process.stdout.write(`$ ${cmd} ${args.join(' ')}\n`);
  execFileSync(cmd, args, { cwd: root, stdio: 'inherit' });
};

// The crate pins wasm-bindgen exactly; the CLI must match or the glue is wrong
// in ways that only show up at runtime.
const cargoToml = readFileSync(resolve(root, 'crates/core/Cargo.toml'), 'utf8');
const pinned = /wasm-bindgen\s*=\s*"=([\d.]+)"/.exec(cargoToml)?.[1];
const cliVersion = execFileSync('wasm-bindgen', ['--version'], { encoding: 'utf8' })
  .trim()
  .split(/\s+/)
  .pop();
if (pinned !== cliVersion) {
  throw new Error(
    `wasm-bindgen mismatch: crate pins ${pinned}, CLI is ${cliVersion}.\n` +
      `Fix with: cargo install wasm-bindgen-cli --version ${pinned} --force`,
  );
}

run('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown', '-p', crate]);

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

run('wasm-bindgen', [
  resolve(root, `target/wasm32-unknown-unknown/release/${artifact}.wasm`),
  '--out-dir',
  outDir,
  '--target',
  'web',
  '--no-typescript',
]);

// Keep the hand-written .d.ts as the public contract rather than the generated
// one: the wrapper's API is narrower than everything the crate exports.
cpSync(resolve(root, 'packages/core/wasm-types.d.ts'), resolve(outDir, `${artifact}.d.ts`));

const wasmBytes = statSync(resolve(outDir, `${artifact}_bg.wasm`)).size;
process.stdout.write(`\nwasm: ${(wasmBytes / 1024).toFixed(1)} kB at ${outDir}\n`);
