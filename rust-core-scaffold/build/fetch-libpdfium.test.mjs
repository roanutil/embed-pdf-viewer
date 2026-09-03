import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { promisify } from 'node:util';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const script = resolve(here, 'fetch-libpdfium.sh');
const run = promisify(execFile);

async function fetchTarget(target) {
  try {
    const { stdout } = await run('bash', [script, target]);
    return { code: 0, stdout, stderr: '' };
  } catch (error) {
    return { code: error.code, stdout: error.stdout ?? '', stderr: error.stderr ?? '' };
  }
}

test('pin file carries the nine published targets and four pending mobile ones', async () => {
  const pin = JSON.parse(await readFile(resolve(here, 'runtime-build.json'), 'utf8'));
  assert.equal(pin.sha, 'd7b4caa26289d4846e94ff54e63b586b9bbe9fd6');

  const published = Object.entries(pin.artifacts)
    .filter(([, a]) => a.url)
    .map(([t]) => t)
    .sort();
  assert.deepEqual(published, [
    'darwin-arm64', 'darwin-x64',
    'linux-arm64', 'linux-x64',
    'linuxmusl-arm64', 'linuxmusl-x64',
    'wasm32',
    'win32-arm64', 'win32-x64',
  ]);

  const pending = Object.entries(pin.artifacts)
    .filter(([, a]) => a.status === 'pending')
    .map(([t]) => t)
    .sort();
  assert.deepEqual(pending, ['android-arm64', 'android-x64', 'ios-arm64', 'ios-sim-arm64']);
  for (const target of pending) {
    assert.equal(pin.artifacts[target].url, undefined);
    assert.equal(pin.artifacts[target].sha256, undefined);
  }
});

test('a pending mobile target exits 2 and names itself', async () => {
  const { code, stderr } = await fetchTarget('ios-arm64');
  assert.equal(code, 2);
  assert.match(stderr, /ios-arm64/);
  assert.match(stderr, /embedpdf\/runtime does not build it yet/);
});

test('an unknown target exits 1', async () => {
  const { code, stderr } = await fetchTarget('solaris-sparc');
  assert.equal(code, 1);
  assert.match(stderr, /unknown target: solaris-sparc/);
});

test('darwin-arm64 extracts a dylib and an include tree', async () => {
  const { code, stdout } = await fetchTarget('darwin-arm64');
  assert.equal(code, 0);
  const dir = stdout.trim();
  await readFile(resolve(dir, 'lib/libembedpdf.dylib'));
  await readFile(resolve(dir, 'include/fpdfview.h'));
});
