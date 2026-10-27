import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites as assets } from '../src/suites/stamp-assets';
import { suites as api } from '../src/suites/stamp-api';
import { runScenario } from '../src/scenario';

test('current asset append commits a real descriptor and resource bytes', async () => {
  const suite = assets[0];
  assert.equal(suite.kind, 'scenario');
  const input = suite.cases.find((c) => c.name === 'appends-a-page-to-a-canonical-library');
  assert.ok(input);
  const result = await runScenario(input);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
    [],
  );
  const fulfilled = result.trace.flatMap((t) => t.settled).filter((s) => 'resolved' in s);
  assert.ok(fulfilled.some((s) => JSON.stringify(s.resolved).includes('Signed')));
  assert.ok(fulfilled.some((s) => JSON.stringify(s.resolved).includes('JVBERi1jYW5vbmljYWwtMg==')));
});

import { appendDoc, ONE, named, mutationDoc } from '../src/suites/stamp-mutation-fixtures';
import { str, nm, arr } from '../src/suites/stamp-current-fixtures';
import { pr, objRef } from '../src/suites/annotation-live-fixtures';
import { stampPieceInfo } from '../../../packages/plugin/stamp/src/convention';

test('append static answers retain the existing key-sorted registry and page metadata siblings', () => {
  const fixture = appendDoc(ONE, 'New', { number: 101, namedPages: [named('Old=Original', 100)] });
  const answer = fixture.responses?.find(
    (r) => r.path === 'pages.setName' && JSON.stringify(r.args).includes('New=New'),
  );
  assert.ok(answer);
  assert.deepEqual(answer.value, {
    layout: {
      pageCount: 2,
      pages: [
        ...ONE,
        {
          ...ONE[0],
          index: 1,
          ref: pr(101),
          size: { width: 300, height: 120 },
          pdfCropBox: { left: 0, bottom: 0, right: 300, top: 120 },
          boxes: Object.fromEntries(
            ['media', 'crop', 'bleed', 'trim', 'art'].map((k) => [
              k,
              { x: 0, y: 0, width: 300, height: 120 },
            ]),
          ),
        },
      ],
      namedPages: [named('New=New', 101), named('Old=Original', 100)],
    },
    meta: {
      affectedPages: [pr(101)],
      cacheDelta: null,
      opId: 'asset-engine-write',
      undoable: true,
    },
  });
  const edit = mutationDoc(ONE, {
    page: {
      100: {
        Id: str('owned'),
        Keep: arr('a', 'b'),
        SubjectOverride: str('old'),
        Name: str('obsolete'),
      },
    },
    pagePatches: [{ page: 100, patch: stampPieceInfo('signature', { categories: ['legal'] }) }],
  });
  const piece = edit.responses?.find((r) => r.path === 'page.pieceInfo.update');
  assert.deepEqual(piece?.value, {
    pieceInfo: {
      entries: {
        Id: str('owned'),
        Keep: arr('a', 'b'),
        Version: { type: 'number', value: 2 },
        Kind: nm('Signature'),
        Categories: arr('legal'),
      },
      modifiedAt: null,
    },
    meta: {
      affectedPages: [pr(100)],
      cacheDelta: null,
      opId: 'asset-engine-write',
      undoable: true,
    },
  });
});

test('canonical append case preserves the imported Stamp1 registration in its static result', () => {
  const suite = assets[0];
  assert.equal(suite.kind, 'scenario');
  const fixture = suite.cases.find((c) => c.name === 'appends-a-page-to-a-canonical-library')
    ?.engine?.documents?.append;
  const answer = fixture?.responses?.find(
    (r) => r.path === 'pages.setName' && JSON.stringify(r.args).includes('Signed=Signed'),
  );
  assert.ok(answer);
  assert.deepEqual((answer.value as { layout: { namedPages: unknown[] } }).layout.namedPages, [
    named('Signed=Signed', 101),
    named('Stamp1=Stamp1', 100),
  ]);
});

import type { ScenarioCase, TraceEntry } from '../src/scenario';
import type { StampAsset, StampLibrary } from '../../../packages/plugin/stamp/src/contract';
import type { StampState } from '../../../packages/plugin/stamp/src/model';
import { canonical } from '../src/canonical';
import { operationAt } from '../src/suites/annotation-live-fixtures';
import { PDF, png, jpeg, pdf, bytes } from '../src/suites/stamp-current-fixtures';
const cases: ScenarioCase[] = [...assets, ...api].flatMap((s) => {
  assert.equal(s.kind, 'scenario');
  return s.cases;
});
const cache = new Map<string, ReturnType<typeof runScenario>>();
function input(name: string) {
  const c = cases.find((c) => c.name === name);
  assert.ok(c, name);
  return c;
}
function run(name: string) {
  let result = cache.get(name);
  if (!result) {
    result = runScenario(input(name));
    cache.set(name, result);
  }
  return result;
}
function resolved<T = unknown>(trace: TraceEntry[], step: number): T {
  const r = trace[step].settled.find((s) => 'resolved' in s);
  assert.ok(r, `resolved ${step}`);
  return r.resolved as T;
}
function value<T = unknown>(trace: TraceEntry[], step: number): T {
  const r = trace[step].outcome;
  assert.ok(r && 'returned' in r, `returned ${step}`);
  return r.returned as T;
}
function state(trace: TraceEntry[], step = trace.length - 1): StampState {
  const state = trace[step].state as { plugins?: { stamp?: StampState }; stamp?: StampState };
  const s = state.plugins?.stamp ?? state.stamp;
  assert.ok(s);
  return s;
}
type Call = { doc: string | null; path: string; pon?: number; args: unknown[] };
function calls(trace: TraceEntry[]): Call[] {
  return trace.flatMap((t) => t.engine) as Call[];
}
const plain = (message: string, name = 'Error') => ({ $error: { name, message } });
const error = (
  message: string,
  code = 'operation-failed',
  cause?: unknown,
  permission: string | null = null,
) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'stamp',
    permission,
    ...(cause ? { cause } : {}),
  },
});
const engine = (message: string, code = 'Unknown', pluginCode = 'operation-failed') =>
  error(message, pluginCode, { $error: { name: 'EngineError', message, code } });
