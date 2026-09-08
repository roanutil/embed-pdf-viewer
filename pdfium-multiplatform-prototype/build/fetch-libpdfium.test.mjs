import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { promisify } from 'node:util';
import { tmpdir } from 'node:os';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const here = dirname(fileURLToPath(import.meta.url));
const script = resolve(here, 'fetch-libpdfium.sh');
const run = promisify(execFile);

async function fetchTarget(target, env = {}) {
  try {
    const { stdout } = await run('bash', [script, target], { env: { ...process.env, ...env } });
    return { code: 0, stdout, stderr: '' };
  } catch (error) {
    return { code: error.code, stdout: error.stdout ?? '', stderr: error.stderr ?? '' };
  }
}

test('pin file carries the nine published targets and five pending ones', async () => {
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

  // wasm32-eh joins the four mobile targets here: the fork builds it (same as
  // ios-arm64, ios-sim-arm64, android-arm64 and android-x64 now do, from
  // commit 76e25559b), but none of the five has shipped in a published
  // release yet.
  const pending = Object.entries(pin.artifacts)
    .filter(([, a]) => a.status === 'pending')
    .map(([t]) => t)
    .sort();
  assert.deepEqual(pending, [
    'android-arm64', 'android-x64',
    'ios-arm64', 'ios-sim-arm64',
    'wasm32-eh',
  ]);
  for (const target of pending) {
    assert.equal(pin.artifacts[target].url, undefined);
    assert.equal(pin.artifacts[target].sha256, undefined);
  }
});

test('a pending mobile target exits 2 and names itself', async () => {
  const { code, stderr } = await fetchTarget('ios-arm64');
  assert.equal(code, 2);
  assert.match(stderr, /ios-arm64/);
  assert.match(stderr, /no published libembedpdf/);
  assert.match(stderr, /EPDF_SCAFFOLD_PIN_FILE/);
});

test('an unknown target exits 1', async () => {
  const { code, stderr } = await fetchTarget('solaris-sparc');
  assert.equal(code, 1);
  assert.match(stderr, /unknown target: solaris-sparc/);
});

test('darwin-arm64 extracts a dylib and an include tree', async () => {
  // Other npm suites load the real addon concurrently. Never remove its live
  // library directory while testing extraction.
  const scratch = await mkdtemp(join(tmpdir(), 'fetch-libpdfium-darwin-'));
  try {
    const { code, stdout } = await fetchTarget('darwin-arm64', { EPDF_SCAFFOLD_LIB_DIR: scratch });
    assert.equal(code, 0);
    const dir = stdout.trim();
    await readFile(resolve(dir, 'lib/libembedpdf.dylib'));
    await readFile(resolve(dir, 'include/fpdfview.h'));
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});

test('a pin entry missing url or sha256 exits 1', async () => {
  const scratch = await mkdtemp(join(tmpdir(), 'fetch-libpdfium-incomplete-'));
  try {
    const pinFile = join(scratch, 'pin.json');
    await writeFile(
      pinFile,
      JSON.stringify({
        artifacts: {
          'incomplete-target': { note: 'missing url and sha256' },
        },
      }),
    );
    const libDir = join(scratch, 'lib');

    const { code, stderr } = await fetchTarget('incomplete-target', {
      EPDF_SCAFFOLD_PIN_FILE: pinFile,
      EPDF_SCAFFOLD_LIB_DIR: libDir,
    });
    assert.equal(code, 1);
    assert.match(stderr, /missing url or sha256 for target: incomplete-target/);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});

test('a sha256 mismatch exits 1, reports expected and actual, and deletes the archive', async () => {
  const scratch = await mkdtemp(join(tmpdir(), 'fetch-libpdfium-mismatch-'));
  try {
    const sourceFile = join(scratch, 'source.bin');
    await writeFile(sourceFile, 'not the bytes the pin expects');
    const actualSha256 = createHash('sha256').update(await readFile(sourceFile)).digest('hex');
    const wrongSha256 = actualSha256.startsWith('f') ? actualSha256.replace(/^f/, '0') : `f${actualSha256.slice(1)}`;

    const pinFile = join(scratch, 'pin.json');
    await writeFile(
      pinFile,
      JSON.stringify({
        artifacts: {
          'mismatch-target': {
            url: `file://${sourceFile}`,
            sha256: wrongSha256,
          },
        },
      }),
    );
    const libDir = join(scratch, 'lib');

    // This target's archive lands in the real build/cache/ dir (the script does
    // not let EPDF_SCAFFOLD_LIB_DIR redirect it), so the target name must not
    // collide with a real one. The mismatch branch deletes it, verified below.
    const archivePath = resolve(here, 'cache', 'libembedpdf-runtime-mismatch-target.tar.gz');
    try {
      const { code, stderr } = await fetchTarget('mismatch-target', {
        EPDF_SCAFFOLD_PIN_FILE: pinFile,
        EPDF_SCAFFOLD_LIB_DIR: libDir,
      });
      assert.equal(code, 1);
      assert.match(stderr, /sha256 mismatch for .*mismatch-target\.tar\.gz/);
      assert.match(stderr, new RegExp(`expected: ${wrongSha256}`));
      assert.match(stderr, new RegExp(`actual:   ${actualSha256}`));
      await assert.rejects(readFile(archivePath));
    } finally {
      await rm(archivePath, { force: true });
    }
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});

test('a missing pin file fails loudly instead of proceeding with empty values', async () => {
  const scratch = await mkdtemp(join(tmpdir(), 'fetch-libpdfium-badpin-'));
  try {
    const { code, stdout, stderr } = await fetchTarget('darwin-arm64', {
      EPDF_SCAFFOLD_PIN_FILE: join(scratch, 'does-not-exist.json'),
    });
    assert.notEqual(code, 0);
    assert.equal(stdout, '');
    assert.match(stderr, /ENOENT/);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
});
