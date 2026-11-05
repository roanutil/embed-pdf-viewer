import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import * as builder from './build-arm.mjs';

const sha = '1'.repeat(40);
const target = 'darwin-static-x64';
const digest = (data) => createHash('sha256').update(data).digest('hex');
function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'fork-cache-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(path.join(root, 'lib'));
  writeFileSync(path.join(root, 'lib/libembedpdf.a'), 'fixture archive');
  writeFileSync(path.join(root, 'BUILD-METADATA.json'), JSON.stringify({ sha, target }));
  return {
    root,
    stamp: { recipe: 4, sha, target, staging: root, libSha256: digest('fixture archive') },
  };
}
function matches(stamp) {
  assert.equal(
    typeof builder.forkStampMatches,
    'function',
    'source/target/content cache guard is required',
  );
  return builder.forkStampMatches(stamp, { recipe: 4, sha, target });
}

test('fork cache accepts only matching source target recipe and actual archive bytes', (t) => {
  const { stamp } = fixture(t);
  assert.equal(matches(stamp), true);
});
test('fork cache rejects changed archive bytes even when staging exists', (t) => {
  const { root, stamp } = fixture(t);
  writeFileSync(path.join(root, 'lib/libembedpdf.a'), 'corrupted');
  assert.equal(matches(stamp), false);
});
test('fork cache rejects old-source metadata behind a new cache stamp', (t) => {
  const { root, stamp } = fixture(t);
  writeFileSync(
    path.join(root, 'BUILD-METADATA.json'),
    JSON.stringify({ sha: '2'.repeat(40), target }),
  );
  assert.equal(matches(stamp), false);
});
test('fork cache rejects wrong target and stale recipe', (t) => {
  const { stamp } = fixture(t);
  assert.equal(matches({ ...stamp, target: 'ios-arm64' }), false);
  assert.equal(matches({ ...stamp, recipe: 3 }), false);
});
test('fork cache rejects missing metadata and partial archives', (t) => {
  const { root, stamp } = fixture(t);
  rmSync(path.join(root, 'BUILD-METADATA.json'));
  assert.equal(matches(stamp), false);
});
test('native dependency coverage requires iOS explicitly in its source stamp', (t) => {
  assert.equal(
    typeof builder.depsSupportTarget,
    'function',
    'native dependency OS coverage guard is required',
  );
  const { root } = fixture(t);
  mkdirSync(path.join(root, '.embedpdf-runtime'));
  writeFileSync(
    path.join(root, '.embedpdf-runtime/deps-sync.stamp'),
    'target_os_list=emscripten mac\n',
  );
  assert.equal(builder.depsSupportTarget(root, 'ios-arm64', 'darwin'), false);
  writeFileSync(
    path.join(root, '.embedpdf-runtime/deps-sync.stamp'),
    'target_os_list=emscripten ios mac\n',
  );
  assert.equal(builder.depsSupportTarget(root, 'ios-sim-arm64', 'darwin'), true);
  assert.equal(builder.depsSupportTarget(root, 'darwin-static-x64', 'darwin'), true);
});
