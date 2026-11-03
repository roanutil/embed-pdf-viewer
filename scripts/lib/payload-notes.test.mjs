import assert from 'node:assert/strict';
import test from 'node:test';
import {
  ALL_BOUND_SINCE,
  abiCheckFunctions,
  assertAllBound,
  committedSignatures,
  forkLines,
  payloadNotes,
} from './payload-notes.mjs';

const PINNED = '8b1e69abdef4cb7c5d6c669c96a162533b2350ef';
const ARCHIVED = '6dfc036978f78baf71663f2554f998f28c490a6f';
const archived = {
  sha: ARCHIVED,
  libSha256: 'lib',
  source: {
    archive: 'libembedpdf-runtime-wasm32-eh-6dfc03697.tar.gz',
    archiveSha256: 'tar',
    emsdk: '4.0.0',
  },
};
const delta = {
  commits: 4,
  paths: ['scripts/embedpdf-runtime/target-args.sh', 'docs/embedpdf/fork-ledger.md'],
  argsMatch: true,
};

const slot = (kind) => ({
  ts: 'number',
  wasm: { kind, cwrap: 'number' },
  native: { kind, cwrap: 'number' },
});
const bindings = [
  '/* Auto-generated - do not edit by hand. */',
  'export interface PdfFunctions {',
  '  A: (arg0: number) => number;',
  '}',
  '',
  'export const pdfFunctionSignatures = {',
  `  A: { params: ${JSON.stringify([slot('i64')])}, result: ${JSON.stringify(slot('i32'))} },`,
  `  B: { params: [], result: null },`,
  '} as const;',
  '',
].join('\n');

test('committedSignatures reads every entry of pdfFunctionSignatures', () => {
  const functions = committedSignatures(bindings);
  assert.deepEqual(
    functions.map((fn) => fn.name),
    ['A', 'B'],
  );
  assert.equal(functions[0].params[0].wasm.kind, 'i64');
  assert.equal(functions[1].result, null);
  assert.throws(() => committedSignatures('export const x = {};'), /no pdfFunctionSignatures/);
});

test('abiCheckFunctions checks what the runtime declares and names the rest', () => {
  const { functions, unbound } = abiCheckFunctions(committedSignatures(bindings), ['A', 'C']);
  assert.deepEqual(
    functions.map((fn) => fn.name),
    ['A'],
  );
  assert.deepEqual(unbound, ['B']);
});

test('a runtime from the structure-position merge on must declare every committed binding in its headers', () => {
  assert.equal(ALL_BOUND_SINCE, '28b8b808f52d544b17a973199410cda0cc43530f');
  const unbound = ['EPDFAnnot_GetStructurePosition'];
  assert.throws(
    () => assertAllBound(unbound, { containsAllBoundSince: true }),
    /must declare every committed binding.*EPDFAnnot_GetStructurePosition/,
  );
  assert.doesNotThrow(() => assertAllBound([], { containsAllBoundSince: true }));
  // An older runtime may still lack one; the notes name it.
  assert.doesNotThrow(() => assertAllBound(unbound, { containsAllBoundSince: false }));
});

test('a library built at the pinned commit names it', () => {
  const lines = forkLines({
    fork: { sha: PINNED, libSha256: 'lib' },
    runtimeSrc: PINNED,
    target: 'wasm32-eh',
  });
  assert.deepEqual(lines, [
    `- Fork commit (runtime-src): \`${PINNED}\`, built for wasm32-eh`,
    '- libembedpdf.a sha256: `lib`',
  ]);
});

test('a library from a release archive says where it came from and how the pin differs', () => {
  const lines = forkLines({ fork: archived, runtimeSrc: PINNED, target: 'wasm32-eh', delta });
  assert.deepEqual(lines, [
    `- PDFium: built from fork \`${ARCHIVED}\`, the wasm32-eh library of its release archive \`libembedpdf-runtime-wasm32-eh-6dfc03697.tar.gz\` (sha256 \`tar\`), with emsdk 4.0.0`,
    `- runtime-src: \`${PINNED}\`, which adds 4 commits on top of \`${ARCHIVED}\` that change only scripts/ and docs/. The source and the gn args are identical.`,
    '- libembedpdf.a sha256: `lib`',
  ]);
  assert.ok(!lines.join('\n').includes('built for'), 'never labelled as built from the pin');
});

test('a delta that could change the library is refused', () => {
  const at = (over) => () =>
    forkLines({
      fork: archived,
      runtimeSrc: PINNED,
      target: 'wasm32-eh',
      delta: { ...delta, ...over },
    });
  assert.throws(at({ paths: ['core/fpdfapi/page/cpdf_page.cpp'] }), /changes the build/);
  assert.throws(at({ argsMatch: false }), /args.gn/);
  assert.throws(at({ commits: 0 }), /no commits/);
  assert.throws(
    () =>
      forkLines({
        fork: { sha: ARCHIVED, libSha256: 'lib' },
        runtimeSrc: PINNED,
        target: 'wasm32-eh',
      }),
    /not the pinned/,
  );
});

test('payloadNotes keeps the release fields and names unbound functions', () => {
  const notes = payloadNotes({
    hash: 'cd135983bd35d8da',
    head: 'head',
    fork: archived,
    runtimeSrc: PINNED,
    target: 'wasm32-eh',
    delta,
    rustc: 'rustc 1.98.0',
    asset: 'engine-runtime-payload-wasm32.tar.gz',
    digest: 'digest',
    unbound: ['EPDFAnnot_GetStructurePosition'],
  });
  const lines = notes.split('\n');
  assert.equal(lines[2], '- Payload hash: `cd135983bd35d8da`');
  assert.equal(lines[3], '- Viewer commit: `head`');
  assert.equal(lines.at(-2), '- engine-runtime-payload-wasm32.tar.gz sha256: `digest`');
  assert.equal(
    lines.at(-1),
    "- Bound in the TypeScript but not declared by the runtime's headers: `EPDFAnnot_GetStructurePosition`",
  );
  const none = payloadNotes({
    hash: 'h',
    head: 'head',
    fork: { sha: PINNED, libSha256: 'lib' },
    runtimeSrc: PINNED,
    target: 'wasm32-eh',
    rustc: 'rustc',
    asset: 'a',
    digest: 'd',
    unbound: [],
  });
  assert.ok(!none.includes('not declared'));
});
