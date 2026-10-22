import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/stamp-library';
import { runScenario } from '../src/scenario';

test('canonical library import returns real current assets, source bytes and cached previews', async () => {
  const suite = suites[0];
  assert.equal(suite.kind, 'scenario');
  const input = suite.cases.find((c) => c.name === 'imports-every-page-as-a-vector-asset');
  assert.ok(input);
  const result = await runScenario(input);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((entry) =>
      entry.outcome && 'threw' in entry.outcome ? [entry.outcome] : [],
    ),
    [],
  );
  const state = result.trace.at(-1)?.state as { plugins: { stamp?: { libraryOrder?: string[] } } };
  assert.deepEqual(state.plugins.stamp?.libraryOrder, ['stamp-lib-0-1']);
});

import type { TraceEntry, ScenarioCase } from '../src/scenario';
import type { FunctionSuite } from '../src/vectors';
import { canonical } from '../src/canonical';
import { suites as pureSuites } from '../src/suites/stamp-functions';
import {
  STAMP,
  start,
  destroy,
  stamp,
  assetDoc,
  assetPage,
  opens,
  PDF,
  pdf,
  bytes,
  localAssetEngine,
  metadata,
  mutationMeta,
} from '../src/suites/stamp-current-fixtures';
import { createFakeEngine } from '../src/fake-engine';
import { createTestContext } from '@embedpdf/core/testing';
import {
  initialStampState,
  armAsset,
  disarmDocuments,
  type StampState,
} from '../../../packages/plugin/stamp/src/model';
import { createStampController } from '../../../packages/plugin/stamp/src/controller';
import { stampPlugin } from '../../../packages/plugin/stamp/src/stamp.plugin';
import {
  STAMP_DEFAULTS,
  type StampAsset,
  type StampLibrary,
} from '../../../packages/plugin/stamp/src/contract';
import {
  memoryStampStore,
  persistStampLibraries,
  restoreStampLibraries,
} from '../../../packages/plugin/stamp/src/persistence';
import { customStampName } from '../../../packages/plugin/stamp/src/convention';
const selectedSuite = suites[0];
assert.equal(selectedSuite.kind, 'scenario');
const scenarioSuite = selectedSuite;
const pure = pureSuites[0] as FunctionSuite;
const cache = new Map<string, ReturnType<typeof runScenario>>();
function input(name: string): ScenarioCase {
  assert.equal(scenarioSuite.kind, 'scenario');
  const found = scenarioSuite.cases.find((c) => c.name === name);
  assert.ok(found, name);
  return found;
}
function run(name: string) {
  let found = cache.get(name);
  if (!found) {
    found = runScenario(input(name));
    cache.set(name, found);
  }
  return found;
}
function value<T = unknown>(trace: TraceEntry[], step: number): T {
  const outcome = trace[step]?.outcome;
  assert.ok(outcome && 'returned' in outcome, `returned step ${step}`);
  return outcome.returned as T;
}
function resolved<T = unknown>(trace: TraceEntry[], step: number): T {
  const result = trace[step]?.settled.find((s) => 'resolved' in s);
  assert.ok(result, `resolved step ${step}`);
  return result.resolved as T;
}
function state(trace: TraceEntry[], step = trace.length - 1): StampState {
  const data = trace[step].state as { plugins?: { stamp?: StampState }; stamp?: StampState };
  const result = data.plugins?.stamp ?? data.stamp;
  assert.ok(result);
  return result;
}
const pluginError = (message: string, code = 'operation-failed', cause?: unknown) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'stamp',
    permission: null,
    ...(cause ? { cause } : {}),
  },
});
const engineError = (message: string, code = 'Unknown') => ({
  $error: { name: 'EngineError', message, code },
});
const plainError = (message: string, name = 'Error') => ({ $error: { name, message } });
const refused = (message: string, code = 'Unknown', pluginCode = 'operation-failed') =>
  pluginError(message, pluginCode, engineError(message, code));
