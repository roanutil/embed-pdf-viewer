import test from 'node:test';
import assert from 'node:assert/strict';
import { canonicalForNative, compare } from './gate.mjs';

test('native Swift arm e agrees with native arm a', async () => {
  // PDFium's process globals cannot initialize two native arms concurrently.
  const a = await canonicalForNative('a');
  const e = await canonicalForNative('e');
  assert.equal(compare('a', a, 'e', e).ok, true);
});