const missing = (kind: string, id: string) => error(`unknown ${kind} '${id}'`, 'not-found');
const badSource = error(
  'asset source must be PNG, JPEG, or single-page PDF bytes',
  'invalid-input',
);
const noCanonical = error("canonical bytes are missing for library 'stamp-lib-0-1'");
const gone = error("library 'stamp-lib-0-1' no longer exists", 'not-found');
const neg = (method: string, error: unknown) => ({ method, error });
const rejected: Record<string, unknown[]> = {
  'asset-sources-that-are-refused': [
    neg('createAsset', missing('library', 'missing')),
    neg('createAsset', badSource),
    neg('createAsset', badSource),
    neg(
      'createAsset',
      error(
        "Cannot destructure property 'data' of 'source' as it is null.",
        'operation-failed',
        plain("Cannot destructure property 'data' of 'source' as it is null.", 'TypeError'),
      ),
    ),
    neg(
      'createAsset',
      error(
        "Cannot read properties of null (reading 'arrayBuffer')",
        'operation-failed',
        plain("Cannot read properties of null (reading 'arrayBuffer')", 'TypeError'),
      ),
    ),
    neg('createAsset', badSource),
  ],
  'into-an-existing-loose-library': [
    neg('createAsset', error("library '' no longer exists", 'not-found')),
  ],
  'appends-that-are-refused': [
    neg('createAsset', error('a library asset must be a single-page PDF', 'invalid-input')),
    neg('createAsset', error('the new page is missing from the library layout')),
    neg('createAsset', engine('page PieceInfo unsupported', 'Unsupported')),
    neg('createAsset', engine('page 555 disappeared', 'NotFound', 'not-found')),
  ],
  'an-unsaved-append-changes-nothing': [
    neg('createAsset', error('save failed', 'operation-failed', plain('save failed'))),
  ],
  'a-save-that-answers-nothing': [
    neg('exportLibrary', missing('library', 'stamp-lib-0-1')),
    neg('createAsset', noCanonical),
    neg('deleteAsset', noCanonical),
  ],
  'removing-the-last-page-drops-the-library': [
    neg(
      'deleteAsset',
      engine('a document must keep at least one page', 'InvalidArg', 'invalid-input'),
    ),
  ],
  'a-failed-page-delete-keeps-the-asset': [neg('deleteAsset', engine('locked', 'Refused'))],
  'remove-library': [neg('exportLibrary', missing('library', 'stamp-lib-0-1'))],
  'an-append-queued-behind-a-removed-library': [neg('createAsset', gone)],
  'a-loose-append-queued-behind-a-removed-library': [neg('createAsset', gone)],
  'create-update-and-delete-libraries': [
    neg('updateLibrary', missing('library', 'nope')),
    neg('exportLibrary', missing('library', 'nope')),
  ],
  'library-writes-that-fail': [
    neg('updateLibrary', noCanonical),
    neg('updateAsset', noCanonical),
    neg(
      'createAsset',
      error('a raster asset needs a positive `size` (its page size in points)', 'invalid-input'),
    ),
    neg('createLibrary', engine('metadata unsupported', 'Unsupported')),
    neg('createLibrary', engine('disk full', 'Io')),
  ],
  'registry-refusals': [
    neg(
      'importLibrary',
      error("library 'Stamps': empty stamp identifier in key '=Label'", 'invalid-input'),
    ),
    neg(
      'importLibrary',
      error("library 'Stamps': duplicate stamp identifier 'A'", 'invalid-input'),
    ),
    neg(
      'importLibrary',
      error(
        'the asset engine reports no named-page registry; an engine with named pages is needed to import libraries',
        'unsupported',
      ),
    ),
  ],
  'marks-that-are-refused': [
    ...[0, 1].map(() =>
      neg(
        'createAsset',
        error('createAsset takes exactly one of `source` / `mark`', 'invalid-input'),
      ),
    ),
    ...[
      "mark color must be #rrggbb, got 'red'",
      'an ink mark needs at least one point',
      'the PDF has no page 4 (1 pages)',
      "invalid stamp identifier '': must be non-empty and contain no '='",
      "invalid stamp identifier 'a=b': must be non-empty and contain no '='",
      "mark color must be #rrggbb, got '#12345'",
      "mark color must be #rrggbb, got '#zzzzzz'",
      'the PDF has no page -1 (1 pages)',
    ].map((m) => neg('createAsset', error(m, 'invalid-input'))),
  ],
  'an-ink-mark-with-defaults': [neg('createAsset', error('the scratch document has no page'))],
  'edit-move-and-copy-assets': [
    neg('updateAsset', missing('asset', 'nope')),
    neg('duplicateAsset', missing('asset', 'nope')),
    neg('moveAsset', missing('asset', 'nope')),
    neg('createAsset', error("library 'Other' already has a stamp named 'Dup'", 'invalid-input')),
  ],
  'edits-queued-behind-removals': [
    neg('updateAsset', missing('asset', 'stamp-lib-0-1:Stamp1')),
    neg('updateLibrary', missing('library', 'stamp-lib-0-1')),
  ],
  'raster-pages-that-fail': [
    neg('createAsset', error('flattening the raster into its page failed')),
    neg('createAsset', engine('flatten unsupported', 'Unsupported')),
  ],
  'an-asset-from-annotations': [
    neg(
      'createAssetFromAnnotations',
      error('createAsset takes exactly one of `source` / `mark`', 'invalid-input'),
    ),
    neg(
      'createAssetFromAnnotations',
      error(
        'stamp.createAssetFromAnnotations requires doc.download',
        'permission-denied',
        undefined,
        'doc.download',
      ),
    ),
    neg('createAssetFromAnnotations', error("no page 999 in document 'a'", 'not-found')),
    neg(
      'createAssetFromAnnotations',
      error(
        'stamp.createAssetFromAnnotations requires doc.download',
        'permission-denied',
        undefined,
        'doc.download',
      ),
    ),
  ],
  'rendered-previews': [
    neg('renderAssetPreview', missing('asset', 'nope')),
    neg('renderAssetPreview', missing('asset', 'stamp-lib-0-1:Stamp1')),
  ],
  'place-without-the-pointer': [
    neg('placeAsset', missing('asset', 'nope')),
    neg(
      'placeAssetOnPages',
      error(
        'stamp.placeAssetOnPages requires annotations:create',
        'permission-denied',
        undefined,
        'annotations:create',
      ),
    ),
    neg('placeAssetOnPages', error("no page 999 in document 'a'", 'not-found')),
  ],
  'a-store-that-cannot-list': [neg('restore', plain('offline'))],
};
const consoles: Record<string, unknown[]> = {
  'import-a-registered-library': [
    {
      fn: 'console.warn',
      args: [
        "[stamp] library 'Goedkeuringen' carries 1 hidden-template or dangling registration(s); they are not stamps and were ignored",
      ],
    },
  ],
  'rendered-previews': [
    {
      fn: 'console.warn',
      args: [
        '[stamp] ghost render failed, using the thumbnail:',
        { $error: { name: 'EngineError', message: 'render failed', code: 'Io' } },
      ],
    },
  ],
  'restore-stored-libraries': [
    {
      fn: 'console.warn',
      args: [
        "[stamp] stored library 'junk' could not be restored:",
        error(
          'importLibrary needs PDF bytes (use createAsset for a raster image)',
          'invalid-input',
        ),
      ],
    },
  ],
  'persistence-failures-are-reported': [
    {
      fn: 'console.warn',
      args: ["[stamp] persisting library 'a' failed:", plain('quota exceeded')],
    },
    { fn: 'console.warn', args: ["[stamp] deleting stored library 'a' failed:", plain('locked')] },
    {
      fn: 'console.warn',
      args: ["[stamp] persisting library 'b' failed:", plain('quota exceeded')],
    },
  ],
};
const reports: Record<string, unknown[]> = {
  'create-update-and-delete-libraries': [0, 1, 2].map(() => plain('listener broke')),
  'a-queued-mutation-after-destroy': [
    error('a state update after the instance closed (stamp#2); dropped', 'instance-closed'),
  ],
};
for (const c of cases)
  test(`all current channels are exact: ${c.name}`, async () => {
    const { trace, unmatched } = await run(c.name);
    assert.deepEqual(unmatched, []);
    assert.deepEqual(
      trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
      [],
    );
    const methods = new Map<number, string>();
    const negatives: unknown[] = [];
    trace.forEach((t, i) => {
      if (t.outcome && 'promise' in t.outcome) {
        const step = c.steps[i];
        methods.set(
          t.outcome.promise,
          step.op === 'call' ? (step.method ?? '<missing-method>') : step.op,
        );
      }
      for (const s of t.settled)
        if ('rejected' in s) negatives.push(neg(methods.get(s.promise)!, s.rejected));
    });
    assert.deepEqual(negatives, rejected[c.name] ?? []);
    assert.deepEqual(
      trace.flatMap((t) => t.callbacks).filter((c) => c.fn.startsWith('console.')),
      consoles[c.name] ?? [],
    );
    assert.deepEqual(
      trace.flatMap((t) => t.reports),
      reports[c.name] ?? [],
    );
  });

