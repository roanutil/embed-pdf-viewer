import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, mkdirSync, copyFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const script = fileURLToPath(new URL('./local-pin.mjs', import.meta.url));
test('local pins hash archives, encode paths, and preserve other targets and the release pin', () => {
  const dir = mkdtempSync(join(tmpdir(), 'epdf-local-pin-'));
  try {
    const archive = join(dir, 'local artifact.tar.gz');
    writeFileSync(join(dir, 'marker'), 'local artifact');
    execFileSync('tar', ['-czf', archive, '-C', dir, 'marker']);
    const canonical = new URL('./runtime-build.json', import.meta.url);
    const before = readFileSync(canonical, 'utf8');
    const pin = JSON.parse(execFileSync(process.execPath, [script, `wasm32-eh=${archive}`], { encoding: 'utf8' }));
    assert.deepEqual(pin.artifacts['wasm32-eh'], {
      url: pathToFileURL(archive).href,
      sha256: createHash('sha256').update(readFileSync(archive)).digest('hex'),
    });
    assert.deepEqual(pin.artifacts['darwin-arm64'], JSON.parse(before).artifacts['darwin-arm64']);
    assert.equal(pin.artifacts['ios-arm64'].status, 'pending');
    assert.equal(readFileSync(canonical, 'utf8'), before);
    // Run the real fetcher in an isolated build root, without touching real caches.
    mkdirSync(join(dir, 'build'));
    const fetcher = join(dir, 'build/fetch-libpdfium.sh');
    copyFileSync(new URL('./fetch-libpdfium.sh', import.meta.url), fetcher);
    const pinFile = join(dir, 'pin.json');
    writeFileSync(pinFile, JSON.stringify(pin));
    const fetched = execFileSync('bash', [fetcher, 'wasm32-eh'], {
      encoding: 'utf8', env: { ...process.env, EPDF_SCAFFOLD_PIN_FILE: pinFile, EPDF_SCAFFOLD_LIB_DIR: join(dir, 'lib') },
    }).trim();
    assert.equal(readFileSync(join(fetched, 'marker'), 'utf8'), 'local artifact');
    assert.throws(() => execFileSync(process.execPath, [script, `typo=${archive}`], { stdio: 'pipe' }));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
