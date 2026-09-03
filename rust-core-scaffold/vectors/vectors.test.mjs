import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
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

  // A fixture swap invalidates every value in this file, and the recorded
  // digest is the one kind of drift the file can self-police.
  const fixtureBytes = await readFile(resolve(here, '..', v.fixture));
  const fixtureSha256 = createHash('sha256').update(fixtureBytes).digest('hex');
  assert.equal(fixtureSha256, v.fixtureSha256);

  const golden = resolve(here, v.render.golden);
  const { size } = await stat(golden);
  assert.equal(size, v.render.stride * v.render.height);
  assert.equal(v.render.stride, v.render.width * 4);

  // Asserted by size alone, 121,176 bytes of garbage would pass. Catch
  // that class: alpha must be opaque everywhere, the first pixel must be
  // the FillRect white (proving FillRect ran before the render), and the
  // buffer must not be a single repeated byte.
  const goldenBytes = await readFile(golden);
  assert.ok(
    goldenBytes.length % 4 === 0,
    'golden buffer length must be a whole number of BGRA pixels',
  );
  for (let i = 3; i < goldenBytes.length; i += 4) {
    assert.equal(goldenBytes[i], 0xff, `alpha at byte offset ${i} must be 0xFF`);
  }
  assert.deepEqual(
    [...goldenBytes.subarray(0, 4)],
    [0xff, 0xff, 0xff, 0xff],
    'first pixel must be opaque white, proving FPDFBitmap_FillRect ran',
  );
  assert.ok(
    !goldenBytes.every((byte) => byte === goldenBytes[0]),
    'golden buffer must not be a single repeated byte value',
  );
});
