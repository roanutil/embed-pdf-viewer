// node scripts/verify-core.mjs <worktree ...>: checks that a built worktree's
// engine payload carries the Rust engine core, and records it in its stamp,
// so a `rustReads` arm on a recipe other than `rust-core` passes the gate.
// The stamp's key stays as it is: the build itself is unchanged.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { workDir, worktreePath } from './materialize.mjs';

const ids = process.argv.slice(2);
if (ids.length === 0) throw new Error('usage: node scripts/verify-core.mjs <worktree ...>');
for (const id of ids) {
  const stampFile = path.join(workDir(), 'stamps', `${id}.json`);
  if (!existsSync(stampFile)) throw new Error(`${id}: no stamp; build it first (scripts/build-entry.mjs)`);
  execFileSync('node', ['packages/engine/runtime/scripts/verify-rust-core.mjs'], {
    cwd: worktreePath(id),
    stdio: 'inherit',
  });
  const stamp = JSON.parse(readFileSync(stampFile, 'utf8'));
  stamp.rustCore = true;
  writeFileSync(stampFile, JSON.stringify(stamp, null, 2) + '\n');
  console.log(`${id}: rustCore recorded`);
}
