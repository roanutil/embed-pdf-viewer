import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { promisify } from 'node:util';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadVectors, meanAbsDiff, maxAbsDiff } from '../../packages/test-support/vectors.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const run = promisify(execFile);

const { vectors, golden } = await loadVectors();

test('the CLI reports the frozen numbers and writes a readable BMP', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'epdf-cli-'));
  const out = join(dir, 'page.bmp');

  const { stdout } = await run('node', [
    resolve(here, 'main.mjs'),
    '--page', String(vectors.pageIndex),
    '--scale', String(vectors.render.scale),
    '--out', out,
  ]);

  assert.match(stdout, new RegExp(`pages: ${vectors.pageCount}\\b`));
  assert.match(stdout, new RegExp(`chars: ${vectors.charCount}\\b`));
  assert.match(stdout, new RegExp(`hits: ${vectors.search.hitCount}\\b`));
  assert.match(stdout, new RegExp(`${vectors.render.width}x${vectors.render.height}`));

  const bmp = await readFile(out);
  assert.equal(bmp.subarray(0, 2).toString('ascii'), 'BM');
  assert.equal(bmp.readUInt32LE(2), bmp.length);
  assert.equal(bmp.readInt32LE(18), vectors.render.width);
  assert.equal(bmp.readInt32LE(22), vectors.render.height);

  // None of the assertions above touch a single pixel byte: a scrambled copy
  // loop that preserves width, height and file size would still pass them.
  // Undo the BMP's bottom-up row order and diff against the frozen golden.
  const bfOffBits = bmp.readUInt32LE(10);
  const width = bmp.readInt32LE(18);
  const height = bmp.readInt32LE(22);
  const rowBytes = width * 4;
  const pixels = Buffer.alloc(rowBytes * height);
  for (let fileRow = 0; fileRow < height; fileRow += 1) {
    const imageRow = height - 1 - fileRow;
    const source = bfOffBits + fileRow * rowBytes;
    bmp.copy(pixels, imageRow * rowBytes, source, source + rowBytes);
  }

  const diff = meanAbsDiff(pixels, golden);
  assert.ok(diff <= vectors.render.meanAbsDiffTolerance, `mean abs diff ${diff}`);

  const maxDiff = maxAbsDiff(pixels, golden);
  assert.ok(maxDiff <= vectors.render.maxAbsDiffTolerance, `max abs diff ${maxDiff}`);
});

test('a non-numeric --page or --scale is rejected before any PDFium call', async () => {
  for (const args of [['--page', 'four'], ['--page', ''], ['--page', '1.5'], ['--scale', 'big'], ['--scale', '0']]) {
    await assert.rejects(
      run('node', [resolve(here, 'main.mjs'), ...args, '--out', join(tmpdir(), 'epdf-cli-invalid.bmp')]),
      (err) => err.code === 1 && /invalid --(page|scale)/.test(err.stderr),
      `expected ${args.join(' ')} to be rejected`,
    );
  }
});
