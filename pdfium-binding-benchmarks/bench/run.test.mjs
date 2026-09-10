import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { test } from 'node:test';

const here = import.meta.dirname;

/**
 * `--out` is not optional here.
 *
 * bench/results/ holds the 56 committed files behind every table in
 * RESULTS.md, and run.mjs clears the ones it is about to replace. Without
 * --out this test traded seven rounds at a 500 ms budget for two rounds at
 * 100 ms, which is the same data loss commit a31c1123 was written to undo.
 */
test('a two-round run writes eight result files with plausible cells', (t) => {
  const out = mkdtempSync(resolve(tmpdir(), 'pdfium-spike-bench-'));
  t.after(() => rmSync(out, { recursive: true, force: true }));

  execFileSync(
    'node',
    [resolve(here, 'run.mjs'), '--rounds', '2', '--budget', '100', '--out', out],
    { stdio: ['ignore', 'inherit', 'inherit'] },
  );

  for (const arm of ['a', 'b', 'c', 'd']) {
    for (const r of [1, 2]) {
      const f = resolve(out, `${arm}-r${r}.json`);
      assert.ok(existsSync(f), `missing ${f}`);
      const j = JSON.parse(readFileSync(f, 'utf8'));
      assert.equal(j.arm, arm);
      assert.ok(j.wasmBytes > 100_000, `implausible wasm size ${j.wasmBytes}`);
      assert.ok(j.cells.geometry.ns > 0);
      assert.ok(j.env.cpu && j.env.node, 'environment stamp missing');
      assert.notEqual(j.emsdk, 'unknown');
    }
  }

  const a = JSON.parse(readFileSync(resolve(out, 'a-r1.json'), 'utf8'));
  assert.ok(
    a.cells['render-4x'].ns > a.cells['render-1x'].ns,
    'rendering at 4x should cost more than at 1x; if not, the bitmap is not being drawn',
  );
  assert.ok(
    a.cells.geometry.ns > a.cells['null-call'].ns * 100,
    'the glyph loop should dwarf a single null call',
  );
});
