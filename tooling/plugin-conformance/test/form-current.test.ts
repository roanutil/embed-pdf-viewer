import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/form-core';
import { definePlugin } from '@embedpdf/core';
import { registerPlugin, runScenario, type TraceEntry } from '../src/scenario';
import { suites as lifecycle } from '../src/suites/form-core-lifecycle';
import { ref, objRef, pr } from '../src/suites/form-current-fixtures';
import { generateUuidV7 } from '../../../packages/engine/core/src/identity/uuid';
import { operationAt } from '../src/suites/annotation-live-fixtures';
import { FormSnapshotSchema } from '../../../packages/engine/core/src/forms/schema';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { canonical } from '../src/canonical';

registerPlugin('form-current-entropy-probe', {
  factory: (_, env) =>
    definePlugin({
      id: 'form-current-entropy-probe',
      token: env.token('form-current-entropy-probe'),
      create: () => ({ api: { next: () => generateUuidV7() } }),
    }),
});
registerPlugin('form-current-entropy-at-factory', {
  factory: (config, env) => {
    env.log('factoryUuid', [generateUuidV7()]);
    return definePlugin({
      id: String(config),
      token: env.token(String(config)),
      create: () => ({ api: {} }),
    });
  },
});
registerPlugin('form-current-factory-failure', {
  factory: () => {
    throw new Error('intentional current fixture failure');
  },
});
test('form-only and combined factories share one repeatable UUID sequence and restore globals', async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues');
  const prototype = Object.getPrototypeOf(globalThis.crypto);
  const warn = console.warn,
    error = console.error;
  for (const factories of [
    ['interaction', 'form-current'],
    ['interaction', 'annotation-live', 'form-current'],
    ['interaction', 'form-current', 'annotation-live'],
  ]) {
    for (let repeat = 0; repeat < 2; repeat++) {
      const result = await runScenario({
        name: 'current-form-entropy',
        covers: 'One sequence shared by both current factories',
        plugins: [
          ...factories.map((factory) => ({ factory })),
          { factory: 'form-current-entropy-probe' },
        ],
        snapshot: 'none',
        reason: 'UUID host input only',
        steps: [1, 2, 3].map(() => ({
          op: 'call',
          target: { token: 'form-current-entropy-probe' },
          method: 'next',
        })),
      });
      assert.deepEqual(result.unmatched, []);
      assert.deepEqual(
        result.trace.map((t) => t.outcome),
        [1, 2, 3].map((n) => ({ returned: operationAt(n) })),
      );
      assert.deepEqual(
        Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
        descriptor,
      );
      assert.equal(Object.getPrototypeOf(globalThis.crypto), prototype);
      assert.equal(console.warn, warn);
      assert.equal(console.error, error);
    }
  }
});
test('a factory between form and annotation consumes the shared sequence without a nested reset', async () => {
  for (const [first, second] of [
    ['form-current', 'annotation-live'],
    ['annotation-live', 'form-current'],
  ] as const) {
    const result = await runScenario({
      name: 'entropy-between-factories',
      covers: 'Sequence exists before the second fixture joins',
      plugins: [
        { factory: 'interaction' },
        { factory: first },
        { factory: 'form-current-entropy-at-factory', config: 'between' },
        { factory: second },
        { factory: 'form-current-entropy-at-factory', config: 'after' },
        { factory: 'form-current-entropy-probe' },
      ],
      snapshot: 'none',
      reason: 'Factory and call UUID inputs',
      steps: [{ op: 'call', target: { token: 'form-current-entropy-probe' }, method: 'next' }],
    });
    assert.deepEqual(result.trace[0]!.callbacks, [
      { fn: 'factoryUuid', args: [operationAt(1)] },
      { fn: 'factoryUuid', args: [operationAt(2)] },
    ]);
    assert.deepEqual(result.trace[0]!.outcome, { returned: operationAt(3) });
  }
});
test('form-only and combined construction failures and early host getter errors restore exact globals', async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues');
  const prototype = Object.getPrototypeOf(globalThis.crypto),
    error = console.error,
    warn = console.warn;
  for (const factories of [
    ['interaction', 'form-current'],
    ['interaction', 'form-current', 'annotation-live'],
    ['interaction', 'annotation-live', 'form-current'],
  ]) {
    await assert.rejects(
      runScenario({
        name: 'entropy-construction-failure',
        covers: 'Factory failure after hooks',
        plugins: [
          ...factories.map((factory) => ({ factory })),
          { factory: 'form-current-factory-failure' },
        ],
        steps: [],
      }),
      /intentional current fixture failure/,
    );
    assert.deepEqual(
      Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
      descriptor,
    );
    assert.equal(Object.getPrototypeOf(globalThis.crypto), prototype);
    assert.equal(console.error, error);
    assert.equal(console.warn, warn);
  }
  const throwing = {
    configurable: true,
    enumerable: false,
    get: () => {
      throw new Error('current form entropy getter refused');
    },
  };
  try {
    Object.defineProperty(globalThis.crypto, 'getRandomValues', throwing);
    await assert.rejects(
      runScenario({
        name: 'form-entropy-getter-failure',
        covers: 'Read host before installing hooks',
        plugins: [{ factory: 'interaction' }, { factory: 'form-current' }],
        steps: [],
      }),
      /current form entropy getter refused/,
    );
    assert.deepEqual(Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'), {
      ...throwing,
      set: undefined,
    });
    assert.equal(console.error, error);
    assert.equal(console.warn, warn);
    assert.equal(Object.getPrototypeOf(globalThis.crypto), prototype);
  } finally {
    if (descriptor) Object.defineProperty(globalThis.crypto, 'getRandomValues', descriptor);
    else Reflect.deleteProperty(globalThis.crypto, 'getRandomValues');
  }
});