const cloud = pluginError(
  "importing a library PDF needs a local engine, and this viewer's engine is a cloud engine. Pass stampPlugin({ assetEngine: () => import('@embedpdf/engine').then((module) => module.createLocalEngine()) }): it loads lazily, on first import.",
  'unsupported',
);
const badPdf = pluginError(
  'importLibrary needs PDF bytes (use createAsset for a raster image)',
  'invalid-input',
);
const rejections: Record<string, unknown[]> = {
  'imports-every-page-as-a-vector-asset': [pluginError("unknown library 'nope'", 'not-found')],
  'a-payload-source-and-its-errors': [
    badPdf,
    badPdf,
    badPdf,
    pluginError(
      "Cannot destructure property 'data' of 'source' as it is null.",
      'operation-failed',
      plainError("Cannot destructure property 'data' of 'source' as it is null.", 'TypeError'),
    ),
    pluginError(
      'data.arrayBuffer is not a function',
      'operation-failed',
      plainError('data.arrayBuffer is not a function', 'TypeError'),
    ),
  ],
  'a-cloud-kernel-engine-names-the-fix': [cloud, cloud, cloud],
  'a-configured-engine-keeps-its-invalid-arg': [
    refused('no bytes here', 'InvalidArg', 'invalid-input'),
  ],
  'missing-services-refuse-before-any-write': [
    refused('catalog piece info unsupported', 'Unsupported'),
    refused('page piece info unsupported', 'Unsupported'),
    pluginError('a canonical stamp library PDF must contain at least one page', 'invalid-input'),
    refused('no page 100', 'NotFound', 'not-found'),
  ],
  'an-engine-failure-mid-import-adds-nothing': [
    refused('catalog is read-only', 'Refused'),
    refused('page is read-only', 'Refused'),
    refused('extract failed'),
    refused('render failed'),
    pluginError('save failed', 'operation-failed', plainError('save failed')),
    refused('read failed'),
  ],
  'a-failed-close-fails-the-import': [refused('close failed')],
  'previews-must-be-inline-bytes': [
    pluginError(
      'the asset engine returned a URL-sourced render; asset engines must be local',
      'unsupported',
    ),
  ],
  'a-render-with-no-image-throws': ['source', 'kind'].map((key) =>
    pluginError(
      `Cannot read properties of null (reading '${key}')`,
      'operation-failed',
      plainError(`Cannot read properties of null (reading '${key}')`, 'TypeError'),
    ),
  ),
  'a-throwing-asset-engine-factory-is-retried': [0, 1].map(() =>
    pluginError('wasm failed to load', 'operation-failed', plainError('wasm failed to load')),
  ),
  'a-rejecting-asset-engine-factory-is-memoized': [0, 1].map(() =>
    pluginError('chunk load failed', 'operation-failed', plainError('chunk load failed')),
  ),
  'the-plugin-factory-defaults': [cloud],
};
const callbacks: Record<string, unknown[]> = {
  'import-options-override-the-pdf': [0, 1, 2].map((index) => ({ fn: 'assetName', args: [index] })),
  'an-asset-engine-factory-is-called-once': [{ fn: 'assetEngine', args: [] }],
  'an-async-asset-engine-factory': [{ fn: 'assetEngine', args: [] }],
  'a-throwing-asset-engine-factory-is-retried': [0, 1].map(() => ({ fn: 'assetEngine', args: [] })),
  'a-rejecting-asset-engine-factory-is-memoized': [{ fn: 'assetEngine', args: [] }],
  'named-registry-order-filters-and-preview-cache': [
    {
      fn: 'console.warn',
      args: [
        "[stamp] library 'PDF Title' carries 1 hidden-template or dangling registration(s); they are not stamps and were ignored",
      ],
    },
    {
      fn: 'console.warn',
      args: ['[stamp] ghost render failed, using the thumbnail:', engineError('ghost failed')],
    },
  ],
};
for (const c of scenarioSuite.cases)
  test(`${c.name}: exact channels, current calls and complete settlements`, async () => {
    const result = await run(c.name);
    assert.deepEqual(result.unmatched, []);
    assert.equal(result.trace.length, c.steps.length);
    assert.deepEqual(
      result.trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
      [],
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.settled.filter((s) => 'rejected' in s).map((s) => s.rejected)),
      rejections[c.name] ?? [],
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.reports),
      [],
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.callbacks),
      callbacks[c.name] ?? [],
    );
    const started = result.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const settled = result.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(
      [...settled].sort((a, b) => a - b),
      [...started].sort((a, b) => a - b),
    );
    const returned = result.trace.flatMap((t) =>
      t.outcome && 'returned' in t.outcome ? [t.outcome.returned] : [],
    );
    assert.ok(
      returned.every((v) => !v || typeof v !== 'object' || (!('error' in v) && !('errors' in v))),
    );
  });
const engineCalls = (trace: TraceEntry[], path?: string) =>
  trace.flatMap((t) => t.engine) as {
    doc: string | null;
    path: string;
    pon?: number;
    args: unknown[];
  }[];

test('canonical imports preserve full order, source bytes, preview bytes and missing-read channels', async () => {
  const { trace } = await run('imports-every-page-as-a-vector-asset');
  const id = 'stamp-lib-0-1';
  const library: StampLibrary = {
    id,
    name: 'Approvals',
    kind: 'stamps',
    assetIds: [id + ':Stamp1', id + ':Stamp2'],
  };
  const assets: StampAsset[] = [100, 101].map((n, index) => ({
    id: `${id}:Stamp${index + 1}`,
    libraryId: id,
    kind: 'stamp',
    name: `Stamp${index + 1}`,
    label: `Stamp${index + 1}`,
    size: { width: 200 + index, height: 100 + index },
    page: { kind: 'objectNumber', objectNumber: n },
  }));
  assert.deepEqual(resolved(trace, 1), { library });
  assert.deepEqual(value(trace, 2), [library]);
  assert.deepEqual(value(trace, 3), library);
  assert.equal(value(trace, 4), null);
  assert.deepEqual(value(trace, 5), assets);
  assert.deepEqual(value(trace, 6), assets);
  assert.deepEqual(value(trace, 7), []);
  assert.deepEqual(value(trace, 8), assets);
  assert.deepEqual(value(trace, 9), assets[0]);
  assert.equal(value(trace, 10), null);
  assert.deepEqual(value(trace, 11), canonical(pdf('page-101')));
  assert.equal(value(trace, 12), null);
  assert.deepEqual(value(trace, 13), { bytes: canonical(bytes('png-100')), mimeType: 'image/png' });
  assert.equal(value(trace, 14), null);
  assert.deepEqual(resolved(trace, 15), canonical(pdf('canonical')));
  const calls = engineCalls(trace);
  assert.deepEqual(calls[0], {
    doc: null,
    path: 'open',
    args: [{ kind: 'bytes', id: 'stamp-import-0-0', bytes: canonical(PDF) }, { scope: ['*'] }],
  });
  assert.deepEqual(
    calls.filter((c) => c.path === 'pages.extract').map((c) => c.args),
    [
      [[{ kind: 'objectNumber', objectNumber: 100 }]],
      [[{ kind: 'objectNumber', objectNumber: 101 }]],
    ],
  );
  assert.deepEqual(
    calls.filter((c) => c.path === 'page.render.image').map((c) => c.args),
    [100, 101].map(() => [
      {
        viewport: { kind: 'width', width: 256 },
        background: 'transparent',
        includeAnnotations: true,
        format: 'png',
      },
    ]),
  );
  assert.equal(calls.at(-1)?.path, 'close');
  assert.deepEqual(state(trace), {
    libraries: { [id]: library },
    libraryOrder: [id],
    assets: Object.fromEntries(assets.map((asset) => [asset.id, asset])),
    armed: {},
  });
});

