process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { suites } from '../src/suites/i18n';
import { runScenario } from '../src/scenario';
test('every i18n occurrence has a unique name including repeated plural tag sources', () => {
  for (const s of suites)
    assert.equal(new Set(s.cases.map((c) => c.name)).size, s.cases.length, s.file);
});
test('all pure cases invoke current source arguments without accidental throws', () => {
  for (const s of suites)
    if (s.kind === 'function')
      for (const c of s.cases) {
        assert.doesNotThrow(
          () => (s.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args),
          c.name,
        );
      }
});
test('Arabic direction uses the current Locale direction field', async () => {
  const s = suites[2];
  assert.ok(s.kind === 'scenario');
  const r = await runScenario(s.cases.find((c) => c.name === 'dir-and-locales')!);
  assert.deepEqual(r.trace[1].outcome, { returned: 'rtl' });
});
import { canonical, snapshotHooks } from '../src/canonical';
import type { TraceEntry } from '../src/scenario';
import { createTestContext } from '../../../packages/core/main/src/testing';
import { createI18nController } from '../../../packages/plugin/i18n/src/controller';
import { I18N_DEFAULTS, type Locale } from '../../../packages/plugin/i18n/src/contract';
import * as model from '../../../packages/plugin/i18n/src/model';
const pure = (name: string): unknown => {
  for (const s of suites)
    if (s.kind === 'function') {
      const c = s.cases.find((c) => c.name === name);
      if (c) return (s.functions[c.fn] as (...a: readonly unknown[]) => unknown)(...c.args);
    }
  throw Error(name);
};
const live = suites[2];
if (live.kind !== 'scenario') throw Error('scenario');
const run = (name: string) => runScenario(live.cases.find((c) => c.name === name)!);
const value = (t: TraceEntry) =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const callbacks = (r: Awaited<ReturnType<typeof run>>, fn: string) =>
  r.trace.flatMap((t) => t.callbacks.filter((c) => c.fn === fn));
