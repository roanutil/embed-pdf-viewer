import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/stamp-placement';
import { runScenario, type TraceEntry, type ScenarioCase } from '../src/scenario';
import { canonical } from '../src/canonical';
import { pr, objRef, operationAt, LOCAL } from '../src/suites/annotation-live-fixtures';
import { bytes, pdf, png } from '../src/suites/stamp-current-fixtures';
const suite = suites[0];
assert.equal(suite.kind, 'scenario');
const cases = suite.cases;
const input = (name: string): ScenarioCase => {
  const c = cases.find((c) => c.name === name);
  assert.ok(c, name);
  return c;
};
const cache = new Map<string, ReturnType<typeof runScenario>>();
function run(name: string) {
  let r = cache.get(name);
  if (!r) {
    r = runScenario(input(name));
    cache.set(name, r);
  }
  return r;
}
function value<T = unknown>(trace: TraceEntry[], n: number): T {
  const o = trace[n].outcome;
  assert.ok(o && 'returned' in o);
  return o.returned as T;
}
function resolved<T = unknown>(trace: TraceEntry[], n: number): T {
  const s = trace[n].settled.find((s) => 'resolved' in s);
  assert.ok(s);
  return s.resolved as T;
}
type Call = { doc: string | null; path: string; pon?: number; args: unknown[] };
const calls = (trace: TraceEntry[]) => trace.flatMap((t) => t.engine) as Call[];
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
const engine = (message: string, code: string) =>
  error(message, 'operation-failed', { $error: { name: 'EngineError', message, code } });
const denied = (method: string) =>
  error(
    `stamp.${method} requires annotations:create`,
    'permission-denied',
    undefined,
    'annotations:create',
  );
