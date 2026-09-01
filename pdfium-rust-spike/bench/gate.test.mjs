import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor } from './gate.mjs';

test('arm a produces a stable canonical answer for every workload it runs', async () => {
  const first = await canonicalFor('a');
  const second = await canonicalFor('a');

  assert.ok(first.glyphCount > 500);
  assert.equal(JSON.stringify(first.geometry), JSON.stringify(second.geometry));
  assert.equal(first.text, second.text);
  assert.equal(first['render-1x'], second['render-1x']);
  assert.equal(first['render-4x'], second['render-4x']);
  assert.equal(JSON.stringify(first.search), JSON.stringify(second.search));

  assert.equal(first.geometry.length, first.glyphCount);
  assert.ok(first.text.length > 100, 'expected real text on the fixture page');
  assert.ok(first.search.length > 0, 'expected the search term to hit');
  assert.notEqual(first['render-1x'], '00000000');
  assert.notEqual(first['render-1x'], first['render-4x']);
});
