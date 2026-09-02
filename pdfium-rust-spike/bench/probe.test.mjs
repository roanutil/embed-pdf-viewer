import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadArm } from './arm.mjs';
import { openFixture } from './fixture.mjs';

test('a Rust function linked into the PDFium wasm module returns the same page size as PDFium', async () => {
  const arm = await loadArm('b');

  try {
    const fixture = openFixture(arm);

    try {
      const out = arm.mem.alloc(8);

      const direct = arm.fn.EPDF_GetPageSizeByIndexNormalized(fixture.doc, 0, out);
      assert.equal(direct, 1, 'EPDF_GetPageSizeByIndexNormalized failed');
      const directW = arm.mem.peekF32(out);
      const directH = arm.mem.peekF32(out + 4);

      const viaRust = arm.fn.rs_probe_page_size(fixture.doc, 0, out);
      assert.equal(viaRust, 1, 'rs_probe_page_size failed');
      assert.equal(arm.mem.peekF32(out), directW, 'width mismatch');
      assert.equal(arm.mem.peekF32(out + 4), directH, 'height mismatch');

      assert.ok(directW > 0 && directH > 0, `implausible page size ${directW}x${directH}`);

      arm.mem.free(out);
    } finally {
      fixture.close();
    }
  } finally {
    arm.close();
  }
});
