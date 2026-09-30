import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { loadConfig } from '../lib/config.mjs';

const write = (obj) => {
  const file = path.join(mkdtempSync(path.join(tmpdir(), 'perf-')), 'arms.json');
  writeFileSync(file, JSON.stringify(obj));
  return file;
};

const valid = {
  baseline: 'a',
  noiseArm: 'b',
  worktrees: { w: { ref: 'main', sha: 'f'.repeat(40), recipe: 'typescript' } },
  arms: {
    a: { worktree: 'w', kernel: 'typescript', rustReads: false },
    b: { worktree: 'w', kernel: 'typescript', rustReads: false },
  },
  pairs: [['a', 'b']],
};

test('the tracked arms.json loads', () => {
  const config = loadConfig();
  assert.deepEqual(config.armOrder, ['main', 'main-aa', 'impl-ts', 'impl-rust', 'plugins', 'plugins-lto', 'plugins-fix', 'plugins-fix34', 'plugins-fix-leak', 'plugins-fix-zoom']);
});

test('armOrder follows file order', () => {
  assert.deepEqual(loadConfig(write(valid)).armOrder, ['a', 'b']);
});

test('rejects an arm naming an unknown worktree', () => {
  const bad = structuredClone(valid);
  bad.arms.a.worktree = 'nope';
  assert.throws(() => loadConfig(write(bad)), /arm a: unknown worktree nope/);
});

test('rejects a short sha', () => {
  const bad = structuredClone(valid);
  bad.worktrees.w.sha = 'abc123';
  assert.throws(() => loadConfig(write(bad)), /worktree w: sha must be 40 hex characters/);
});

test('rejects a pair naming an unknown arm', () => {
  const bad = structuredClone(valid);
  bad.pairs = [['a', 'zzz']];
  assert.throws(() => loadConfig(write(bad)), /pair a\/zzz: unknown arm zzz/);
});

test('rejects rustReads on the rust kernel', () => {
  const bad = structuredClone(valid);
  bad.arms.a = { worktree: 'w', kernel: 'rust', rustReads: true };
  assert.throws(() => loadConfig(write(bad)), /arm a: rustReads needs the typescript kernel/);
});
