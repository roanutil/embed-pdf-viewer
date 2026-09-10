import { test } from 'node:test';
import assert from 'node:assert/strict';
import { resolveTarget } from './platform.mjs';

test('Linux build and runtime selection distinguish glibc, musl, and unavailable reports', () => {
  for (const arch of ['arm64', 'x64']) {
    const host = { platform: 'linux', arch };
    assert.equal(resolveTarget({ ...host, report: { getReport: () => ({ header: { glibcVersionRuntime: '2.39' } }) } }), `linux-${arch}`);
    assert.equal(resolveTarget({ ...host, report: { getReport: () => ({ header: {} }) } }), `linuxmusl-${arch}`);
    assert.equal(resolveTarget(host), `linux-${arch}`);
    assert.equal(resolveTarget({ ...host, report: {} }), `linux-${arch}`);
  }
});

test('unsupported architectures are rejected instead of being mislabeled as x64', () => {
  assert.equal(resolveTarget({ platform: 'linux', arch: 'riscv64' }), null);
  assert.equal(resolveTarget({ platform: 'darwin', arch: 'arm64' }), 'darwin-arm64');
  assert.equal(resolveTarget({ platform: 'win32', arch: 'x64' }), 'win32-x64');
});
