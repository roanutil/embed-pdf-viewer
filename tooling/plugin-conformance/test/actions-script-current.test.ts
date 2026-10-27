process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/actions';
import { runScenario } from '../src/scenario';
const files = ['plugins/actions-script.json', 'plugins/actions-hover.json'];
const owned = suites.flatMap((s) =>
  s.kind === 'scenario' && files.includes(s.file) ? s.cases : [],
);
const run = async (name: string) => {
  const entry = owned.find((e) => e.name === name);
  assert.ok(entry, name);
  return runScenario(entry);
};
const page = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
const ref = (objectNumber: number) => ({ kind: 'objectNumber', page: page(1), objectNumber });
const trigger = (event: string, number: number) => ({
  scope: 'annotation',
  event,
  ref: ref(number),
  page: page(1),
});
test('current hover driver calls the shipping pump with paired exit and enter triggers', async () => {
  const result = await run('pairs-are-submitted-back-to-back');
  const calls = result.trace.flatMap((s) => s.callbacks).filter((c) => c.fn === 'dispatch');
  assert.deepEqual(
    calls.map((c) => c.args[0]),
    [trigger('cursorEnter', 1), trigger('cursorExit', 1), trigger('cursorEnter', 2)],
  );
});
const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const callbacks = (result: Awaited<ReturnType<typeof runScenario>>, fn?: string) =>
  result.trace.flatMap((s) => s.callbacks).filter((c) => !fn || c.fn === fn);
const resultValues = (result: Awaited<ReturnType<typeof runScenario>>) =>
  result.trace
    .flatMap((s) => s.settled)
    .flatMap((s) =>
      s.resolved && typeof s.resolved === 'object' && 'nodes' in s.resolved ? [s.resolved] : [],
    );
const node = (status = 'executed', detail?: string, path: number[] = []) => ({
  path,
  type: 'javascript',
  status,
  ...(detail ? { detail } : {}),
});
const diagnostic = (code: string, message: string) => ({ code, message });
const outcome = (nodes = [node()], diagnostics: unknown[] = [], status = 'executed') => ({
  status,
  nodes,
  diagnostics,
});
const failed = (message: string) =>
  outcome(
    [node('failed', message)],
    [diagnostic('executor-failed', `javascript: ${message}`)],
    'partial',
  );