test('raster operations carry exact dimensions, resources and canonical PDF caches', async () => {
  const { trace } = await run('a-raster-asset-is-sniffed-and-previews-itself');
  const all = calls(trace);
  assert.deepEqual(
    all.filter((c) => c.path === 'pages.insertBlank').map((c) => c.args),
    [[{ size: { width: 100, height: 50 } }], [{ size: { width: 64, height: 32 } }]],
  );
  assert.deepEqual(
    all.filter((c) => c.path === 'page.annotations.create').map((c) => c.args),
    [
      [
        { subtype: 'stamp', box: { x: 0, y: 0, width: 100, height: 50 }, fit: 'fill' },
        { resources: { appearance: canonical(png(100, 50)) } },
      ],
      [
        { subtype: 'stamp', box: { x: 0, y: 0, width: 64, height: 32 }, fit: 'fill' },
        { resources: { appearance: canonical(jpeg()) } },
      ],
    ],
  );
  assert.deepEqual(
    all.filter((c) => c.path === 'pages.flatten').map((c) => c.args),
    [0, 1].map(() => [[pr(900)], { usage: 'display' }]),
  );
  assert.deepEqual(value(trace, 13), { bytes: canonical(bytes('png-900')), mimeType: 'image/png' });
  assert.deepEqual(value(trace, 14), canonical(pdf('page-900')));
  assert.deepEqual(resolved(trace, 15), canonical(pdf('canonical-2')));
  const assets = value<StampAsset[]>(trace, 12);
  assert.deepEqual(
    assets.map((a) => [a.id, a.kind, a.size, a.subject, a.categories]),
    [
      ['stamp-lib-0-0:Logo', 'stamp', { width: 100, height: 50 }, undefined, undefined],
      ['stamp-lib-0-3:Photo', 'signature', { width: 64, height: 32 }, 'Scan', ['Ink']],
    ],
  );
  assert.equal(all.filter((c) => c.path === 'close').length, 4);
  assert.deepEqual(
    trace.flatMap((t) => t.callbacks).map((c) => c.fn),
    [
      'LibraryChanged',
      'LibraryCreated',
      'LibraryChanged',
      'AssetCreated',
      'LibraryChanged',
      'LibraryCreated',
      'LibraryChanged',
      'AssetCreated',
    ],
  );
});

test('supplied previews avoid engine rendering and PDF sizes come from page geometry', async () => {
  const a = await run('supplied-previews-and-sizes');
  assert.equal(calls(a.trace).filter((c) => c.path === 'page.render.image').length, 0);
  assert.deepEqual(value(a.trace, 5), { bytes: canonical(jpeg()), mimeType: 'image/jpeg' });
  assert.deepEqual(value(a.trace, 6), { bytes: canonical(png(8, 8)), mimeType: 'image/png' });
  assert.deepEqual(value(a.trace, 7), { bytes: canonical(png(8, 8)), mimeType: 'image/png' });
  assert.deepEqual(value<StampAsset[]>(a.trace, 4)[0].size, { width: 40, height: 20 });
  const b = await run('a-loose-pdf-needs-a-size');
  assert.deepEqual(
    value<StampAsset[]>(b.trace, 4).map((a) => a.size),
    [0, 1, 2].map(() => ({ width: 300, height: 120 })),
  );
  const c = await run('a-supplied-preview-skips-the-render');
  assert.equal(calls(c.trace).filter((c) => c.path === 'page.render.image').length, 1);
  assert.deepEqual(value(c.trace, 3), { bytes: canonical(png()), mimeType: 'image/webp' });
});

