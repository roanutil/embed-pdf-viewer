import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

// LP64 aliases uint64_t to unsigned long: resolving it first loses its fixed width.
test('keeps fixed-width integer ABI when platform typedefs resolve to long', (t) => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'epdf-function-width-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'build/libpdfium/width/include'), { recursive: true });
  fs.mkdirSync(path.join(root, 'src/core'), { recursive: true });
  fs.writeFileSync(
    path.join(root, 'build/libpdfium/width/include/functions.h'),
    `
    typedef unsigned long uint64_t;
    typedef long int64_t;
    uint64_t EPDFTest_Unsigned(uint64_t value);
    int64_t EPDFTest_Signed(int64_t value);
    unsigned long EPDFTest_Flags(unsigned long value);
  `,
  );
  fs.copyFileSync(
    new URL('../build/generate-functions.mjs', import.meta.url),
    path.join(root, 'build/generate-functions.mjs'),
  );
  execFileSync(process.execPath, [path.join(root, 'build/generate-functions.mjs'), 'width']);
  const { functions } = JSON.parse(
    fs.readFileSync(path.join(root, 'build/generated/functions.json')),
  );
  for (const name of ['EPDFTest_Unsigned', 'EPDFTest_Signed']) {
    const fn = functions.find((fn) => fn.name === name);
    for (const value of [fn.result, fn.params[0]]) {
      assert.equal(value.ts, 'bigint', name);
      assert.deepEqual(value.wasm, { kind: 'i64', cwrap: 'bigint' }, name);
      assert.deepEqual(value.native, { kind: 'i64', cwrap: 'bigint' }, name);
    }
  }
  const flags = functions.find((fn) => fn.name === 'EPDFTest_Flags');
  assert.equal(flags.params[0].ts, 'number');
  assert.equal(flags.result.ts, 'number');
});
