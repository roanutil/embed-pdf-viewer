import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { parseRunOptions } from './run-options.mjs';

test('valid options preserve defaults and explicit values', () => {
  assert.deepEqual(parseRunOptions([]), { native: false, rounds: 7, budgetMs: 500, out: undefined });
  assert.deepEqual(parseRunOptions(['--native', '--rounds', '2', '--budget', '0.5', '--out', 'my results']),
    { native: true, rounds: 2, budgetMs: 0.5, out: 'my results' });
});

test('malformed invocations fail before touching existing results', (t) => {
  const out = mkdtempSync(join(tmpdir(), 'pdfium-invalid-options-'));
  t.after(() => rmSync(out, { recursive: true, force: true }));
  const sentinel = join(out, 'a-r1.json');
  const nativeSentinel = join(out, 'native-a-r1.json');
  writeFileSync(sentinel, 'existing measurements');
  writeFileSync(nativeSentinel, 'existing native measurements');
  const cases = [
    ['--rounds'], ['--budget'], ['--rounds', '--native'],
    ...['0', '-1', 'NaN', 'Infinity', '1.5', '9007199254740992'].map((v) => ['--rounds', v]),
    ...['0', '-1', 'NaN', 'Infinity', ''].map((v) => ['--budget', v]),
    ['--rounds', '1', '--rounds', '2'], ['--typo'], ['--native', '--rounds', 'nope'],
  ];
  for (const args of cases) {
    const result = spawnSync(process.execPath,
      [fileURLToPath(new URL('./run.mjs', import.meta.url)), '--out', out, ...args],
      { encoding: 'utf8', timeout: 10_000 });
    assert.equal(result.status, 1, JSON.stringify({ args, ...result }));
    assert.match(result.stderr, /Invalid benchmark arguments/);
    assert.equal(readFileSync(sentinel, 'utf8'), 'existing measurements');
    assert.equal(readFileSync(nativeSentinel, 'utf8'), 'existing native measurements');
  }
  assert.throws(() => parseRunOptions(['--out']), /requires a value/);
});