test('canonical append returns exact asset, writes, extracted bytes and fact payloads', async () => {
  const { trace } = await run('appends-a-page-to-a-canonical-library');
  const asset: StampAsset = {
    id: 'stamp-lib-0-1:Signed',
    libraryId: 'stamp-lib-0-1',
    name: 'Signed',
    label: 'Signed',
    kind: 'signature',
    page: pr(101),
    size: { width: 300, height: 120 },
    subject: 'Customer sign-off',
    categories: ['Signature'],
  };
  assert.deepEqual(resolved(trace, 10), { asset });
  assert.deepEqual(value<StampAsset[]>(trace, 11).at(-1), asset);
  assert.deepEqual(value(trace, 13), canonical(pdf('page-101')));
  assert.deepEqual(value(trace, 14), { bytes: canonical(bytes('png-101')), mimeType: 'image/png' });
  const a = calls(trace).filter((c) => c.doc === 'append');
  assert.deepEqual(
    a.map((c) => c.path),
    [
      'pages.insert',
      'pages.list',
      'pages.setName',
      'page.pieceInfo.update',
      'pages.extract',
      'page.render.image',
      'download',
      'close',
    ],
  );
  assert.deepEqual(a[0].args, [canonical(PDF)]);
  assert.deepEqual(a[2].args, [{ name: 'Signed=Signed', page: pr(101) }]);
  assert.deepEqual(a[3].args, [
    'EMBD_Stamp',
    {
      Version: 2,
      Kind: { name: 'Signature' },
      Name: null,
      Subject: null,
      SubjectOverride: 'Customer sign-off',
      Categories: ['Signature'],
    },
  ]);
  assert.deepEqual(a[4].args, [[pr(101)]]);
  assert.deepEqual(a[5].args, [
    {
      viewport: { kind: 'width', width: 256 },
      background: 'transparent',
      includeAnnotations: true,
      format: 'png',
    },
  ]);
  assert.deepEqual(trace[10].callbacks, [
    { fn: 'LibraryChanged', args: [{ libraryId: asset.libraryId, reason: 'asset-added' }] },
    { fn: 'AssetCreated', args: [{ assetId: asset.id, libraryId: asset.libraryId, asset }] },
  ]);
});

test('failed append and delete preserve confirmed resources and recover from the same canonical bytes', async () => {
  const a = await run('an-unsaved-append-changes-nothing');
  assert.deepEqual(
    value<StampAsset[]>(a.trace, 11).map((a) => a.id),
    ['stamp-lib-0-1:Stamp1'],
  );
  assert.deepEqual(resolved(a.trace, 12), canonical(pdf('canonical')));
  assert.deepEqual(resolved(a.trace, 14), canonical(pdf('canonical-2')));
  assert.deepEqual(a.trace[10].callbacks, []);
  assert.deepEqual(
    calls(a.trace)
      .filter((c) => c.path === 'open')
      .slice(1)
      .map((c) => (c.args[0] as { bytes: unknown }).bytes),
    [canonical(pdf('canonical')), canonical(pdf('canonical'))],
  );
  assert.deepEqual(
    calls(a.trace)
      .filter((c) => c.doc === 'broken')
      .at(-1)?.path,
    'close',
  );
  const b = await run('a-failed-page-delete-keeps-the-asset');
  assert.equal(value<StampAsset>(b.trace, 11).id, 'stamp-lib-0-1:Stamp1');
  assert.deepEqual(value(b.trace, 12), canonical(pdf('page-100')));
  assert.deepEqual(resolved(b.trace, 13), canonical(pdf('canonical')));
  assert.equal(value(b.trace, 15), null);
  assert.deepEqual(b.trace[10].callbacks, []);
  assert.equal(calls(b.trace).filter((c) => c.path === 'close').length, 3);
  const failed = await run('appends-that-are-refused');
  assert.deepEqual(
    value<StampAsset[]>(failed.trace, 6).map((a) => a.id),
    ['stamp-lib-0-1:Stamp1'],
  );
  assert.deepEqual(resolved(failed.trace, 7), canonical(pdf('canonical')));
  assert.equal(calls(failed.trace).filter((c) => c.path === 'close').length, 5);
});

test('delete removes binaries only after rewrite and leaves an empty seed-backed library', async () => {
  const { trace } = await run('removes-a-canonical-page-first');
  assert.equal(value(trace, 12), null);
  assert.equal(value(trace, 13), null);
  assert.deepEqual(
    value<StampAsset[]>(trace, 14).map((a) => a.id),
    ['stamp-lib-0-1:Stamp2'],
  );
  assert.deepEqual(resolved(trace, 15), canonical(pdf('canonical-rewritten')));
  const deletion = calls(trace).find((c) => c.path === 'pages.delete');
  assert.deepEqual(deletion?.args, [[pr(100)]]);
  assert.deepEqual(trace[11].callbacks, [
    { fn: 'LibraryChanged', args: [{ libraryId: 'stamp-lib-0-1', reason: 'asset-removed' }] },
    {
      fn: 'AssetDeleted',
      args: [{ assetId: 'stamp-lib-0-1:Stamp1', libraryId: 'stamp-lib-0-1', asset: null }],
    },
  ]);
  assert.deepEqual(trace[10].callbacks, []);
  const last = await run('removing-the-last-page-drops-the-library');
  assert.equal(value<StampLibrary[]>(last.trace, 7).length, 2);
  assert.equal(value(last.trace, 8), null);
  assert.deepEqual(value<StampLibrary[]>(last.trace, 7)[1].assetIds, []);
  const drop = await run('remove-library');
  assert.deepEqual(value(drop.trace, 12), []);
  assert.deepEqual(value(drop.trace, 13), []);
  assert.equal(value(drop.trace, 14), null);
  assert.deepEqual(drop.trace[10].callbacks, []);
  assert.deepEqual(drop.trace[16].callbacks, []);
});

test('library queues serialize rewrites while another library progresses independently', async () => {
  const { trace } = await run('removals-are-serialized-per-library');
  assert.deepEqual(
    trace.slice(3, 6).map((t) => t.settled),
    [[], [], []],
  );
  assert.equal(
    trace[6].settled.length,
    1,
    'other library completes before the deferred first queue',
  );
  assert.equal(trace[7].settled.length, 3, 'both queued removals and no-op settle after reopen');
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'pages.delete')
      .map((c) => [c.doc, c.args]),
    [
      ['otherRemove', [[pr(100)]]],
      ['first', [[pr(100)]]],
      ['second', [[pr(101)]]],
    ],
  );
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'open')
      .map((c) => (c.args[0] as { bytes: unknown }).bytes)
      .at(-1),
    canonical(pdf('canonical-rewritten')),
  );
  assert.deepEqual(
    value<StampAsset[]>(trace, 10).map((a) => a.id),
    ['stamp-lib-0-1:Stamp3', 'other:Stamp2'],
  );
  for (const name of [
    'an-append-queued-behind-a-removed-library',
    'a-loose-append-queued-behind-a-removed-library',
    'edits-queued-behind-removals',
  ]) {
    const r = await run(name);
    assert.deepEqual(value(r.trace, r.trace.length - 1), []);
    assert.equal(calls(r.trace).filter((c) => c.path === 'open').length, 2);
  }
  const a = await run('a-loose-append-finds-its-library-canonical');
  assert.deepEqual(
    value<StampAsset[]>(a.trace, 6).map((a) => a.id),
    ['stamp-lib-0-1:Stamp2', 'stamp-lib-0-1:Late'],
  );
});

