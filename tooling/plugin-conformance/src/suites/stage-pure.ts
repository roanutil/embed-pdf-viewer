/**
 * `packages/plugin/stage`'s pure modules: destinations, settings, motion
 * laws, responsive rules and the reducer. Replayed by
 * `crates/plugins/stage/tests/conformance.rs`.
 *
 * Three wrappers stand in for what a function case cannot carry, and
 * `crates/plugins/stage/src/harness.rs` mirrors each:
 *
 *   - `resolveResponsive`: a rule whose `when` is `{"$returns": v}` is the
 *     predicate `() => v`, so a function rule is exercised without a closure
 *     in the vector.
 *   - `stageReducer` reports `{state, same}`, `same` being `next === state`:
 *     the reference identity the store keys its notification on.
 *   - `constants` reports the registry tables and the motion constants.
 */
import { destinationToReveal } from '../../../../packages/plugin/stage/src/destination';
import {
  FLING_DECAY,
  FLING_STOP,
  RUBBER,
  SPRING_OMEGA,
  easeOutCubic,
  glideStep,
  rubberIn,
  rubberOut,
  springStep,
  zoomLerp,
} from '../../../../packages/plugin/stage/src/motion';
import { initialStageState, stageReducer } from '../../../../packages/plugin/stage/src/reducer';
import {
  boxOf,
  eqSetting,
  matchesQuery,
  mergeSettings,
  resolveResponsive,
} from '../../../../packages/plugin/stage/src/responsive';
import {
  DEFAULT_RESPONSIVE,
  DEFAULT_SETTINGS,
  SETTINGS_EFFECT,
  SETTING_KEYS,
  settingsEqual,
} from '../../../../packages/plugin/stage/src/settings';
import type {
  ResponsiveRule,
  StageAction,
  StageBox,
  StageConfig,
  StageSettings,
  StageState,
} from '../../../../packages/plugin/stage/src/types';

import type { FunctionCase, Suite } from '../vectors';

type Fn = (...args: never[]) => unknown;
const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

// ── destinations ──────────────────────────────────────────────────────────────

const box = (left: number, bottom: number, right: number, top: number) => ({ left, bottom, right, top });
const layout = (over: Record<string, unknown> = {}) => ({
  index: 3,
  pageObjectNumber: 12,
  label: null,
  size: { width: 612, height: 792 },
  rotation: 0,
  userUnit: 1,
  boxes: { media: box(0, 0, 612, 792), crop: box(0, 0, 612, 792) },
  ...over,
});
const letter = layout();
const cropped = layout({ size: { width: 600, height: 780 }, boxes: { media: box(0, 0, 612, 792), crop: box(10, 8, 610, 788) } });
const turned = layout({ rotation: 90 });
const scaled = layout({ userUnit: 2 });
const bbox = { x: 50, y: 60, width: 400, height: 500 };

