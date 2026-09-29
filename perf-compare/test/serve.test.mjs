import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { HEADERS, serveArms } from '../lib/serve.mjs';

test('serves every file with the isolation headers and wasm as application/wasm', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'perf-serve-'));
  writeFileSync(path.join(dir, 'index.html'), '<!doctype html>');
  writeFileSync(path.join(dir, 'a.wasm'), Buffer.from([0, 97, 115, 109]));
  const { urls, close } = await serveArms({ x: dir }, { basePort: 5391 });
  try {
    for (const p of ['/', '/a.wasm']) {
      const res = await fetch(urls.x + p);
      assert.equal(res.status, 200);
      for (const [k, v] of Object.entries(HEADERS)) assert.equal(res.headers.get(k), v, `${p} ${k}`);
    }
    assert.equal((await fetch(urls.x + '/a.wasm')).headers.get('content-type'), 'application/wasm');
    assert.equal((await fetch(urls.x + '/missing.js')).status, 404);
    assert.equal((await fetch(urls.x + '/../../etc/passwd')).status, 404);
  } finally {
    await close();
  }
});