test('PieceInfo, actual title precedence, typed callbacks, kinds and categories read back independently', async () => {
  const rows = value<StampAsset[]>(
    (await run('reads-ids-and-display-metadata-from-pieceinfo')).trace,
    3,
  );
  assert.deepEqual(
    rows.map((asset) => [asset.id, asset.kind, asset.label, asset.subject, asset.categories]),
    [
      ['review-library:Approved', 'signature', 'Approval signature', undefined, ['Review']],
      ['review-library:Stamp2', 'initials', 'Stamp2', undefined, undefined],
      ['review-library:Stamp3', 'stamp', 'Stamp3', undefined, undefined],
      ['review-library:Stamp4', 'stamp', 'Stamp4', undefined, undefined],
    ],
  );
  const options = (await run('import-options-override-the-pdf')).trace;
  assert.equal(value<StampLibrary[]>(options, 2)[0].name, 'Embedded');
  assert.deepEqual(value<StampLibrary[]>(options, 2)[0].categories, []);
  assert.deepEqual(
    value<StampAsset[]>(options, 3).map((a) => [a.name, a.label, a.kind]),
    [
      ['Stamp1', 'First', 'initials'],
      ['From PDF', 'From PDF', 'initials'],
      ['Stamp3', 'Stamp3', 'initials'],
    ],
  );
  const kinds = value<StampAsset[]>((await run('a-name-kind-and-stamp-kinds-read-back')).trace, 3);
  assert.deepEqual(
    kinds.map((a) => a.kind),
    ['stamp', 'stamp'],
  );
});

test('all import failures close opened documents, commit nothing and preserve exact malformed source/render refusals', async () => {
  for (const name of [
    'missing-services-refuse-before-any-write',
    'an-engine-failure-mid-import-adds-nothing',
    'a-failed-close-fails-the-import',
    'a-render-with-no-image-throws',
  ]) {
    const { trace } = await run(name);
    assert.deepEqual(state(trace), initialStampState(), name);
    const calls = engineCalls(trace);
    assert.equal(
      calls.filter((c) => c.path === 'open').length,
      calls.filter((c) => c.path === 'close').length,
      name,
    );
    assert.deepEqual(
      trace.flatMap((t) => t.stateChanges),
      [],
      name,
    );
  }
  const payload = (await run('a-payload-source-and-its-errors')).trace;
  assert.equal(engineCalls(payload).filter((c) => c.path === 'open').length, 1);
  assert.equal(state(payload).libraryOrder.length, 1);
  const preview = (await run('previews-must-be-inline-bytes')).trace;
  assert.deepEqual(value(preview, 3), { bytes: canonical(bytes('png-100')) });
  assert.deepEqual(
    engineCalls(preview)
      .filter((c) => c.path === 'page.render.image')
      .map((c) => c.args[0]),
    [0, 1].map(() => ({
      viewport: { kind: 'width', width: 96 },
      background: 'transparent',
      includeAnnotations: true,
      format: 'png',
    })),
  );
});

test('uids use a fresh module, clock and collision retry, and concurrent imports preserve current derived ownership', async () => {
  const ids = value<StampLibrary[]>(
    (await run('a-second-import-allocates-fresh-ids')).trace,
    5,
  ).map((l) => l.id);
  assert.deepEqual(ids, ['shared-library', 'stamp-lib-0-2', 'stamp-lib-0-5', 'stamp-lib-0-6']);
  const clock = (await run('ids-carry-the-clock')).trace;
  assert.deepEqual(state(clock).libraryOrder, ['stamp-lib-21i3v9-1']);
  const concurrent = (await run('concurrent-imports-read-the-store-before-either-lands')).trace;
  assert.deepEqual(
    value<StampAsset[]>(concurrent, 4).map((a) => a.id),
    ['lib-a:Stamp1'],
  );
  assert.deepEqual(value(concurrent, 6), []);
  assert.deepEqual(
    value<StampAsset[]>(concurrent, 7).map((a) => a.id),
    ['lib-b:Stamp1'],
  );
  assert.deepEqual(
    value<StampLibrary[]>(concurrent, 13).map((l) => l.id),
    ['lib-b', 'lib-c', 'lib-c'],
  );
  assert.deepEqual(state(concurrent).libraryOrder, ['lib-b']);
  assert.deepEqual(Object.keys(state(concurrent).assets), ['lib-b:Stamp1']);
});

test('asset engine factories are lazy, memoized, retried correctly and own only produced engines', async () => {
  for (const name of [
    'an-asset-engine-factory-is-called-once',
    'an-async-asset-engine-factory',
    'an-engine-instance-is-not-destroyed',
    'a-throwing-asset-engine-factory-is-retried',
    'a-rejecting-asset-engine-factory-is-memoized',
  ]) {
    const { trace } = await run(name);
    assert.deepEqual(trace[0].callbacks, []);
    const calls = engineCalls(trace);
    const owned =
      name === 'an-asset-engine-factory-is-called-once' || name === 'an-async-asset-engine-factory';
    assert.equal(calls.filter((c) => c.path === 'destroy').length, owned ? 1 : 0, name);
    if (owned)
      assert.ok(
        calls.map((c) => c.path).lastIndexOf('close') <
          calls.findIndex((c) => c.path === 'destroy'),
      );
    if (!name.includes('throwing') && !name.includes('rejecting'))
      assert.equal(
        Object.keys(state(trace, trace.length - 2).libraries).length,
        name === 'an-asset-engine-factory-is-called-once' ? 2 : 1,
      );
  }
  const cloudTrace = (await run('a-cloud-kernel-engine-names-the-fix')).trace;
  assert.equal(value(cloudTrace, 1), true);
  assert.equal(value(cloudTrace, 3), false);
  assert.deepEqual(engineCalls(cloudTrace), []);
  assert.deepEqual(value((await run('the-plugin-factory-defaults')).trace, 2), []);
});