test('translation lookup independently resolves app, bundled, fallback key, fallback language and plural branches', () => {
  const expected: Record<string, [string, boolean]> = {
    'translate-current-locale': ['Guardar', true],
    'translate-fallback-locale': ['Acercar', true],
    'translate-options-fallback': ['Hi Bob', false],
    'translate-options-fallback-no-params': ['Hi {name}', false],
    'translate-key-itself': ['nope.nothing', false],
    'translate-empty-key': ['', false],
    'translate-interpolates': ['Zoom Level (150%)', true],
    'translate-plural-one': ['1 page', true],
    'translate-plural-other': ['5 pages', true],
    'translate-plural-in-current-locale': ['1 página', true],
    'translate-branch-without-count': ['pages', false],
    'translate-branch-with-string-count': ['pages', false],
    'translate-branch-missing-category-uses-other': ['2 x', true],
    'translate-branch-with-no-other-falls-through': ['3 páginas', true],
    'translate-branch-category-is-a-branch': ['pages', false],
    'translate-walks-through-a-leaf': ['commands.save.more', false],
    'translate-unregistered-current-locale': ['Save', true],
    'translate-unregistered-both': ['commands.save', false],
    'translate-fallback-plural-uses-fallback-locale-rules': ['صفحتان', true],
    'translate-nan-count': ['NaN pages', true],
    'translate-fallback-key-in-current-language': ['Guardar', true],
    'translate-fallback-key-in-fallback-language': ['Zoom In', true],
    'translate-bundled-strings-before-fallback-app': ['Alejar', true],
  };
  for (const [name, [text, found]] of Object.entries(expected))
    assert.deepEqual(pure(name), { text, found }, name);
});
test('interpolation keeps unknown placeholders, grammar and replacement dollars literal', () => {
  assert.equal(pure('interpolate-unknown-slot-verbatim'), 'x {b}');
  assert.equal(pure('interpolate-slot-grammar'), 'uUn{}{a-b}{ a}{é}{x}{a');
  assert.equal(pure('interpolate-dollar-patterns'), '$& $1 $$');
  assert.equal(pure('interpolate-repeated'), '{a}{a}{a}');
});
test('merge and dotted nesting preserve siblings and insertion precedence without modifying their inputs', () => {
  assert.deepEqual(pure('merge-translations-replaces-leaves-and-merges-branches'), {
    a: { b: 'new', c: 'C' },
    flat: { leaf: 'L' },
  });
  assert.deepEqual(pure('nest-keys-colliding-branches-in-insertion-order'), {
    review: { accept: 'Approve', reject: 'Reject' },
  });
  assert.deepEqual(pure('nest-keys-with-nested-dotted-leaves'), {
    review: { status: { ready: 'Ready', done: 'Done' } },
  });
  const dictionary = { nested: { key: 'V' } };
  assert.equal(model.nestKeys(dictionary), dictionary);
  const patch = { nested: { second: 'S' } };
  assert.deepEqual(model.mergeTranslations(dictionary, patch), {
    nested: { key: 'V', second: 'S' },
  });
  assert.deepEqual(dictionary, { nested: { key: 'V' } });
  assert.deepEqual(patch, { nested: { second: 'S' } });
});
test('transitions preserve identity for ignored calls and expose exact waiting, seeded and removal state', () => {
  const en: Locale = { code: 'en', name: 'English', translations: { hi: 'Hello' } };
  const state = { ...model.initialI18nState(), locale: 'en', locales: { en } };
  assert.equal(model.setLocale(state, 'de'), state);
  assert.equal(model.setLocale(state, 'en'), state);
  assert.equal(model.unregisterLocale(state, 'de'), state);
  assert.equal(model.endLocaleLoad(state, 'de'), state);
  const loading = model.startLocaleLoad(state, 'nl');
  assert.equal(model.startLocaleLoad(loading, 'nl'), loading);
  assert.deepEqual(loading, { ...state, loading: 'nl' });
  assert.deepEqual(model.endLocaleLoad(loading, 'nl'), state);
  assert.deepEqual(model.unregisterLocale(state, 'en'), { ...state, locales: {} });
  const waiting = model.addTranslations(state, 'nl', { 'review.accept': 'App' });
  assert.deepEqual(waiting, {
    ...state,
    waitingTranslations: { nl: { review: { accept: 'App' } } },
  });
  const pack: Locale = {
    code: 'nl',
    name: 'Nederlands',
    translations: { 'review.accept': 'Pack', 'review.reject': 'Reject' },
  };
  assert.deepEqual(model.registerLocale(waiting, pack), {
    ...state,
    locales: { en, nl: { ...pack, translations: { review: { accept: 'App', reject: 'Reject' } } } },
  });
  assert.deepEqual(pure('initial-defaults'), {
    locale: 'en',
    locales: {},
    loading: null,
    waitingTranslations: {},
  });
  const startup = pure('initial-lazy-startup-seeds-loading') as model.I18nState;
  assert.equal(startup.locale, 'en');
  assert.equal(startup.loading, 'ar');
  assert.deepEqual(startup.waitingTranslations, {});
});
test('plural rows preserve all counts and independently known Arabic, Russian, English and invalid-tag results', () => {
  const s = suites[1];
  assert.ok(s.kind === 'function');
  const plural = s.functions.plurals as (tag: string, counts: number[]) => string[];
  assert.deepEqual(plural('ar', [0, 1, 2, 3, 11, 100]), [
    'zero',
    'one',
    'two',
    'few',
    'many',
    'other',
  ]);
  assert.deepEqual(plural('ru', [1, 2, 5, 21, 22, 25, 1.5]), [
    'one',
    'few',
    'many',
    'one',
    'few',
    'many',
    'other',
  ]);
  assert.deepEqual(plural('en', [0, 1, 2, -1, NaN, Infinity]), [
    'other',
    'one',
    'other',
    'one',
    'other',
    'other',
  ]);
  assert.deepEqual(plural('en_US', [0, 1, 2, -1]), ['other', 'one', 'other', 'other']);
  assert.equal(s.cases.length, 532);
  for (const c of s.cases) {
    assert.equal((c.args[1] as number[]).length, 50);
    assert.equal((pure(c.name) as unknown[]).length, 50);
  }
});
test('capability translation, bundle precedence, warn-once and runtime removal use actual results', async () => {
  const r = await run('translate-through-the-capability');
  assert.deepEqual(r.trace.slice(1, 14).map(value), [
    'حفظ',
    'Zoom Out',
    'Zoom Level (75%)',
    'لا صفحات',
    'صفحتان',
    '11 صفحة',
    'pages',
    'pages',
    'Hello Ann, you have 3 {unknown}',
    'Default 1',
    '',
    'missing.key',
    'missing.key',
  ]);
  assert.deepEqual(
    callbacks(r, 'console.warn'),
    [
      '[i18n] missing translation "pages" (locale: ar)',
      '[i18n] missing translation "missing.key" (locale: ar)',
      '[i18n] missing translation "another" (locale: es)',
    ].map((message) => ({ fn: 'console.warn', args: [message] })),
  );
  const removed = await run('register-returns-a-remover');
  assert.deepEqual(
    [4, 5, 9, 10, 13, 17].map((i) => value(removed.trace[i])),
    ['ar', 'rtl', 'ar', 'ltr', 'Save', 'ar'],
  );
  assert.deepEqual(value(removed.trace[15]), [
    { code: 'en', name: 'English', direction: 'ltr', loaded: true },
    { code: 'he', name: 'he', direction: 'ltr', loaded: false },
  ]);
  const add = await run('add-translations-and-has-key');
  assert.deepEqual(
    [1, 2, 3, 4, 6, 7, 8, 9, 11, 16, 17].map((i) => value(add.trace[i])),
    [true, true, false, false, 'Ajustar', 'Acercar', 'Alejar', true, 'Alejar', 'b', 'Pending save'],
  );
});
test('lazy dedup replaces only the waiter and applies the loaded pack at the exact settle turn', async () => {
  const r = await run('lazy-switch');
  assert.deepEqual(callbacks(r, 'load-ar'), [{ fn: 'load-ar', args: [] }]);
  assert.deepEqual(
    [2, 3, 4, 7, 9, 10, 11, 13, 16].map((i) => value(r.trace[i])),
    ['en', 'ltr', 'ar', 'Save', 'ar', 'rtl', null, 'حفظ', 'ar'],
  );
  assert.deepEqual(r.trace[6].settled, [{ promise: 1, rejected: cancelled('ar') }]);
  assert.deepEqual(r.trace[8].settled, [{ promise: 2, resolved: { $undefined: true } }]);
  const race = await run('lazy-two-in-flight');
  assert.deepEqual(
    race.trace.flatMap((t) => t.callbacks),
    [
      { fn: 'load-ar', args: [] },
      { fn: 'load-he', args: [] },
    ],
  );
  assert.deepEqual(
    [3, 5, 6, 7, 10, 11, 12].map((i) => value(race.trace[i])),
    ['he', 'he', 'rtl', null, 'he', 'rtl', null],
  );
  assert.deepEqual(value(race.trace[13]), [
    { code: 'en', name: 'English', direction: 'ltr', loaded: true },
    { code: 'he', name: 'עברית', direction: 'rtl', loaded: true },
    { code: 'ar', name: 'العربية', direction: 'rtl', loaded: true },
  ]);
});
test('startup fallback, immediate loads, mismatch codes and unregister replacements have successful witnesses', async () => {
  const r = await run('lazy-startup-locale');
  assert.deepEqual(
    [0, 1, 2, 4, 6, 8, 10, 12, 13, 14, 16].map((i) => value(r.trace[i])),
    ['en', 'ltr', 'ar', 'Save', 'en', 'ar', 'Save', 'ar', 'rtl', null, 'حفظ'],
  );
  assert.deepEqual(r.trace[5].callbacks, [{ fn: 'load-ar', args: [] }]);
  const immediate = await run('lazy-resolves-immediately');
  assert.deepEqual(immediate.trace.slice(2, 5).map(value), ['he', 'rtl', null]);
  const mismatch = await run('lazy-pack-with-a-different-code');
  assert.deepEqual(callbacks(mismatch, 'load-ar'), [
    { fn: 'load-ar', args: [] },
    { fn: 'load-ar', args: [] },
  ]);
  assert.deepEqual(
    [2, 3, 4, 7, 8, 9].map((i) => value(mismatch.trace[i])),
    ['ar-EG', 'rtl', null, 'ar-EG', 'rtl', null],
  );
  const registration = await run('register-at-runtime');
  assert.deepEqual(
    [5, 6, 10, 11, 14, 16].map((i) => value(registration.trace[i])),
    ['he', 'rtl', 'he', 'ltr', 'Save (he)', 'Zoom in'],
  );
});
const error = (message: string) => ({ $error: { name: 'Error', message } });
const pluginError = (code: string, message: string, details?: unknown) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'i18n',
    permission: null,
    ...(details === undefined ? {} : { details }),
  },
});
const unknown = (code: string) =>
  pluginError('not-found', `unknown locale '${code}': not loaded and not in loaders`);
