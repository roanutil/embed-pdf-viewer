import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runAlloc } from './workloads.mjs';

test('allocation windows include typed-array backing stores', async () => {
  const bytesPerBuffer = 32_768;
  const result = await runAlloc({
    name: 'backing-store-probe',
    async init() {},
    build() {
      const keep = [];
      return {
        frame() {
          const buffer = new Float64Array(bytesPerBuffer / 8);
          keep.push(buffer);
          return buffer.length;
        },
        dispose() { keep.length = 0; },
      };
    },
  });
  assert.equal(result.gcDuringWindow, 0);
  assert.equal(result.allocationMetric, 'heapUsed+arrayBuffers');
  assert.equal(result.arrayBufferBytesPerFrame, bytesPerBuffer);
  assert.ok(result.heapBytesPerFrame > 0);
  assert.equal(result.bytesPerFrame, result.heapBytesPerFrame + bytesPerBuffer);
});