const neg = (method: string, error: unknown) => ({ method, error });
const missingBytes = "Cannot destructure property 'data' of 'source' as it is undefined.";
const negatives: Record<string, unknown[]> = {
  'a-raster-asset-arms-with-its-own-preview': [
    neg('importLibrary', engine('no thumbnail', 'Io')),
    neg('armAsset', error("unknown asset 'pdf:Stamp1'", 'not-found')),
  ],
  'arming-what-is-not-there': [
    neg('armAsset', error("unknown asset 'nope'", 'not-found')),
    neg('armAsset', denied('armAsset')),
    neg('armAsset', denied('armAsset')),
  ],
  'dynamic-stamp-refusals': [
    neg('armAsset', denied('armAsset')),
    neg('armAsset', error('canonical page 100 no longer exists', 'invalid-input')),
    neg('armAsset', engine('flatten unavailable', 'Unsupported')),
    neg('armAsset', engine('extract unavailable', 'Io')),
  ],
  'a-failed-recalculation-fails-the-arm': [
    neg(
      'armAsset',
      error('dynamic stamp scripting failed: Form script transaction exceeded 1 effects'),
    ),
  ],
  'an-error-observer-that-throws': [
    neg(
      'armAsset',
      error('dynamic stamp scripting failed: Form script transaction exceeded 0 effects'),
    ),
  ],
  'observers-see-a-successful-run': [
    neg('armAsset', error('observer broke', 'operation-failed', plain('observer broke'))),
  ],
  'a-flatten-that-fails-or-skips': [neg('armAsset', engine('flatten failed', 'Refused'))],
  'a-failed-copy-close-fails-the-arm': [
    neg('armAsset', engine('close failed', 'Io')),
    neg(
      'armAsset',
      error(
        'the asset engine returned a URL-sourced render; asset engines must be local',
        'unsupported',
      ),
    ),
  ],
  'a-library-without-canonical-bytes': [
    neg('armAsset', error(missingBytes, 'operation-failed', plain(missingBytes, 'TypeError'))),
  ],
  'dynamic-placement-writes-materialized-bytes-and-batch-facts': [
    neg('placeAssetOnPages', error("no page 999 in document 'a'", 'not-found')),
  ],
  'permission-refusal-recovers-on-an-authorized-document': [
    neg('armAsset', denied('armAsset')),
    neg('placeAsset', denied('placeAsset')),
  ],
};
const sb = ['sandbox.factory', 'sandbox.boot', 'sandbox.dispose'];
const callbackNames: Record<string, string[]> = {
  'arm-delegates-to-the-annotation-plugin': ['armChanged', 'armChanged', 'armChanged'],
  'materializes-a-dynamic-stamp': [
    'sandbox.factory',
    'now',
    'offset',
    'seed',
    'sandbox.boot',
    'sandbox.dispose',
  ],
  'a-loose-dynamic-stamp': sb,
  'dynamic-stamp-refusals': [...sb, ...sb],
  'a-failed-boot-script-is-a-diagnostic': [
    'sandbox.factory',
    'sandbox.boot',
    'onUiEffect',
    'onDiagnostic',
    'onDiagnostic',
    'sandbox.dispose',
  ],
  'a-failed-recalculation-fails-the-arm': [
    'sandbox.factory',
    'sandbox.boot',
    'onUiEffect',
    'onDiagnostic',
    'onError',
    'sandbox.dispose',
  ],
  'an-error-observer-that-throws': [
    'sandbox.factory',
    'sandbox.boot',
    'onError',
    'sandbox.dispose',
  ],
  'observers-see-a-successful-run': [
    'sandbox.factory',
    'sandbox.boot',
    'onUiEffect',
    'onUiEffect',
    'sandbox.dispose',
  ],
  'a-flatten-that-fails-or-skips': [...sb, ...sb],
  'a-failed-copy-close-fails-the-arm': [...sb, ...sb],
  'a-library-without-canonical-bytes': sb,
  'teardown-destroys-a-factory-engine-once': ['assetEngine', ...sb],
  'dynamic-placement-writes-materialized-bytes-and-batch-facts': [
    ...sb,
    'annotationCreated',
    ...sb,
    'annotationWriteFailed',
    ...sb,
    'annotationCreated',
  ],
  'file-picker-places-cancels-and-expires-the-click': ['filePicker', 'filePicker', 'filePicker'],
  'permission-refusal-recovers-on-an-authorized-document': sb,
};
for (const c of cases)
  test(`${c.name}: exact named diagnostic channels`, async () => {
    const result = await run(c.name);
    assert.deepEqual(result.unmatched, []);
    const methods = new Map<number, string>();
    const actual: unknown[] = [];
    result.trace.forEach((t, i) => {
      assert.ok(!t.outcome || !('threw' in t.outcome), `${c.name} step ${i}`);
      if (t.outcome && 'promise' in t.outcome) {
        const step = c.steps[i];
        assert.equal(step.op, 'call');
        methods.set(t.outcome.promise, step.method!);
      }
      for (const s of t.settled)
        if ('rejected' in s) actual.push(neg(methods.get(s.promise)!, s.rejected));
    });
    assert.deepEqual(actual, negatives[c.name] ?? []);
    assert.deepEqual(
      result.trace.flatMap((t) => t.callbacks).map((c) => c.fn),
      callbackNames[c.name] ?? [],
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.reports),
      c.name === 'an-error-observer-that-throws'
        ? [plain('observer is not ready', 'TypeError')]
        : [],
    );
    const promises = result.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    assert.deepEqual(
      result.trace
        .flatMap((t) => t.settled)
        .map((s) => s.promise)
        .sort((a, b) => a - b),
      promises.sort((a, b) => a - b),
    );
  });