const cancelled = (code: string) =>
  pluginError('operation-cancelled', `switching to '${code}' was replaced`);
const failed = (code: string, message: string) =>
  pluginError('operation-failed', `locale '${code}' failed to load`, {
    code: 'operation-failed',
    message,
    capability: 'i18n',
    permission: null,
  });
test('locale change and failure events have exact payloads, loader order and unsubscribe silence', async () => {
  const r = await run('locale-events');
  assert.deepEqual(
    r.trace.flatMap((t) => t.callbacks),
    [
      { fn: 'changed', args: [{ locale: 'es', previousLocale: 'en' }] },
      { fn: 'load-ar', args: [] },
      { fn: 'changed', args: [{ locale: 'ar', previousLocale: 'es' }] },
      { fn: 'load-he', args: [] },
      {
        fn: 'failed',
        args: [
          {
            locale: 'he',
            error: {
              code: 'operation-failed',
              message: 'offline',
              capability: 'i18n',
              permission: null,
            },
          },
        ],
      },
      { fn: 'load-he', args: [] },
      { fn: 'changed', args: [{ locale: 'en', previousLocale: 'ar' }] },
    ],
  );
  assert.deepEqual(r.trace[8].settled, [{ promise: 4, rejected: failed('he', 'offline') }]);
  assert.equal(value(r.trace.at(-1)!), 'es');
  const startup = await run('lazy-startup-locale-announces-nothing');
  assert.deepEqual(callbacks(startup, 'changed'), [
    { fn: 'changed', args: [{ locale: 'ar', previousLocale: 'en' }] },
    { fn: 'changed', args: [{ locale: 'en', previousLocale: 'ar' }] },
  ]);
});
test('every intentional negative channel is exact and all other diagnostics are absent', async () => {
  const throws: Record<string, Record<number, unknown>> = {
    'no-config': { 0: error('[scenario] "capability-check-is-not-a-member" is not a function') },
    'alive-before-start-and-across-documents': {
      12: error('[kernel] capability("i18n") on a destroyed kernel'),
    },
    'loader-throws-at-startup': Object.fromEntries(
      [1, 2, 3, 4].map((i) => [i, error('[kernel] capability("i18n") on a failed kernel')]),
    ),
    'add-translations-and-has-key': { 12: unknown('xx') },
    'destroy-releases-the-watch': Object.fromEntries(
      [4, 5, 6, 7, 8].map((i) => [i, error('[kernel] capability("i18n") on a destroyed kernel')]),
    ),
  };
  const rejects: Record<string, [number, number, unknown][]> = {
    'set-locale-unknown-warns-once': [
      [1, 1, unknown('de')],
      [2, 2, unknown('de')],
      [3, 3, unknown('it')],
      [4, 4, unknown('')],
    ],
    'lazy-switch': [[6, 1, cancelled('ar')]],
    'lazy-switch-superseded': [
      [2, 1, cancelled('ar')],
      [9, 4, unknown('fr')],
      [10, 3, cancelled('he')],
    ],
    'lazy-two-in-flight': [[2, 1, cancelled('ar')]],
    'lazy-failure': [
      [1, 1, failed('ar', 'offline')],
      [7, 2, cancelled('he')],
    ],
    'loader-throws-at-startup': [[0, 0, error('no loader today')]],
    'locale-events': [
      [8, 4, failed('he', 'offline')],
      [10, 5, cancelled('he')],
      [12, 7, unknown('xx')],
    ],
  };
  const reports: Record<string, [number, unknown][]> = {
    'loader-throws-in-the-watch': [[1, error('no loader today')]],
    'destroy-releases-the-watch': [
      [
        3,
        pluginError(
          'instance-closed',
          'a state update after the instance closed (i18n#2); dropped',
        ),
      ],
    ],
  };
  for (const c of live.cases) {
    const r = await runScenario(c);
    assert.deepEqual(r.unmatched, [], c.name);
    const warnings: Record<string, [number, string][]> = {
      'no-config': [
        [5, '[i18n] missing translation "a.b" (locale: en)'],
        [7, '[i18n] missing translation "c" (locale: en)'],
      ],
      'translate-through-the-capability': [
        [7, '[i18n] missing translation "pages" (locale: ar)'],
        [12, '[i18n] missing translation "missing.key" (locale: ar)'],
        [16, '[i18n] missing translation "another" (locale: es)'],
      ],
    };
    assert.deepEqual(
      r.trace.flatMap((t, i) =>
        t.callbacks.filter((c) => c.fn === 'console.warn').map((c) => [i, c.args]),
      ),
      (warnings[c.name] ?? []).map(([i, message]) => [i, [message]]),
      c.name + ' warnings',
    );
    assert.deepEqual(
      callbacks(r, 'failed'),
      c.name === 'locale-events'
        ? [
            {
              fn: 'failed',
              args: [
                {
                  locale: 'he',
                  error: {
                    code: 'operation-failed',
                    message: 'offline',
                    capability: 'i18n',
                    permission: null,
                  },
                },
              ],
            },
          ]
        : [],
      c.name + ' failure callbacks',
    );

    assert.deepEqual(
      Object.fromEntries(
        r.trace.flatMap((t, i) =>
          t.outcome && 'threw' in t.outcome ? [[i, t.outcome.threw]] : [],
        ),
      ),
      throws[c.name] ?? {},
      c.name + ' throws',
    );
    assert.deepEqual(
      r.trace.flatMap((t, i) =>
        t.settled.flatMap((s) => ('rejected' in s ? [[i, s.promise, s.rejected]] : [])),
      ),
      rejects[c.name] ?? [],
      c.name + ' rejects',
    );
    assert.deepEqual(
      r.trace.flatMap((t, i) => t.reports.map((p) => [i, p])),
      reports[c.name] ?? [],
      c.name + ' reports',
    );
    for (const t of r.trace) {
      if (t.outcome && 'returned' in t.outcome)
        assert.ok(
          !JSON.stringify(t.outcome.returned)?.includes('"$error"'),
          c.name + ' returned diagnostic',
        );
      for (const s of t.settled)
        if ('resolved' in s)
          assert.ok(
            !JSON.stringify(s.resolved)?.includes('"$error"'),
            c.name + ' resolved diagnostic',
          );
      for (const call of t.callbacks)
        if (call.fn !== 'failed')
          assert.ok(
            !JSON.stringify(call.args).includes('"$error"'),
            c.name + ' callback diagnostic',
          );
    }
  }
});
const english: Locale = {
  code: 'en',
  name: 'English',
  translations: { hi: 'Hello', only: 'English only' },
};
const dutch: Locale = { code: 'nl', name: 'Nederlands', translations: { hi: 'Hallo' } };
const context = (
  registered: NonNullable<
    Parameters<typeof createTestContext<model.I18nState, typeof I18N_DEFAULTS>>[0]
  >['settings'],
) => {
  const ctx = createTestContext({
    id: 'i18n',
    state: model.initialI18nState(),
    settings: registered,
    doc: null,
  });
  return { ctx, api: ctx.connect(createI18nController(ctx)) };
};
test('immutable settings follow locale, fallback, loader replacement and reset while memoized reads track only their inputs', async () => {
  const h = context({
    defaults: I18N_DEFAULTS,
    registered: { locales: [english], loaders: { nl: async () => dutch } },
  });
  try {
    const initial = h.api.getTranslator(),
      list = h.api.listLocales();
    assert.equal(h.api.getTranslator(), initial);
    assert.equal(h.api.listLocales(), list);
    h.api.updateSettings({ locales: [english, dutch] });
    assert.notEqual(h.api.listLocales(), list);
    h.api.updateSettings({ locale: 'nl' });
    assert.equal(h.api.getLocale(), 'nl');
    assert.equal(h.api.t('hi'), 'Hallo');
    assert.equal(h.api.t('only'), 'English only');
    const translator = h.api.getTranslator();
    assert.notEqual(translator, initial);
    assert.equal(h.api.getTranslator(), translator);
    h.api.updateSettings({ fallbackLocale: 'nl' });
    assert.equal(h.api.hasKey('only'), false);
    assert.notEqual(h.api.getTranslator(), translator);
    h.api.resetSettings();
    assert.equal(h.api.getLocale(), 'en');
    assert.deepEqual(h.api.getSettings().locales, [english]);
    assert.deepEqual(h.api.listLocales(), [
      { code: 'en', name: 'English', direction: 'ltr', loaded: true },
      { code: 'nl', name: 'nl', direction: 'ltr', loaded: false },
    ]);
    let calls = 0;
    h.api.updateSettings({
      loaders: {
        nl: async () => {
          calls++;
          return dutch;
        },
      },
    });
    await h.api.setLocale('nl');
    assert.equal(calls, 1);
    assert.equal(h.api.t('hi'), 'Hallo');
  } finally {
    h.ctx.dispose();
  }
});
test('cancellation stops the waiter, preserves late registration, and an already aborted signal never calls the loader', async () => {
  let resolve!: (value: Locale) => void;
  let calls = 0;
  const pack = new Promise<Locale>((r) => {
    resolve = r;
  });
  const h = context({
    defaults: I18N_DEFAULTS,
    registered: {
      locales: [english],
      loaders: {
        nl: () => {
          calls++;
          return pack;
        },
      },
    },
  });
  try {
    const signal = new AbortController(),
      events: unknown[] = [];
    h.api.onLocaleChanged((e) => events.push(e));
    const before = h.api.getTranslator();
    const pending = h.api.setLocale('nl', { signal: signal.signal });
    assert.equal(h.api.getTranslator(), before);
    assert.equal(h.api.getLoadingLocale(), 'nl');
    assert.equal(calls, 1);
    signal.abort();
    await assert.rejects(pending, (e) => {
      assert.deepEqual(canonical(e, snapshotHooks), {
        $error: {
          ...pluginError('operation-cancelled', 'operation cancelled').$error,
          cause: { $error: { name: 'AbortError', message: 'This operation was aborted' } },
        },
      });
      return true;
    });
    assert.equal(h.api.getLoadingLocale(), null);
    resolve(dutch);
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(h.api.getLocale(), 'en');
    assert.deepEqual(events, []);
    assert.deepEqual(h.api.listLocales(), [
      { code: 'en', name: 'English', direction: 'ltr', loaded: true },
      { code: 'nl', name: 'Nederlands', direction: 'ltr', loaded: true },
    ]);
    await assert.rejects(h.api.setLocale('nl', { signal: signal.signal }), (e) => {
      assert.deepEqual(
        canonical(e, snapshotHooks),
        pluginError('operation-cancelled', 'operation cancelled'),
      );
      return true;
    });
    assert.equal(calls, 1);
  } finally {
    h.ctx.dispose();
  }
});
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
type Saved = {
  file: string;
  kind: string;
  name: string;
  input: unknown;
  inputSha256: string;
  outcome?: unknown;
  result?: unknown;
  runnerException?: unknown;
};
const history = new URL('../historical/task-12b2c9/', import.meta.url);
const saved = (p: string): unknown => JSON.parse(readFileSync(new URL(p, history), 'utf8'));
const hash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
test('prior actions160 commands79 link37 metadata17 and page-edit19 retain all inputs and every output channel', async () => {
  const available = [
    ...(await import('../src/suites/actions')).suites,
    ...(await import('../src/suites/commands')).suites,
    ...(await import('../src/suites/link')).suites,
    ...(await import('../src/suites/metadata')).suites,
    ...(await import('../src/suites/page-edit')).suites,
  ];
  let count = 0;
  for (const task of [
    'task-12b2c2',
    'task-12b2c3',
    'task-12b2c4',
    'task-12b2c5',
    'task-12b2c6',
    'task-12b2c7',
    'task-12b2c8',
  ]) {
    const rows = JSON.parse(
      readFileSync(
        new URL('../historical/' + task + '/current-inputs-runtime.json', import.meta.url),
        'utf8',
      ),
    ) as Saved[];
    for (const row of rows) {
      count++;
      const s = available.find((s) => s.file === row.file)!;
      const c = s.cases.find((c) => c.name === row.name)!;
      assert.deepEqual(canonical(c), row.input, row.name);
      if (s.kind === 'scenario') {
        assert.ok('steps' in c);
        assert.deepEqual(await runScenario(c), row.result, row.name);
      } else {
        assert.ok('fn' in c);
        assert.deepEqual(
          {
            returned: canonical(
              (s.functions[c.fn] as (...a: unknown[]) => unknown)(...c.args),
              snapshotHooks,
            ),
          },
          row.outcome,
          row.name,
        );
      }
    }
  }
  assert.equal(count, 312);
});
test('all current i18n inputs and complete outcomes remain independently replayable', async () => {
  const rows = saved('current-inputs-runtime.json') as Saved[];
  assert.equal(rows.length, 642);
  for (const row of rows) {
    const s = suites.find((s) => s.file === row.file)!;
    const c = s.cases.find((c) => c.name === row.name)!;
    assert.deepEqual(canonical(c), row.input, row.name);
    assert.equal(hash(row.input), row.inputSha256);
    if (s.kind === 'scenario') {
      assert.ok('steps' in c);
      assert.deepEqual(await runScenario(c), row.result, row.name);
    } else {
      assert.ok('fn' in c);
      assert.deepEqual(
        {
          returned: canonical(
            (s.functions[c.fn] as (...a: unknown[]) => unknown)(...c.args),
            snapshotHooks,
          ),
        },
        row.outcome,
        row.name,
      );
    }
  }
});
test('locale negotiation preserves preference passes, casing and available ordering', () => {
  assert.equal(pure('negotiate-exact-case-insensitive'), 'es-MX');
  assert.equal(pure('negotiate-none'), null);
  assert.equal(pure('negotiate-empty'), null);
  assert.equal(pure('negotiate-no-request'), null);
  assert.equal(pure('negotiate-widen-keeps-available-order'), 'pt-PT');
  assert.equal(pure('negotiate-widen-first-request-wins'), null);
});
test('all 635 immutable original occurrences retain full definitions and outcomes with exact unique mappings', () => {
  const expected: Record<string, string> = {
    'original-runtime.json': '8be3da23c4f1e13d9b852dc8e60de4b6c0df09e6dc4f0004b6999ec0ee486d08',
    'original-i18n.ts.txt': '9f9a3aeab41526783de77bdbd59b48356b9eaa63e515bd51aa1c22a2a021d8f0',
    'original-commands-current.test.ts.txt':
      '19b3fa809b3d8e98b659a522c74c8dc9d3fb51a6dfb5eaae54d0fc0ee48c620f',
  };
  for (const [p, want] of Object.entries(expected))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
  const old = saved('original-runtime.json') as Saved[];
  const account = saved('accounting.json') as {
    summary: unknown;
    mappings: {
      originalOccurrence: number;
      originalName: string;
      originalInput: unknown;
      originalOutcome: unknown;
      currentInput: unknown;
      currentOutcome: unknown;
      currentFile: string;
      currentName: string;
      currentIndex: number;
    }[];
  };
  assert.deepEqual(account.summary, {
    original: 635,
    originalPure: 611,
    originalLive: 24,
    current: 642,
    currentPure: 618,
    currentLive: 24,
    mapped: 635,
    pluralRows: 532,
    duplicateExtras: 4,
    wholeRetirements: 1,
    survivingOriginals: 634,
    replacementRows: 1,
    totalNewRows: 8,
    partialRetirements: 32,
    additions: 7,
    changedInputs: 83,
  });
  const rows = saved('current-inputs-runtime.json') as Saved[];
  const mapped = new Set<string>();
  for (const [i, o] of old.entries()) {
    const m = account.mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.originalName, o.name);
    assert.deepEqual(m.originalInput, o.input);
    assert.deepEqual(m.originalOutcome, o.outcome ?? o.result ?? o.runnerException);
    const s = suites.find((s) => s.file === m.currentFile)!;
    assert.equal(s.cases[m.currentIndex].name, m.currentName);
    const c = rows.find((c) => c.file === m.currentFile && c.name === m.currentName)!;
    assert.deepEqual(m.currentInput, c.input);
    assert.deepEqual(m.currentOutcome, c.outcome ?? c.result);
    const key = m.currentFile + ':' + m.currentIndex;
    assert.ok(!mapped.has(key));
    mapped.add(key);
  }
  assert.equal(old.filter((o) => o.file.endsWith('plurals.json')).length, 532);
  assert.equal(mapped.size, 635);
  assert.deepEqual(saved('task11-current-drift.json'), []);
  // Historical artifacts only. No permanent guard is imposed on future-owned sources.
  for (const [p, want] of Object.entries(saved('integrity.json') as Record<string, string>))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
});
test('only explicitly adversarial synchronous-throw and empty-code switches remain unsettled', async () => {
  for (const c of live.cases) {
    const r = await runScenario(c);
    const started = r.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const done = r.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(
      started.filter((p) => !done.includes(p)),
      c.name === 'loader-throws-in-the-watch' ? [1] : c.name === 'empty-code-loader' ? [2] : [],
      c.name,
    );
  }
  const destroyed = await run('destroy-releases-the-watch');
  assert.deepEqual(
    destroyed.trace.flatMap((t) => t.callbacks),
    [{ fn: 'load-ar', args: [] }],
  );
  assert.deepEqual(destroyed.trace[3].state, destroyed.trace[2].state);
});
