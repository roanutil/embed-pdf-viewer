/**
 * `packages/plugin/i18n`. Replayed by `crates/plugins/i18n/tests/conformance.rs`.
 *
 * Three files: the pure core (`translate`, `interpolate`, `negotiateLocale`,
 * the reducer and initial state) as function cases, the CLDR plural table
 * (`Intl.PluralRules` as `translate` reaches it) as function cases, and the
 * plugin (capability, the lazy-pack effect, config) as scenarios.
 *
 * Two things the plugin does are not in a trace by default, and this suite
 * puts them there:
 *
 * - `console.warn`. The capability's warn-once signal is behaviour (which
 *   misses warn, and how often), so while an i18n case runs every
 *   `console.warn` is logged as the callback `console.warn`. The Rust port
 *   takes a warn sink in its config; the harness logs it the same way.
 * - Loaders. `config.loaders` is code → `() => Promise<Locale>`. A vector
 *   writes each as a `$fn` spec (`resolves`, `rejects`, `defer`, `throws`,
 *   `seq`); the factory turns it into a function that logs its call and
 *   answers as the spec says, which is what `$fn` means everywhere else.
 */
import { i18nPlugin, interpolate, negotiateLocale, translate } from '../../../../packages/plugin/i18n/src/index';
import { I18nToken, i18nReducer, initialI18nState } from '../../../../packages/plugin/i18n/src/internal';
import type { I18nConfig, Locale } from '../../../../packages/plugin/i18n/src/contract';
import type { I18nAction, I18nState } from '../../../../packages/plugin/i18n/src/model';

