import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  fileDigests,
  identityOf,
  lockHeld,
  parseProcessRows,
  recordedGroupAlive,
} from './publish-guard.mjs';

const PS = [
  '  100   100 Tue Nov  3 19:11:02 2026 node scripts/publish-engine-payload.mjs --dry-run',
  '  200   200 Tue Nov  3 19:12:00 2026 bash -euo pipefail -c pnpm --filter x build',
  '  201   200 Tue Nov  3 19:12:01 2026 node build/generate-wasm-inline.mjs',
  '  300   300 Tue Nov  3 19:13:00 2026 sleep 60',
  '  401   400 Tue Nov  3 19:14:00 2026 sleep 5',
].join('\n');
const rows = parseProcessRows(PS);

test('parseProcessRows reads pid, pgid, start and command', () => {
  assert.equal(rows.length, 5);
  assert.deepEqual(rows[1], {
    pid: 200,
    pgid: 200,
    lstart: 'Tue Nov 3 19:12:00 2026',
    args: 'bash -euo pipefail -c pnpm --filter x build',
  });
  assert.deepEqual(identityOf(rows, 100), {
    start: 'Tue Nov 3 19:11:02 2026',
    args: 'node scripts/publish-engine-payload.mjs --dry-run',
  });
  assert.equal(identityOf(rows, 999), null);
});

test('a recorded group is alive only as itself', () => {
  const ours = { pgid: 200, start: 'Tue Nov 3 19:12:00 2026', args: 'bash -euo pipefail -c' };
  assert.equal(recordedGroupAlive(rows, ours), true);
  // The same id, but a leader started at another time.
  assert.equal(recordedGroupAlive(rows, { ...ours, start: 'Tue Nov 3 18:00:00 2026' }), false);
  assert.equal(
    recordedGroupAlive(rows, { pgid: 300, start: 'Mon Nov 2 19:13:00 2026', args: 'bash -euo' }),
    false,
  );
  // No process in the group.
  assert.equal(recordedGroupAlive(rows, { ...ours, pgid: 500 }), false);
  // Leader gone, a member left: still ours.
  assert.equal(recordedGroupAlive(rows, { ...ours, pgid: 400 }), true);
  assert.equal(recordedGroupAlive(rows, null), false);
});

test('a run lock is held only by its own process', () => {
  assert.equal(lockHeld(rows, { pid: 100, start: 'Tue Nov 3 19:11:02 2026' }), true);
  assert.equal(lockHeld(rows, { pid: 100, start: 'Mon Nov 2 10:00:00 2026' }), false);
  assert.equal(lockHeld(rows, { pid: 999, start: 'Tue Nov 3 19:11:02 2026' }), false);
  assert.equal(lockHeld(rows, null), false);
});

test('fileDigests walks subdirectories and leaves out the marker', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'digests-'));
  try {
    writeFileSync(path.join(dir, 'a.js'), 'a');
    writeFileSync(path.join(dir, '.payload-source'), 'x');
    mkdirSync(path.join(dir, 'sub'));
    writeFileSync(path.join(dir, 'sub', 'b.wasm'), 'b');
    const digests = fileDigests(dir);
    assert.deepEqual(Object.keys(digests), ['a.js', 'sub/b.wasm']);
    assert.equal(
      digests['a.js'],
      'ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb',
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