const asset = {
  id: 'stamp-lib-0-1:Stamp1',
  libraryId: 'stamp-lib-0-1',
  kind: 'stamp',
  name: 'Stamp1',
  label: 'Stamp1',
  size: { width: 200, height: 100 },
  page: pr(100),
};
test('placement arms the actual current annotation stamp tool', async () => {
  const { trace } = await run('arm-delegates-to-the-annotation-plugin');
  assert.deepEqual(value(trace, 5), { width: 120, height: 60, name: 'Stamp1', subject: 'Stamp1' });
  assert.deepEqual(value(trace, 6), asset);
  assert.equal(value(trace, 8), null);
  assert.equal(value(trace, 9), null);
  assert.deepEqual(value(trace, 11), {
    width: 200,
    height: 100,
    name: 'Stamp1',
    subject: 'Stamp1',
  });
  assert.deepEqual(value(trace, 12), asset);
  assert.deepEqual(
    trace.flatMap((t) => t.callbacks),
    [asset, null, asset].map((asset) => ({ fn: 'armChanged', args: [{ documentId: 'a', asset }] })),
  );
});
test('ghost geometry and width bucket cache read through current host APIs', async () => {
  const { trace } = await run('the-target-width-sizes-the-ghost-and-a-template-ghost-is-cached');
  assert.deepEqual(value(trace, 5), {
    page: pr(1),
    box: { x: 240, y: 370, width: 120, height: 60 },
    rot: 0,
    opacity: 0.5,
  });
  assert.deepEqual(resolved(trace, 6), {
    bytes: canonical(bytes('png-100')),
    mimeType: 'image/png',
  });
  assert.deepEqual(resolved(trace, 9), resolved(trace, 6));
  const all = calls(trace);
  assert.equal(all.filter((c) => c.doc === 'ghost' && c.path === 'page.render.image').length, 1);
  assert.deepEqual(
    all.filter((c) => c.doc === 'ghost' && c.path === 'page.render.image').map((c) => c.args),
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
  assert.equal(all.filter((c) => c.doc === 'ghost' && c.path === 'close').length, 1);
});
test('raster authoring retains exact size, thumbnail and arm after a failed PDF import', async () => {
  const { trace } = await run('a-raster-asset-arms-with-its-own-preview');
  assert.deepEqual(resolved(trace, 2), {
    asset: {
      id: 'stamp-lib-0-0:Logo',
      libraryId: 'stamp-lib-0-0',
      kind: 'stamp',
      name: 'Logo',
      label: 'Logo',
      size: { width: 100, height: 50 },
      page: pr(900),
    },
  });
  assert.deepEqual(value(trace, 4), { width: 50, height: 25, name: 'Logo', subject: 'Logo' });
  assert.deepEqual(value(trace, 9), value(trace, 4));
  assert.deepEqual(value(trace, 6), { bytes: canonical(bytes('png-900')), mimeType: 'image/png' });
  assert.equal(value(trace, 10), null);
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'page.annotations.create')
      .map((c) => c.args),
    [
      [
        { subtype: 'stamp', box: { x: 0, y: 0, width: 100, height: 50 }, fit: 'fill' },
        { resources: { appearance: canonical(png(100, 50)) } },
      ],
    ],
  );
});
test('dynamic setting, absent actions and unselected fields skip sandbox construction', async () => {
  for (const name of [
    'scripting-off-arms-the-template',
    'no-sandbox-host',
    'a-raster-asset-is-never-evaluated',
    'a-stamp-without-fields-on-its-page-arms-the-template',
  ]) {
    const { trace } = await run(name);
    assert.deepEqual(
      trace.flatMap((t) => t.callbacks),
      [],
    );
    assert.equal(
      calls(trace).filter((c) => c.path === 'open').length,
      name.startsWith('a-raster') ? 4 : name.startsWith('a-stamp') ? 3 : 2,
    );
    assert.equal(
      calls(trace).filter((c) => c.doc === 'copy' && c.path === 'close').length,
      name.startsWith('a-') ? 1 : 0,
    );
  }
});
test('detached materialization uses target identity, environment and native form write before flatten', async () => {
  const { trace } = await run('materializes-a-dynamic-stamp');
  const all = calls(trace);
  const step = trace[6];
  const boot = step.callbacks.find((c) => c.fn === 'sandbox.boot');
  assert.ok(boot);
  assert.deepEqual(boot.args, [
    0,
    ['this.getField("date").value = util.printd(...);'],
    {
      document: { id: 'copy', fileName: 'document.pdf', pageCount: 1, pageNumber: 0 },
      identity: {
        name: 'Alex Morgan',
        loginName: 'alex',
        corporation: 'EmbedPDF',
        email: 'alex@example.test',
      },
      environment: { nowMs: 1784107800000, utcOffsetMinutes: 180, randomSeed: 7 },
      fields: [
        {
          ref: { kind: 'objectNumber', objectNumber: 1 },
          name: 'date',
          family: 'text',
          value: null,
          defaultValue: null,
          display: 'visible',
          readOnly: false,
          required: false,
        },
      ],
      event: { kind: 'name-tree-boot' },
    },
    {
      maxExecutionMs: 50,
      maxMemoryBytes: 16777216,
      maxStackBytes: 524288,
      maxEffects: 256,
      maxOutputBytes: 1048576,
    },
  ]);
  assert.deepEqual(
    all.filter((c) => c.doc === 'copy').map((c) => c.path),
    [
      'pages.list',
      'forms.list',
      'actions.get',
      'apply',
      'pages.flatten',
      'pages.extract',
      'page.render.image',
      'close',
    ],
  );
  assert.deepEqual(all.find((c) => c.doc === 'copy' && c.path === 'apply')?.args, [
    {
      ops: [
        {
          type: 'forms.setValue',
          field: { kind: 'objectNumber', objectNumber: 1 },
          value: { value: '2026-07-15' },
        },
      ],
    },
  ]);
  assert.deepEqual(all.find((c) => c.doc === 'copy' && c.path === 'pages.flatten')?.args, [
    [pr(100)],
    { usage: 'display' },
  ]);
  assert.deepEqual(all.find((c) => c.doc === 'copy' && c.path === 'page.render.image')?.args, [
    {
      viewport: { kind: 'width', width: 256 },
      background: 'transparent',
      includeAnnotations: false,
      format: 'png',
    },
  ]);
  assert.deepEqual(value(trace, 7), { width: 80, height: 40, name: 'Stamp1', subject: 'Stamp1' });
  assert.deepEqual(resolved(trace, 8), {
    bytes: canonical(bytes('png-100')),
    mimeType: 'image/png',
  });
  assert.deepEqual(resolved(trace, 9), canonical(pdf('canonical')));
  assert.deepEqual(value(trace, 10), canonical(pdf('page-100')));
  assert.deepEqual(
    all
      .filter((c) => c.path === 'open')
      .slice(-2)
      .map((c) => c.args),
    [
      [
        { kind: 'bytes', id: 'stamp-import-0-2', bytes: canonical(pdf('canonical')) },
        { scope: ['*'] },
      ],
      [
        { kind: 'bytes', id: 'stamp-import-0-3', bytes: canonical(pdf('flat-100')) },
        { scope: ['*'] },
      ],
    ],
  );
});
test('single-asset canonical dynamic library selects its authored page and identityless target', async () => {
  const { trace } = await run('a-loose-dynamic-stamp');
  assert.deepEqual(value(trace, 4), {
    width: 300,
    height: 120,
    name: 'Dynamic',
    subject: 'Dynamic',
  });
  assert.deepEqual(value(trace, 5), canonical(pdf('page-900')));
  assert.deepEqual(calls(trace).find((c) => c.doc === 'copy' && c.path === 'pages.flatten')?.args, [
    [pr(900)],
    { usage: 'display' },
  ]);
  const boot = trace.flatMap((t) => t.callbacks).find((c) => c.fn === 'sandbox.boot');
  assert.ok(boot);
  assert.deepEqual((boot.args[2] as { identity: unknown }).identity, {
    name: '',
    loginName: '',
    corporation: '',
    email: '',
  });
});
test('failed script channels and throwing observers follow the actual actions surface', async () => {
  const boot = (await run('a-failed-boot-script-is-a-diagnostic')).trace;
  assert.deepEqual(
    boot.flatMap((t) => t.callbacks).filter((c) => c.fn.startsWith('on')),
    [
      { fn: 'onUiEffect', args: ['Stamp expired', { origin: 'user', phase: 'boot', icon: 0 }] },
      { fn: 'onDiagnostic', args: [{ code: 'unsupported-api', message: 'app.viewerVersion' }] },
      {
        fn: 'onDiagnostic',
        args: [
          {
            code: 'script-error',
            message: 'Document boot script failed (continuing without it): boom',
          },
        ],
      },
    ],
  );
  assert.deepEqual(value(boot, 7), { width: 200, height: 100, name: 'Stamp1', subject: 'Stamp1' });
  for (const [name, n] of [
    ['a-failed-recalculation-fails-the-arm', 1],
    ['an-error-observer-that-throws', 0],
  ] as const) {
    const { trace } = await run(name);
    assert.deepEqual(trace.flatMap((t) => t.callbacks).find((c) => c.fn === 'onError')?.args, [
      {
        error: { kind: 'budget', message: `Form script transaction exceeded ${n} effects` },
        source: null,
      },
    ]);
    assert.equal(value(trace, 7), null);
    assert.equal(
      calls(trace).filter((c) => c.doc === 'copy' && c.path === 'pages.flatten').length,
      0,
    );
    assert.equal(calls(trace).filter((c) => c.doc === 'copy' && c.path === 'close').length, 1);
  }
  const observed = (await run('observers-see-a-successful-run')).trace;
  assert.deepEqual(
    observed.flatMap((t) => t.callbacks).filter((c) => c.fn === 'onUiEffect'),
    ['one', 'two'].map((message) => ({
      fn: 'onUiEffect',
      args: [message, { origin: 'user', phase: 'boot', icon: 0 }],
    })),
  );
  assert.equal(value(observed, 7), null);
  assert.equal(calls(observed).filter((c) => c.doc === 'copy' && c.path === 'close').length, 1);
});
test('flatten refusal closes the first realm and unchanged flatten allows the next arm', async () => {
  const { trace } = await run('a-flatten-that-fails-or-skips');
  assert.equal(value(trace, 4), null);
  assert.deepEqual(value(trace, 6), { width: 200, height: 100, name: 'Stamp1', subject: 'Stamp1' });
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'close')
      .map((c) => c.doc),
    ['lib', 'failed', 'unchanged'],
  );
  assert.deepEqual(
    trace
      .flatMap((t) => t.callbacks)
      .filter((c) => c.fn === 'sandbox.dispose')
      .map((c) => c.args),
    [[0], [1]],
  );
});
test('missing canonical bytes use extracted page and malformed missing asset bytes refuse exactly', async () => {
  const { trace } = await run('a-library-without-canonical-bytes');
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'open')
      .map((c) => c.args[0]),
    [
      { kind: 'bytes', id: 'a', bytes: canonical(new Uint8Array()) },
      {
        kind: 'bytes',
        id: 'stamp-import-0-0',
        bytes: canonical(bytes('%PDF-1.7\n%stamp fixture\n')),
      },
      { kind: 'bytes', id: 'stamp-import-0-2', bytes: canonical(pdf('page-100')) },
      { kind: 'bytes', id: 'stamp-import-0-3', bytes: canonical(new Uint8Array()) },
    ],
  );
  assert.equal(value(trace, 6), null);
});
test('dynamic direct and batch placement use exact resources, reserved refs, operation ids, facts and public reads', async () => {
  const { trace } = await run('dynamic-placement-writes-materialized-bytes-and-batch-facts');
  const all = calls(trace);
  const expected = [
    { page: 1, objectNumber: 10001, box: { x: 60, y: 80, width: 80, height: 40 } },
    { page: 1, objectNumber: 10002, box: { x: 170, y: 185, width: 60, height: 30 } },
    { page: 2, objectNumber: 10003, box: { x: 170, y: 185, width: 60, height: 30 } },
  ];
  assert.deepEqual(
    all.filter((c) => c.doc === 'a' && c.path === 'apply').map((c) => c.args),
    expected.map((e, i) => [
      {
        ops: [
          {
            type: 'annotations.create',
            page: pr(e.page),
            data: {
              subtype: 'stamp',
              box: e.box,
              rotation: null,
              fit: 'contain',
              name: 'Stamp1',
              subject: 'Stamp1',
            },
            objectNumber: e.objectNumber,
            resources: { appearance: canonical(pdf('flat-100')) },
          },
        ],
      },
      { opId: operationAt(i + 1) },
    ]),
  );
  type Placed = {
    subtype: string;
    ref: unknown;
    page: unknown;
    box: unknown;
    name: string;
    subject: string;
    rotation: unknown;
    fit: string;
  };
  const project = (a: Placed) => ({
    subtype: a.subtype,
    ref: a.ref,
    page: a.page,
    box: a.box,
    name: a.name,
    subject: a.subject,
    rotation: a.rotation,
    fit: a.fit,
  });
  const literal = (e: (typeof expected)[number]) => ({
    subtype: 'stamp',
    ref: objRef(e.objectNumber, e.page),
    page: pr(e.page),
    box: e.box,
    name: 'Stamp1',
    subject: 'Stamp1',
    rotation: null,
    fit: 'contain',
  });
  assert.deepEqual(
    project(resolved<{ annotation: Placed }>(trace, 5).annotation),
    literal(expected[0]),
  );
  const batch = resolved<{ applied: Placed[]; skipped: unknown[]; failed: unknown[] }>(trace, 6);
  assert.deepEqual(batch.applied.map(project), [literal(expected[2])]);
  assert.deepEqual(batch.skipped, []);
  assert.deepEqual(batch.failed, [
    {
      ref: pr(1),
      error: {
        code: 'operation-failed',
        message: 'page locked',
        capability: 'annotation',
        permission: null,
      },
    },
  ]);
  assert.deepEqual(value<Placed[]>(trace, 8).map(project), [
    literal(expected[0]),
    literal(expected[2]),
  ]);
  const facts = trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'annotationCreated');
  assert.deepEqual(
    facts.map((c) => ({
      annotation: project((c.args[0] as { annotation: Placed }).annotation),
      origin: (c.args[0] as { origin: unknown }).origin,
    })),
    [0, 2].map((i) => ({
      annotation: literal(expected[i]),
      origin: { ...LOCAL, tx: { id: operationAt(i + 1), index: 0, count: 1 } },
    })),
  );
  assert.deepEqual(
    trace.flatMap((t) => t.callbacks).find((c) => c.fn === 'annotationWriteFailed')?.args,
    [
      {
        refs: [objRef(10002)],
        error: {
          code: 'operation-failed',
          message: 'page locked',
          capability: 'annotation',
          permission: null,
        },
      },
    ],
  );
  assert.equal(trace[7].engine.length, 0);
  assert.deepEqual(value(trace, 9), canonical(pdf('page-100')));
  assert.deepEqual(resolved(trace, 10), canonical(pdf('canonical')));
  assert.deepEqual(
    all.filter((c) => c.path === 'reserveObjectNumbers').map((c) => c.args),
    [],
  ); // The authored initial reservation supplies 10001..10200.
  assert.deepEqual(
    all.filter((c) => c.path === 'close').map((c) => c.doc),
    ['lib', 'copy1', 'copy2', 'copy3'],
  );
});
test('file picker sends exact requests and only nonexpired selected PNG creates a stamp', async () => {
  const { trace } = await run('file-picker-places-cancels-and-expires-the-click');
  assert.deepEqual(
    trace.flatMap((t) => t.callbacks),
    [100, 200, 300].map((n) => ({
      fn: 'filePicker',
      args: [
        {
          toolId: 'stamp',
          subtype: 'stamp',
          accept: 'image/png,image/jpeg,application/pdf',
          page: pr(1),
          point: { x: n, y: n },
        },
      ],
    })),
  );
  assert.deepEqual(
    [4, 5, 6].map((i) => value(trace, i)),
    [true, true, true],
  );
  assert.deepEqual(
    calls(trace)
      .filter((c) => c.path === 'apply')
      .map((c) => c.args),
    [
      [
        {
          ops: [
            {
              type: 'annotations.create',
              page: pr(1),
              data: {
                subtype: 'stamp',
                box: { x: 50, y: 75, width: 100, height: 50 },
                rotation: null,
                fit: 'contain',
              },
              objectNumber: 10001,
              resources: { appearance: canonical(png(100, 50)) },
            },
          ],
        },
        { opId: operationAt(1) },
      ],
    ],
  );
  const read = value<{ ref: unknown; box: unknown }[]>(trace, 9);
  assert.equal(read.length, 1);
  assert.deepEqual(
    read.map((a) => ({ ref: a.ref, box: a.box })),
    [{ ref: objRef(10001), box: { x: 50, y: 75, width: 100, height: 50 } }],
  );
  assert.deepEqual(
    trace
      .slice(5)
      .flatMap((t) => t.engine)
      .filter((c) => (c as Call).path === 'apply'),
    [],
  );
});
test('permission refusals do no detached work and authorized later document arms successfully', async () => {
  const { trace } = await run('permission-refusal-recovers-on-an-authorized-document');
  assert.equal(value(trace, 3), false);
  assert.deepEqual(
    trace.slice(3, 6).flatMap((t) => t.engine),
    [],
  );
  assert.equal(value(trace, 7), true);
  assert.deepEqual(value(trace, 9), { width: 200, height: 100, name: 'Stamp1', subject: 'Stamp1' });
  assert.deepEqual(value(trace, 10), asset);
  assert.equal(value(trace, 12), null);
});
test('factory teardown actually executes between saved global descriptors and restores all owners', async () => {
  const descriptors = () => ({
    random: Object.getOwnPropertyDescriptor(Math, 'random'),
    error: Object.getOwnPropertyDescriptor(console, 'error'),
    warn: Object.getOwnPropertyDescriptor(console, 'warn'),
    crypto: Object.getOwnPropertyDescriptor(globalThis, 'crypto'),
    uuid: Object.getOwnPropertyDescriptor(globalThis.crypto, 'randomUUID'),
  });
  const before = descriptors();
  const result = await runScenario(input('teardown-destroys-a-factory-engine-once'));
  assert.deepEqual(descriptors(), before);
  assert.deepEqual(result.unmatched, []);
  const all = calls(result.trace);
  assert.equal(all.filter((c) => c.path === 'destroy').length, 1);
  assert.deepEqual(
    all.filter((c) => c.path === 'close').map((c) => c.doc),
    ['lib', 'copy', 'a'],
  );
  assert.equal(
    result.trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'assetEngine').length,
    1,
  );
  assert.deepEqual(
    result.trace.flatMap((t) => t.reports),
    [],
  );
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { suites as measurement } from '../src/suites/measurement';
import { suites as redaction } from '../src/suites/redaction';
import { suites as foundation } from '../src/suites/stamp-functions';
import { suites as libraries } from '../src/suites/stamp-library';
import { suites as assets } from '../src/suites/stamp-assets';
import { suites as api } from '../src/suites/stamp-api';
const history = new URL('../historical/task-12b2b3-v/', import.meta.url);
const sha = (value: string | Uint8Array) => createHash('sha256').update(value).digest('hex');
type Row = { file: string; name: string; inputSha256: string };
type Mapping = { before: Row; after?: Row | null; disposition: string };
type Evidence = { path: string; sha256: string; commit: string };
interface Accounting {
  original: number;
  current: number;
  mapped: number;
  added: number;
  renamed: number;
  wholeCaseRetirements: number;
  changedMappedInputs: number;
  partialRetirements: number;
  mappings: Mapping[];
  additions: Row[];
  sourceEvidence: Evidence[];
  currentSourceHashes: { path: string; sha256: string }[];
  removalEvidence: { before: Evidence; after: Evidence }[];
}
test('all nineteen originals have exact mappings and all twenty-two complete current inputs are recorded', async () => {
  const account: Accounting = JSON.parse(
    await readFile(new URL('accounting.json', history), 'utf8'),
  );
  assert.deepEqual(
    [
      account.original,
      account.current,
      account.mapped,
      account.added,
      account.renamed,
      account.wholeCaseRetirements,
      account.changedMappedInputs,
      account.partialRetirements,
    ],
    [19, 22, 19, 3, 0, 0, 19, 9],
  );
  const stored: (Row & { input: unknown })[] = JSON.parse(
    await readFile(new URL('current-inputs.json', history), 'utf8'),
  );
  const original: Row[] = JSON.parse(
    await readFile(new URL('../task-12b2b3/original-inputs.json', history), 'utf8'),
  );
  assert.equal(stored.length, 22);
  assert.equal(new Set(stored.map((r) => r.name)).size, 22);
  for (const c of cases) {
    const row = stored.find((r) => r.name === c.name);
    assert.ok(row);
    assert.deepEqual(row.input, canonical(c));
    assert.equal(row.inputSha256, sha(JSON.stringify(canonical(c))));
  }
  assert.deepEqual(
    account.mappings.map((m) => m.before),
    original
      .filter((r) => r.file === suite.file)
      .map(({ file, name, inputSha256 }) => ({ file, name, inputSha256 })),
  );
  for (const m of account.mappings) {
    assert.ok(m.after);
    const row = stored.find((r) => r.name === m.after!.name);
    assert.ok(row);
    assert.equal(row.inputSha256, m.after.inputSha256);
    assert.notEqual(m.before.inputSha256, m.after.inputSha256);
  }
  assert.deepEqual(
    account.additions.map((a) => a.name),
    [
      'dynamic-placement-writes-materialized-bytes-and-batch-facts',
      'file-picker-places-cancels-and-expires-the-click',
      'permission-refusal-recovers-on-an-authorized-document',
    ],
  );
  for (const a of account.additions)
    assert.equal(a.inputSha256, stored.find((r) => r.name === a.name)?.inputSha256);
});
test('source evidence is actual shipping bytes at full last-owner commits and removal transitions', async () => {
  const account: Accounting = JSON.parse(
    await readFile(new URL('accounting.json', history), 'utf8'),
  );
  for (const row of account.sourceEvidence) {
    assert.match(row.commit, /^[a-f0-9]{40}$/);
    const path = new URL('../../../' + row.path, import.meta.url);
    const data = await readFile(path);
    assert.equal(sha(data), row.sha256, row.path);
    assert.equal(
      sha(execFileSync('git', ['show', `${row.commit}:${row.path}`])),
      row.sha256,
      row.path,
    );
  }
  for (const transition of account.removalEvidence)
    for (const row of [transition.before, transition.after])
      assert.equal(
        sha(execFileSync('git', ['show', `${row.commit}:${row.path}`])),
        row.sha256,
        row.path,
      );
  for (const row of account.currentSourceHashes)
    assert.equal(
      sha(await readFile(new URL('../../../' + row.path, import.meta.url))),
      row.sha256,
      row.path,
    );
});
test('aggregate accounts for all original211 and preserves every reviewed earlier disposition exactly', async () => {
  const aggregate: {
    original: number;
    current: number;
    mapped: number;
    wholeCaseRetirements: number;
    grossAdditions: number;
    checkpointBeforeV: number;
    slices: {
      slice: string;
      path: string;
      sha256: string;
      original: number;
      current: number;
      mappings: Mapping[];
      additions: Row[];
    }[];
    sourceEvidence: Evidence[];
  } = JSON.parse(await readFile(new URL('aggregate-accounting.json', history), 'utf8'));
  assert.deepEqual(
    [
      aggregate.original,
      aggregate.current,
      aggregate.mapped,
      aggregate.wholeCaseRetirements,
      aggregate.grossAdditions,
      aggregate.checkpointBeforeV,
    ],
    [211, 227, 209, 2, 18, 224],
  );
  const current = new Map<string, string>();
  for (const s of [
    ...measurement,
    ...redaction,
    ...foundation,
    ...libraries,
    ...assets,
    ...api,
    ...suites,
  ])
    for (const c of s.cases) {
      const key = JSON.stringify([s.file, c.name]);
      assert.equal(current.has(key), false, key);
      current.set(key, sha(JSON.stringify(canonical(c))));
    }
  assert.equal(current.size, 227);
  const original: Row[] = JSON.parse(
    await readFile(new URL('../task-12b2b3/original-inputs.json', history), 'utf8'),
  );
  const before = new Set<string>();
  const mapped = new Set<string>();
  let retired = 0,
    added = 0;
  for (const s of aggregate.slices) {
    const data = await readFile(new URL('../../../' + s.path, import.meta.url));
    assert.equal(sha(data), s.sha256);
    const originalAccounting = JSON.parse(data.toString());
    assert.deepEqual(s.mappings, originalAccounting.mappings);
    assert.deepEqual(s.additions, originalAccounting.additions);
    for (const m of s.mappings) {
      const key = JSON.stringify([m.before.file, m.before.name]);
      assert.equal(before.has(key), false, key);
      before.add(key);
      assert.equal(
        original.find((r) => r.file === m.before.file && r.name === m.before.name)?.inputSha256,
        m.before.inputSha256,
      );
      if (m.after) {
        const key = JSON.stringify([m.after.file, m.after.name]);
        assert.equal(current.get(key), m.after.inputSha256);
        assert.equal(mapped.has(key), false);
        mapped.add(key);
      } else {
        assert.equal(m.disposition, 'retired');
        retired++;
      }
    }
    for (const a of s.additions) {
      const key = JSON.stringify([a.file, a.name]);
      assert.equal(current.get(key), a.inputSha256);
      assert.equal(mapped.has(key), false);
      mapped.add(key);
      added++;
    }
  }
  assert.equal(before.size, 211);
  assert.equal(mapped.size, 227);
  assert.equal(retired, 2);
  assert.equal(added, 18);
  for (const row of aggregate.sourceEvidence) {
    assert.match(row.commit, /^[a-f0-9]{40}$/);
    assert.equal(
      sha(await readFile(new URL('../../../' + row.path, import.meta.url))),
      row.sha256,
      row.path,
    );
  }
});
