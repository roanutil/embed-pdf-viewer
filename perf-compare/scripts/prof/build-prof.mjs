// node scripts/prof/build-prof.mjs <arm>: a profiling build of a built
// Rust-kernel arm, in .work/dist/<arm>-prof. The bundle is unminified and
// the kernel keeps its function names (`wasm-opt -O3 -g` in place of
// `--strip-debug`), so a CPU profile names Rust and viewer frames. It first
// checks that the stripped rebuild is byte-identical to the arm's shipped
// kernel, so the profile is of the code the harness timed. Run
// scripts/build-entry.mjs <arm> first.
import { execFileSync } from 'node:child_process';
import { copyFileSync, createReadStream, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../../lib/config.mjs';
import { workDir, worktreePath } from '../materialize.mjs';

const FEATURES = [
  '--enable-reference-types',
  '--enable-bulk-memory',
  '--enable-mutable-globals',
  '--enable-sign-ext',
  '--enable-nontrapping-float-to-int',
  '--enable-multivalue',
];
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { stdio: 'inherit', ...opts });
const sha = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');
const kernelIn = (dir) => {
  const assets = path.join(dir, 'assets');
  const name = readdirSync(assets).find((f) => /^epdf_web_bg-.*\.wasm$/.test(f));
  if (!name) throw new Error(`${dir}: no epdf_web_bg-*.wasm; is this a Rust-kernel arm?`);
  return path.join(assets, name);
};

const id = process.argv[2];
if (!id) throw new Error('usage: node scripts/prof/build-prof.mjs <arm>');
const config = loadConfig();
const arm = { id, ...config.arms[id] };
if (!config.arms[id]) throw new Error(`unknown arm ${id}`);
if (arm.kernel !== 'rust') throw new Error(`${id}: only Rust-kernel arms have a kernel to name`);
const dir = worktreePath(arm.worktree);
const shipped = path.join(workDir(), 'dist', id);
const out = path.join(workDir(), 'dist', `${id}-prof`);
const scratch = path.join(workDir(), 'prof', `kernel-${id}`);
mkdirSync(scratch, { recursive: true });

// The kernel, named and stripped, from the arm's own cargo output.
run('wasm-bindgen', ['--target', 'web', '--out-dir', scratch, 'target/wasm32-unknown-unknown/release/epdf_web.wasm'], { cwd: dir });
const raw = path.join(scratch, 'epdf_web_bg.wasm');
const stripped = path.join(scratch, 'stripped.wasm');
const named = path.join(scratch, 'named.wasm');
run('wasm-opt', ['-O3', '--strip-debug', ...FEATURES, '-o', stripped, raw]);
run('wasm-opt', ['-O3', '-g', ...FEATURES, '-o', named, raw]);
if (sha(stripped) !== sha(kernelIn(shipped))) {
  throw new Error(`${id}: the stripped rebuild differs from the shipped kernel; rebuild the arm first`);
}

// The bundle, unminified, from the staged entry build-entry.mjs left.
const staged = path.join(dir, 'packages/viewer/main/.perf-entry');
const viteConfig = readFileSync(path.join(staged, 'vite.config.mjs'), 'utf8');
if (!viteConfig.includes('sourcemap: false,')) throw new Error(`${staged}/vite.config.mjs: no 'sourcemap: false,' to extend`);
writeFileSync(path.join(staged, 'vite.prof.mjs'), viteConfig.replace('sourcemap: false,', 'sourcemap: false, minify: false,'));
run('pnpm', ['exec', 'vite', 'build', '--config', path.join(staged, 'vite.prof.mjs')], {
  cwd: path.join(dir, 'packages/viewer/main'),
  env: {
    ...process.env,
    PERF_ARM: JSON.stringify({ id: arm.id, kernel: arm.kernel, rustReads: arm.rustReads, dedupeCompat: arm.dedupeCompat === true }),
    PERF_WORKTREE: dir,
    PERF_ENTRY: staged,
    PERF_FIXTURES: fileURLToPath(new URL('../../fixtures', import.meta.url)),
    PERF_OUT: out,
  },
});
copyFileSync(named, kernelIn(out));
console.log(`${id}-prof: ${out} (kernel ${sha(stripped).slice(0, 12)}, named)`);