const scriptExpectations: Record<string, unknown[]> = {
  'a-script-node-is-one-transaction': [outcome()],
  'script-effects-commit': [outcome()],
  'script-commit-failures': [
    outcome(),
    failed('form effects: value refused'),
    failed('form effects: failed'),
    outcome(),
    failed('annotation 6: locked'),
    failed('annotation 6: failed'),
  ],
  'boot-effects-and-a-refetched-world': [outcome([node(), node('executed', undefined, [0])])],
  'boot-without-effects': [outcome()],
  'run-errors-and-diagnostics': [failed('interrupted: time budget'), failed('output too large')],
  'external-effects-run-after-the-transaction': [outcome()],
  'script-print-obeys-the-print-policy': [outcome(), outcome()],
  'script-node-budget': [
    outcome(
      [
        node(),
        node('executed', undefined, [0]),
        node('inert', 'dispatch script budget exhausted (2 JS nodes)', [0, 0]),
        node('inert', 'dispatch script budget exhausted (2 JS nodes)', [0, 0, 0]),
      ],
      [
        diagnostic('executor-inert', 'javascript: dispatch script budget exhausted (2 JS nodes)'),
        diagnostic('executor-inert', 'javascript: dispatch script budget exhausted (2 JS nodes)'),
      ],
    ),
    outcome(),
  ],
  'script-event-provenance': Array.from({ length: 24 }, () => outcome()),
  'no-page-to-anchor': [
    outcome(
      [node('inert', 'no page to anchor the world on')],
      [diagnostic('executor-inert', 'javascript: no page to anchor the world on')],
      'inert',
    ),
  ],
  'an-unknown-page-fails-the-node': [failed('no page 42')],
  'environment-inputs': [outcome()],
  'environment-defaults': [outcome([node(), node('executed', undefined, [0])])],
  'environment-file-name-fallback': [outcome()],
  'a-fault-rebuilds-the-realm': [outcome(), outcome()],
  'a-throwing-sandbox-fails-the-node': [failed('vm exploded')],
  'budget-reaches-the-sandbox': [outcome()],
  'script-transaction-port': [outcome()],
  'closing-the-document-disposes-the-realm': [outcome()],
  'script-boot-without-a-catalog': [outcome(), outcome()],
  'detached-script-realm': [outcome()],
  'detached-script-realm-without-metadata': [],
  'no-detached-realm-without-scripting': [],
  'scripting-disabled': [
    outcome(
      [node('inert')],
      [diagnostic('no-executor', 'javascript: no executor registered (scripting unavailable)')],
      'inert',
    ),
  ],
};
test('script cases pin complete successful, inert, failed and budget outcomes independently', async () => {
  for (const [name, expected] of Object.entries(scriptExpectations))
    assert.deepEqual(resultValues(await run(name)), expected, name);
});
const hoverExpectations: Record<string, unknown[]> = {
  'pairs-are-submitted-back-to-back': [
    trigger('cursorEnter', 1),
    trigger('cursorExit', 1),
    trigger('cursorEnter', 2),
  ],
  'a-slow-transition-is-never-split': [
    trigger('cursorEnter', 1),
    trigger('cursorExit', 1),
    trigger('cursorEnter', 2),
    trigger('cursorExit', 2),
    trigger('cursorEnter', 3),
  ],
  'intermediate-targets-are-skipped': [
    trigger('cursorEnter', 1),
    trigger('cursorExit', 1),
    trigger('cursorEnter', 4),
  ],
  'same-target-is-free-and-exit-happens-once': [
    trigger('cursorEnter', 1),
    trigger('cursorExit', 1),
  ],
  'event-flags-skip-tree-less-halves': [trigger('cursorExit', 1), trigger('cursorEnter', 2)],
  'reset-forgets-without-dispatching': [trigger('cursorEnter', 1), trigger('cursorEnter', 2)],
  'a-rejected-dispatch-still-releases': [
    trigger('cursorEnter', 1),
    trigger('cursorExit', 1),
    trigger('cursorEnter', 2),
  ],
  'an-unknown-ref-kind-has-no-key-and-matches-itself': [
    { scope: 'annotation', event: 'cursorEnter', ref: { kind: 'mystery' }, page: page(1) },
  ],
};
test('hover coalesces only intermediate targets and releases paired work atomically', async () => {
  for (const [name, expected] of Object.entries(hoverExpectations)) {
    const result = await run(name);
    assert.deepEqual(
      callbacks(result, 'dispatch').map((c) => c.args[0]),
      expected,
      name,
    );
    if (name === 'a-slow-transition-is-never-split') {
      assert.deepEqual(result.trace[3]!.callbacks, []);
      assert.equal(result.trace[4]!.callbacks.filter((c) => c.fn === 'dispatch').length, 2);
      assert.deepEqual(result.trace[5]!.callbacks, [], 'queuing C cannot split Exit(A)/Enter(B)');
      assert.deepEqual(result.trace[6]!.callbacks, [], 'settling Exit(A) leaves Enter(B) pending');
      assert.deepEqual(
        result.trace[7]!.callbacks,
        [
          { fn: 'dispatch', args: [trigger('cursorExit', 2)] },
          { fn: 'dispatch', args: [trigger('cursorEnter', 3)] },
        ],
        'settling Enter(B) submits Exit(B)/Enter(C) back to back',
      );
      assert.deepEqual(result.trace[8]!.callbacks, []);
      assert.deepEqual(result.trace[9]!.callbacks, []);
    }
    if (name === 'pairs-are-submitted-back-to-back')
      assert.equal(result.trace[4]!.callbacks.filter((c) => c.fn === 'dispatch').length, 2);
  }
});
test('hover compares stable object and base-index addresses and preserves current scope and source', async () => {
  const base = (baseIndex: number) => ({ kind: 'baseIndex', page: page(1), baseIndex });
  const target = (event: string, ref: unknown, pon = 1) => ({
    scope: 'annotation',
    event,
    ref,
    page: page(pon),
  });
  const identities = [
    base(0),
    base(1),
    base(2),
    base(3),
    ref(1),
    { ...ref(1), page: page(2) },
    { ...ref(2), page: page(2) },
  ];
  assert.deepEqual(
    callbacks(await run('target-identity'), 'dispatch').map((c) => c.args[0]),
    identities.flatMap((r, i) =>
      i === 0
        ? [target('cursorEnter', r)]
        : [
            target('cursorExit', identities[i - 1], i > 5 ? 2 : 1),
            target('cursorEnter', r, i > 4 ? 2 : 1),
          ],
    ),
  );
  const result = await run('drives-the-real-dispatcher');
  assert.deepEqual(
    callbacks(result, 'exec.named').map((c) => c.args),
    [
      [
        { type: 'named', subtype: 'Named', name: 'show', next: [] },
        {
          origin: 'hover',
          source: { kind: 'link', annotation: ref(5), page: page(1) },
          event: { scope: 'annotation', name: 'cursorEnter' },
        },
      ],
      [
        { type: 'named', subtype: 'Named', name: 'hide', next: [] },
        {
          origin: 'hover',
          source: { kind: 'link', annotation: ref(5), page: page(1) },
          event: { scope: 'annotation', name: 'cursorExit' },
        },
      ],
      [
        { type: 'named', subtype: 'Named', name: 'show-6', next: [] },
        {
          origin: 'hover',
          source: { kind: 'annotation', annotation: ref(6), page: page(1) },
          event: { scope: 'annotation', name: 'cursorEnter' },
        },
      ],
    ],
  );
  assert.deepEqual(
    callbacks(result, 'onExecuted'),
    ['show', 'hide', 'show-6'].map((name, index) => ({
      fn: 'onExecuted',
      args: [
        {
          tree: {
            root: { type: 'named', subtype: 'Named', name, next: [] },
            incomplete: false,
            warningFlags: 0,
            warnings: [],
          },
          result: {
            status: 'executed',
            nodes: [{ path: [], type: 'named', status: 'executed' }],
            diagnostics: [],
          },
          source: {
            kind: index < 2 ? 'link' : 'annotation',
            annotation: ref(index < 2 ? 5 : 6),
            page: page(1),
          },
        },
      ],
    })),
  );
});
const formWrite = {
  kind: 'setValue',
  ref: { kind: 'objectNumber', objectNumber: 100 },
  value: { value: 'x' },
};
const annotPatch = { strokeColor: ['RGB', 0.1, 0.2, 0.3], contents: 'Annot:Mouse Enter' };
test('scripts commit typed current form effects before annotation effects and skip failed streams', async () => {
  const result = await run('script-effects-commit');
  assert.deepEqual(
    callbacks(result).filter((c) => c.fn.startsWith('sink.')),
    [
      { fn: 'sink.form', args: [[formWrite]] },
      {
        fn: 'sink.annot',
        args: [
          [
            { annotObjectNumber: 6, page: page(1), patch: annotPatch },
            { annotObjectNumber: -1, patch: annotPatch },
          ],
        ],
      },
    ],
  );
  const failure = await run('script-commit-failures');
  assert.deepEqual(
    callbacks(failure)
      .filter((c) => c.fn.startsWith('sink.'))
      .map((c) => c.fn),
    ['sink.form', 'sink.form', 'sink.form', 'sink.form', 'sink.annot', 'sink.form', 'sink.annot'],
  );
  const boot = await run('boot-effects-and-a-refetched-world');
  assert.deepEqual(
    boot.trace
      .flatMap((s) => s.engine)
      .map(object)
      .filter((e) => ['forms.list', 'page.annotations.list'].includes(String(e.path)))
      .map((e) => [e.path, e.pon]),
    [
      ['forms.list', undefined],
      ['page.annotations.list', 1],
      ['forms.list', undefined],
      ['page.annotations.list', 1],
      ['forms.list', undefined],
      ['page.annotations.list', 1],
    ],
  );
  assert.deepEqual(
    callbacks(boot)
      .filter((c) => ['sandbox.boot', 'sink.form', 'sandbox.run', 'ui.alert'].includes(c.fn))
      .map((c) => c.fn),
    ['sandbox.boot', 'sink.form', 'ui.alert', 'sandbox.run', 'sandbox.run'],
  );
  assert.deepEqual(callbacks(boot, 'ui.alert')[0]?.args, [
    'booted',
    { origin: 'user', phase: 'boot', icon: 3 },
  ]);
});
const emptyIdentity = { name: 'Ada', loginName: 'u-1', corporation: '', email: '' };
const budget = {
  maxExecutionMs: 50,
  maxMemoryBytes: 16777216,
  maxStackBytes: 524288,
  maxEffects: 256,
  maxOutputBytes: 1048576,
};
test('script world preserves current form values, page-space annotation conversion, identity and event refs', async () => {
  const result = await run('a-script-node-is-one-transaction');
  const boot = callbacks(result, 'sandbox.boot')[0]!;
  const call = callbacks(result, 'sandbox.run')[0]!;
  assert.deepEqual(boot.args.slice(0, 2), [0, ['function lib() {}\n;\nvar x = 1;']]);
  const world = object(call.args[2]);
  assert.deepEqual(world, {
    document: { id: 'a', fileName: 'report.pdf', pageCount: 2, pageNumber: 0 },
    identity: { ...emptyIdentity, email: 'ada@x.test' },
    environment: { nowMs: 0, utcOffsetMinutes: 0, randomSeed: 3686612585 },
    fields: [
      {
        ref: { kind: 'objectNumber', objectNumber: 100 },
        name: 'name',
        family: 'text',
        value: 'name-value',
        defaultValue: null,
        display: 'visible',
        readOnly: false,
        required: false,
      },
      {
        ref: { kind: 'objectNumber', objectNumber: 101 },
        name: 'choice',
        family: 'combobox',
        value: ['1'],
        defaultValue: null,
        display: 'visible',
        readOnly: false,
        required: false,
        options: [{ label: 'One', value: '1' }],
      },
      {
        ref: { kind: 'objectNumber', objectNumber: 102 },
        name: 'empty',
        family: 'text',
        value: null,
        defaultValue: null,
        display: 'visible',
        readOnly: false,
        required: false,
      },
    ],
    annots: [
      {
        ref: ref(6),
        name: 'tip',
        subtype: 'square',
        page: 0,
        rect: [100, 600, 200, 700],
        contents: 'hello',
        author: 'Ada',
        subject: 'Note',
        strokeColor: ['RGB', 1, 0, 0.2],
        fillColor: ['T'],
        opacity: 0.5,
        width: 2,
        borderStyle: 'D',
        dash: [3, 1],
        hidden: true,
        print: false,
        readOnly: true,
        locked: true,
        noView: false,
        toggleNoView: true,
        opaqueBody: false,
      },
      {
        ref: ref(9),
        name: '',
        subtype: 'stamp',
        page: 0,
        rect: [100, 600, 200, 700],
        contents: '',
        author: '',
        subject: '',
        strokeColor: ['T'],
        fillColor: ['T'],
        opacity: 1,
        width: 1,
        borderStyle: 'S',
        dash: [],
        hidden: false,
        print: true,
        readOnly: false,
        locked: false,
        noView: false,
        toggleNoView: false,
        opaqueBody: true,
      },
    ],
    annotPages: [0],
    annotsCoverDocument: false,
    event: {
      kind: 'widget-activate',
      type: 'Field',
      name: 'Mouse Up',
      target: { kind: 'objectNumber', objectNumber: 100 },
      source: { kind: 'objectNumber', objectNumber: 100 },
      value: 'name-value',
    },
  });
  assert.deepEqual(call.args[3], budget);
  assert.deepEqual(object(boot.args[2]).event, { kind: 'name-tree-boot' });
});
test('script provenance retains all 24 field/link/annotation/page/document event inputs', async () => {
  const field = (target: unknown, name = 'Mouse Up', value?: string) => ({
    kind: 'widget-activate',
    type: 'Field',
    name,
    target,
    source: target,
    ...(value ? { value } : {}),
  });
  const event = (type: string, name: string) => ({ kind: 'widget-activate', type, name });
  assert.deepEqual(
    callbacks(await run('script-event-provenance'), 'sandbox.run').map(
      (c) => object(c.args[2]).event,
    ),
    [
      field({ kind: 'objectNumber', objectNumber: 101 }),
      field({ kind: 'fqn', name: 'name' }, 'Focus', 'name-value'),
      field({ kind: 'fqn', name: 'ghost' }),
      field({ kind: 'objectNumber', objectNumber: 102 }),
      field({ kind: 'fqn', name: 'empty' }),
      event('Link', 'Mouse Enter'),
      event('Link', 'Mouse Enter'),
      event('Annot', 'Mouse Down'),
      event('Annot', 'Mouse Exit'),
      event('Annot', 'Mouse Down'),
      event('Annot', 'Mouse Up'),
      event('Annot', 'Blur'),
      ...['Open', 'Close', 'Visible', 'Invisible'].map((name) => event('Page', name)),
      ...['Open', 'WillSave', 'DidSave', 'WillPrint', 'DidPrint', 'WillClose'].map((name) =>
        event('Doc', name),
      ),
      event('Link', 'Mouse Up'),
      event('Annot', 'Mouse Up'),
    ],
  );
});
test('configured and default environment inputs use owner claims, target seed, host clock and exact budgets', async () => {
  const defaults = callbacks(await run('environment-defaults')).filter((c) =>
    ['sandbox.boot', 'sandbox.run'].includes(c.fn),
  );
  assert.deepEqual(
    defaults.map((c) => object(c.args[2]).environment),
    [3669834966, 3686612585, 3636279728].map((randomSeed) => ({
      nowMs: 2500,
      utcOffsetMinutes: 0,
      randomSeed,
    })),
  );
  assert.deepEqual(
    defaults.map((c) => object(c.args[2]).identity),
    [emptyIdentity, emptyIdentity, emptyIdentity],
  );
  assert.deepEqual(
    defaults.map((c) => object(c.args[2]).document),
    Array.from({ length: 3 }, () => ({
      id: 'a',
      fileName: 'Opened.pdf',
      pageCount: 2,
      pageNumber: 0,
    })),
  );
  const configured = callbacks(await run('environment-inputs'), 'sandbox.run')[0]!;
  assert.deepEqual(object(configured.args[2]).environment, {
    nowMs: 1720000000000,
    utcOffsetMinutes: 180,
    randomSeed: 7,
  });
  assert.deepEqual(object(configured.args[2]).identity, {
    name: 'Fn User',
    loginName: 'u-1',
    corporation: 'Fn Inc',
    email: '',
  });
  const bounded = callbacks(await run('budget-reaches-the-sandbox')).filter((c) =>
    ['sandbox.boot', 'sandbox.run'].includes(c.fn),
  );
  assert.deepEqual(
    bounded.map((c) => c.args[3]),
    Array.from({ length: 2 }, () => ({
      maxExecutionMs: 5,
      maxMemoryBytes: 1024,
      maxStackBytes: 256,
      maxEffects: 2,
      maxOutputBytes: 64,
    })),
  );
  const detached = await run('detached-script-realm');
  const worlds = callbacks(detached, 'sandbox.run').map((c) => object(c.args[2]));
  assert.deepEqual(
    worlds.map((w) => w.identity),
    [emptyIdentity, emptyIdentity],
  );
  assert.deepEqual(worlds[1]?.document, {
    id: 'b',
    fileName: 'Asset.pdf',
    pageCount: 2,
    pageNumber: 0,
  });
  assert.deepEqual(worlds[1]?.environment, {
    nowMs: 0,
    utcOffsetMinutes: 0,
    randomSeed: 916021364,
  });
  assert.deepEqual(callbacks(detached, 'sandbox.boot')[1]?.args.slice(0, 2), [
    1,
    ['var asset = 1;'],
  ]);
  assert.deepEqual(
    object(
      callbacks(await run('detached-script-realm-without-metadata'), 'sandbox.run')[0]!.args[2],
    ).document,
    { id: 'b', fileName: 'document.pdf', pageCount: 0, pageNumber: 0 },
  );
});
test('script realm faults rebuild and caller/document cleanup disposes actual resources', async () => {
  const fault = await run('a-fault-rebuilds-the-realm');
  assert.deepEqual(
    callbacks(fault)
      .filter((c) => c.fn.startsWith('sandbox.'))
      .map((c) => [c.fn, c.args[0]]),
    [
      ['sandbox.factory', 0],
      ['sandbox.boot', 0],
      ['sandbox.run', 0],
      ['sandbox.factory', 1],
      ['sandbox.boot', 1],
      ['sandbox.run', 1],
    ],
  );
  const close = await run('closing-the-document-disposes-the-realm');
  assert.deepEqual(callbacks(close, 'sandbox.dispose'), [{ fn: 'sandbox.dispose', args: [0] }]);
  assert.deepEqual(close.trace.at(-1)!.engineState, { a: { subscribers: 0 } });
  const detached = await run('detached-script-realm');
  assert.deepEqual(callbacks(detached, 'sandbox.dispose'), [{ fn: 'sandbox.dispose', args: [1] }]);
  const absent = await run('no-detached-realm-without-scripting');
  assert.deepEqual(absent.trace.at(-1)?.outcome, { returned: null });
});
const cb = (fn: string, ...args: unknown[]) => ({ fn, args });
const ui = (phase = 'user', origin = 'user') => ({ origin, phase });
const diagCall = (code: string, message: string) =>
  cb('onScriptDiagnosticReported', { code, message });