const destinationCases: FunctionCase[] = [
  c('destinationToReveal', 'xyz-all', { kind: 'xyz', pageObjectNumber: 12, left: 100, top: 700, zoom: 1.5 }, letter),
  c('destinationToReveal', 'xyz-null-left-zero-zoom', { kind: 'xyz', pageObjectNumber: 12, left: null, top: 700, zoom: 0 }, letter),
  c('destinationToReveal', 'xyz-null-top', { kind: 'xyz', pageObjectNumber: 12, left: 100, top: null, zoom: null }, letter),
  c('destinationToReveal', 'xyz-absent', { kind: 'xyz', pageObjectNumber: 12 }, letter),
  c('destinationToReveal', 'xyz-cropped', { kind: 'xyz', pageObjectNumber: 12, left: 110, top: 708, zoom: 2 }, cropped),
  c('destinationToReveal', 'xyz-rotated', { kind: 'xyz', pageObjectNumber: 12, left: 100, top: 700, zoom: 1 }, turned),
  c('destinationToReveal', 'xyz-user-unit', { kind: 'xyz', pageObjectNumber: 12, left: 100, top: 700 }, scaled),
  c('destinationToReveal', 'xyz-negative-zoom', { kind: 'xyz', pageObjectNumber: 12, left: 0, top: 0, zoom: -1 }, letter),
  c('destinationToReveal', 'fit', { kind: 'fit', pageObjectNumber: 12 }, letter),
  c('destinationToReveal', 'fit-with-bbox', { kind: 'fit', pageObjectNumber: 12 }, letter, bbox),
  c('destinationToReveal', 'fitH', { kind: 'fitH', pageObjectNumber: 12, top: 700 }, letter),
  c('destinationToReveal', 'fitH-null', { kind: 'fitH', pageObjectNumber: 12, top: null }, letter),
  c('destinationToReveal', 'fitH-absent-cropped', { kind: 'fitH', pageObjectNumber: 12 }, cropped),
  c('destinationToReveal', 'fitH-cropped', { kind: 'fitH', pageObjectNumber: 12, top: 500 }, cropped),
  c('destinationToReveal', 'fitV', { kind: 'fitV', pageObjectNumber: 12, left: 200 }, letter),
  c('destinationToReveal', 'fitV-null', { kind: 'fitV', pageObjectNumber: 12, left: null }, cropped),
  c('destinationToReveal', 'fitV-cropped', { kind: 'fitV', pageObjectNumber: 12, left: 200 }, cropped),
  c('destinationToReveal', 'fitR', { kind: 'fitR', pageObjectNumber: 12, left: 100, bottom: 200, right: 300, top: 600 }, letter),
  c('destinationToReveal', 'fitR-inverted', { kind: 'fitR', pageObjectNumber: 12, left: 300, bottom: 600, right: 100, top: 200 }, cropped),
  c('destinationToReveal', 'fitB', { kind: 'fitB', pageObjectNumber: 12 }, letter),
  c('destinationToReveal', 'fitB-bbox', { kind: 'fitB', pageObjectNumber: 12 }, letter, bbox),
  c('destinationToReveal', 'fitBH', { kind: 'fitBH', pageObjectNumber: 12, top: 700 }, letter, bbox),
  c('destinationToReveal', 'fitBH-null-bbox', { kind: 'fitBH', pageObjectNumber: 12, top: null }, letter, bbox),
  c('destinationToReveal', 'fitBH-absent-page', { kind: 'fitBH', pageObjectNumber: 12 }, letter),
  c('destinationToReveal', 'fitBV', { kind: 'fitBV', pageObjectNumber: 12, left: 150 }, cropped, bbox),
  c('destinationToReveal', 'fitBV-null-bbox', { kind: 'fitBV', pageObjectNumber: 12, left: null }, letter, bbox),
  c('destinationToReveal', 'fitBV-absent-page', { kind: 'fitBV', pageObjectNumber: 12 }, letter),
  c('destinationToReveal', 'zero-user-unit-throws', { kind: 'fit', pageObjectNumber: 12 }, layout({ userUnit: 0 })),
  c('destinationToReveal', 'absent-user-unit', { kind: 'xyz', pageObjectNumber: 12, left: 5, top: 6 }, layout({ userUnit: { $undefined: true } })),
];

// ── settings ──────────────────────────────────────────────────────────────────

const S = DEFAULT_SETTINGS;
const settingsCases: FunctionCase[] = [
  c('settingsEqual', 'defaults', S, { ...S }),
  c('settingsEqual', 'fresh-objects', S, { ...S, fitAlign: { x: 'center', y: 'center' }, zoom: { mode: 'automatic' } }),
  c('settingsEqual', 'primitive-differs', S, { ...S, padding: 4 }),
  c('settingsEqual', 'object-member-differs', S, { ...S, arrivalAlign: { x: 'start', y: 0.35 } }),
  c('settingsEqual', 'number-vs-object', S, { ...S, gap: { px: 16 } }),
  c('settingsEqual', 'object-vs-number', { ...S, gap: { px: 16 } }, S),
  c('settingsEqual', 'gap-px-objects', { ...S, gap: { px: 16 } }, { ...S, gap: { px: 16 } }),
  c('settingsEqual', 'last-key-differs', S, { ...S, viewUnitsPerPoint: 1 }),
  c('settingsEqual', 'string-vs-string', S, { ...S, flow: 'paged' }),
  c('constants', 'all'),
];

// ── motion ────────────────────────────────────────────────────────────────────