test('asset update clears only the subject; copies and move retain metadata, previews and owned ids', async () => {
  const { trace } = await run('edit-move-and-copy-assets');
  assert.equal(resolved<{ asset: StampAsset }>(trace, 10).asset.subject, 'New');
  assert.equal('subject' in resolved<{ asset: StampAsset }>(trace, 11).asset, false);
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'pages.setName' && c.doc?.startsWith('e'))
      .map((c) => c.args),
    [[{ name: 'Stamp1=Relabelled', page: pr(100), replace: 'Stamp1=Stamp1' }]],
  );
  assert.deepEqual(trace[17].engine, [], 'same-library move opens nothing');
  assert.equal(resolved<{ asset: StampAsset }>(trace, 17).asset.id, 'stamp-lib-0-1:Stamp1');
  assert.deepEqual(
    value<StampAsset[]>(trace, 22).map((a) => [a.id, a.label, a.categories]),
    [
      ['stamp-lib-0-1:#ABCDEFGHIJKLMNOPQRSTUV', 'Relabelled copy', ['d']],
      ['stamp-lib-0-1:#WXYZabcdefghijklmnopqr', 'Named copy', ['d']],
      ['other:#stuvwxyz0123456789ABCD', 'Relabelled', ['d']],
      ['other:Dup', 'Dup', undefined],
    ],
  );
  assert.deepEqual(value(trace, 24), []);
  assert.equal(value<StampAsset[]>(trace, 25).length, 3);
  assert.equal(value<StampAsset[]>(trace, 26).length, 2);
  assert.deepEqual(
    calls(trace)
      .filter((c) => ['d1', 'd2', 'move'].includes(c.doc ?? '') && c.path === 'pages.insert')
      .map((c) => c.args),
    [0, 1, 2].map(() => [canonical(pdf('page-100'))]),
  );
  assert.equal(
    calls(trace).filter(
      (c) => ['d1', 'd2', 'move'].includes(c.doc ?? '') && c.path === 'page.render.image',
    ).length,
    0,
  );
  const b = await run('copies-carry-their-metadata');
  const assets = value<StampAsset[]>(b.trace, 6);
  const copied = assets.filter((a) => a.subject === 'Subj');
  assert.deepEqual(
    copied.map((a) => [a.libraryId, a.categories]),
    [
      ['stamp-lib-0-1', ['k']],
      ['other', ['k']],
    ],
  );
  assert.ok(!assets.some((a) => a.id === 'stamp-lib-0-1:Stamp2'));
});

test('mark authoring sends current ink/free-text drafts and closes each scratch export', async () => {
  const { trace } = await run('marks-become-assets');
  const all = calls(trace);
  const drafts = all.filter(
    (c) => c.path === 'page.annotations.create' && ['ink', 'text'].includes(c.doc ?? ''),
  );
  assert.deepEqual(
    drafts.map((c) => c.args),
    [
      [
        {
          subtype: 'ink',
          inkList: [
            [
              { x: 10, y: 20 },
              { x: 40, y: 5 },
            ],
            [{ x: 0, y: 30 }],
          ],
          color: '#ff8000',
          strokeWidth: 3,
        },
      ],
      [
        {
          subtype: 'free-text',
          intent: 'free-text',
          contents: 'Ada',
          fontFamily: 'Helvetica',
          fontSize: 36,
          textAlign: 'center',
          verticalAlign: 'middle',
          box: { x: 0, y: 0, width: 117, height: 57.6 },
          fontColor: '#1d2b53',
          strokeWidth: 0,
        },
      ],
    ],
  );
  assert.deepEqual(
    all.filter((c) => c.path === 'page.annotations.exportAppearance').map((c) => c.args),
    [0, 1].map(() => [[objRef(40, 3)]]),
  );
  assert.deepEqual(
    all.filter((c) => c.doc === 'source' && c.path === 'pages.extract').map((c) => c.args),
    [[[pr(101)]]],
  );
  assert.deepEqual(
    value<StampAsset[]>(trace, 6).map((a) => [a.name, a.label]),
    [
      ['Ink', 'Ink'],
      ['#ABCDEFGHIJKLMNOPQRSTUV', 'Typed'],
      ['Picture', 'Picture'],
      ['Whole', 'Whole'],
      ['Second page', 'Second page'],
    ],
  );
  assert.equal(all.filter((c) => c.path === 'close').length, 13);
  const defaults = await run('an-ink-mark-with-defaults');
  assert.deepEqual(
    calls(defaults.trace).find((c) => c.doc === 'ink' && c.path === 'page.annotations.create')
      ?.args,
    [{ subtype: 'ink', inkList: [[{ x: 1, y: 2 }]], color: '#1d2b53', strokeWidth: 2 }],
  );
  assert.equal(value<StampAsset[]>(defaults.trace, 3).length, 1);
  const bad = await run('marks-that-are-refused');
  assert.equal(calls(bad.trace).filter((c) => c.path === 'close').length, 6);
  assert.deepEqual(value(bad.trace, 11), []);
  assert.equal(calls(bad.trace).filter((c) => c.path === 'page.annotations.create').length, 0);
});

