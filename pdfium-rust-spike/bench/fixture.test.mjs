import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadArm } from './arm.mjs';
import { openFixture } from './fixture.mjs';

test('arm a opens the fixture and finds a page with real text on it', async () => {
  const arm = await loadArm('a');
  const fx = openFixture(arm);
  assert.ok(fx.glyphCount > 500, `expected a text-heavy page, got ${fx.glyphCount} glyphs`);
  assert.ok(fx.width > 0 && fx.height > 0);
  fx.close();
  arm.close();
});