const motionCases: FunctionCase[] = [
  c('rubberOut', 'zero', 0, 800),
  c('rubberOut', 'small', 10, 800),
  c('rubberOut', 'large', 5000, 320),
  c('rubberOut', 'far', 1e9, 800),
  c('rubberIn', 'zero', 0, 800),
  c('rubberIn', 'inverse', rubberOut(200, 800), 800),
  c('rubberIn', 'near-asymptote', 799, 800),
  c('rubberIn', 'at-asymptote', 800, 800),
  c('glideStep', 'moving', 0, 1.2, 16),
  c('glideStep', 'negative', 100, -2, 32),
  c('glideStep', 'stops', 50, 0.0201, 16),
  c('glideStep', 'stops-negative', 50, -0.02, 1),
  c('glideStep', 'zero-dt', 0, 1, 0),
  c('glideStep', 'exactly-the-stop', 0, 0.02, 0),
  c('springStep', 'far', 100, 0, 0, 16),
  c('springStep', 'moving', -40, 0.5, 0, 16),
  c('springStep', 'lands', 0.2, 0.001, 0, 16),
  c('springStep', 'near-but-fast', 0.2, 1, 0, 16),
  c('springStep', 'close-but-moving', 10.1, 0.0, 10, 1),
  c('springStep', 'exactly-half-a-pixel', 10.5, 0, 10, 0),
  c('springStep', 'exactly-the-speed-bound', 10, 0.01, 10, 0),
  c('easeOutCubic', 'zero', 0),
  c('easeOutCubic', 'half', 0.5),
  c('easeOutCubic', 'one', 1),
  c('easeOutCubic', 'quarter', 0.25),
  c('zoomLerp', 'start', 1, 4, 0),
  c('zoomLerp', 'half', 1, 4, 0.5),
  c('zoomLerp', 'end', 0.5, 2, 1),
  c('zoomLerp', 'down', 3, 1, 0.3),
];

// ── responsive ────────────────────────────────────────────────────────────────

const portrait: StageBox = { width: 500, height: 800, orientation: 'portrait' };
const landscape: StageBox = { width: 1200, height: 700, orientation: 'landscape' };

const responsiveCases: FunctionCase[] = [
  c('boxOf', 'portrait', { width: 500, height: 800 }),
  c('boxOf', 'landscape', { width: 1200, height: 700 }),
  c('boxOf', 'square', { width: 600, height: 600 }),
  c('boxOf', 'empty', { width: 0, height: 0 }),
  c('matchesQuery', 'empty-query', {}, portrait),
  c('matchesQuery', 'min-width-pass', { minWidth: 500 }, portrait),
  c('matchesQuery', 'min-width-fail', { minWidth: 501 }, portrait),
  c('matchesQuery', 'max-width-pass', { maxWidth: 500 }, portrait),
  c('matchesQuery', 'max-width-fail', { maxWidth: 499 }, portrait),
  c('matchesQuery', 'min-height-pass', { minHeight: 800 }, portrait),
  c('matchesQuery', 'min-height-fail', { minHeight: 801 }, portrait),
  c('matchesQuery', 'max-height-pass', { maxHeight: 800 }, portrait),
  c('matchesQuery', 'max-height-fail', { maxHeight: 799 }, portrait),
  c('matchesQuery', 'orientation-pass', { orientation: 'landscape' }, landscape),
  c('matchesQuery', 'orientation-fail', { orientation: 'portrait' }, landscape),
  c('matchesQuery', 'all-pass', { minWidth: 1000, maxWidth: 1300, minHeight: 600, maxHeight: 800, orientation: 'landscape' }, landscape),
  c('mergeSettings', 'patch', S, { padding: 4, gap: { px: 8 } }),
  c('mergeSettings', 'undefined-skipped', S, { padding: { $undefined: true }, layout: 'grid' }),
  c('mergeSettings', 'empty', S, {}),
  c('eqSetting', 'same-primitive', 4, 4),
  c('eqSetting', 'different-primitive', 4, 5),
  c('eqSetting', 'nan', { $number: 'NaN' }, { $number: 'NaN' }),
  c('eqSetting', 'strings', 'grid', 'grid'),
  c('eqSetting', 'objects-equal', { x: 'start', y: 0.5 }, { x: 'start', y: 0.5 }),
  c('eqSetting', 'objects-differ', { x: 'start', y: 0.5 }, { x: 'start', y: 0.25 }),
  c('eqSetting', 'object-primitive', { px: 4 }, 4),
  c('eqSetting', 'primitive-object', 4, { px: 4 }),
  c('resolveResponsive', 'defaults-compact', S, DEFAULT_RESPONSIVE, portrait),
  c('resolveResponsive', 'defaults-wide', S, DEFAULT_RESPONSIVE, landscape),
  c('resolveResponsive', 'no-rules', S, [], portrait),
  c(
    'resolveResponsive',
    'later-wins-per-key',
    S,
    [
      { name: 'a', when: { maxWidth: 600 }, settings: { padding: 4, gap: 8 } },
      { when: { orientation: 'portrait' }, settings: { padding: 2 } },
      { name: 'b', when: { minWidth: 700 }, settings: { gap: 99 } },
      { name: 'pure', when: {} },
      { name: 'c', when: { maxHeight: 900 }, settings: { layout: 'grid', columns: 'auto' } },
    ],
    portrait,
  ),
  c(
    'resolveResponsive',
    'predicates',
    S,
    [
      { name: 'yes', when: { $returns: 1 }, settings: { bounded: false } },
      { name: 'no', when: { $returns: 0 }, settings: { padding: 0 } },
      { name: 'empty-string', when: { $returns: '' } },
      { name: 'object', when: { $returns: { any: true } }, settings: { spread: 'odd' } },
    ],
    landscape,
  ),
  c('resolveResponsive', 'undefined-in-patch', S, [{ name: 'u', when: {}, settings: { padding: { $undefined: true } } }], portrait),
];

