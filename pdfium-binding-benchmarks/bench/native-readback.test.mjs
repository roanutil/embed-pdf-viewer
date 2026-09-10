import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadNativeArm } from './arm.mjs';

for (const name of ['b', 'c', 'd']) {
  test(`native ${name} reuses the owned readback Buffer`, async () => {
    const arm = await loadNativeArm(name);
    const ptr = arm.mem.writeU16('A😀B');
    try {
      const bytes = arm.mem.readBytes(ptr, 8);
      assert.ok(Buffer.isBuffer(bytes), 'must not copy the Buffer into another Uint8Array');
      assert.equal(arm.mem.readU16(ptr, 4), 'A😀B');
      // The native helper owns the copy, so releasing scratch memory is safe.
      arm.mem.free(ptr);
      assert.equal(bytes.toString('utf16le'), 'A😀B');
    } finally {
      arm.close();
    }
  });
}