test('imported canonical owner survives a preferred-id collision and unknown asset deletion', async () => {
  const { trace } = await run('a-loose-asset-under-a-canonical-id');
  const importedLibrary: StampLibrary = {
    id: 'stamp-lib-0-1',
    name: 'Stamps',
    kind: 'stamps',
    assetIds: ['stamp-lib-0-1:Stamp1', 'stamp-lib-0-1:Stamp2'],
  };
  const freshLibrary: StampLibrary = {
    id: 'stamp-lib-0-2',
    name: 'Logo',
    kind: 'stamps',
    assetIds: [],
  };
  const importedAssets: Record<string, StampAsset> = {
    'stamp-lib-0-1:Stamp1': {
      id: 'stamp-lib-0-1:Stamp1',
      libraryId: 'stamp-lib-0-1',
      kind: 'stamp',
      name: 'Stamp1',
      label: 'Stamp1',
      page: { kind: 'objectNumber', objectNumber: 100 },
      size: { width: 200, height: 100 },
    },
    'stamp-lib-0-1:Stamp2': {
      id: 'stamp-lib-0-1:Stamp2',
      libraryId: 'stamp-lib-0-1',
      kind: 'stamp',
      name: 'Stamp2',
      label: 'Stamp2',
      page: { kind: 'objectNumber', objectNumber: 101 },
      size: { width: 201, height: 101 },
    },
  };
  assert.deepEqual(resolved(trace, 1), { library: importedLibrary });
  assert.deepEqual(resolved(trace, 2), { library: freshLibrary });
  assert.deepEqual(value(trace, 3), [importedLibrary, freshLibrary]);
  assert.deepEqual(state(trace, 3).libraries, {
    'stamp-lib-0-1': importedLibrary,
    'stamp-lib-0-2': freshLibrary,
  });
  assert.deepEqual(state(trace, 3).assets, importedAssets);
  assert.deepEqual(resolved(trace, 4), { $undefined: true });
  assert.deepEqual(trace[4].engine, []);
  assert.deepEqual(trace[4].callbacks, []);
  assert.deepEqual(trace[4].stateChanges, []);
  assert.deepEqual(state(trace, 4), state(trace, 3));
  assert.deepEqual(state(trace, 4).assets, importedAssets);
  assert.deepEqual(value(trace, 5), importedAssets['stamp-lib-0-1:Stamp1']);
  assert.deepEqual(state(trace, 5).assets, importedAssets);
});

test('library edits write current metadata and complete catalog patches with exact fact counts', async () => {
  const { trace } = await run('create-update-and-delete-libraries');
  assert.deepEqual(
    value<StampLibrary[]>(trace, 13).map((l) => l.id),
    ['stamp-lib-0-0', 'sigs', 'stamp-lib-0-3'],
  );
  assert.deepEqual(
    value<StampLibrary[]>(trace, 14).map((l) => l.id),
    ['sigs'],
  );
  assert.deepEqual(value(trace, 16), []);
  assert.equal(value(trace, 18), null);
  const edits = calls(trace).filter((c) => c.doc === 'edit');
  assert.deepEqual(
    edits.filter((c) => c.path === 'metadata.update').map((c) => c.args),
    [[{ title: 'Signatures' }]],
  );
  assert.deepEqual(
    edits.filter((c) => c.path === 'pieceInfo.update').map((c) => c.args),
    [['A'], ['B'], ['B']].map((categories) => [
      'EMBD_StampLibrary',
      {
        Version: 2,
        Id: 'sigs',
        Kind: { name: 'SignatureLibrary' },
        Name: null,
        Categories: categories,
        Locale: null,
      },
    ]),
  );
  assert.deepEqual(resolved(trace, 23), canonical(pdf('edited')));
  assert.deepEqual(
    value<StampLibrary[]>(trace, 27).map((l) => l.id),
    ['stamp-lib-0-0', 'stamp-lib-0-3'],
  );
  const facts = trace.flatMap((t) => t.callbacks).filter((c) => !c.fn.endsWith('.bad'));
  assert.equal(facts.filter((c) => c.fn === 'LibraryCreated').length, 3);
  assert.equal(facts.filter((c) => c.fn === 'LibraryUpdated').length, 3);
  assert.equal(facts.filter((c) => c.fn === 'LibraryDeleted').length, 1);
  assert.deepEqual(trace[26].callbacks, []);
  const bad = await run('library-writes-that-fail');
  assert.deepEqual(
    value<StampLibrary[]>(bad.trace, 7).map((l) => l.id),
    ['stamp-lib-0-1'],
  );
  assert.equal(calls(bad.trace).filter((c) => c.path === 'close').length, 3);
});

test('registered import respects page order and current registry refusals recover into a custom kind', async () => {
  const { trace } = await run('import-a-registered-library');
  assert.deepEqual(value<StampLibrary[]>(trace, 2), [
    {
      id: 'nl',
      name: 'Goedkeuringen',
      kind: 'signatures',
      locale: 'nl-NL',
      assetIds: ['nl:Approved', 'nl:#LBG'],
    },
  ]);
  assert.deepEqual(
    value<StampAsset[]>(trace, 3).map((a) => [a.id, a.label, a.kind, a.subject, a.categories]),
    [
      ['nl:Approved', 'Goedgekeurd', 'initials', 'Paraaf', ['x']],
      ['nl:#LBG', 'Custom', 'stamp', undefined, undefined],
    ],
  );
  assert.equal(calls(trace).filter((c) => c.path === 'pages.setName').length, 0);
  const b = await run('registry-refusals');
  assert.deepEqual(
    value<StampLibrary[]>(b.trace, 5).map((l) => [l.id, l.kind]),
    [['stamp-lib-0-7', 'toolbar']],
  );
  assert.equal(calls(b.trace).filter((c) => c.path === 'close').length, 4);
});

test('annotation extraction enforces egress and page preflight before export and creates a real asset', async () => {
  const { trace } = await run('an-asset-from-annotations');
  assert.equal(resolved<{ asset: StampAsset }>(trace, 2).asset.id, 'stamp-lib-0-0:Selection');
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'page.annotations.exportAppearance')
      .map((c) => [c.doc, c.pon, c.args]),
    [
      ['a', 1, [[objRef(7)]]],
      ['a', 1, [[objRef(7)]]],
    ],
  );
  assert.deepEqual(calls(trace).find((c) => c.path === 'pages.insert')?.args, [
    canonical(pdf('selection')),
  ]);
  assert.equal(value(trace, 5), true);
  assert.equal(value(trace, 8), false);
  assert.deepEqual(trace[6].engine, []);
  assert.deepEqual(trace[9].engine, []);
  assert.deepEqual(
    value<StampAsset[]>(trace, 10).map((a) => a.id),
    ['stamp-lib-0-0:Selection'],
  );
});

