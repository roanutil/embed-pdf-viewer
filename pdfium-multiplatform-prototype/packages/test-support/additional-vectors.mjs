import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { scaffoldRoot } from './vectors.mjs';

// Both native Node and WASM run these same fixtures and assertions.
export async function additionalVectors(openDocument) {
  const encrypted = await readFile(resolve(scaffoldRoot, 'fixtures/encrypted-hello.pdf'));
  const colors = await readFile(resolve(scaffoldRoot, 'fixtures/colors.pdf'));
  const vector = JSON.parse(await readFile(resolve(scaffoldRoot, 'vectors/colors.json'), 'utf8'));

  test('encrypted documents reject absent/wrong passwords and accept UTF-8 passwords', () => {
    for (const password of [undefined, '', 'wrong']) {
      assert.throws(() => openDocument(encrypted, password), /password/i);
    }
    const doc = openDocument(encrypted, 'hôtel');
    try {
      assert.equal(doc.pageCount(), 1);
      assert.equal(doc.pageText(0), 'Hello, world!\r\nGoodbye, world!');
    } finally {
      doc.close();
    }
  });

  test('DeviceRGB interior pixels preserve exact color values and channel order', () => {
    const doc = openDocument(colors);
    try {
      const bitmap = doc.renderPage(0, vector.scale);
      assert.equal(bitmap.width, vector.width);
      assert.equal(bitmap.height, vector.height);
      for (const { x, y, bgra } of vector.samples) {
        const offset = y * bitmap.stride + x * 4;
        assert.deepEqual(Array.from(bitmap.bgra.subarray(offset, offset + 4)), bgra, `pixel ${x},${y}`);
      }
    } finally {
      doc.close();
    }
  });
}