test('named pages sort by page order, respect title/locale/kind and cache each preview width independently', async () => {
  const { trace } = await run('named-registry-order-filters-and-preview-cache');
  assert.deepEqual(state(trace).libraryOrder, ['registered']);
  const library = value<StampLibrary[]>(trace, 2)[0];
  assert.deepEqual(library, {
    id: 'registered',
    name: 'PDF Title',
    kind: 'signatures',
    locale: 'en-GB',
    assetIds: ['registered:First', 'registered:Later'],
  });
  assert.deepEqual(
    value<StampAsset[]>(trace, 3).map((a) => [a.id, a.label, a.subject, a.categories]),
    [['registered:First', 'Approved', 'Override', ['Legal']]],
  );
  assert.deepEqual(resolved(trace, 5), value(trace, 4));
  assert.deepEqual(resolved(trace, 6), value(trace, 4));
  assert.deepEqual(resolved(trace, 7), value(trace, 4));
  assert.deepEqual(value(trace, 8), value(trace, 4));
  assert.deepEqual(value(trace, 9), canonical(pdf('page-100')));
  assert.deepEqual(resolved(trace, 10), canonical(pdf('canonical')));
  const calls = engineCalls(trace);
  assert.equal(calls.filter((c) => c.path === 'open').length, 3);
  assert.deepEqual(
    calls.filter((c) => c.path === 'page.render.image').map((c) => c.args[0]),
    [256, 256, 512, 1024].map((width) => ({
      viewport: { kind: 'width', width },
      background: 'transparent',
      includeAnnotations: true,
      format: 'png',
    })),
  );
  assert.equal(calls.filter((c) => c.path === 'close').length, 3);
  assert.equal(calls.filter((c) => c.path === 'pages.setName').length, 0);
});

test('real persistence saves canonical bytes, restores file identity, deletes and stops cleanly', async () => {
  const { trace } = await run('persistence-restores-canonical-bytes-and-unsubscribes');
  assert.deepEqual(resolved(trace, 2), ['persisted']);
  assert.deepEqual(resolved(trace, 5), [
    { id: 'hint-only', bytes: canonical(PDF) },
    { id: 'persisted', bytes: canonical(pdf('canonical')) },
  ]);
  assert.deepEqual(resolved(trace, 7), [{ id: 'hint-only', bytes: canonical(PDF) }]);
  assert.deepEqual(resolved(trace, 10), resolved(trace, 7));
  assert.deepEqual(state(trace).libraryOrder, ['fresh']);
  const store = memoryStampStore();
  const original = PDF.slice();
  await store.put('copy', original);
  original.fill(0);
  assert.deepEqual((await store.list())[0].bytes, PDF);
});

test('all current pure inputs invoke named transitions with exact identity and descriptor effects', () => {
  const expected: Record<
    string,
    { order?: string[]; assetIds?: string[]; keys?: string[]; same?: boolean }
  > = {
    'library-added-to-empty': { order: ['L'] },
    'library-added-appends-order': { order: ['L', 'M'] },
    'library-added-with-a-taken-id': { order: ['L', 'M', 'L'] },
    'library-removed': { order: ['M'], keys: ['c'] },
    'library-removed-unknown': { same: true },
    'library-removed-with-missing-assets': { order: [], keys: ['stray'] },
    'library-removed-duplicate-order': { order: ['M'], keys: ['c'] },
    'asset-added': { keys: ['a', 'b', 'c', 'd'] },
    'asset-added-to-unknown-library': { same: true },
    'asset-added-twice': { keys: ['a', 'b'] },
    'asset-removed': { keys: ['b', 'c'] },
    'asset-removed-unknown': { same: true },
    'asset-removed-without-its-library': { keys: ['c'] },
    'library-updated': {},
    'library-updated-unknown': { same: true },
    'asset-updated': {},
    'asset-updated-unknown': { same: true },
    'already-armed-is-stable': { same: true },
    'disarm-documents': {},
    'initial-state': { order: [], keys: [] },
  };
  assert.equal(pure.cases.length, 20);
  for (const c of pure.cases) {
    const result = pure.functions[c.fn](...(c.args as never[])) as StampState;
    const expect = expected[c.name];
    assert.ok(expect, c.name);
    if (expect.same) assert.equal(result, c.args[0], c.name);
    if (expect.order) assert.deepEqual(result.libraryOrder, expect.order, c.name);
    if (expect.keys) assert.deepEqual(Object.keys(result.assets), expect.keys, c.name);
    assert.ok(result.armed);
  }
  const execute = (name: string) => {
    const c = pure.cases.find((c) => c.name === name)!;
    return pure.functions[c.fn](...(c.args as never[])) as StampState;
  };
  assert.deepEqual(execute('asset-added-twice').libraries.L.assetIds, ['a', 'b', 'a']);
  assert.equal(execute('asset-updated').assets.c.label, 'Relabelled');
  assert.equal(execute('library-updated').libraries.M.name, 'Renamed');
  assert.deepEqual(execute('disarm-documents').armed, { b: 'c' });
  const initial = initialStampState();
  const armed = armAsset(initial, 'a', 'x');
  assert.deepEqual(armed.armed, { a: 'x' });
  assert.equal(disarmDocuments(initial, ['missing']), initial);
  assert.deepEqual(disarmDocuments(armed, ['a']).armed, {});
});

