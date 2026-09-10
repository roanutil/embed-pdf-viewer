import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loadVectors, meanAbsDiff, maxAbsDiff } from '../test-support/vectors.mjs';
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

    const maxDiff = maxAbsDiff(bitmap.bgra, golden);
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

test('errors carry a stable code distinct from the message', () => {
  const doc = openDocument(fixture);
  doc.close();
  assert.throws(() => doc.pageCount(), (err) => err.code === 'CLOSED');

  assert.throws(() => openDocument(Buffer.from('not a pdf')), (err) => err.code === 'LOAD_FAILED');

  let closedCode;
  try {
    doc.pageCount();
  } catch (err) {
    closedCode = err.code;
  }
  let loadFailedCode;
  try {
    openDocument(Buffer.from('not a pdf'));
  } catch (err) {
    loadFailedCode = err.code;
  }
  assert.notEqual(closedCode, loadFailedCode);
});

test('a page past the end carries PAGE_OUT_OF_RANGE', () => {
  const doc = openDocument(fixture);
  try {
    assert.throws(() => doc.pageSize(doc.pageCount()), (err) => err.code === 'PAGE_OUT_OF_RANGE');
  } finally {
    doc.close();
  }
});