test('programmatic placement carries exact ops/resources/opIds, confirmed facts and independent annotation readbacks', async () => {
  const { trace } = await run('place-without-the-pointer');
  const apply = calls(trace).filter((c) => c.path === 'apply');
  assert.equal(apply.length, 4);
  assert.deepEqual(
    apply.map((c) => c.args[1]),
    [1, 2, 3, 4].map((i) => ({ opId: operationAt(i) })),
  );
  const expected = [
    { page: pr(1), n: 10001, box: { x: 75, y: 87.5, width: 50, height: 25 }, rotation: null },
    { page: pr(1), n: 10002, box: { x: 0, y: 0, width: 200, height: 100 }, rotation: null },
    { page: pr(2), n: 10003, box: { x: 0, y: 0, width: 200, height: 100 }, rotation: null },
    { page: pr(2), n: 10004, box: { x: -50, y: 50, width: 200, height: 100 }, rotation: 90 },
  ];
  assert.deepEqual(
    apply.map((c) => c.args[0]),
    expected.map(({ page, n, box, rotation }) => ({
      ops: [
        {
          type: 'annotations.create',
          page,
          objectNumber: n,
          data: {
            subtype: 'stamp',
            box,
            rotation,
            fit: 'contain',
            name: 'Stamp1',
            subject: 'Stamp1',
          },
          resources: { appearance: canonical(pdf('page-100')) },
        },
      ],
    })),
  );
  const first = resolved<{ annotation: { ref: unknown; box: unknown } }>(trace, 5).annotation;
  assert.deepEqual(first.ref, objRef(10001));
  assert.deepEqual(first.box, expected[0].box);
  const batch = resolved<{ applied: { ref: unknown }[]; failed: unknown[]; skipped: unknown[] }>(
    trace,
    7,
  );
  assert.deepEqual(
    batch.applied.map((a) => a.ref),
    [objRef(10002)],
  );
  assert.deepEqual(batch.skipped, []);
  assert.deepEqual(batch.failed, [
    {
      ref: pr(2),
      error: {
        code: 'operation-failed',
        message: 'page locked',
        capability: 'annotation',
        permission: null,
      },
    },
  ]);
  const read = value<{ ref: unknown; rotation: unknown; box: unknown }[]>(trace, 10);
  assert.deepEqual(
    read.map((a) => a.ref),
    [objRef(10001), objRef(10002), objRef(10004, 2)],
  );
  assert.deepEqual(read[2].box, expected[3].box);
  assert.equal(read[2].rotation, 90);
  const facts = trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'annotationCreated');
  assert.equal(facts.length, 3);
  assert.deepEqual(
    facts.map((c) => (c.args[0] as { origin: unknown }).origin),
    [1, 2, 4].map((i) => ({
      kind: 'local',
      sessionId: 's1',
      sub: null,
      ts: 0,
      serverId: null,
      tx: { id: operationAt(i), index: 0, count: 1 },
    })),
  );
  assert.equal(
    trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'annotationWriteFailed').length,
    1,
  );
  assert.deepEqual(
    trace[11].engine,
    [],
    'missing second page refuses the whole batch before its first write',
  );
});

test('armed read follows current tool lifetime, and absent-document grants are false', async () => {
  const { trace } = await run('the-armed-asset');
  assert.equal(value(trace, 4), null);
  assert.equal(value<StampAsset>(trace, 6).id, 'stamp-lib-0-1:Stamp1');
  assert.equal(value(trace, 8), null);
  assert.deepEqual(
    trace
      .flatMap((t) => t.callbacks)
      .filter((c) => c.fn === 'armChanged')
      .map(
        (c) => (c.args[0] as { documentId: string; asset: StampAsset | null }).asset?.id ?? null,
      ),
    ['stamp-lib-0-1:Stamp1', null],
  );
  assert.deepEqual(
    [11, 12, 13, 14].map((i) => value(trace, i)),
    [true, true, false, true],
  );
  const b = await run('can-place-without-a-document');
  assert.equal(value(b.trace, 1), false);
  assert.equal(value(b.trace, 2), false);
});

test('preview cache buckets avoid reopening, fall back on failure and clear after deletion', async () => {
  const { trace } = await run('rendered-previews');
  assert.deepEqual(resolved(trace, 2), { bytes: canonical(bytes('png-5')), mimeType: 'image/png' });
  assert.deepEqual(resolved(trace, 3), resolved(trace, 2));
  assert.deepEqual(trace[3].engine, []);
  assert.deepEqual(resolved(trace, 4), {
    bytes: canonical(bytes('png-100')),
    mimeType: 'image/png',
  });
  assert.equal(resolved(trace, 5), null);
  assert.deepEqual(resolved(trace, 7), { bytes: canonical(pdf('no-type')), mimeType: 'image/png' });
  assert.equal(calls(trace).filter((c) => c.path === 'open').length, 6);
  assert.equal(calls(trace).filter((c) => c.path === 'close').length, 6);
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'page.render.image')
      .map((c) => (c.args[0] as { viewport: { width: number } }).viewport.width),
    [256, 256, 512, 128, 64],
  );
});

test('persistence uses current immediate burst semantics, file identity, exclusions and unsubscribe', async () => {
  const restore = await run('restore-stored-libraries');
  assert.deepEqual(resolved(restore.trace, 2), ['stamp-lib-0-1']);
  assert.equal(value<StampLibrary[]>(restore.trace, 3)[0].name, 'Kept');
  const a = await run('persist-library-changes');
  assert.deepEqual(resolved(a.trace, 5), [{ id: 'a', bytes: canonical(pdf('A2')) }]);
  assert.deepEqual(resolved(a.trace, 8), [{ id: 'b', bytes: canonical(pdf('seed-b')) }]);
  assert.deepEqual(resolved(a.trace, 11), resolved(a.trace, 8));
  const b = await run('stop-flushes-a-write-still-pending-behind-another');
  assert.deepEqual(resolved(b.trace, 4), [{ id: 'b', bytes: canonical(pdf('seed-b')) }]);
  assert.deepEqual(resolved(b.trace, 7), [
    { id: 'b', bytes: canonical(pdf('seed-b')) },
    { id: 'a', bytes: canonical(pdf('seed-a')) },
  ]);
  assert.deepEqual(resolved(b.trace, 9), resolved(b.trace, 7));
  assert.deepEqual(
    b.trace
      .flatMap((t) => t.callbacks)
      .filter((c) => c.fn === 'store.put')
      .map((c) => c.args),
    [
      ['a', canonical(pdf('seed-a'))],
      ['b', canonical(pdf('seed-b'))],
    ],
  );
  const fail = await run('persistence-failures-are-reported');
  assert.deepEqual(resolved(fail.trace, 5), []);
});