function decodeBinary(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(decodeBinary);
  if (!value || typeof value !== 'object') return value;
  if ('$bytes' in value && typeof value.$bytes === 'string')
    return new Uint8Array(Buffer.from(value.$bytes, 'base64'));
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, decodeBinary(item)]));
}
test('the unconfigured local defaults succeed and binary cache clears on controller disposal', async () => {
  const fake = createFakeEngine(
    { documents: { lib: assetDoc([assetPage(0, 100)]) }, responses: opens('lib') },
    decodeBinary,
  );
  const local = localAssetEngine(fake.engine);
  const ctx = createTestContext({
    id: 'stamp',
    state: initialStampState(),
    settings: { defaults: STAMP_DEFAULTS, whole: ['assetEngine'] },
    engine: local,
    doc: null,
  });
  const api = ctx.connect(createStampController(ctx));
  assert.deepEqual(api.getSettings(), STAMP_DEFAULTS);
  assert.equal(api.canImport(), true);
  assert.equal(api.canPlace(), false);
  assert.equal(api.canCreateFromAnnotations(), true);
  // This direct controller owns the ordinary source instance's uid stream; supply an embedded id.
  const fakeWithId = createFakeEngine(
    {
      documents: {
        lib: assetDoc([assetPage(0, 100)], {
          catalog: { Id: { type: 'string', value: 'default-local' } },
          catalogWrites: [{ id: 'default-local' }],
        }),
      },
      responses: opens('lib'),
    },
    decodeBinary,
  );
  const localContext = createTestContext({
    id: 'stamp',
    state: initialStampState(),
    settings: { defaults: STAMP_DEFAULTS, whole: ['assetEngine'] },
    engine: localAssetEngine(fakeWithId.engine),
    doc: null,
  });
  const current = localContext.connect(createStampController(localContext));
  const events: unknown[] = [];
  const off = current.onLibraryChanged((event) => events.push(['changed', event]));
  current.onLibraryCreated((event) => events.push(['created', event]));
  current.onAssetCreated((event) => events.push(['asset', event]));
  const result = await current.importLibrary(PDF);
  assert.equal(result.library.id, 'default-local');
  assert.deepEqual(events, [
    ['changed', { libraryId: 'default-local', reason: 'imported' }],
    [
      'created',
      {
        libraryId: 'default-local',
        library: {
          id: 'default-local',
          name: 'Stamps',
          kind: 'stamps',
          categories: undefined,
          assetIds: [],
        },
      },
    ],
    [
      'asset',
      {
        assetId: 'default-local:Stamp1',
        libraryId: 'default-local',
        asset: {
          id: 'default-local:Stamp1',
          libraryId: 'default-local',
          kind: 'stamp',
          name: 'Stamp1',
          label: 'Stamp1',
          size: { width: 200, height: 100 },
          page: { kind: 'objectNumber', objectNumber: 100 },
        },
      },
    ],
  ]);
  off();
  assert.deepEqual(current.readAssetBytes('default-local:Stamp1'), pdf('page-100'));
  const copy = current.readAssetBytes('default-local:Stamp1')!;
  copy.fill(0);
  assert.deepEqual(current.readAssetBytes('default-local:Stamp1'), pdf('page-100'));
  const exported = await current.exportLibrary('default-local');
  exported.fill(0);
  assert.deepEqual(await current.exportLibrary('default-local'), pdf('canonical'));
  assert.deepEqual(
    fakeWithId.log
      .filter((call) => 'path' in call && call.path === 'page.render.image')
      .map((call) => ('args' in call ? call.args : [])),
    [
      [
        {
          viewport: { kind: 'width', width: 256 },
          background: 'transparent',
          includeAnnotations: true,
          format: 'png',
        },
      ],
    ],
  );
  await localContext.dispose();
  assert.equal(current.readAssetBytes('default-local:Stamp1'), null);
  assert.equal(current.getAssetPreview('default-local:Stamp1'), null);
  await assert.rejects(current.exportLibrary('default-local'), {
    code: 'not-found',
    capability: 'stamp',
  });
  await ctx.dispose();
});

import {
  doc as annotationDoc,
  hydrate,
  layout,
  operationAt,
} from '../src/suites/annotation-live-fixtures';
import { snapshot as formSnapshot } from '../src/suites/form-current-fixtures';
const hosts = () => ({
  random: Object.getOwnPropertyDescriptor(Math, 'random'),
  error: Object.getOwnPropertyDescriptor(console, 'error'),
  warn: Object.getOwnPropertyDescriptor(console, 'warn'),
  crypto: Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
});
const composed = (
  name: string,
  mode: 'success' | 'refusal' | 'construction' = 'success',
): ScenarioCase => ({
  name,
  covers:
    'The authored stamp outermost host composes with real current form and annotation diagnostics and scoped token reads.',
  plugins: STAMP({ assetEngine: 'instance', dynamic: false }, [
    { factory: 'form-live-diagnostics' },
    { factory: 'interaction' },
    { factory: 'annotation-live' },
    { factory: 'form-current', config: { validation: 'none' } },
    ...(mode === 'construction' ? [{ factory: 'interaction' }] : []),
  ]),
  engine: {
    documents: {
      lib: assetDoc([assetPage(0, 100)]),
      target: annotationDoc([hydrate(), { path: 'forms.list', value: formSnapshot([]) }], {
        pages: [layout(0, 1)],
        security: { allows: mode === 'refusal' ? [] : '*' },
      }),
    },
    responses: opens('lib', 'target'),
  },
  steps: [
    start,
    stamp('importLibrary', [PDF]),
    {
      op: 'call',
      target: 'documents',
      method: 'open',
      args: [{ kind: 'bytes', id: 'target', bytes: { $bytes: '' } }],
    },
    { op: 'call', target: { token: 'stamp', doc: 'target' }, method: 'canPlace' },
    {
      op: 'call',
      target: { token: 'stamp', doc: 'target' },
      method: 'armAsset',
      args: ['stamp-lib-0-1:Stamp1'],
    },
    { op: 'call', target: { token: 'stamp', doc: 'target' }, method: 'getArmedAsset' },
    { op: 'call', target: { token: 'stamp', doc: 'target' }, method: 'disarm' },
    { op: 'call', target: { token: 'stamp', doc: 'target' }, method: 'getArmedAsset' },
    destroy,
  ],
});
test('outermost stamp restores descriptors after three-owner success/refusal/construction and repeats deterministically', async () => {
  const prior = hosts();
  for (const mode of ['success', 'refusal', 'construction'] as const) {
    const c = composed('three-owner-' + mode, mode);
    const result = await runScenario(c);
    assert.deepEqual(hosts(), prior, mode);
    assert.deepEqual(result.unmatched, []);
    if (mode === 'construction') {
      assert.equal(result.trace.length, 1);
      assert.ok(result.trace[0].outcome && 'threw' in result.trace[0].outcome);
      continue;
    }
    assert.equal(value(result.trace, 3), mode === 'success');
    if (mode === 'success') {
      assert.equal(value<StampAsset>(result.trace, 5).id, 'stamp-lib-0-1:Stamp1');
      assert.deepEqual(state(result.trace, 5).armed, { target: 'stamp-lib-0-1:Stamp1' });
    } else
      assert.deepEqual(
        result.trace[4].settled.map((s) => s.rejected),
        [
          {
            $error: {
              name: 'PluginError',
              message: 'stamp.armAsset requires annotations:create',
              code: 'permission-denied',
              capability: 'stamp',
              permission: 'annotations:create',
            },
          },
        ],
      );
    assert.equal(value(result.trace, 7), null);
    assert.deepEqual(await runScenario(c), result);
    assert.deepEqual(hosts(), prior);
  }
});

