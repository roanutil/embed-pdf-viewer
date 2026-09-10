import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bgraToRgba } from './src/bgra-to-rgba.mjs';

test('bgraToRgba swaps R and B, leaves G and alpha untouched', () => {
  // Two pixels: (B=10, G=20, R=30, A=40) and (B=200, G=150, R=100, A=255).
  const bgra = Uint8Array.from([10, 20, 30, 40, 200, 150, 100, 255]);

  const rgba = bgraToRgba(bgra);

  assert.deepEqual(Array.from(rgba), [30, 20, 10, 40, 100, 150, 200, 255]);
  // The input itself is untouched: the function copies rather than aliasing.
  assert.deepEqual(Array.from(bgra), [10, 20, 30, 40, 200, 150, 100, 255]);
});
