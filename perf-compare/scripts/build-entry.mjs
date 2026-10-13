// Builds the shared entry (entry/) into every arm. Kept out of build-arm.mjs
// because that file's contents are the worktree cache key: an entry edit
// shouldn't rebuild three worktrees.
//
// `node scripts/build-entry.mjs [arm ...]` builds each named arm's worktree
// (cached) and then its entry, which is rebuilt every time: it takes seconds,
// and caching it would hide edits to entry/.
import { execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../lib/config.mjs';
import { buildWorktree } from './build-arm.mjs';
import { workDir, worktreePath } from './materialize.mjs';

const entrySource = fileURLToPath(new URL('../entry', import.meta.url));
const fixturesDir = fileURLToPath(new URL('../fixtures', import.meta.url));

function sh(cmd, cwd, env = {}) {
  console.log(`  $ ${cmd}`);
  execFileSync('bash', ['-euo', 'pipefail', '-c', cmd], {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
}

const rustKernelShim = (dir) => path.join(dir, 'examples/rust-kernel-eval/perf-kernel-rust.ts');

// Copies entry/ into the worktree so every import in it, including the Vite
// config's own, resolves from that worktree's packages/viewer/main.
function stageEntry(arm) {
  const dir = worktreePath(arm.worktree);
  const staged = path.join(dir, 'packages/viewer/main/.perf-entry');
  rmSync(staged, { recursive: true, force: true });
  cpSync(entrySource, staged, { recursive: true });
  if (arm.kernel === 'rust') copyFileSync(path.join(entrySource, 'kernel-rust.ts'), rustKernelShim(dir));
  return staged;
}

// `tsc` doesn't take `paths` on the command line, so each arm gets its own
// tsconfig extending the viewer's.
function typecheckEntry(arm, staged) {
  const dir = worktreePath(arm.worktree);
  const viewerMain = path.join(dir, 'packages/viewer/main');
  const kernel = arm.kernel === 'rust' ? rustKernelShim(dir) : path.join(staged, 'kernel-typescript.ts');
  const file = path.join(workDir(), 'tsconfig', `${arm.id}.json`);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(
    file,
    JSON.stringify({
      // Inherit the viewer's options: its source needs vite/client types for `?inline` imports.
      extends: path.join(viewerMain, 'tsconfig.json'),
      compilerOptions: {
        noEmit: true,
        // The entry builds for es2022 (entry/vite.config.mjs), and the Rust
        // kernel's proxy uses WeakRef and FinalizationRegistry, as the eval
        // app's own tsconfig allows.
        target: 'ES2022',
        lib: ['ES2022', 'DOM', 'DOM.Iterable'],
        baseUrl: viewerMain,
        paths: { '@perf/door': ['src/doors/local.ts'], '@perf/kernel': [kernel] },
      },
      files: [path.join(staged, 'main.ts'), path.join(staged, 'globals.d.ts')],
      include: [], // otherwise the viewer's own `include` is inherited
    }),
  );
  sh(`pnpm exec tsc -p ${file}`, viewerMain);
}

export function buildEntry(armId, config) {
  const arm = { id: armId, ...config.arms[armId] };
  const dir = worktreePath(arm.worktree);
  const out = path.join(workDir(), 'dist', armId);
  const staged = stageEntry(arm);
  typecheckEntry(arm, staged);
  sh(`pnpm exec vite build --config ${path.join(staged, 'vite.config.mjs')}`, path.join(dir, 'packages/viewer/main'), {
    PERF_ARM: JSON.stringify({ id: arm.id, kernel: arm.kernel, rustReads: arm.rustReads, dedupeCompat: arm.dedupeCompat === true }),
    PERF_WORKTREE: dir,
    PERF_ENTRY: staged,
    PERF_FIXTURES: fixturesDir,
    PERF_OUT: out,
  });
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const config = loadConfig();
  const only = process.argv.slice(2);
  const arms = only.length ? config.armOrder.filter((id) => only.includes(id)) : config.defaultArms;
  const built = new Set();
  for (const id of arms) {
    const worktree = config.arms[id].worktree;
    if (!built.has(worktree)) {
      console.log(buildWorktree(worktree, config));
      built.add(worktree);
    }
    console.log(`${id}: ${buildEntry(id, config)}`);
  }
}
