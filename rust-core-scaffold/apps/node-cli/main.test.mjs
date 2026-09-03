import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { promisify } from 'node:util';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadVectors } from '../../packages/test-support/vectors.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const run = promisify(execFile);

const { vectors } = await loadVectors();

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
});
