import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { test } from 'node:test';

const here = import.meta.dirname;

test('a two-round run writes eight result files with plausible cells', () => {
  execFileSync('node', [resolve(here, 'run.mjs'), '--rounds', '2', '--budget', '100'], {
    stdio: ['ignore', 'inherit', 'inherit'],
  });

  for (const arm of ['a', 'b', 'c', 'd']) {
    for (const r of [1, 2]) {
      const f = resolve(here, `results/${arm}-r${r}.json`);
      assert.ok(existsSync(f), `missing ${f}`);
      const j = JSON.parse(readFileSync(f, 'utf8'));
      assert.equal(j.arm, arm);
      assert.ok(j.wasmBytes > 100_000, `implausible wasm size ${j.wasmBytes}`);
      assert.ok(j.cells.geometry.ns > 0);
      assert.ok(j.env.cpu && j.env.node, 'environment stamp missing');
      assert.notEqual(j.emsdk, 'unknown');
    }
  }

  const a = JSON.parse(readFileSync(resolve(here, 'results/a-r1.json'), 'utf8'));
  assert.ok(
    a.cells['render-4x'].ns > a.cells['render-1x'].ns,
    'rendering at 4x should cost more than at 1x; if not, the bitmap is not being drawn',
  );
  assert.ok(
    a.cells.geometry.ns > a.cells['null-call'].ns * 100,
    'the glyph loop should dwarf a single null call',
  );
});
