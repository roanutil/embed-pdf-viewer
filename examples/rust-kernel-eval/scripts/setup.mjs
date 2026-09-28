#!/usr/bin/env node
/**
 * Fresh-worktree setup for this eval app: the gitignored build output that
 * `pnpm dev` assumes but does not produce itself.
 *
 * `pnpm dev` already runs `pnpm --filter @embedpdf/core-wasm build:wasm`,
 * which builds the Rust kernel. Everything below is the REST of the viewer
 * chrome's dev-time dependencies, none of which that build touches:
 *
 *   - packages/engine/main/workers/         (this script builds it)
 *   - packages/default-stamps/locales/      (this script builds it)
 *   - packages/engine/runtime/dist/ and
 *     packages/engine/runtime/npm/wasm32/lib/ (the PDFium runtime artifacts;
 *     this script only CHECKS for them — see the error below for how to get
 *     them)
 *
 * Run once per checkout, before `pnpm dev`. Kept separate from `dev` rather
 * than chained in front of it: the engine rebuild alone costs ~15-20s with
 * no cache, on every single `pnpm dev` (see examples/rust-kernel-eval/README.md).
 */
import { spawnSync } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const engineMain = path.join(root, 'packages/engine/main');
const runtimeDist = path.join(root, 'packages/engine/runtime/dist/index.browser.js');
const runtimeWasm = path.join(root, 'packages/engine/runtime/npm/wasm32/lib/embedpdf.wasm');

function fail(lines) {
  console.error(
    ['✖ setup aborted (examples/rust-kernel-eval/scripts/setup.mjs):', ...lines].join('\n'),
  );
  process.exit(1);
}

function run(command, args, cwd) {
  const where = cwd === root ? '' : `  (in ${path.relative(root, cwd)})`;
  console.log(`$ ${command} ${args.join(' ')}${where}`);
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.status !== 0) {
    fail([`  \`${command} ${args.join(' ')}\` exited ${result.status ?? result.signal}.`]);
  }
}

// The PDFium runtime artifacts are gitignored build output this repo does
// not fetch or build for you. Nothing below can substitute for them, so
// check first and fail with the exact commands rather than let the engine
// build silently produce a dist/ with no working WASM.
const haveWasm = existsSync(runtimeWasm) && statSync(runtimeWasm).size > 0;
const haveDist = existsSync(runtimeDist);
if (!haveWasm || !haveDist) {
  fail([
    '  packages/engine/runtime is missing its PDFium runtime artifacts:',
    `    - ${path.relative(root, runtimeDist)} (${haveDist ? 'ok' : 'missing'})`,
    `    - ${path.relative(root, runtimeWasm)} (${haveWasm ? 'ok' : 'missing'})`,
    '',
    '  Fetch the prebuilt payload, then build the JS wrapper around it:',
    '    pnpm --filter @embedpdf/engine-runtime run fetch:payload',
    '    pnpm --filter @embedpdf/engine-runtime build',
    '',
    '  If no payload is published for this checkout (fetch:payload prints the',
    '  release URL it tried and exits nonzero), build it locally instead. That',
    '  needs the Emscripten SDK 3.1.72, cmake, and depot_tools (gn/ninja/gclient)',
    '  on PATH:',
    '    pnpm --filter @embedpdf/engine-runtime run dev        # bootstraps emsdk, builds this host',
    '  or, for every platform target instead of just this host:',
    '    pnpm --filter @embedpdf/engine-runtime run build:local',
    '  (packages/engine/runtime/scripts/test-pdfium-docker.sh also builds the',
    '  runtime in Docker, but to run the PDFium test suite, not to produce',
    '  this payload — it is not a substitute for the two commands above.)',
    '',
    '  Fastest for a one-off eval session: copy',
    '  packages/engine/runtime/dist/ and',
    '  packages/engine/runtime/npm/wasm32/lib/ from another checkout that has',
    '  already built or fetched them.',
  ]);
}

// packages/engine/main/workers/: gitignored. @embedpdf/engine's
// `./worker-source` export always points there, dev or not, so the eval
// app's `dev` (which only builds the Rust kernel) never produces it.
run('pnpm', ['--filter', '@embedpdf/engine^...', 'build'], root);
run('node', ['scripts/build-workers.mjs'], engineMain);

// packages/default-stamps/locales/: gitignored. `./library` always imports
// the generated locale modules, dev or not.
run('pnpm', ['--filter', '@embedpdf/default-stamps', 'build'], root);

console.log(
  '✓ setup complete: packages/engine/main/workers/ and packages/default-stamps/locales/ are built.',
);