test('getter and factory failures restore the exact outer host before another case', async () => {
  const prior = hosts();
  const badConfig = { assetEngine: 'instance' };
  Object.defineProperty(badConfig, 'previewWidth', {
    get() {
      throw new Error('settings getter failed');
    },
  });
  await assert.rejects(
    runScenario({
      name: 'getter-failure',
      covers: 'A throwing input getter fails after host installation.',
      plugins: [{ factory: 'stamp-current', config: badConfig }],
      steps: [],
    }),
    /settings getter failed/,
  );
  assert.deepEqual(hosts(), prior);
  await assert.rejects(
    runScenario({
      name: 'factory-failure',
      covers: 'A missing later factory fails after all diagnostics installed.',
      plugins: STAMP({ assetEngine: 'instance' }, [
        { factory: 'form-live-diagnostics' },
        { factory: 'annotation-live' },
        { factory: 'missing-stamp-test-factory' },
      ]),
      steps: [],
    }),
    /no plugin factory/,
  );
  assert.deepEqual(hosts(), prior);
  const descriptor = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues')!;
  try {
    Object.defineProperty(globalThis.crypto, 'getRandomValues', {
      configurable: true,
      get() {
        throw new Error('crypto getter failed');
      },
    });
    await assert.rejects(
      runScenario({
        name: 'crypto-getter-failure',
        covers: 'A crypto getter fails before globals install.',
        plugins: STAMP(),
        steps: [],
      }),
      /crypto getter failed/,
    );
  } finally {
    if (descriptor) Object.defineProperty(globalThis.crypto, 'getRandomValues', descriptor);
    else Reflect.deleteProperty(globalThis.crypto, 'getRandomValues');
  }
  assert.deepEqual(hosts(), prior);
  const result = await runScenario(input('imports-every-page-as-a-vector-asset'));
  assert.deepEqual(state(result.trace).libraryOrder, ['stamp-lib-0-1']);
  assert.deepEqual(hosts(), prior);
});

test('the local host rejects a remote handle and unused ports loudly without changing arguments', async () => {
  const fake = createFakeEngine(
    { documents: { remote: { pages: [] } }, responses: opens('remote') },
    decodeBinary,
  );
  const local = localAssetEngine(fake.engine);
  await assert.rejects(
    local.open({ kind: 'bytes', id: 'test', bytes: PDF }, { scope: ['*'] }),
    /asset document must be local/,
  );
  assert.deepEqual(fake.log, [
    {
      doc: null,
      path: 'open',
      args: [{ kind: 'bytes', id: 'test', bytes: canonical(PDF) }, { scope: ['*'] }],
    },
  ]);
  assert.throws(() => local.warmup(), /unused local engine port/);
  assert.throws(() => local.fonts.list(), /unused local engine port/);
  const waiting = createFakeEngine({ responses: [{ path: 'open', defer: 'open' }] }, decodeBinary);
  const promise = localAssetEngine(waiting.engine).open({ kind: 'bytes', id: 'abort', bytes: PDF });
  promise.abort('stop');
  await assert.rejects(promise, { name: 'AbortError' });
  assert.ok(waiting.log.some((entry) => 'abort' in entry));
});

import { withStampModuleIsolation } from '../src/stamp-module-host';
import { StampToken } from '../../../packages/plugin/stamp/src/contract';
test('the module hook isolates its namespace and deregisters on successful and failed preload', async () => {
  const contract = new URL('../../../packages/plugin/stamp/src/contract.ts', import.meta.url).href;
  const isolated = await withStampModuleIsolation(
    () => import(contract + '?stamp-instance=current-proof'),
  );
  assert.notEqual(isolated.StampToken, StampToken);
  assert.equal(
    (await import(contract + '?stamp-instance=current-after-success')).StampToken,
    StampToken,
  );
  await assert.rejects(
    withStampModuleIsolation(
      () =>
        import(
          new URL(
            '../../../packages/plugin/stamp/src/missing-stamp-fixture.ts?stamp-instance=current-failure',
            import.meta.url,
          ).href
        ),
    ),
    /Cannot find module/,
  );
  assert.equal(
    (await import(contract + '?stamp-instance=current-after-failure')).StampToken,
    StampToken,
  );
});

