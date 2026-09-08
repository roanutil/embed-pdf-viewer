/**
 * The one JS reader of vectors/report-page4.json, the frozen file
 * build/generate-vectors.mjs wrote once and nothing regenerates. The Rust
 * suites read the same file through crates/test-support.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const scaffoldRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

export async function loadVectors() {
  const vectors = JSON.parse(await readFile(resolve(scaffoldRoot, 'vectors/report-page4.json'), 'utf8'));
  return {
    vectors,
    golden: await readFile(resolve(scaffoldRoot, 'vectors', vectors.render.golden)),
    fixture: await readFile(resolve(scaffoldRoot, 'fixtures/report.pdf')),
  };
}

/**
 * Mean absolute per-byte difference. Rendering is compared this way rather than
 * by hash: a hash pins float and rasterization behaviour across four
 * architectures and fails for reasons unrelated to this code.
 */
export function meanAbsDiff(a, b) {
  if (a.length !== b.length) {
    throw new Error(`buffer lengths differ: ${a.length} vs ${b.length}`);
  }
  let total = 0;
  for (let i = 0; i < a.length; i += 1) total += Math.abs(a[i] - b[i]);
  return total / a.length;
}

/**
 * Largest single-byte absolute difference. `meanAbsDiff` can stay near zero
 * while one byte is wildly off; this catches that case.
 */
export function maxAbsDiff(a, b) {
  if (a.length !== b.length) {
    throw new Error(`buffer lengths differ: ${a.length} vs ${b.length}`);
  }
  let max = 0;
  for (let i = 0; i < a.length; i += 1) max = Math.max(max, Math.abs(a[i] - b[i]));
  return max;
}
