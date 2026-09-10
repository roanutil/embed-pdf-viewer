import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadArm } from './arm.mjs';
import { openFixture, SEARCH_TERM } from './fixture.mjs';
import { RECORD_BYTES } from './workloads.mjs';

/**
 * The coarse ops report overflow, and the workloads refuse to read it.
 *
 * They used to return the unclamped total, which let `searchCoarse` and
 * `geometryCoarse` (bench/workloads.mjs) build a `DataView` over `n` records of
 * a `cap`-record allocation: at `cap = 16` on this fixture the search op
 * returned 120, spanned 1,920 bytes of a 256-byte allocation, and handed back
 * 104 rects of whatever followed. Nothing threw. The wasm arms cannot notice
 * for themselves, because bench/arm.mjs's `viewBuffer` ignores its length
 * argument.
 *
 * Both ops answer the same way now: the count, or NEGATIVE the count when it
 * did not fit.
 */
for (const [arm, fname] of [
  ['c', 'rs_search_page'],
  ['d', 'cc_search_page'],
]) {
  test(`${fname} (arm ${arm}) reports overflow instead of an unreadable count`, async () => {
    const loaded = await loadArm(arm);
    try {
      const fx = openFixture(loaded);
      try {
        const { fn, mem } = loaded;
        const termPtr = mem.writeU16(SEARCH_TERM);

        const cap = 4096;
        const ptr = mem.alloc(cap * 16);
        const total = fn[fname](fx.textPage, termPtr, ptr, cap);
        assert.ok(total > 0, `expected hits for "${SEARCH_TERM}", got ${total}`);
        mem.free(ptr);

        // The sizing probe, same contract as the geometry op below: no buffer,
        // so nothing is written and the count comes back POSITIVE. Both spellings
        // of "no buffer" count, because a caller that reached for the geometry
        // op's convention writes the cap it wants and passes a null pointer.
        assert.equal(fn[fname](fx.textPage, termPtr, 0, 0), total);
        assert.equal(fn[fname](fx.textPage, termPtr, 0, cap), total);

        // One rect short of fitting: the count still has to be reported, and it
        // has to be reported as an overflow.
        const tight = total - 1;
        const tightPtr = mem.alloc(tight * 16);
        assert.equal(fn[fname](fx.textPage, termPtr, tightPtr, tight), -total);
        mem.free(tightPtr);

        // Exactly fitting is not an overflow.
        const exactPtr = mem.alloc(total * 16);
        assert.equal(fn[fname](fx.textPage, termPtr, exactPtr, total), total);
        mem.free(exactPtr);

        mem.free(termPtr);
      } finally {
        fx.close();
      }
    } finally {
      loaded.close();
    }
  });
}

for (const [arm, fname] of [
  ['c', 'rs_read_page_geometry'],
  ['d', 'cc_read_page_geometry'],
]) {
  test(`${fname} (arm ${arm}) reports overflow instead of an unreadable count`, async () => {
    const loaded = await loadArm(arm);
    try {
      const fx = openFixture(loaded);
      try {
        const { fn, mem } = loaded;
        const total = fx.glyphCount;
        assert.ok(total > 500, `expected a text-heavy page, got ${total} glyphs`);

        // The sizing probe: no buffer, no cap, so nothing is written and the
        // count comes back POSITIVE for a caller about to allocate for it.
        assert.equal(fn[fname](fx.textPage, 0, 0), total);

        // One record short of fitting.
        const tight = total - 1;
        const tightPtr = mem.alloc(tight * RECORD_BYTES);
        assert.equal(fn[fname](fx.textPage, tightPtr, tight), -total);
        mem.free(tightPtr);

        // Exactly fitting is not an overflow. This is the cap the bench uses:
        // `recordCap` in bench/workloads.mjs is the fixture's glyph count.
        const exactPtr = mem.alloc(total * RECORD_BYTES);
        assert.equal(fn[fname](fx.textPage, exactPtr, total), total);
        mem.free(exactPtr);
      } finally {
        fx.close();
      }
    } finally {
      loaded.close();
    }
  });
}

/**
 * A failed FPDFText_FindStart is not "zero hits". PDFium returns a null
 * search handle for a null text page (and for a null term); the coarse ops
 * used to answer 0 for that, indistinguishable from a page the term simply
 * does not appear on. -1 is free for the purpose: overflow is reported as
 * NEGATIVE the count, but only when `written > cap >= 1`, so an overflow is
 * always <= -2.
 */
for (const [arm, fname] of [
  ['c', 'rs_search_page'],
  ['d', 'cc_search_page'],
]) {
  test(`${fname} (arm ${arm}) returns -1, not 0, when FindStart fails`, async () => {
    const loaded = await loadArm(arm);
    try {
      const { fn, mem } = loaded;
      const termPtr = mem.writeU16(SEARCH_TERM);
      const ptr = mem.alloc(16 * 16);
      // Null text page, both with and without a buffer.
      assert.equal(fn[fname](0, termPtr, 0, 0), -1);
      assert.equal(fn[fname](0, termPtr, ptr, 16), -1);
      mem.free(ptr);
      mem.free(termPtr);
    } finally {
      loaded.close();
    }
  });
}
