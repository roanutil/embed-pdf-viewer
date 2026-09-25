/**
 * The Node addon, loaded and driven the way the server worker will.
 *
 * Run from the repository root:
 *   cargo build -p epdf-node --release
 *   node crates/epdf-node/tests/addon.mjs
 *
 * Node rather than Rust, and deliberately. A `napi::Buffer` cannot be built
 * outside a Node environment, so a Rust integration test could reach
 * `Engine::call`'s body and nothing else: it would skip the Node-API
 * marshalling, which is the only part of this crate that is not already
 * covered by `epdf-engine`'s own suites.
 *
 * What this asserts is therefore the marshalling and nothing else: that a
 * `Buffer` survives the trip in and out, that a `Vec<Buffer>` arrives as an
 * array, and that the object shape `napi-derive` generates is the one the
 * worker expects. The semantics are `epdf-engine`'s and are tested there.
 */
import { createRequire } from 'node:module';
import { readFileSync, copyFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');
const FIXTURE = 'tooling/engine-conformance/fixtures/annotation_kinds.pdf';

/**
 * Node loads an addon by extension, and Cargo writes a `.dylib` or a `.so`.
 * Copied rather than renamed in place so a `cargo build` after this does not
 * find a file it did not write.
 */
function loadAddon() {
  const built = [
    'target/release/libepdf_node.dylib',
    'target/release/libepdf_node.so',
    'target/debug/libepdf_node.dylib',
    'target/debug/libepdf_node.so',
  ]
    .map((relative) => join(repoRoot, relative))
    .find((path) => {
      try {
        readFileSync(path);
        return true;
      } catch {
        return false;
      }
    });
  if (!built) {
    throw new Error('no epdf-node library built; run `cargo build -p epdf-node --release` first');
  }
  const staged = join(mkdtempSync(join(tmpdir(), 'epdf-node-')), 'epdf.node');
  copyFileSync(built, staged);
  return createRequire(import.meta.url)(staged);
}

const { Engine } = loadAddon();
const bytes = readFileSync(join(repoRoot, FIXTURE));

function check(name, run) {
  run();
  process.stdout.write(`  ok      ${name}\n`);
}

check('a document opens from a Buffer payload', () => {
  const engine = new Engine();
  const response = engine.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  assert.equal(JSON.parse(response.json).error, undefined, response.json);
  assert.equal(engine.sessionCount(), 1);
  engine.shutdown();
});

check('a read comes back as JSON with no payloads', () => {
  const engine = new Engine();
  engine.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  const response = engine.call(JSON.stringify({ kind: 'pages.list', docId: 'd' }), []);
  assert.ok(JSON.parse(response.json).result.pages.length > 0);
  assert.equal(response.payloads.length, 0);
  engine.shutdown();
});

check('a saved document comes back as a Buffer beside the JSON', () => {
  // The marshalling that matters most. A PDF through the JSON would be
  // base64 and half again as large, and the worker hands this straight to a
  // stream.
  const engine = new Engine();
  engine.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  const response = engine.call(
    JSON.stringify({ kind: 'document.saveBuffer', docId: 'd', mode: 'incremental' }),
    [],
  );
  assert.equal(response.payloads.length, 1);
  const saved = response.payloads[0];
  assert.ok(Buffer.isBuffer(saved), 'the payload is not a Buffer');
  assert.equal(JSON.parse(response.json).result.size, saved.length);
  assert.equal(saved.subarray(0, 5).toString(), '%PDF-');
  engine.shutdown();
});

check('a refused operation returns rather than throwing', () => {
  // One error path, not two. A thrown exception for a document problem and a
  // returned envelope for everything else would make the worker handle both.
  const engine = new Engine();
  engine.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  const response = engine.call(
    JSON.stringify({ kind: 'pages.text', docId: 'd', pageObjectNumber: 99999 }),
    [],
  );
  assert.ok(JSON.parse(response.json).error.code, response.json);
  engine.shutdown();
});

check('shutdown is terminal and closing a document is not', () => {
  // The distinction a pool depends on. `close` is what it does between
  // checkouts; `shutdown` is what it does to an engine it is discarding, and
  // an engine that came back to life after one would let a supervisor keep
  // using something it had decided to throw away.
  const reused = new Engine();
  reused.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  reused.call(JSON.stringify({ kind: 'close', docId: 'd' }), []);
  assert.equal(reused.sessionCount(), 0);
  const reopened = reused.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  assert.equal(JSON.parse(reopened.json).error, undefined, reopened.json);
  reused.shutdown();

  const discarded = new Engine();
  discarded.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  discarded.shutdown();
  assert.equal(discarded.sessionCount(), 0);
  const refused = discarded.call(JSON.stringify({ kind: 'open.fatMem', docId: 'again' }), [bytes]);
  assert.equal(JSON.parse(refused.json).error.code, 'RuntimeUnavailable', refused.json);
});

check('two engines hold separate sessions', () => {
  // The pool's other requirement. An engine reaching for shared state would
  // pass every single-engine check and lose documents the moment two were
  // checked out at once.
  const first = new Engine();
  const second = new Engine();
  first.call(JSON.stringify({ kind: 'open.fatMem', docId: 'd' }), [bytes]);
  assert.equal(first.sessionCount(), 1);
  assert.equal(second.sessionCount(), 0);
  const response = second.call(JSON.stringify({ kind: 'pages.list', docId: 'd' }), []);
  assert.ok(JSON.parse(response.json).error, 'the second engine saw the first engine\'s document');
  first.shutdown();
  second.shutdown();
});

process.stdout.write('  6 checks passed\n');