test('every current pure form case executes without obsolete targets or malformed ordinary inputs', () => {
  const failures: { name: string; message: string }[] = [];
  for (const suite of suites) {
    if (suite.kind !== 'function') continue;
    for (const entry of suite.cases) {
      try {
        Reflect.apply(suite.functions[entry.fn]!, undefined, entry.args);
      } catch (error) {
        failures.push({ name: entry.name, message: String(error) });
      }
    }
  }
  assert.deepEqual(failures, []);
});

const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const pure = (name: string): unknown => {
  for (const suite of suites)
    if (suite.kind === 'function') {
      const entry = suite.cases.find((c) => c.name === name);
      if (entry) return Reflect.apply(suite.functions[entry.fn]!, undefined, entry.args);
    }
  assert.fail(`missing ${name}`);
};
const returned = (trace: TraceEntry[], i: number): unknown => {
  const outcome = trace[i]!.outcome;
  assert.ok(outcome && 'returned' in outcome);
  return outcome.returned;
};
test('indexes use current refs, rebuild joins, isolate pages and preserve no-op identity', () => {
  assert.equal(pure('fieldKeyOf/object-number'), 'obj:4');
  assert.equal(pure('fieldKeyOf/fqn-when-direct'), 'fqn:loose.field');
  assert.equal(pure('fieldKeyOf/negative-object-number'), 'fqn:neg');
  assert.deepEqual(pure('initialModel/empty'), {
    snapshot: null,
    byKey: {},
    byName: {},
    byWidget: {},
    rowByWidget: {},
    pictureVersions: {},
  });
  const indexed = object(pure('update/snapshot-indexes-keys-and-widgets'));
  FormSnapshotSchema.parse(indexed.snapshot);
  assert.equal(object(indexed.byKey)['obj:4'], 0);
  assert.equal(object(indexed.byName).size, 5);
  assert.equal(object(indexed.byWidget)['110'], 5);
  assert.equal(object(indexed.rowByWidget)['104'], 0);
  assert.equal(
    object(object(pure('update/snapshot-later-field-wins-a-shared-widget')).byWidget)['50'],
    1,
  );
  for (const name of [
    'fieldByKey/unknown',
    'fieldByKey/no-snapshot',
    'fieldByKey/index-past-the-end',
    'fieldForWidget/direct-widget-is-not-indexed',
    'fieldForWidget/unknown',
    'fieldForWidget/index-past-the-end',
  ])
    assert.equal(pure(name), null, name);
  assert.equal(object(pure('fieldByKey/found')).name, 'maxlen_text');
  assert.equal(object(pure('fieldByKey/fqn')).name, 'loose.field');
  assert.equal(object(pure('fieldForWidget/found')).name, 'size');
  const replaced = object(pure('update/write-done-patches-and-reindexes'));
  assert.equal(object(replaced.byWidget)['204'], 0);
  assert.equal(object(replaced.byWidget)['104'], undefined);
  const ignored = object(pure('update/write-done-unknown-key'));
  assert.equal(object(ignored.byKey)['obj:404'], undefined);
  assert.equal(object((object(ignored.snapshot).fields as unknown[])[0]).value, 'abcde');
  const afterPageRemoval = object(object(pure('update/clear-geom-one-page')).snapshot);
  assert.deepEqual(
    (afterPageRemoval.widgets as unknown[]).map((w) => object(object(w).page).objectNumber),
    [4],
  );
  for (const name of ['update/page-geom-replaces-a-page', 'update/clear-geom-all'])
    assert.deepEqual(pure(name), {});
  assert.deepEqual(pure('initialFormState/empty'), {});
  assert.equal(pure('fieldRefByObjectNumber/object-number'), 'obj:12');
  assert.deepEqual(pure('fieldRefByName/fqn'), { kind: 'fqn', name: 'parent.child' });
});
test('predictions carry real values and undo restores the confirmed field and widget rows', () => {
  const start = object(object(pure('update/write-start')).snapshot);
  assert.equal(object((start.fields as unknown[])[0]).value, 'abcde');
  const second = object(object(pure('update/write-start-twice')).snapshot);
  assert.deepEqual(
    (second.fields as unknown[]).slice(0, 2).map((f) => object(f).value),
    ['abcde', 'y'],
  );
  const undo = pure('update/write-failed') as unknown[];
  assert.equal(undo.length, 1);
  assert.equal(object(undo[0]).type, 'forms.restore');
  assert.equal(object(object(undo[0]).field).value, 'abc');
  assert.equal((object(undo[0]).widgets as unknown[]).length, 1);
  assert.deepEqual(pure('update/write-failed-not-writing'), []);
});
test('projection preserves every family, explicit geometry, permissions, toggle state and signature state', () => {
  const get = (label: string) => object(pure(`projectWidget/${label}`));
  assert.deepEqual(
    {
      key: get('text').key,
      value: get('text').value,
      label: get('text').label,
      box: get('text').box,
      annotationRef: get('text').annotationRef,
    },
    {
      key: 'obj:4',
      value: 'abc',
      label: 'Max length',
      box: { x: 1, y: 2, width: 20, height: 10 },
      annotationRef: objRef(104, 3),
    },
  );
  assert.equal(get('text-flags').multiline, true);
  assert.equal(get('text-flags').password, true);
  assert.equal(get('text-flags').comb, true);
  assert.equal(get('read-only-is-disabled').disabled, true);
  assert.equal(get('writing-is-disabled').disabled, true);
  assert.equal(get('checkbox-on-state').exportValue, 'On');
  assert.equal(get('checkbox-on-state').checked, true);
  assert.equal(get('checkbox-default-on-state').exportValue, 'Yes');
  assert.equal(get('checkbox-unknown-widget').annotationRef, null);
  for (const [label, checked, exportValue] of [
    ['radio-checked', true, 'M'],
    ['radio-unchecked', false, 'S'],
    ['radio-bare-widget', false, ''],
    ['radio-unknown-widget', false, ''],
  ] as const) {
    assert.equal(get(label).checked, checked);
    assert.equal(get(label).exportValue, exportValue);
  }
  assert.deepEqual(get('combobox').selected, ['r']);
  assert.equal(get('combobox').edit, true);
  assert.deepEqual(get('combobox-empty').selected, []);
  assert.deepEqual(get('listbox').selected, ['r', 'g']);
  assert.equal(get('listbox').multi, true);
  assert.equal(get('listbox-single').multi, false);
  assert.equal(get('pushbutton').control, 'button');
  assert.equal(pure('projectWidget/unknown-has-no-control'), null);
  assert.equal(get('signature-has-no-control').control, 'signature');
  assert.equal(get('signature-unsigned').signed, false);
  assert.equal(get('signature-signed').signed, true);
  assert.equal(get('fqn-key').key, 'fqn:loose.field');
  const pending = pure('fillItems/writing') as unknown[];
  assert.equal(object(pending[0]).value, 'abcde');
  assert.equal(object(pending[0]).disabled, false);
  for (const label of ['page-without-geometry', 'no-snapshot'])
    assert.deepEqual(pure(`fillItems/${label}`), []);
  assert.deepEqual(
    (pure('fillItems/partial-geometry') as unknown[]).map((i) => object(i).annotObjectNumber),
    [110],
  );
  for (const label of [
    'without-geometry',
    'other-page-geometry',
    'unknown',
    'index-without-the-widget',
    'an-unplaced-widget-has-no-box',
  ])
    assert.equal(pure(`fillItemForWidget/${label}`), null);
  for (const [label, number] of [
    ['the-smallest-containing-box-wins', 105],
    ['edges-are-inside', 104],
    ['a-box-without-a-field-is-skipped', 104],
    ['equal-areas-keep-the-lower-object-number', 104],
    ['a-larger-box-later-loses', 105],
  ] as const)
    assert.equal(object(pure(`widgetAt/${label}`)).annotObjectNumber, number);
  assert.equal(pure('widgetAt/no-geometry-for-the-page'), null);
  assert.equal(pure('widgetAt/nothing-under-the-point'), null);
});
test('ISO selection and submission preserve exact entries, NoExport vetoes and named malformed negatives', () => {
  const selected = (label: string) =>
    (object(pure(`resolveFieldSelection/${label}`)).selected as unknown[]).map(
      (f) => object(f).name,
    );
  assert.deepEqual(selected('a-parent-name-selects-descendants'), ['parent.c1', 'parent.c2']);
  assert.deepEqual(selected('exclude-a-subtree'), ['parentheses', 'other', 'byNumber', 'fqnRef']);
  assert.deepEqual(selected('object-number'), ['byNumber']);
  assert.deepEqual(selected('object-number-never-matches-an-fqn-ref'), []);
  assert.deepEqual(selected('a-numberless-target-finds-a-numberless-ref'), ['bare']);
  assert.deepEqual(selected('mixed-targets'), ['parentheses', 'byNumber']);
  assert.equal(selected('null-selects-all').length, 6);
  assert.equal(selected('null-ignores-exclude').length, 6);
  assert.deepEqual(selected('empty-include-selects-nothing'), []);
  assert.equal(selected('empty-exclude-selects-all').length, 6);
  const submit = (label: string) => object(pure(`buildSubmitEntries/${label}`));
  assert.deepEqual(submit('fields-absent-silently-drops-ineligible'), {
    entries: [{ name: 'plain', value: 'plain-value' }],
    diagnostics: [],
  });
  assert.deepEqual(submit('valueless-skipped-by-default'), { entries: [], diagnostics: [] });
  assert.deepEqual(submit('valueless-name-only-when-included'), {
    entries: [{ name: 'empty', value: null }],
    diagnostics: [],
  });
  assert.deepEqual(submit('descendants-and-arrays'), {
    entries: [
      { name: 'parent.c1', value: 'parent.c1-value' },
      { name: 'parent.c2', value: ['a', 'b'] },
    ],
    diagnostics: [],
  });
  for (const [label, messages] of [
    [
      'explicit-no-export-is-diagnosed',
      [
        "submit: 'secret' carries the NoExport flag — the veto is unconditional (ISO 12.7.6.2), skipped",
      ],
    ],
    [
      'explicit-button-and-signature-are-diagnosed',
      [
        "submit: 'button' (pushbutton) has no representable submission value in v1 — skipped",
        "submit: 'sig' (signature) has no representable submission value in v1 — skipped",
      ],
    ],
    [
      'unsupported-value-shape-is-always-diagnosed',
      [
        "submit: 'weird' has an unsupported /V shape — skipped (would otherwise be silent data loss)",
      ],
    ],
    [
      'unknown-value-kind-is-diagnosed',
      ["submit: 'odd' has an unsupported /V shape — skipped (would otherwise be silent data loss)"],
    ],
  ] as const)
    assert.deepEqual(
      submit(label).diagnostics,
      messages.map((message) => ({ code: 'submit-entry-unsupported', message })),
    );
  assert.deepEqual(submit('exclude-mode-is-never-explicit'), { entries: [], diagnostics: [] });
});

