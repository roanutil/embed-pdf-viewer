import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
  existsSync,
  symlinkSync,
  readlinkSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
const api = await import('../macos-consumer.mjs').catch(() => ({}));
const manifest = `let engineRelease = "apple-engine-3816be131b65bdee"
let engineChecksum = "88cfb206b6d3fd74e017062411bbd6a5133616882e52df299754aa9487c0a116"
platforms: [.iOS(.v17),]
linkerSettings: [
  .linkedFramework("CoreGraphics"),
]
linkerSettings: [
  .linkedFramework("CoreGraphics"),
]
`;

test('private Mac manifest preserves pins and enables both linker branches idempotently', () => {
  assert.equal(typeof api.macosConsumerManifest, 'function');
  const result = api.macosConsumerManifest(manifest);
  assert.match(result, /\.macOS\(\.v14\)/);
  assert.equal((result.match(/\.linkedFramework\("AppKit"/g) ?? []).length, 2);
  assert.equal((result.match(/\.linkedFramework\("Foundation"/g) ?? []).length, 2);
  assert.match(result, /apple-engine-3816be131b65bdee/);
  assert.match(result, /88cfb206b6d3fd74e017062411bbd6a5133616882e52df299754aa9487c0a116/);
  assert.equal(api.macosConsumerManifest(result), result);
});
test('an unsupported manifest shape fails rather than guessing platform or linker edits', () => {
  assert.equal(typeof api.macosConsumerManifest, 'function');
  assert.throws(() => api.macosConsumerManifest('platforms: []'), /manifest/);
});
test('private Mac staging preserves source bytes and excludes old build caches', (t) => {
  assert.equal(typeof api.stageMacConsumer, 'function');
  const root = mkdtempSync(path.join(os.tmpdir(), 'mac-consumer-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source');
  mkdirSync(path.join(source, '.build'), { recursive: true });
  writeFileSync(path.join(source, 'Package.swift'), manifest);
  writeFileSync(path.join(source, '.build/stale'), 'old cache');
  writeFileSync(path.join(source, 'wrapper.swift'), 'public struct Viewer {}');
  const output = path.join(root, 'private');
  api.stageMacConsumer(source, output);
  assert.equal(readFileSync(path.join(source, 'Package.swift'), 'utf8'), manifest);
  assert.match(readFileSync(path.join(output, 'Package.swift'), 'utf8'), /\.macOS\(\.v14\)/);
  assert.equal(existsSync(path.join(output, '.build')), false);
  assert.equal(readFileSync(path.join(output, 'wrapper.swift'), 'utf8'), 'public struct Viewer {}');
  assert.throws(() => api.stageMacConsumer(source, output), /existing/);
});

test('private Mac staging preserves relative versioned-framework links', (t) => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'mac-symlink-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = path.join(root, 'source');
  mkdirSync(path.join(source, 'Archive.framework/Versions/A'), { recursive: true });
  writeFileSync(path.join(source, 'Package.swift'), manifest);
  writeFileSync(path.join(source, 'Archive.framework/Versions/A/binary'), 'fixture bytes');
  symlinkSync('A', path.join(source, 'Archive.framework/Versions/Current'));
  const output = path.join(root, 'private');
  api.stageMacConsumer(source, output);
  assert.equal(readlinkSync(path.join(output, 'Archive.framework/Versions/Current')), 'A');
  rmSync(source, { recursive: true, force: true });
  assert.equal(
    readFileSync(path.join(output, 'Archive.framework/Versions/Current/binary'), 'utf8'),
    'fixture bytes',
  );
});
