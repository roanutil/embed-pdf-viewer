// Run after EPDF_PANIC_PROBE=1 bash build/link-wasm.sh. Not part of normal builds.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import createModule from '../../build/out/web/epdf.mjs';
const module = await createModule({ printErr: () => {} });
assert.equal(typeof module._epdf_test_panic, 'function', 'build with EPDF_PANIC_PROBE=1');
const bytes = await readFile(new URL('../../fixtures/report.pdf', import.meta.url));
const pointer = module._malloc(bytes.length);
module.HEAPU8.set(bytes, pointer);
const doc = module._epdf_open(pointer, bytes.length, 0, 0);
module._free(pointer);
assert.notEqual(doc, 0);
try {
  for (let i = 0; i < 2; i++) {
    assert.equal(module._epdf_test_panic(), -1, 'dispatch must catch the panic');
    assert.match(module.UTF8ToString(module._epdf_last_error()), /deliberate panic inside PDFium dispatch/);
    assert.equal(module._epdf_page_count(doc), 14, 'the borrow guard must allow subsequent work');
  }
} finally {
  module._epdf_close(doc);
}
console.log('Two in-dispatch panics recovered; the same document remains usable.');
