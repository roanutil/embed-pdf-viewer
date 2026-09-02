import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadArm } from './arm.mjs';
import { openFixture, SEARCH_TERM } from './fixture.mjs';

/**
 * The coarse search ops report overflow, and `searchCoarse` refuses to read it.
 *
 * They used to return the unclamped total, which let `searchCoarse`
 * (bench/workloads.mjs) build a `DataView` over `n * 16` bytes of a
 * `cap * 16`-byte allocation: at `cap = 16` on this fixture it returned 120,
 * spanned 1,920 bytes of a 256-byte allocation, and handed back 104 rects of
 * whatever followed. Nothing threw. The wasm arms cannot notice for themselves,
 * because bench/arm.mjs's `viewBuffer` ignores its length argument.
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