import { definePlugin } from '@embedpdf/core';
import { registerPlugin } from '../src/scenario';
registerPlugin('stamp-host-probe', {
  factory: (config, env) => {
    assert.equal(typeof config, 'string');
    const id = config as string;
    const sample = new Uint8Array(16);
    crypto.getRandomValues(sample);
    env.log('probe.uuid', [id, sample]);
    return definePlugin({
      id,
      scope: 'workspace',
      token: env.token(id),
      create: () => ({ api: { name: () => customStampName() } }),
    });
  },
});
test('one shared crypto sequence spans all factories while scoped stamp randomness restores exact descriptors', async () => {
  const prior = hosts();
  const c: ScenarioCase = {
    name: 'shared-stamp-entropy',
    covers:
      'All factories share one UUID sequence; stamp names consume their own scoped Math.random port.',
    plugins: STAMP({ assetEngine: 'instance' }, [
      { factory: 'stamp-host-probe', config: 'before' },
      { factory: 'form-live-diagnostics' },
      { factory: 'interaction' },
      { factory: 'annotation-live' },
      { factory: 'form-current' },
      { factory: 'stamp-host-probe', config: 'after' },
    ]),
    steps: [
      start,
      { op: 'call', target: { token: 'before' }, method: 'name' },
      { op: 'call', target: { token: 'after' }, method: 'name' },
    ],
  };
  const result = await runScenario(c);
  assert.deepEqual(hosts(), prior);
  assert.deepEqual(
    result.trace[0].callbacks,
    [1, 2].map((n, index) => ({
      fn: 'probe.uuid',
      args: [index ? 'after' : 'before', canonical(new Uint8Array([...Array(15).fill(0), n]))],
    })),
  );
  assert.deepEqual(
    result.trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
    [],
  );
  assert.equal(value(result.trace, 1), '#ABCDEFGHIJKLMNOPQRSTUV');
  assert.equal(value(result.trace, 2), '#WXYZabcdefghijklmnopqr');
  assert.deepEqual(await runScenario(c), result);
  assert.deepEqual(hosts(), prior);
});

test('a rejected preload keeps isolation alive until every delayed source import settles', async () => {
  const contract = new URL('../../../packages/plugin/stamp/src/contract.ts', import.meta.url).href;
  let finish!: () => void;
  const gate = new Promise<void>((resolve) => {
    finish = resolve;
  });
  let delayedToken: unknown;
  const pending = withStampModuleIsolation(async () => {
    const settled = await Promise.allSettled([
      import(
        new URL(
          '../../../packages/plugin/stamp/src/missing-delayed.ts?stamp-instance=current-missing-delayed',
          import.meta.url,
        ).href
      ),
      gate.then(async () => {
        delayedToken = (await import(contract + '?stamp-instance=current-delayed')).StampToken;
      }),
    ]);
    const failure = settled.find((result) => result.status === 'rejected');
    if (failure && failure.status === 'rejected') throw failure.reason;
  });
  // Suppress unhandled reporting while the deliberately rejected peer waits for the gate.
  const observed = assert.rejects(pending, /Cannot find module/);
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.notEqual(
    (await import(contract + '?stamp-instance=current-during-delayed')).StampToken,
    StampToken,
  );
  finish();
  await observed;
  assert.notEqual(delayedToken, StampToken);
  assert.equal(
    (await import(contract + '?stamp-instance=current-after-delayed')).StampToken,
    StampToken,
  );
});

test('a later legacy import and full sorted discovery retain host identities around current cases', async () => {
  await import(new URL('../src/suites/stamp-fixtures.ts', import.meta.url).href);
  const prior = hosts();
  const first = await runScenario(composed('after-legacy'));
  assert.deepEqual(hosts(), prior);
  assert.deepEqual(first.unmatched, []);
  const { readdir } = await import('node:fs/promises');
  for (const file of (await readdir(new URL('../src/suites/', import.meta.url)))
    .filter((file) => file.endsWith('.ts'))
    .sort())
    await import(new URL('../src/suites/' + file, import.meta.url).href);
  const discovered = hosts();
  const second = await runScenario(composed('after-legacy'));
  assert.deepEqual(second, first);
  assert.deepEqual(hosts(), discovered);
});

test('required PieceInfo refusals recover through a complete current local import', async () => {
  const original = input('missing-services-refuse-before-any-write');
  const recovered: ScenarioCase = {
    ...original,
    name: 'required-service-refusal-recovery',
    engine: {
      ...original.engine,
      documents: {
        ...original.engine?.documents,
        good: assetDoc([assetPage(0, 100)], { catalogWrites: [{ id: 'stamp-lib-0-7' }] }),
      },
      responses: [...(original.engine?.responses ?? []), ...opens('good')],
    },
    steps: [
      ...original.steps,
      stamp('importLibrary', [PDF]),
      stamp('readAssetBytes', ['stamp-lib-0-7:Stamp1']),
    ],
  };
  const result = await runScenario(recovered);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(state(result.trace, 5), initialStampState());
  assert.deepEqual(state(result.trace).libraryOrder, ['stamp-lib-0-7']);
  assert.deepEqual(value(result.trace, 7), canonical(pdf('page-100')));
});