import { makeError, type ErrorSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type FnSpec, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

// ── the factory ───────────────────────────────────────────────────────────────

let warnEnv: PluginEnv | null = null;
onCaseEnd(() => {
  warnEnv = null;
});
const realWarn = console.warn.bind(console);
console.warn = (...args: unknown[]) => {
  if (warnEnv) warnEnv.log('console.warn', args);
  else realWarn(...args);
};

type Answer = Omit<FnSpec, '$fn' | 'seq'>;

function scriptedLoader(spec: FnSpec, env: PluginEnv): () => Promise<Locale> {
  let calls = 0;
  return () => {
    env.log(spec.$fn, []);
    const answer: Answer = spec.seq ? spec.seq[Math.min(calls, spec.seq.length - 1)] : spec;
    calls += 1;
    if (answer.throws) throw makeError(answer.throws);
    if (answer.defer !== undefined) {
      const label = answer.defer;
      return new Promise<Locale>((resolve, reject) => env.defer(label, (v) => resolve(v as Locale), reject));
    }
    if (answer.rejects) return Promise.reject(makeError(answer.rejects as ErrorSpec));
    return Promise.resolve(answer.resolves as Locale);
  };
}

interface VectorConfig extends Omit<I18nConfig, 'loaders'> {
  loaders?: Record<string, FnSpec>;
}

registerPlugin('i18n', {
  factory: (raw, env) => {
    warnEnv = env;
    if (raw === undefined) return i18nPlugin();
    const { loaders, ...rest } = raw as VectorConfig;
    const config: I18nConfig = { ...rest };
    if (loaders !== undefined) {
      // The Rust runner reads JSON objects sorted by key, so a loader order
      // that is behaviour (locales() lists them) must be written sorted.
      const named = Object.keys(loaders).filter((k) => !/^(0|[1-9][0-9]*)$/.test(k));
      if (named.some((k, i) => i > 0 && named[i - 1] > k)) {
        throw new Error(`[i18n suite] write config.loaders in key order: ${named.join(', ')}`);
      }
      config.loaders = {};
      for (const [code, spec] of Object.entries(loaders)) config.loaders[code] = scriptedLoader(spec, env);
    }
    return i18nPlugin(config);
  },
  tokens: [I18nToken],
});

// ── packs ─────────────────────────────────────────────────────────────────────

const en: Locale = {
  code: 'en',
  name: 'English',
  translations: {
    commands: { zoom: { in: 'Zoom In', out: 'Zoom Out' }, save: 'Save' },
    zoomLevel: 'Zoom Level ({level}%)',
    pages: { one: '{count} page', other: '{count} pages' },
    greeting: 'Hello {name}, you have {count} {unknown}',
  },
};
const es: Locale = {
  code: 'es',
  name: 'Español',
  translations: { commands: { save: 'Guardar' }, pages: { one: '{count} página', other: '{count} páginas' } },
};
const ar: Locale = {
  code: 'ar',
  name: 'العربية',
  dir: 'rtl',
  translations: { commands: { save: 'حفظ' }, pages: { zero: 'لا صفحات', one: 'صفحة', two: 'صفحتان', few: '{count} صفحات', many: '{count} صفحة', other: '{count} صفحة' } },
};
const he: Locale = { code: 'he', name: 'עברית', dir: 'rtl', translations: { commands: { save: 'שמור' } } };
const fr: Locale = { code: 'fr', name: 'Français', dir: 'ltr', translations: { commands: { save: 'Enregistrer' } } };

// ── function cases: the pure core ─────────────────────────────────────────────

const state = (over: Partial<I18nState> = {}): I18nState => ({
  locale: 'es',
  fallbackLocale: 'en',
  locales: { en, es },
  loading: null,
  ...over,
});

const fnCase = (name: string, fn: string, args: unknown[], covers?: string): FunctionCase =>
  covers === undefined ? { name, fn, args } : { name, fn, args, covers };

/** The reducer, with its reference identity made visible. */
const reduce = (s: I18nState, action: I18nAction) => {
  const next = i18nReducer(s, action);
  return { same: next === s, next };
};

/** `initialI18nState` reads only whether a loader exists, so a vector's
 *  loaders are any non-undefined value. */
const initial = (config: I18nConfig) => initialI18nState(config);

const coreCases: FunctionCase[] = [
  // translate
  fnCase('translate-current-locale', 'translate', [state(), 'commands.save']),
  fnCase('translate-fallback-locale', 'translate', [state(), 'commands.zoom.in']),
  fnCase('translate-options-fallback', 'translate', [state(), 'nope.nothing', { fallback: 'Hi {name}', params: { name: 'Bob' } }]),
  fnCase('translate-options-fallback-no-params', 'translate', [state(), 'nope', { fallback: 'Hi {name}' }]),
  fnCase('translate-key-itself', 'translate', [state(), 'nope.nothing']),
  fnCase('translate-empty-key', 'translate', [state(), '']),
  fnCase('translate-empty-options', 'translate', [state(), 'nope', {}]),
  fnCase('translate-interpolates', 'translate', [state(), 'zoomLevel', { params: { level: 150 } }]),
  fnCase('translate-plural-one', 'translate', [state({ locale: 'en' }), 'pages', { params: { count: 1 } }]),
  fnCase('translate-plural-other', 'translate', [state({ locale: 'en' }), 'pages', { params: { count: 5 } }]),
  fnCase('translate-plural-in-current-locale', 'translate', [state(), 'pages', { params: { count: 1 } }]),
  fnCase('translate-branch-without-count', 'translate', [state(), 'pages']),
  fnCase('translate-branch-with-string-count', 'translate', [state(), 'pages', { params: { count: '1' } }]),
  fnCase('translate-branch-missing-category-uses-other', 'translate', [
    state({ locale: 'ar', locales: { en, es, ar: { ...ar, translations: { pages: { other: '{count} x' } } } } }),
    'pages',
    { params: { count: 2 } },
  ]),
  fnCase('translate-branch-with-no-other-falls-through', 'translate', [
    state({ locale: 'en', fallbackLocale: 'es', locales: { en: { ...en, translations: { pages: { one: 'uno' } } }, es } }),
    'pages',
    { params: { count: 3 } },
  ]),
  fnCase('translate-branch-category-is-a-branch', 'translate', [
    state({ locale: 'en', fallbackLocale: 'en', locales: { en: { ...en, translations: { pages: { other: { deeper: 'x' } } } } } }),
    'pages',
    { params: { count: 3 } },
  ]),
  fnCase('translate-walks-through-a-leaf', 'translate', [state(), 'commands.save.more']),
  fnCase('translate-unregistered-current-locale', 'translate', [state({ locale: 'de' }), 'commands.save']),
  fnCase('translate-unregistered-both', 'translate', [state({ locale: 'de', fallbackLocale: 'it' }), 'commands.save']),
  fnCase('translate-fallback-plural-uses-fallback-locale-rules', 'translate', [
    state({ locale: 'de', fallbackLocale: 'ar', locales: { ar } }),
    'pages',
    { params: { count: 2 } },
  ]),
  fnCase('translate-nan-count', 'translate', [state({ locale: 'en' }), 'pages', { params: { count: { $number: 'NaN' } } }]),
  fnCase('translate-proto-member-key', 'translate', [state(), 'toString']),
  fnCase('translate-proto-member-name', 'translate', [state(), 'toString.name']),
  fnCase('translate-proto-constructor-name', 'translate', [state(), 'constructor.name']),
  fnCase('translate-proto-member-count', 'translate', [state(), 'valueOf', { params: { count: 1 } }]),
  fnCase('translate-proto-member-length', 'translate', [state(), 'toString.length', { params: { count: 1 } }]),
  fnCase('translate-proto-proto-member', 'translate', [state(), '__proto__.hasOwnProperty.name']),
  fnCase('translate-proto-member-deeper', 'translate', [state(), 'toString.name.x']),
  fnCase('translate-proto-locale', 'translate', [state({ locale: 'toString' }), 'commands.save']),
  fnCase('translate-proto-param', 'translate', [state(), 'nope', { fallback: '{toString}|{constructor}|{__proto__}|{valueOf}|{x}', params: { x: 1 } }]),

  // interpolate
  fnCase('interpolate-no-params', 'interpolate', ['{a} {b}']),
  fnCase('interpolate-unknown-slot-verbatim', 'interpolate', ['{a} {b}', { a: 'x' }]),
  fnCase('interpolate-numbers', 'interpolate', [
    '{a}|{b}|{c}|{d}|{e}|{f}|{g}|{h}|{i}|{j}|{k}|{l}|{m}|{n}',
    {
      a: 0,
      b: -0,
      c: 1.5,
      d: 1e21,
      e: 1e-7,
      f: 123456789012345680000,
      g: 0.000001,
      h: -2.5e-8,
      i: { $number: 'NaN' },
      j: { $number: 'Infinity' },
      k: { $number: '-Infinity' },
      l: 0.1 + 0.2,
      m: 2 ** 53,
      n: 1.7976931348623157e308,
    },
  ]),
  fnCase('interpolate-empty-string-param', 'interpolate', ['[{e}]', { e: '' }]),
  fnCase('interpolate-slot-grammar', 'interpolate', ['{a_1}{A}{9}{}{a-b}{ a}{é}{{a}}{a', { a_1: 'u', A: 'U', 9: 'n', a: 'x', 'a-b': 'no', ' a': 'no', é: 'no' }]),
  fnCase('interpolate-empty-slot-is-not-a-slot', 'interpolate', ['{}|{a}', { '': 'no', a: 'x' }]),
  fnCase('interpolate-repeated', 'interpolate', ['{a}{a}{a}', { a: '{a}' }]),
  fnCase('interpolate-dollar-patterns', 'interpolate', ['{a}', { a: '$& $1 $$' }]),

  // negotiateLocale
  fnCase('negotiate-exact-case-insensitive', 'negotiateLocale', [['en', 'es-MX'], ['ES-mx', 'en']]),
  fnCase('negotiate-narrow', 'negotiateLocale', [['en', 'es'], ['en-GB']]),
  fnCase('negotiate-widen', 'negotiateLocale', [['zh-Hans', 'en'], ['zh']]),
  fnCase('negotiate-preference-across-passes', 'negotiateLocale', [['fr', 'en'], ['de', 'fr-CA']]),
  fnCase('negotiate-none', 'negotiateLocale', [['en'], ['ja', 'ko']]),
  fnCase('negotiate-empty', 'negotiateLocale', [[], ['en']]),
  fnCase('negotiate-no-request', 'negotiateLocale', [['en'], []]),
  fnCase('negotiate-duplicate-canon-keeps-first', 'negotiateLocale', [['EN', 'en', 'en-gb', 'en-GB'], ['en-GB', 'en']]),
  fnCase('negotiate-widen-keeps-available-order', 'negotiateLocale', [['pt-PT', 'pt-BR', 'en'], ['pt-AO']]),
  fnCase('negotiate-widen-first-request-wins', 'negotiateLocale', [['zh-Hant', 'sr-Latn'], ['sr', 'zh']]),
  fnCase('negotiate-underscore-is-not-a-separator', 'negotiateLocale', [['en_US', 'en'], ['en_US-x', 'en_GB']]),
  fnCase('negotiate-empty-strings', 'negotiateLocale', [['', 'en'], ['-x']]),
  fnCase('negotiate-unicode-lowercase', 'negotiateLocale', [['İ', 'Straße'], ['i̇', 'STRASSE', 'straße']]),

  // initial state
  fnCase('initial-defaults', 'initialI18nState', [{}]),
  fnCase('initial-eager-packs', 'initialI18nState', [{ locales: [en, es], locale: 'es' }]),
  fnCase('initial-duplicate-codes-keep-position', 'initialI18nState', [{ locales: [en, es, { ...en, name: 'English 2' }] }]),
  fnCase('initial-integer-like-codes', 'initialI18nState', [{ locales: [{ ...en, code: 'b' }, { ...en, code: '10' }, { ...en, code: '2' }] }]),
  fnCase('initial-lazy-startup-seeds-loading', 'initialI18nState', [{ locales: [en], locale: 'ar', loaders: { ar: true } }]),
  fnCase('initial-lazy-but-registered', 'initialI18nState', [{ locales: [en, ar], locale: 'ar', loaders: { ar: true } }]),
  fnCase('initial-unknown-locale', 'initialI18nState', [{ locales: [en], locale: 'xx', loaders: { ar: true } }]),
  fnCase('initial-custom-fallback', 'initialI18nState', [{ fallbackLocale: 'es', locales: [es] }]),
  fnCase('initial-null-loader-counts', 'initialI18nState', [{ locale: 'ar', loaders: { ar: null } }]),
  fnCase('initial-proto-locale', 'initialI18nState', [{ locale: 'toString', loaders: { toString: true } }]),

  // reducer
  fnCase('set-locale-unregistered-is-ignored', 'i18nReducer', [state(), { type: 'I18N/SET_LOCALE', locale: 'ar' }]),
  fnCase('set-locale-same-and-idle-is-ignored', 'i18nReducer', [state(), { type: 'I18N/SET_LOCALE', locale: 'es' }]),
  fnCase('set-locale-same-but-loading-clears', 'i18nReducer', [state({ loading: 'ar' }), { type: 'I18N/SET_LOCALE', locale: 'es' }]),
  fnCase('set-locale-switches', 'i18nReducer', [state({ loading: 'en' }), { type: 'I18N/SET_LOCALE', locale: 'en' }]),
  fnCase('set-locale-proto-member', 'i18nReducer', [state(), { type: 'I18N/SET_LOCALE', locale: 'hasOwnProperty' }]),
  fnCase('register-adds', 'i18nReducer', [state(), { type: 'I18N/REGISTER_LOCALE', locale: ar }]),
  fnCase('register-replaces-in-place', 'i18nReducer', [state(), { type: 'I18N/REGISTER_LOCALE', locale: { ...en, name: 'Inglés' } }]),
  fnCase('load-started', 'i18nReducer', [state(), { type: 'I18N/LOAD_STARTED', locale: 'ar' }]),
  fnCase('load-started-again-is-ignored', 'i18nReducer', [state({ loading: 'ar' }), { type: 'I18N/LOAD_STARTED', locale: 'ar' }]),
  fnCase('load-started-replaces', 'i18nReducer', [state({ loading: 'ar' }), { type: 'I18N/LOAD_STARTED', locale: 'he' }]),
  fnCase('load-failed-other-code-is-ignored', 'i18nReducer', [state({ loading: 'ar' }), { type: 'I18N/LOAD_FAILED', locale: 'fr' }]),
  fnCase('load-failed-clears', 'i18nReducer', [state({ loading: 'ar' }), { type: 'I18N/LOAD_FAILED', locale: 'ar' }]),
  fnCase('unknown-action-is-ignored', 'i18nReducer', [state(), { type: 'I18N/NOPE' }]),
  fnCase('unregister-removes', 'i18nReducer', [state(), { type: 'I18N/UNREGISTER_LOCALE', locale: 'es' }]),
  fnCase('unregister-unknown-is-ignored', 'i18nReducer', [state(), { type: 'I18N/UNREGISTER_LOCALE', locale: 'ar' }]),
  fnCase(
    'unregister-proto-member-copies',
    'i18nReducer',
    [state(), { type: 'I18N/UNREGISTER_LOCALE', locale: 'toString' }],
    'An Object.prototype member reads as registered, so the reducer copies the packs and deletes nothing: a new state with the same content.',
  ),
  fnCase(
    'add-translations-merges-deep',
    'i18nReducer',
    [
      state(),
      {
        type: 'I18N/ADD_TRANSLATIONS',
        locale: 'en',
        translations: { commands: { zoom: { in: 'Zoom in!', fit: 'Fit' }, save: { short: 'S' } }, zoomLevel: { one: 'x' }, pages: 'flat', extra: 'new' },
      },
    ],
    'Branches merge key by key, a leaf replaces whatever was there (string or branch), and a branch replaces a leaf.',
  ),
  fnCase('add-translations-unknown-is-ignored', 'i18nReducer', [state(), { type: 'I18N/ADD_TRANSLATIONS', locale: 'ar', translations: { a: 'b' } }]),
];

// ── function cases: plural categories, as translate reaches them ──────────────

/** A pack whose `k` branch names every CLDR category, so the text a count
 *  selects IS the category `Intl.PluralRules(locale)` picked. */
const pluralState = (locale: string): I18nState => ({
  locale,
  fallbackLocale: locale,
  locales: {
    [locale]: {
      code: locale,
      name: locale,
      translations: { k: { zero: 'zero', one: 'one', two: 'two', few: 'few', many: 'many', other: 'other' } },
    },
  },
  loading: null,
});
const plurals = (locale: string, counts: number[]) =>
  counts.map((count) => translate(pluralState(locale), 'k', { params: { count } }).text);

const counts: unknown[] = [
  0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 14, 15, 19, 20, 21, 22, 25, 100, 101, 102, 111, 1000, 1000000, 2000000, 3e6, 1e7,
  0.5, 1.5, 2.5, 0.1, 1.1, 2.1, 1.25, 1.0004, 1.0005, 0.9995, 2.0001, 1.001, 10.5, -1, -2, -0.5, 1e21, 1234567.5,
  9.9995, 99.9999,
  { $number: 'NaN' }, { $number: 'Infinity' }, { $number: '-Infinity' },
];

/** Every language `Intl.PluralRules.supportedLocalesOf` accepts among all two-
 *  and three-letter codes, plus aliases, scripts, regions, extensions, the
 *  default-locale fallback for unsupported ones, and invalid tags (the
 *  `count === 1` catch arm). */
function supportedLanguages(): string[] {
  const letters = 'abcdefghijklmnopqrstuvwxyz';
  const out: string[] = [];
  for (const a of letters) for (const b of letters) out.push(a + b);
  for (const a of letters) for (const b of letters) for (const c of letters) out.push(a + b + c);
  return out.filter((code) => Intl.PluralRules.supportedLocalesOf(code).length > 0);
}

const pluralTags = [
  ...supportedLanguages(),
  // scripts, regions, variants, extensions
  'pt-PT', 'pt-BR', 'pt-AO', 'pt-MZ', 'sr-Latn', 'sr-Cyrl', 'zh-Hans', 'zh-Hant', 'zh-TW', 'az-Cyrl', 'uz-Arab', 'pa-Arab',
  'ks-Deva', 'sd-Deva', 'bs-Cyrl', 'ha-NE', 'ff-Adlm', 'shi-Latn', 'vai-Latn', 'yue-Hans', 'es-419', 'en-GB', 'en-US-POSIX',
  'EN-gb', 'en-US-u-nu-thai', 'ca-ES-valencia', 'de-CH-1996', 'en-t-ja', 'en-u-ca-gregory-x-foo', 'en-x-foo', 'ar-EG', 'fr-CA',
  'it-CH', 'es-ES', 'ca-ES', 'sh-RS', 'no-NO', 'nb-NO', 'iw-IL', 'in-ID', 'tl-PH', 'mo-MD', 'cnr', 'drw', 'swc', 'hbs', 'zh-min',
  // unsupported → the default locale (en-US)
  'und', 'xx', 'qaa', 'tlh', 'aam', 'xx-Latn-ZZ', 'abcdefgh', 'zz-ZZ', 'und-Latn', 'root-x-y',
  // invalid → RangeError → the catch arm (with a few valid neighbours)
  '', 'root', 'en_US', 'x-foo', 'i-klingon', 'de-DE-1996-1996', 'en-a-bb-a-cc', 'e', 'en-', '-en', 'en--US', 'toolongtag',
  'en-US-u', 'en-t', 'en-x', 'en-u-a1', 'en-a', 'en-1', 'en-12', 'en-1234', 'en-Latn-Latn', 'en-US-GB', '123', 'en-é', 'toString',
  'en-t-a1', 'en-t-en-a1-abc', 'en-t-h0', 'en-u-ca', 'en-u-aaa-ca-bbb', 'en-u-ca-ca', 'en-u-ca-x', 'en-t-en-US-h0-hybrid',
  'en-t-en-US-1996-1996', 'en-x-a-b', 'en-x-toolongpriv', 'en-u-attribute-ca', 'en-u-ca-abcdefghi', 'en-0-ab', 'en-US-12345',
  'en-US-1abc', 'en-US-abcd', 'en-u-cu-usd-t-ja', 'en-t-ja-u-ca-gregory', 'en-US-t-x', 'en-US-t-h0-abc', 'en-x-1',
  'en-t-h0-abc-ab-abc', 'en-t-h0-abc-a1-abc', 'en-t-ab', 'en-t-a0b-abc',
];

const pluralCases: FunctionCase[] = pluralTags.map((tag) => fnCase(`plural ${JSON.stringify(tag)}`, 'plurals', [tag, counts]));

// ── scenarios ─────────────────────────────────────────────────────────────────

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const destroy: Step = { op: 'call', target: 'kernel', method: 'destroy' };
const i18n = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'i18n' }, method, args });
const t = (key: string, options?: unknown): Step => i18n('t', ...(options === undefined ? [key] : [key, options]));
const reads: Step[] = [i18n('getLocale'), i18n('getDirection'), i18n('getLoadingLocale'), i18n('listLocales')];
const settle = (label: string, value?: unknown): Step => ({ op: 'settle', label, value });
const fail = (label: string, message: string): Step => ({ op: 'settle', label, error: { name: 'Error', message } });
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const box = { left: 0, bottom: 0, right: 600, top: 800 };
const page = { index: 0, ref: { kind: 'objectNumber', pageObjectNumber: 1 }, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } };

