import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { parseRunOptions } from './run-options.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));

test('valid options preserve defaults and explicit values', () => {
  assert.deepEqual(parseRunOptions([]), { rounds: 5, budgetMs: 500, allocRounds: 3 });
  assert.deepEqual(parseRunOptions(['--rounds', '2', '--budget', '0.5', '--alloc-rounds', '1']),
    { rounds: 2, budgetMs: 0.5, allocRounds: 1 });
});

test('malformed invocations fail before touching per-round results', (t) => {
  // A file matching run.mjs's PER_ROUND pattern, which a run is entitled to
  // delete. It lives in bench/results because run.mjs has no --out; the
  // pattern is gitignored so a failed test cannot dirty the checkout.
  const sentinel = join(here, 'results', 'sentinel-probe-r99.json');
  writeFileSync(sentinel, 'existing measurements');
  t.after(() => rmSync(sentinel, { force: true }));
  const cases = [
    ['--rounds'], ['--budget'], ['--alloc-rounds'], ['--rounds', '--budget', '1'],
    ...['0', '-1', 'NaN', 'Infinity', '1.5', '9007199254740992'].map((v) => ['--rounds', v]),
    ...['0', '-1', 'NaN', 'Infinity', ''].map((v) => ['--budget', v]),
    ...['0', '-2', '1.5'].map((v) => ['--alloc-rounds', v]),
    ['--rounds', '1', '--rounds', '2'], ['--typo'], ['--out', 'somewhere'],
  ];
  for (const args of cases) {
    const result = spawnSync(process.execPath, [join(here, 'run.mjs'), ...args],
      { encoding: 'utf8', timeout: 20_000 });
    assert.equal(result.status, 1, JSON.stringify({ args, status: result.status, stderr: result.stderr }));
    assert.match(result.stderr, /Invalid benchmark arguments/, JSON.stringify(args));
    assert.ok(existsSync(sentinel), `${args.join(' ')} deleted a result file`);
    assert.equal(readFileSync(sentinel, 'utf8'), 'existing measurements');
  }
});