test('destroyed-instance queued writes keep their precise source outcome and restore host globals', async () => {
  const saved = [
    Object.getOwnPropertyDescriptor(Math, 'random'),
    Object.getOwnPropertyDescriptor(console, 'error'),
    Object.getOwnPropertyDescriptor(console, 'warn'),
    Object.getOwnPropertyDescriptor(crypto, 'randomUUID'),
  ];
  const { trace, unmatched } = await runScenario(input('a-queued-mutation-after-destroy'));
  assert.deepEqual(unmatched, []);
  assert.deepEqual(
    trace.flatMap((entry) => entry.reports),
    reports['a-queued-mutation-after-destroy'],
  );
  assert.deepEqual(trace[2].settled, []);
  assert.deepEqual(trace[3].settled, []);
  assert.equal(trace[5].settled.length, 2);
  assert.deepEqual(
    trace[5].settled.map((s) => s.promise),
    [2, 3],
  );
  assert.ok(trace[5].settled.every((s) => 'resolved' in s));
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'close')
      .map((c) => c.doc),
    ['lib', 'remove', 'late'],
  );
  assert.deepEqual(
    [
      Object.getOwnPropertyDescriptor(Math, 'random'),
      Object.getOwnPropertyDescriptor(console, 'error'),
      Object.getOwnPropertyDescriptor(console, 'warn'),
      Object.getOwnPropertyDescriptor(crypto, 'randomUUID'),
    ],
    saved,
  );
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

test('all original41 map exhaustively to complete current inputs with verified source owners', async () => {
  const root = new URL('../historical/task-12b2b3-iv/', import.meta.url);
  const manifest = JSON.parse(await readFile(new URL('accounting.json', root), 'utf8')) as {
    original: number;
    current: number;
    mapped: number;
    renamed: number;
    wholeCaseRetirements: number;
    added: number;
    mappings: {
      before: { file: string; name: string; inputSha256: string };
      after: { file: string; name: string; inputSha256: string };
      inputChanged: boolean;
      source: string[];
      reason: string;
    }[];
    sourceEvidence: { path: string; sha256: string; commit: string }[];
  };
  const original = JSON.parse(
    await readFile(new URL('../task-12b2b3/original-inputs.json', root), 'utf8'),
  ) as { file: string; name: string; inputSha256: string }[];
  const stored = JSON.parse(await readFile(new URL('current-inputs.json', root), 'utf8')) as {
    file: string;
    name: string;
    input: unknown;
    inputSha256: string;
  }[];
  const digest = (data: string | Buffer) => createHash('sha256').update(data).digest('hex');
  assert.deepEqual(
    [
      manifest.original,
      manifest.current,
      manifest.mapped,
      manifest.renamed,
      manifest.wholeCaseRetirements,
      manifest.added,
    ],
    [41, 41, 41, 0, 0, 0],
  );
  assert.equal(new Set(manifest.mappings.map((m) => m.before.file + ':' + m.before.name)).size, 41);
  assert.equal(new Set(cases.map((c) => c.name)).size, 41);
  assert.equal(stored.length, 41);
  for (const row of manifest.mappings) {
    const before = original.find((c) => c.file === row.before.file && c.name === row.before.name);
    assert.ok(before);
    const after = stored.find((c) => c.file === row.after.file && c.name === row.after.name);
    assert.ok(after);
    assert.equal(before.inputSha256, row.before.inputSha256);
    assert.deepEqual(after.input, canonical(input(after.name)));
    assert.equal(digest(JSON.stringify(after.input)), row.after.inputSha256);
    assert.equal(after.inputSha256, row.after.inputSha256);
    assert.equal(row.inputChanged, true);
    assert.ok(row.reason.length > 20);
    for (const source of row.source)
      assert.ok(
        manifest.sourceEvidence.some((e) => e.path === source),
        source,
      );
  }
  const repository = new URL('../../../', import.meta.url);
  for (const evidence of manifest.sourceEvidence) {
    const data = await readFile(new URL(evidence.path, repository));
    assert.equal(digest(data), evidence.sha256);
    assert.match(evidence.commit, /^[a-f0-9]{40}$/);
    assert.deepEqual(
      execFileSync('git', ['show', evidence.commit + ':' + evidence.path], { cwd: repository }),
      data,
    );
    assert.equal(
      execFileSync('git', ['log', '-1', '--format=%H', '--', evidence.path], {
        cwd: repository,
        encoding: 'utf8',
      }).trim(),
      evidence.commit,
    );
  }
});

test('reopened mutation answers preserve every surviving page size across copies and queued deletes', () => {
  const dimensions = (name: string, doc: string, path: string) => {
    const answer = input(name).engine?.documents?.[doc].responses?.find((r) => r.path === path);
    assert.ok(answer);
    return (
      answer.value as {
        layout: {
          pages: { ref: { objectNumber: number }; size: { width: number; height: number } }[];
        };
      }
    ).layout.pages.map((p) => [p.ref.objectNumber, p.size.width, p.size.height]);
  };
  assert.deepEqual(dimensions('edit-move-and-copy-assets', 'remove', 'pages.delete'), [
    [900, 300, 120],
    [901, 300, 120],
  ]);
  assert.deepEqual(dimensions('edit-move-and-copy-assets', 'dup', 'pages.insert'), [
    [3, 612, 792],
    [900, 300, 120],
    [901, 300, 120],
  ]);
  assert.deepEqual(dimensions('copies-carry-their-metadata', 'd2', 'pages.insert'), [
    [100, 200, 100],
    [101, 201, 101],
    [900, 300, 120],
    [901, 300, 120],
  ]);
  assert.deepEqual(dimensions('copies-carry-their-metadata', 'remove', 'pages.delete'), [
    [100, 200, 100],
    [900, 300, 120],
    [901, 300, 120],
  ]);
  assert.deepEqual(dimensions('removals-are-serialized-per-library', 'second', 'pages.delete'), [
    [102, 202, 102],
  ]);
  assert.deepEqual(
    dimensions('a-loose-append-finds-its-library-canonical', 'append', 'pages.insert'),
    [
      [101, 201, 101],
      [102, 100, 50],
    ],
  );
  assert.deepEqual(dimensions('a-queued-mutation-after-destroy', 'late', 'pages.insert'), [
    [101, 201, 101],
    [102, 202, 102],
    [900, 300, 120],
  ]);
});