const plugin = (config?: unknown) => [config === undefined ? { factory: 'i18n' } : { factory: 'i18n', config }];
const loader = (name: string, answer: Omit<FnSpec, '$fn'>): FnSpec => ({ $fn: name, ...answer });

const scenarios: ScenarioCase[] = [
  {
    name: 'no-config',
    covers: 'i18nPlugin() with no config: locale and fallback are en, nothing registered, every key misses (and warns once per key).',
    plugins: plugin(),
    steps: [i18n('capability-check-is-not-a-member'), ...reads, t('a.b'), t('a.b'), t('c'), start, ...reads],
  },
  {
    name: 'alive-before-start-and-across-documents',
    covers:
      'The capability exists from createKernel(): t() works before start, after start, while a document is open and after it closes. The plugin is workspace-scoped and never touches the engine.',
    plugins: plugin({ locales: [en, es], locale: 'es' }),
    engine: { documents: { a: { pages: [page] } } },
    steps: [
      t('commands.save'),
      i18n('setLocale', 'en'),
      t('commands.save'),
      start,
      t('commands.save'),
      open('a'),
      i18n('setLocale', 'es'),
      t('commands.save'),
      { op: 'call', target: { token: 'i18n', doc: 'a' }, method: 'getLocale' },
      { op: 'call', target: 'documents', method: 'close', args: ['a'] },
      t('commands.zoom.in'),
      destroy,
      t('commands.save'),
    ],
  },
  {
    name: 'translate-through-the-capability',
    covers:
      't(): the current locale, the fallback locale, params, plurals by the current locale rules, options.fallback (no warning), and the key itself (warned once per key, whatever the params).',
    plugins: plugin({ locales: [en, es, ar], locale: 'ar' }),
    steps: [
      start,
      t('commands.save'),
      t('commands.zoom.out'),
      t('zoomLevel', { params: { level: 75 } }),
      t('pages', { params: { count: 0 } }),
      t('pages', { params: { count: 2 } }),
      t('pages', { params: { count: 11 } }),
      t('pages'),
      t('pages'),
      t('greeting', { params: { name: 'Ann', count: 3 } }),
      t('missing.key', { fallback: 'Default {x}', params: { x: 1 } }),
      t('missing.key', { fallback: '' }),
      t('missing.key'),
      t('missing.key', { params: { count: 1 } }),
      i18n('setLocale', 'es'),
      t('missing.key'),
      t('another'),
    ],
  },
  {
    name: 'dir-and-locales',
    covers:
      'dir() is the current pack\'s dir, ltr by default, ltr for a locale with no pack; locales() lists registered packs in registration order (dir defaulted, loaded) then lazy ones not yet loaded (code as name).',
    plugins: plugin({
      locales: [en, ar, { ...es, code: '7' }, { ...fr, code: '0' }],
      locale: 'ar',
      loaders: { '3': loader('load-3', { resolves: { ...fr, code: '3' } }), en: loader('load-en', { resolves: en }), fr: loader('load-fr', { resolves: fr }), he: loader('load-he', { resolves: he }) },
    }),
    steps: [...reads, i18n('setLocale', 'en'), i18n('getDirection'), i18n('setLocale', '7'), i18n('getDirection'), i18n('setLocale', 'he'), ...reads],
  },
  {
    name: 'set-locale-registered',
    covers: 'setLocale(code) for a registered pack dispatches SET_LOCALE; the same locale again is dispatched and changes nothing.',
    plugins: plugin({ locales: [en, es] }),
    steps: [start, i18n('setLocale', 'es'), ...reads, i18n('setLocale', 'es'), i18n('setLocale', 'en'), i18n('getLocale')],
  },
  {
    name: 'set-locale-unknown-warns-once',
    covers: 'An unregistered code with no loader dispatches nothing, warns nothing, and rejects not-found each time.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { resolves: ar }) } }),
    steps: [start, i18n('setLocale', 'de'), i18n('setLocale', 'de'), i18n('setLocale', 'it'), i18n('setLocale', ''), i18n('getLocale')],
  },
  {
    name: 'lazy-switch',
    covers:
      'setLocale(lazy) dispatches LOAD_STARTED; the watch on loading calls the loader; its pack is registered, then the switch completes. A second setLocale while loading is ignored by the reducer, so the loader runs once. Once loaded, setLocale is a plain SET.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { defer: 'ar' }) } }),
    steps: [
      start,
      i18n('setLocale', 'ar'),
      ...reads,
      i18n('setLocale', 'ar'),
      t('commands.save'),
      settle('ar', ar),
      ...reads,
      t('commands.save'),
      i18n('setLocale', 'en'),
      i18n('setLocale', 'ar'),
      i18n('getLocale'),
    ],
  },
  {
    name: 'lazy-switch-superseded',
    covers:
      'A lazy load that lands after the user switched elsewhere registers the pack but does not switch: a registered switch clears loading, and a second lazy switch replaces it (both loaders run; only the wanted one completes).',
    plugins: plugin({ locales: [en, es], loaders: { ar: loader('load-ar', { defer: 'ar' }), he: loader('load-he', { defer: 'he' }) } }),
    steps: [
      start,
      i18n('setLocale', 'ar'),
      i18n('setLocale', 'es'),
      settle('ar', ar),
      ...reads,
      i18n('setLocale', 'he'),
      i18n('setLocale', 'fr'),
      i18n('setLocale', 'ar'),
      i18n('setLocale', 'he'),
      ...reads,
    ],
  },
  {
    name: 'lazy-two-in-flight',
    covers: 'Two lazy switches in flight: both loaders run, the first to land only registers, the wanted one registers and switches.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { defer: 'ar' }), he: loader('load-he', { defer: 'he' }) } }),
    steps: [start, i18n('setLocale', 'ar'), i18n('setLocale', 'he'), i18n('getLoadingLocale'), settle('he', he), ...reads, settle('ar', ar), ...reads],
  },
  {
    name: 'lazy-failure',
    covers:
      'A loader that rejects dispatches LOAD_FAILED, which clears loading when it is still the wanted code and is ignored otherwise; the code can be retried and its loader runs again.',
    plugins: plugin({
      locales: [en],
      loaders: {
        ar: loader('load-ar', { seq: [{ rejects: { name: 'Error', message: 'offline' } }, { defer: 'ar-2' }] }),
        he: loader('load-he', { defer: 'he' }),
      },
    }),
    steps: [
      start,
      i18n('setLocale', 'ar'),
      ...reads,
      i18n('setLocale', 'he'),
      i18n('setLocale', 'ar'),
      fail('he', 'gone'),
      ...reads,
      settle('ar-2', ar),
      ...reads,
    ],
  },
  {
    name: 'lazy-startup-locale',
    covers:
      'A startup locale that is a lazy pack seeds loading; the effect fetches it when the kernel starts (not before), t() shows the fallback chain until then, and the pack switches in when it lands.',
    plugins: plugin({ locales: [en], locale: 'ar', loaders: { ar: loader('load-ar', { defer: 'ar' }) } }),
    steps: [...reads, t('commands.save'), start, ...reads, t('commands.save'), settle('ar', ar), ...reads, t('commands.save')],
  },
  {
    name: 'lazy-startup-locale-fails',
    covers: 'A lazy startup pack that fails to load clears loading; the locale stays the requested code and t() keeps the fallback chain.',
    plugins: plugin({ locales: [en], locale: 'ar', loaders: { ar: loader('load-ar', { rejects: { name: 'TypeError', message: 'fetch failed' } }) } }),
    steps: [start, ...reads, t('commands.save')],
  },
  {
    name: 'lazy-resolves-immediately',
    covers: 'A loader whose promise is already resolved still lands a turn later: LOAD_STARTED, then REGISTER and SET within the same step.',
    plugins: plugin({ locales: [en], loaders: { he: loader('load-he', { resolves: he }) } }),
    steps: [start, i18n('setLocale', 'he'), ...reads],
  },
  {
    name: 'lazy-pack-with-a-different-code',
    covers: 'The effect registers the pack under the code it carries and switches to that code, not the requested one.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { resolves: { ...ar, code: 'ar-EG' } }) } }),
    steps: [start, i18n('setLocale', 'ar'), ...reads, i18n('setLocale', 'ar'), ...reads],
  },
  {
    name: 'loader-throws-in-the-watch',
    covers: 'A loader that throws synchronously inside the loading watch is reported by the store; loading stays set, nothing else is dispatched.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { throws: { name: 'Error', message: 'no loader today' } }) } }),
    steps: [start, i18n('setLocale', 'ar'), ...reads],
  },
  {
    name: 'loader-throws-at-startup',
    covers: 'A lazy startup pack whose loader throws synchronously makes the effect throw while the kernel starts.',
    plugins: plugin({ locales: [en], locale: 'ar', loaders: { ar: loader('load-ar', { throws: { name: 'Error', message: 'no loader today' } }) } }),
    steps: [start, ...reads, { op: 'call', target: 'kernel', method: 'status' }],
  },
  {
    name: 'empty-code-loader',
    covers: 'A loader keyed by the empty string: LOAD_STARTED is dispatched, but the watch skips a falsy code, so nothing loads (at startup or on a switch).',
    plugins: plugin({ locales: [en], locale: '', loaders: { '': loader('load-empty', { resolves: fr }) } }),
    steps: [...reads, start, ...reads, i18n('setLocale', 'en'), i18n('setLocale', ''), ...reads],
  },
  {
    name: 'register-at-runtime',
    covers:
      'registerLocale adds a pack (it appears in locales(), and once registered its lazy loader is no longer listed); re-registering a code replaces it in place, including the current one.',
    plugins: plugin({ locales: [en, es], loaders: { he: loader('load-he', { resolves: he }) } }),
    steps: [
      start,
      i18n('registerLocale', ar),
      i18n('registerLocale', he),
      i18n('listLocales'),
      i18n('setLocale', 'he'),
      ...reads,
      i18n('registerLocale', { ...he, name: 'Hebrew', dir: 'ltr', translations: { commands: { save: 'Save (he)' } } }),
      ...reads,
      t('commands.save'),
      i18n('registerLocale', { ...en, translations: {} }),
      t('commands.zoom.in'),
    ],
  },
  {
    name: 'register-returns-a-remover',
    covers:
      'registerLocale returns a remover that dispatches UNREGISTER_LOCALE: the pack leaves listLocales() (a lazy loader for the code is listed again), the current locale stays but loses its pack, and a second call dispatches again and changes nothing.',
    plugins: plugin({ locales: [en], loaders: { he: loader('load-he', { resolves: he }) } }),
    steps: [
      start,
      i18n('registerLocale', ar),
      i18n('registerLocale', he),
      i18n('setLocale', 'ar'),
      ...reads,
      { op: 'call', target: { ref: 0 } },
      ...reads,
      t('commands.save'),
      { op: 'call', target: { ref: 1 } },
      i18n('listLocales'),
      { op: 'call', target: { ref: 0 } },
      i18n('getLocale'),
    ],
  },
  {
    name: 'add-translations-and-has-key',
    covers:
      'addTranslations merges into a registered pack (later keys win, branches merge); an unknown code, or a lazy one not yet loaded, throws not-found. hasKey is true for a key in the current or fallback pack, never warns, and misses a branch without a count.',
    plugins: plugin({ locales: [en, es], locale: 'es', loaders: { he: loader('load-he', { resolves: he }) } }),
    steps: [
      start,
      i18n('hasKey', 'commands.save'),
      i18n('hasKey', 'commands.zoom.in'),
      i18n('hasKey', 'commands.fit'),
      i18n('hasKey', 'pages'),
      i18n('addTranslations', 'es', { commands: { fit: 'Ajustar', zoom: { in: 'Acercar' } } }),
      t('commands.fit'),
      t('commands.zoom.in'),
      t('commands.zoom.out'),
      i18n('hasKey', 'commands.fit'),
      i18n('addTranslations', 'en', { commands: 'flat' }),
      t('commands.zoom.out'),
      i18n('addTranslations', 'xx', { a: 'b' }),
      i18n('addTranslations', 'he', { a: 'b' }),
      i18n('listLocales'),
    ],
  },
  {
    name: 'locale-events',
    covers:
      'onLocaleChanged fires once the current locale changed and is usable (a registered switch at once, a lazy one when its pack lands), never for the same locale. onLocaleLoadFailed carries the failure as PluginErrorInfo, and setLocale rejects operation-failed with the same info as details. A superseded lazy switch rejects operation-cancelled, an unknown code not-found. After unsubscribing, nothing fires.',
    plugins: plugin({
      locales: [en, es],
      loaders: { ar: loader('load-ar', { defer: 'ar' }), he: loader('load-he', { seq: [{ defer: 'he' }, { defer: 'he-2' }] }) },
    }),
    steps: [
      i18n('onLocaleChanged', { $fn: 'changed', returns: null }),
      i18n('onLocaleLoadFailed', { $fn: 'failed', returns: null }),
      start,
      i18n('setLocale', 'es'),
      i18n('setLocale', 'es'),
      i18n('setLocale', 'ar'),
      settle('ar', ar),
      i18n('setLocale', 'he'),
      fail('he', 'offline'),
      i18n('setLocale', 'he'),
      i18n('setLocale', 'en'),
      settle('he-2', he),
      i18n('setLocale', 'xx'),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 } },
      i18n('setLocale', 'es'),
      i18n('getLocale'),
    ],
  },
  {
    name: 'lazy-startup-locale-announces-nothing',
    covers:
      'A lazy startup locale is already the announced locale, so its pack landing fires no onLocaleChanged; the next switch away announces it as the previous locale.',
    plugins: plugin({ locales: [en], locale: 'ar', loaders: { ar: loader('load-ar', { defer: 'ar' }) } }),
    steps: [i18n('onLocaleChanged', { $fn: 'changed', returns: null }), start, settle('ar', ar), ...reads, i18n('setLocale', 'en')],
  },
  {
    name: 'destroy-releases-the-watch',
    covers:
      'After destroy the watch is gone: a load in flight still lands its REGISTER/SET on the destroyed kernel, and a later lazy switch dispatches LOAD_STARTED but loads nothing.',
    plugins: plugin({ locales: [en], loaders: { ar: loader('load-ar', { defer: 'ar' }), he: loader('load-he', { defer: 'he' }) } }),
    steps: [start, i18n('setLocale', 'ar'), destroy, settle('ar', ar), i18n('setLocale', 'he'), ...reads],
  },
  {
    name: 'proto-member-codes',
    covers:
      'A code naming an Object.prototype member reads as registered, as a JavaScript record lookup does: setLocale("toString") switches to it, dir() is ltr, t() falls back, and it never appears in locales().',
    plugins: plugin({ locales: [en], loaders: { valueOf: loader('load-valueOf', { resolves: fr }) } }),
    steps: [start, i18n('setLocale', 'toString'), ...reads, t('commands.save'), i18n('setLocale', 'valueOf'), ...reads],
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/i18n/core.json',
    describes: 'packages/plugin/i18n: translate, interpolate, negotiateLocale, initialI18nState and the reducer (`same` is reference identity).',
    functions: { translate, interpolate, negotiateLocale, initialI18nState: initial, i18nReducer: reduce },
    cases: coreCases,
  },
  {
    kind: 'function',
    file: 'plugins/i18n/plurals.json',
    describes:
      'packages/plugin/i18n: the CLDR category `translate` selects for each count, per locale tag (Intl.PluralRules under Node\'s ICU; the default locale is en-US; an invalid tag takes the catch arm).',
    functions: { plurals },
    cases: pluralCases,
  },
  { kind: 'scenario', file: 'plugins/i18n/plugin.json', describes: 'packages/plugin/i18n: the capability, the lazy-pack effect and config.', cases: scenarios },
];