test('settings replace the engine port, reset canImport and retain ownership only for produced engines', async () => {
  const make = (id: string) =>
    createFakeEngine(
      {
        documents: {
          lib: assetDoc([assetPage(0, 100)], {
            catalog: { Id: { type: 'string', value: id } },
            catalogWrites: [{ id }],
          }),
        },
        responses: opens('lib'),
      },
      decodeBinary,
    );
  const owned = make('owned'),
    borrowed = make('borrowed'),
    remote = createFakeEngine({}, decodeBinary);
  let calls = 0;
  const localOwned = localAssetEngine(owned.engine),
    localBorrowed = localAssetEngine(borrowed.engine);
  const ctx = createTestContext({
    id: 'stamp',
    state: initialStampState(),
    settings: {
      defaults: STAMP_DEFAULTS,
      registered: {
        assetEngine: () => {
          calls++;
          return localOwned;
        },
      },
      whole: ['assetEngine'],
    },
    engine: remote.engine,
    doc: null,
  });
  const api = ctx.connect(createStampController(ctx));
  assert.equal(calls, 0);
  await api.importLibrary(PDF);
  assert.equal(calls, 1);
  api.updateSettings({ assetEngine: null });
  assert.equal(api.canImport(), true);
  await assert.rejects(api.importLibrary(PDF), { code: 'unsupported' });
  assert.equal(api.canImport(), false);
  api.updateSettings({ assetEngine: localBorrowed });
  assert.equal(api.canImport(), true);
  await api.importLibrary(PDF);
  assert.deepEqual(
    api.listLibraries().map((l) => l.id),
    ['owned', 'borrowed'],
  );
  await ctx.dispose();
  await new Promise<void>((resolve) => setImmediate(resolve));
  assert.equal(owned.log.filter((entry) => 'path' in entry && entry.path === 'destroy').length, 1);
  assert.equal(
    borrowed.log.filter((entry) => 'path' in entry && entry.path === 'destroy').length,
    0,
  );
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
test('all original40 mappings and current43 complete inputs have exact digests and source evidence', async () => {
  const history = new URL('../historical/task-12b2b3-iii/', import.meta.url);
  const original = JSON.parse(
    await readFile(new URL('../task-12b2b3/original-inputs.json', history), 'utf8'),
  ) as { source: string; file: string; name: string; inputSha256: string }[];
  const owned = original.filter(
    (row) => row.source === 'stamp-functions' || row.source === 'stamp-library',
  );
  assert.equal(owned.length, 40);
  const accounting = JSON.parse(await readFile(new URL('accounting.json', history), 'utf8')) as {
    original: number;
    current: number;
    mapped: number;
    wholeCaseRetirements: number;
    added: number;
    changedMappedInputs: number;
    mappings: {
      before: { file: string; name: string; inputSha256: string };
      after?: { file: string; name: string; inputSha256: string };
      disposition: string;
      source: string[];
      removalCommit?: string;
    }[];
    additions: { file: string; name: string; inputSha256: string }[];
    sourceEvidence: { path: string; sha256: string; commit: string }[];
  };
  assert.deepEqual(
    [
      accounting.original,
      accounting.current,
      accounting.mapped,
      accounting.wholeCaseRetirements,
      accounting.added,
      accounting.changedMappedInputs,
    ],
    [40, 43, 39, 1, 4, 38],
  );
  assert.deepEqual(
    accounting.mappings.map((row) => row.before),
    owned.map(({ file, name, inputSha256 }) => ({ file, name, inputSha256 })),
  );
  const current = JSON.parse(await readFile(new URL('current-inputs.json', history), 'utf8')) as {
    file: string;
    name: string;
    input: unknown;
    inputSha256: string;
  }[];
  const digest = (value: unknown) =>
    createHash('sha256').update(JSON.stringify(value)).digest('hex');
  const actual = [
    ...scenarioSuite.cases.map((c) => ({
      file: scenarioSuite.file,
      name: c.name,
      input: canonical(c),
    })),
    ...pure.cases.map((c) => ({ file: pure.file, name: c.name, input: canonical(c) })),
  ];
  assert.equal(actual.length, 43);
  for (const row of actual) {
    const archived = current.find(
      (candidate) => candidate.file === row.file && candidate.name === row.name,
    );
    assert.ok(archived, row.name);
    assert.deepEqual(row.input, archived.input);
    assert.equal(digest(row.input), archived.inputSha256);
    const mapped =
      accounting.mappings.find(
        (candidate) => candidate.after?.file === row.file && candidate.after.name === row.name,
      )?.after ??
      accounting.additions.find(
        (candidate) => candidate.file === row.file && candidate.name === row.name,
      );
    assert.ok(mapped);
    assert.equal(mapped.inputSha256, archived.inputSha256);
  }
  assert.deepEqual(
    accounting.mappings
      .filter((row) => row.disposition === 'retired')
      .map((row) => row.before.name),
    ['unknown-action'],
  );
  assert.deepEqual(
    accounting.additions.map((row) => row.name),
    [
      'named-registry-order-filters-and-preview-cache',
      'persistence-restores-canonical-bytes-and-unsubscribes',
      'already-armed-is-stable',
      'disarm-documents',
    ],
  );
  for (const evidence of accounting.sourceEvidence) {
    const bytes = await readFile(new URL('../../../' + evidence.path, import.meta.url));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), evidence.sha256, evidence.path);
    assert.match(evidence.commit, /^[a-f0-9]{40}$/);
    assert.deepEqual(execFileSync('git', ['show', evidence.commit + ':' + evidence.path]), bytes);
  }
  const drift = JSON.parse(
    await readFile(new URL('../task-12b2b3/task11-stamp-drift.json', history), 'utf8'),
  ) as { frozen: { args: unknown[] }; base: { input: { args: unknown[] } } };
  const frozen = drift.frozen.args[0] as object;
  assert.deepEqual(drift.base.input.args[0], { ...frozen, armed: {} });
  assert.deepEqual(drift.base.input.args[1], drift.frozen.args[1]);
  const added = pure.cases.find((c) => c.name === 'library-added-to-empty')!;
  assert.equal(added.fn, 'addLibrary');
  assert.deepEqual(added.args, [
    initialStampState(),
    { id: 'L', name: 'Library L', kind: 'stamps', assetIds: [] },
  ]);
});

test('named-page write answers describe the exact postwrite registry requested by the real import', async () => {
  const c = input('imports-every-page-as-a-vector-asset');
  const responses = c.engine?.documents?.lib.responses ?? [];
  const answers = responses.filter((response) => response.path === 'pages.setName');
  assert.equal(answers.length, 2);
  const expected = [
    {
      name: 'Stamp1=Stamp1',
      target: { kind: 'page', page: { kind: 'objectNumber', objectNumber: 100 } },
    },
    {
      name: 'Stamp2=Stamp2',
      target: { kind: 'page', page: { kind: 'objectNumber', objectNumber: 101 } },
    },
  ];
  assert.deepEqual((answers[0].value as { layout: { namedPages: unknown[] } }).layout.namedPages, [
    expected[0],
  ]);
  assert.deepEqual(
    (answers[1].value as { layout: { namedPages: unknown[] } }).layout.namedPages,
    expected,
  );
  const calls = engineCalls((await run(c.name)).trace).filter(
    (call) => call.path === 'pages.setName',
  );
  assert.deepEqual(
    calls.map((call) => call.args),
    [
      [{ name: 'Stamp1=Stamp1', page: { kind: 'objectNumber', objectNumber: 100 } }],
      [{ name: 'Stamp2=Stamp2', page: { kind: 'objectNumber', objectNumber: 101 } }],
    ],
  );
  assert.deepEqual(
    calls.map((call) => call.args),
    answers.map((answer) => answer.args),
  );
});
