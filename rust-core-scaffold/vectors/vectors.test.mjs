import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

test('vectors file is internally consistent and frozen', async () => {
  const v = JSON.parse(await readFile(resolve(here, 'report-page4.json'), 'utf8'));

  assert.equal(v.fixture, 'fixtures/report.pdf');
  assert.equal(v.forkSha, 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6');
  assert.equal(v.pageIndex, 4);
  assert.equal(v.charCount, 8106);
  assert.equal(v.text.length, v.charCount);
  assert.ok(v.pageCount > v.pageIndex, 'page count must include page 4');
  assert.ok(v.size.width > 0 && v.size.height > 0);
  assert.ok(v.search.hitCount >= v.search.firstHits.length);

  const golden = resolve(here, v.render.golden);
  const { size } = await stat(golden);
  assert.equal(size, v.render.stride * v.render.height);
  assert.equal(v.render.stride, v.render.width * 4);
});
