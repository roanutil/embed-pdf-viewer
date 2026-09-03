import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loadVectors, meanAbsDiff } from '../test-support/vectors.mjs';
import { openDocument } from './index.mjs';

const { vectors, golden, fixture } = await loadVectors();

test('the six operations match the frozen vectors', () => {
  const doc = openDocument(fixture);
  try {
    assert.equal(doc.pageCount(), vectors.pageCount);

    const size = doc.pageSize(vectors.pageIndex);
    assert.ok(Math.abs(size.width - vectors.size.width) < 0.001);
    assert.ok(Math.abs(size.height - vectors.size.height) < 0.001);

    assert.equal(doc.pageText(vectors.pageIndex), vectors.text);

    const hits = doc.search(vectors.pageIndex, vectors.search.query, vectors.search.caseSensitive);
    assert.equal(hits.length, vectors.search.hitCount);
    for (const [i, want] of vectors.search.firstHits.entries()) {
      assert.equal(hits[i].charIndex, want.charIndex);
      assert.equal(hits[i].charCount, want.charCount);
    }

    const bitmap = doc.renderPage(vectors.pageIndex, vectors.render.scale);
    assert.equal(bitmap.width, vectors.render.width);
    assert.equal(bitmap.height, vectors.render.height);
    assert.equal(bitmap.stride, vectors.render.stride);
    const diff = meanAbsDiff(bitmap.bgra, golden);
    assert.ok(diff <= vectors.render.meanAbsDiffTolerance, `mean abs diff ${diff}`);

    let maxDiff = 0;
    for (let i = 0; i < bitmap.bgra.length; i += 1) {
      maxDiff = Math.max(maxDiff, Math.abs(bitmap.bgra[i] - golden[i]));
    }
    assert.ok(maxDiff <= vectors.render.maxAbsDiffTolerance, `max abs diff ${maxDiff}`);
  } finally {
    doc.close();
  }
});

test('a page past the end throws rather than crashing the process', () => {
  const doc = openDocument(fixture);
  try {
    assert.throws(() => doc.pageSize(doc.pageCount()), /past the end/);
  } finally {
    doc.close();
  }
});

test('garbage bytes throw', () => {
  assert.throws(() => openDocument(Buffer.from('not a pdf')), /PDFium can open/);
});

test('using a closed document throws', () => {
  const doc = openDocument(fixture);
  doc.close();
  doc.close();
  assert.throws(() => doc.pageCount(), /closed/);
});