const errorCall = (message: string) =>
  cb('onScriptFailed', { error: { kind: 'exception', message }, source: null });
const surfaceCallbacks: Record<string, unknown[]> = {
  'surface-ui-effects': [
    cb('ui.alert', 'm1', { ...ui('boot', 'hover'), icon: 0 }),
    cb('ui.alert', 'm2', { ...ui('boot', 'hover'), icon: 2, title: 'T' }),
    cb('ui.gotoPage', 3, ui('boot', 'hover')),
    cb('ui.print', ui('boot', 'hover')),
    diagCall('unsupported-api', 'x'),
    errorCall('boom'),
  ],
  'surface-refusals': [
    cb('onDiagnosticReported', { code: 'no-adapter', action: 'javascript', source: null }),
    diagCall('ui-effect-suppressed', 'script print request withheld: doc.print is not allowed'),
    cb('op.print'),
    cb('b.onScriptDiagnosticReported', {
      code: 'ui-effect-suppressed',
      message: 'script print request during a document print event — suppressed (reentrant)',
    }),
    cb('ui.print', ui()),
  ],
  'surface-a-detached-realm': [
    ...['print', 'gotoPage', 'submitForm'].map((kind) =>
      diagCall(
        'ui-effect-suppressed',
        `script ${kind} request from a detached realm suppressed: no document surface`,
      ),
    ),
    cb('ui.alert', 'from the asset', { ...ui(), icon: 3 }),
    cb('ui.gotoPage', 2, ui()),
  ],
  'surface-script-commit': [
    cb('ui.alert', 'boot', { ...ui('boot'), icon: 0 }),
    cb('ui.gotoPage', 1, ui()),
    cb('ui.alert', 'user', { ...ui(), icon: 1 }),
    diagCall('unsupported-api', 'd'),
    errorCall('e'),
    diagCall('unsupported-api', 'no effects'),
    errorCall('no effects'),
    diagCall(
      'ui-effect-suppressed',
      'script print request from a detached realm suppressed: no document surface',
    ),
    cb('ui.alert', 'asset', { ...ui(), icon: 0 }),
  ],
  'script-hooks-isolate-observers': [
    cb('bad.diag', { code: 'script-error', message: 'd' }),
    cb('good.diag', { code: 'script-error', message: 'd' }),
    cb('bad.error', { error: { kind: 'exception', message: 'e' }, source: null }),
    cb('good.error', { error: { kind: 'exception', message: 'e' }, source: null }),
  ],
};
test('form surface preserves UI arguments, boot/user phases, detached/permission/reentrant refusals and observer isolation', async () => {
  for (const [name, expected] of Object.entries(surfaceCallbacks))
    assert.deepEqual(callbacks(await run(name)), expected, name);
  const submit = await run('surface-submit');
  const request = {
    url: 'https://home.test/submit',
    method: 'post',
    format: 'fdf',
    flagsRaw: 0,
    entries: [{ name: 'plain', value: 'visible' }],
    origin: 'user',
    event: { scope: 'activate' },
  };
  assert.deepEqual(callbacks(submit), [
    cb(
      'resolver',
      {
        url: null,
        fields: null,
        exclude: false,
        includeNoValueFields: false,
        format: 'fdf',
        method: 'post',
        flagsRaw: 0,
      },
      { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } },
      { $function: true },
    ),
    cb('handler', request, { submitToDocumentHome: null }),
  ]);
  const refusal = await run('surface-refusals');
  assert.equal(refusal.trace[9]!.settled.length, 0);
  assert.deepEqual(
    refusal.trace[10]!.settled.map((s) => s.resolved),
    ['done'],
  );
});
test('external print and submit run after transaction outputs and respect executor origin policy', async () => {
  const result = await run('external-effects-run-after-the-transaction');
  assert.deepEqual(
    callbacks(result).map((c) => c.fn),
    [
      'sandbox.factory',
      'sandbox.boot',
      'sandbox.run',
      'ui.gotoPage',
      'ui.alert',
      'ui.print',
      'resolver',
      'handler',
      'ui.print',
    ],
  );
  assert.deepEqual(
    callbacks(result).filter((c) => c.fn.startsWith('ui.')),
    [
      cb('ui.gotoPage', 2, ui()),
      cb('ui.alert', 'hi', { ...ui(), icon: 1, title: 'T' }),
      cb('ui.print', ui('boot')),
      cb('ui.print', ui()),
    ],
  );
  assert.deepEqual(callbacks(result, 'resolver')[0]!.args.slice(0, 2), [
    {
      url: 'https://s.test/',
      fields: [
        { kind: 'name', name: 'name' },
        { kind: 'name', name: 'choice' },
      ],
      exclude: false,
      includeNoValueFields: true,
      format: 'xfdf',
      method: 'get',
      flagsRaw: 0,
    },
    { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } },
  ]);
  const policy = await run('script-print-obeys-the-print-policy');
  assert.deepEqual(
    callbacks(policy).filter((c) => !c.fn.startsWith('sandbox.')),
    [
      cb('onDiagnosticReported', {
        code: 'blocked',
        action: 'javascript',
        source: { kind: 'api' },
      }),
      cb('ui.print', ui()),
    ],
  );
});
const error = (message: string) => ({ $error: { name: 'Error', message } });
test('exact rejection and host-report channels remain separately named with no pending returned calls', async () => {
  const rejected: Record<string, unknown[]> = {
    'no-sandbox-host': [error('script host is disposed')],
    'script-transaction-port': [error('body threw')],
    'detached-script-realm': [error('script host is disposed')],
    'detached-script-realm-without-metadata': [error('script host is disposed')],
  };
  for (const entry of owned) {
    const result = await runScenario(entry);
    assert.deepEqual(result.unmatched, [], entry.name);
    assert.deepEqual(
      result.trace
        .flatMap((s) => s.settled)
        .filter((s) => 'rejected' in s)
        .map((s) => s.rejected),
      rejected[entry.name] ?? [],
      entry.name + ' rejections',
    );
    assert.deepEqual(
      result.trace.flatMap((s) => s.reports),
      entry.name === 'script-hooks-isolate-observers'
        ? [error('diag broke'), error('error broke')]
        : [],
      entry.name + ' reports',
    );
    assert.deepEqual(
      result.trace.filter((s) => s.outcome && 'threw' in s.outcome),
      [],
      entry.name + ' step errors',
    );
    assert.deepEqual(
      callbacks(result).filter((c) => c.fn.startsWith('console.')),
      [],
      entry.name + ' console',
    );
    const promises = result.trace.flatMap((s) =>
      s.outcome && 'promise' in s.outcome ? [s.outcome.promise] : [],
    );
    const settled = result.trace.flatMap((s) => s.settled).map((s) => s.promise);
    assert.deepEqual(
      [...promises].sort((a, b) => a - b),
      [...settled].sort((a, b) => a - b),
      entry.name + ' all promises settle exactly once',
    );
  }
});
test('enabled default factory stays enabled, exposes settings and closes without a scripted VM substitution', async () => {
  const result = await run('no-sandbox-host');
  assert.deepEqual(result.trace[2]!.outcome, { returned: true });
  const settings = object(object(result.trace[3]!.outcome).returned);
  assert.equal(object(settings.javascript).enabled, true);
  assert.deepEqual(
    callbacks(result).filter((c) => c.fn.startsWith('sandbox.')),
    [],
  );
  assert.deepEqual(result.trace.at(-2)!.engineState, { a: { subscribers: 0 } });
  assert.deepEqual(
    result.trace.at(-1)!.settled.map((s) => s.rejected),
    [error('script host is disposed')],
  );
});
test('all external script diagnostic, failure and action diagnostic channels are exact and source-derived', async () => {
  const actions: Record<string, string[]> = {
    'script-commit-failures': [
      'no-commit-sink',
      'no-commit-sink',
      'executor-failed',
      'executor-failed',
      'executor-failed',
      'executor-failed',
      'executor-failed',
      'executor-failed',
      'no-commit-sink',
      'executor-failed',
      'executor-failed',
      'executor-failed',
      'executor-failed',
    ],
    'run-errors-and-diagnostics': ['executor-failed', 'executor-failed'],
    'script-print-obeys-the-print-policy': ['blocked'],
    'script-node-budget': ['executor-inert', 'executor-inert'],
    'no-page-to-anchor': ['executor-inert'],
    'an-unknown-page-fails-the-node': ['executor-failed'],
    'a-throwing-sandbox-fails-the-node': ['executor-failed'],
  };
  const scripts: Record<string, unknown[]> = {
    'boot-effects-and-a-refetched-world': [diagCall('unsupported-api', 'app.beep')],
    'boot-without-effects': [diagCall('script-error', 'boot warned')],
    'run-errors-and-diagnostics': [diagCall('blocked-network', 'fetch refused')],
  };
  const failures: Record<string, unknown[]> = {
    'boot-effects-and-a-refetched-world': [
      cb('onScriptFailed', {
        error: { kind: 'exception', message: 'boot threw', stack: 'at boot' },
        source: { kind: 'api' },
      }),
    ],
    'run-errors-and-diagnostics': [
      cb('onScriptFailed', {
        error: { kind: 'budget', message: 'interrupted: time budget' },
        source: { kind: 'api' },
      }),
      cb('onScriptFailed', {
        error: { kind: 'invalid-output', message: 'output too large' },
        source: { kind: 'api' },
      }),
    ],
  };
  for (const entry of owned.slice(0, 26)) {
    const result = await runScenario(entry);
    assert.deepEqual(
      callbacks(result, 'onDiagnosticReported'),
      (actions[entry.name] ?? []).map((code) =>
        cb('onDiagnosticReported', {
          code,
          action: 'javascript',
          source:
            entry.name === 'an-unknown-page-fails-the-node'
              ? { kind: 'page', page: page(42) }
              : { kind: 'api' },
        }),
      ),
      entry.name + ' action events',
    );
    assert.deepEqual(
      callbacks(result, 'onScriptDiagnosticReported'),
      scripts[entry.name] ?? [],
      entry.name + ' script events',
    );
    assert.deepEqual(
      callbacks(result, 'onScriptFailed'),
      failures[entry.name] ?? [],
      entry.name + ' script failures',
    );
  }
});
import { readFileSync } from 'node:fs';
import { canonical } from '../src/canonical';
const history = new URL('../historical/task-12b2c4/', import.meta.url);
const read = (path: string) => JSON.parse(readFileSync(new URL(path, history), 'utf8'));
test('prior C3 complete current 42 inputs and exact runtime outcomes remain unchanged', async () => {
  const rows = read('../task-12b2c3/current-inputs-runtime.json') as {
    file: string;
    name: string;
    input: unknown;
    result: unknown;
  }[];
  assert.equal(rows.length, 42);
  for (const saved of rows) {
    const suite = suites.find((s) => s.file === saved.file)!;
    assert.equal(suite.kind, 'scenario');
    if (suite.kind !== 'scenario') continue;
    const entry = suite.cases.find((e) => e.name === saved.name)!;
    assert.deepEqual(canonical(entry), saved.input, saved.name + ' input');
    assert.deepEqual(await runScenario(entry), saved.result, saved.name + ' outcome');
  }
});
test('every C4 original occurrence and current input/outcome has an exact accounted mapping', async () => {
  const original = read('original-runtime.json') as { file: string; name: string }[];
  const deferred = read('../task-12b2c2/deferred-runtime.json') as { file: string; name: string }[];
  assert.deepEqual(
    original,
    deferred.filter((e) => files.includes(e.file)),
  );
  assert.equal(original.length, 42);
  const current = read('current-inputs-runtime.json') as {
    file: string;
    name: string;
    input: unknown;
    result: unknown;
  }[];
  const account = read('accounting.json') as {
    mappings: { originalName: string; currentName: string }[];
  };
  assert.equal(current.length, 42);
  assert.equal(account.mappings.length, 42);
  assert.equal(new Set(account.mappings.map((m) => m.originalName)).size, 42);
  assert.equal(new Set(account.mappings.map((m) => m.currentName)).size, 42);
  for (const entry of owned) {
    const saved = current.find((e) => e.name === entry.name)!;
    assert.ok(saved, entry.name);
    assert.deepEqual(canonical(entry), saved.input, entry.name + ' complete current input');
    assert.deepEqual(
      await runScenario(entry),
      saved.result,
      entry.name + ' complete current outcome',
    );
  }
  const aggregate = read('actions-aggregate160.json') as {
    mappings: unknown[];
    currentManifests: unknown[];
  };
  assert.equal(aggregate.mappings.length, 160);
  assert.equal(aggregate.currentManifests.length, 7);
});
test('world reads use actual current operation arguments and never synthesize owner apply or mutation facts', async () => {
  const result = await run('a-script-node-is-one-transaction');
  assert.deepEqual(result.trace.at(-1)!.engine, [
    { doc: 'a', path: 'forms.list', args: [] },
    { doc: 'a', path: 'page.annotations.list', pon: 1, args: [] },
    { doc: 'a', path: 'actions.get', args: [] },
  ]);
  const provenance = await run('script-event-provenance');
  assert.deepEqual(
    provenance.trace
      .flatMap((s) => s.engine)
      .map(object)
      .filter((e) => e.path === 'page.annotations.list')
      .map((e) => e.pon),
    [1, 1, 1, 1, 1, 1, 2, 2, 1, 1, 1, 1, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1],
  );
  assert.deepEqual(callbacks(await run('script-boot-without-a-catalog'), 'sandbox.boot'), []);
  for (const entry of owned) {
    const current = await runScenario(entry);
    assert.deepEqual(
      current.trace
        .flatMap((s) => s.engine)
        .map(object)
        .filter((e) => String(e.path).includes('apply')),
      [],
      entry.name + ' owner apply',
    );
    assert.deepEqual(
      current.trace.flatMap((s) => s.stateChanges),
      [],
      entry.name + ' state',
    );
    assert.deepEqual(
      current.trace
        .flatMap((s) => s.documentEvents)
        .map(object)
        .filter((e) => !['opened', 'closed', 'activeChanged'].includes(String(e.event))),
      [],
      entry.name + ' mutation facts',
    );
  }
});
import { createHash } from 'node:crypto';
test('source provenance, frozen archives and aggregate authority hashes remain exact', () => {
  const evidence = read('source-evidence.json') as {
    sourceHashes: Record<string, string>;
    archiveHashes: Record<string, string>;
    immutableAuthorities: Record<string, string>;
  };
  const hash = (value: Uint8Array) => createHash('sha256').update(value).digest('hex');
  for (const [path, digest] of Object.entries(evidence.sourceHashes))
    assert.equal(hash(readFileSync(new URL('../../../' + path, import.meta.url))), digest, path);
  for (const [path, digest] of Object.entries({
    ...evidence.archiveHashes,
    ...evidence.immutableAuthorities,
  }))
    assert.equal(hash(readFileSync(new URL(path, history))), digest, path);
});
test('transaction ports return literal body and detached boot/run outputs', async () => {
  const output = (uiEffects: unknown[] = []) => ({
    event: { rc: true, value: null, change: '', selStart: 0, selEnd: 0 },
    formEffects: [],
    annotEffects: [],
    uiEffects,
    diagnostics: [],
  });
  const own = await run('script-transaction-port');
  assert.deepEqual(own.trace[4]!.settled, [{ promise: 3, resolved: 'body-result' }]);
  const detached = await run('detached-script-realm');
  assert.deepEqual(detached.trace[6]!.settled, [
    {
      promise: 4,
      resolved: {
        boot: output(),
        run: output([{ kind: 'alert', message: 'hi', icon: 0 }]),
      },
    },
  ]);
  assert.deepEqual(detached.trace[7]!.settled, [{ promise: 5, resolved: 'body-result' }]);
  const noMetadata = await run('detached-script-realm-without-metadata');
  assert.deepEqual(noMetadata.trace[4]!.settled, [
    { promise: 3, resolved: { boot: null, run: output() } },
  ]);
});
test('boot without effects reads the world once and failed runs never invoke either commit sink', async () => {
  const boot = await run('boot-without-effects');
  assert.deepEqual(
    boot.trace
      .flatMap((step) => step.engine)
      .map(object)
      .filter((call) =>
        ['forms.list', 'page.annotations.list', 'actions.get'].includes(String(call.path)),
      ),
    [
      { doc: 'a', path: 'forms.list', args: [] },
      { doc: 'a', path: 'page.annotations.list', pon: 1, args: [] },
      { doc: 'a', path: 'actions.get', args: [] },
    ],
  );
  const failed = await run('run-errors-and-diagnostics');
  assert.deepEqual(
    callbacks(failed).filter((call) => call.fn.startsWith('sink.')),
    [],
  );
});