for (const suite of lifecycle)
  for (const input of suite.cases)
    test(input.name, async () => {
      const result = await runScenario(input);
      assert.deepEqual(result.unmatched, []);
      assert.deepEqual(
        result.trace.flatMap((e, i) =>
          e.outcome && 'threw' in e.outcome ? [i, e.outcome.threw] : [],
        ),
        [],
      );
      assert.deepEqual(
        result.trace.flatMap((e) => e.reports),
        [],
      );
      assert.deepEqual(
        result.trace.flatMap((e) => e.stateChanges),
        [],
        'fields and status are mirror data, never session state',
      );
      if (input.name !== 'current-form-confirmation-refusal-and-permission') {
        assert.deepEqual(
          result.trace.flatMap((e) => e.settled.filter((s) => s.rejected)),
          [],
        );
        assert.deepEqual(
          result.trace.flatMap((e) => e.callbacks),
          [],
        );
      }
      if (input.name === 'formReducer/set-status') {
        assert.equal(returned(result.trace, 2), 'loading');
        assert.equal(returned(result.trace, 3), null);
        assert.equal(returned(result.trace, 5), 'ready');
        assert.equal((object(returned(result.trace, 6)).widgets as unknown[]).length, 1);
      } else if (input.name === 'formReducer/set-status-unchanged-is-no-change') {
        assert.equal(returned(result.trace, 2), 'ready');
        assert.equal(returned(result.trace, 4), 'ready');
        assert.equal(
          result.trace.flatMap((e) => e.engine).filter((e) => object(e).path === 'forms.list')
            .length,
          2,
        );
      } else {
        assert.equal(returned(result.trace, 3), true);
        for (const [i, value] of [
          [4, 'Ada'],
          [6, 'Grace'],
          [9, 'Grace'],
          [11, 'refused'],
          [13, 'Grace'],
          [17, 'Ada'],
        ] as const)
          assert.deepEqual(returned(result.trace, i), { value });
        assert.equal(object((returned(result.trace, 7) as unknown[])[0]).disabled, false);
        assert.deepEqual(
          result.trace[8]!.settled.map((s) => s.rejected),
          [undefined],
          'successful confirmation must resolve',
        );
        assert.equal(object(result.trace[8]!.settled[0]!.resolved).status, 'applied');
        const changes = result.trace
          .flatMap((e) => e.callbacks)
          .filter((c) => c.fn === 'valueChanged');
        assert.equal(changes.length, 1);
        assert.equal(object(object(changes[0]!.args[0]).field).value, 'Grace');
        assert.deepEqual(object(changes[0]!.args[0]).origin, {
          kind: 'local',
          sessionId: 's1',
          sub: null,
          ts: 0,
          serverId: null,
          tx: { id: operationAt(1), index: 0, count: 1 },
        });
        assert.equal(returned(result.trace, 15), false);
        assert.equal(result.trace[16]!.engine.length, 0);
        const rejected = result.trace.flatMap((e, i) =>
          e.settled
            .filter((s) => s.rejected)
            .map((s) => [i, object(object(s.rejected).$error).message]),
        );
        assert.deepEqual(rejected, [
          [12, 'intended form refusal'],
          [16, 'form.setValue requires doc.forms.fill'],
        ]);
        assert.deepEqual(result.trace[12]!.settled[0]!.rejected, {
          $error: {
            name: 'PluginError',
            message: 'intended form refusal',
            code: 'operation-failed',
            capability: 'form',
            permission: null,
            cause: {
              $error: { name: 'EngineError', message: 'intended form refusal', code: 'Refused' },
            },
          },
        });
        assert.deepEqual(result.trace[16]!.settled[0]!.rejected, {
          $error: {
            name: 'PluginError',
            message: 'form.setValue requires doc.forms.fill',
            code: 'permission-denied',
            capability: 'form',
            permission: 'doc.forms.fill',
          },
        });
        assert.deepEqual(returned(result.trace, 18), { valid: true, missing: [] });
        assert.deepEqual(returned(result.trace, 19), { name: 'Grace' });
        const writes = result.trace
          .flatMap((e) => e.engine)
          .filter((e) => object(e).path === 'apply');
        assert.equal(writes.length, 2);
        for (const [i, value] of ['Grace', 'refused'].entries())
          assert.deepEqual(object(writes[i]).args, [
            { ops: [{ type: 'forms.setValue', field: ref(1), value: { value } }] },
            { opId: operationAt(i + 1) },
          ]);
      }
    });