// ── reducer ───────────────────────────────────────────────────────────────────

const base = initialStageState({ viewUnitsPerPoint: 1 });
const reducerCases: FunctionCase[] = [
  c('initialStageState', 'empty', {}),
  c('initialStageState', 'overrides', { layout: 'grid', columns: 3, zoom: { level: 0.2 }, padding: 0, gap: { px: 8 } }),
  c('initialStageState', 'strips-capability-config', { responsive: [], scheduler: 'frames', flow: 'paged' }),
  c('stageReducer', 'camera', base, { type: 'CAMERA', camera: { x: 1, y: 2, zoom: 3 } }),
  c('stageReducer', 'camera-rest-change', base, { type: 'CAMERA_REST', resting: false }),
  c('stageReducer', 'camera-rest-same', base, { type: 'CAMERA_REST', resting: true }),
  c('stageReducer', 'placed', base, { type: 'PLACED' }),
  c('stageReducer', 'placed-again', { ...base, placed: true }, { type: 'PLACED' }),
  c('stageReducer', 'vp', base, { type: 'VP', vp: { width: 1000, height: 700 } }),
  c('stageReducer', 'dpr', base, { type: 'DPR', dpr: 2 }),
  c('stageReducer', 'cursor', base, { type: 'CURSOR', cursor: 4 }),
  c('stageReducer', 'motion-cause', base, { type: 'MOTION_CAUSE', cause: 'programmatic' }),
  c('stageReducer', 'patch', base, { type: 'PATCH', patch: { layout: 'horizontal', padding: { $undefined: true }, zoom: { level: 2 } } }),
  c('stageReducer', 'patch-empty', base, { type: 'PATCH', patch: {} }),
  c('stageReducer', 'responsive', base, { type: 'RESPONSIVE', active: ['compact'] }),
];

// ── the table ─────────────────────────────────────────────────────────────────

/** `when: {"$returns": v}` is the predicate `() => v`. */
const liveRules = (rules: readonly Record<string, unknown>[]): ResponsiveRule[] =>
  rules.map((rule) => {
    const when = rule.when as Record<string, unknown>;
    return '$returns' in when ? ({ ...rule, when: () => when.$returns } as unknown as ResponsiveRule) : (rule as unknown as ResponsiveRule);
  });

const functions: Record<string, Fn> = {
  destinationToReveal: destinationToReveal as Fn,
  settingsEqual: settingsEqual as Fn,
  constants: (() => ({
    DEFAULT_SETTINGS,
    DEFAULT_RESPONSIVE,
    SETTINGS_EFFECT,
    SETTING_KEYS,
    RUBBER,
    FLING_DECAY,
    FLING_STOP,
    SPRING_OMEGA,
  })) as Fn,
  rubberOut: rubberOut as Fn,
  rubberIn: rubberIn as Fn,
  glideStep: glideStep as Fn,
  springStep: springStep as Fn,
  easeOutCubic: easeOutCubic as Fn,
  zoomLerp: zoomLerp as Fn,
  boxOf: boxOf as Fn,
  matchesQuery: matchesQuery as Fn,
  mergeSettings: mergeSettings as Fn,
  eqSetting: eqSetting as Fn,
  resolveResponsive: ((b: StageSettings, rules: Record<string, unknown>[], bx: StageBox) =>
    resolveResponsive(b, liveRules(rules), bx)) as Fn,
  initialStageState: ((config: StageConfig) => initialStageState(config)) as Fn,
  stageReducer: ((state: StageState, action: StageAction) => {
    const next = stageReducer(state, action);
    return { state: next, same: next === state };
  }) as Fn,
};

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/stage-destination.json',
    describes: 'packages/plugin/stage/src/destination.ts: PDF destinations onto reveal options.',
    functions,
    cases: destinationCases,
  },
  {
    kind: 'function',
    file: 'plugins/stage-pure.json',
    describes: 'packages/plugin/stage: the settings registry, motion laws, responsive rules and reducer.',
    functions,
    cases: [...settingsCases, ...motionCases, ...responsiveCases, ...reducerCases],
  },
];