test('all 220 original definitions are archived, all 101 owned cases map exactly and the 59 deferred definitions plus legacy helper stay byte-identical', async () => {
  const root = new URL('../', import.meta.url),
    history = new URL('historical/task-12b2b2-i/', root);
  const original = JSON.parse(await readFile(new URL('original-inputs.json', history), 'utf8')) as {
    file: string;
    name: string;
    input: unknown;
  }[];
  const baseline = JSON.parse(await readFile(new URL('baseline.json', history), 'utf8')) as {
    sources: { path: string; sha256: string }[];
  };
  const accounting = JSON.parse(await readFile(new URL('accounting.json', history), 'utf8')) as {
    originalWholeForms: number;
    originalOwned: number;
    currentOwned: number;
    currentPure: number;
    currentLive: number;
    mapped: number;
    renamed: number;
    retired: unknown[];
    mappings: {
      before: { file: string; name: string; canonicalInputSha256: string };
      after: { file: string; name: string; canonicalInputSha256: string };
      task11InputDrift: boolean;
    }[];
    added: { file: string; name: string; canonicalInputSha256: string }[];
  };
  const digest = (value: string | Uint8Array) => createHash('sha256').update(value).digest('hex');
  assert.equal(original.length, 220);
  assert.equal(new Set(original.map((r) => r.file + ':' + r.name)).size, 220);
  assert.deepEqual(
    [
      accounting.originalWholeForms,
      accounting.originalOwned,
      accounting.currentOwned,
      accounting.currentPure,
      accounting.currentLive,
      accounting.mapped,
      accounting.renamed,
      accounting.retired.length,
      accounting.added.length,
    ],
    [220, 101, 102, 99, 3, 101, 0, 0, 1],
  );
  const current = [...suites, ...lifecycle].flatMap((s) =>
    s.cases.map((input) => ({ file: s.file, name: input.name, input })),
  );
  assert.equal(new Set(current.map((r) => r.file + ':' + r.name)).size, 102);
  for (const mapping of accounting.mappings) {
    const before = original.find(
      (r) => r.file === mapping.before.file && r.name === mapping.before.name,
    );
    assert.ok(before);
    const after = current.find(
      (r) => r.file === mapping.after.file && r.name === mapping.after.name,
    );
    assert.ok(after);
    assert.equal(digest(JSON.stringify(before.input)), mapping.before.canonicalInputSha256);
    assert.equal(
      digest(JSON.stringify(canonical(after.input))),
      mapping.after.canonicalInputSha256,
    );
    assert.equal(mapping.task11InputDrift, false);
  }
  for (const addition of accounting.added) {
    const after = current.find((r) => r.file === addition.file && r.name === addition.name);
    assert.ok(after);
    assert.equal(digest(JSON.stringify(canonical(after.input))), addition.canonicalInputSha256);
  }
  for (const source of baseline.sources) {
    const filename = source.path.split('/').at(-1)!;
    assert.equal(
      digest(await readFile(new URL(filename + '.txt', history))),
      source.sha256,
      filename + ' archive',
    );
    if (['form-scripting.ts', 'form-actions.ts', 'form-fixtures.ts'].includes(filename))
      assert.equal(
        digest(await readFile(new URL('src/suites/' + filename, root))),
        source.sha256,
        filename + ' deferred bytes',
      );
  }
});
test('form-only refusal traces repeat exactly and restore the crypto and console host', async () => {
  const input = lifecycle
    .flatMap((s) => s.cases)
    .find((c) => c.name === 'current-form-confirmation-refusal-and-permission');
  assert.ok(input && 'steps' in input);
  const descriptor = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
    prototype = Object.getPrototypeOf(globalThis.crypto);
  const error = console.error,
    warn = console.warn;
  const first = await runScenario(input),
    second = await runScenario(input);
  assert.deepEqual(second, first);
  assert.deepEqual(first.unmatched, []);
  assert.deepEqual(
    Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
    descriptor,
  );
  assert.equal(Object.getPrototypeOf(globalThis.crypto), prototype);
  assert.equal(console.warn, warn);
  assert.equal(console.error, error);
});
