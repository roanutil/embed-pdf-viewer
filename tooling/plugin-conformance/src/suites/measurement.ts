/**
 * `packages/plugin/measurement`: page scale, calibration and measurement
 * readouts, run beside the real interaction and annotation plugins. Replayed
 * by `crates/plugins/measurement/tests/conformance.rs`.
 *
 * One harness shim, mirrored by `crates/plugins/measurement/src/harness.rs`:
 * while a `measurement` factory is current, a `console.error` whose message
 * starts `[measurement]` logs as the callback `console.error`. That is where
 * a throwing event listener's error goes.
 */
import type { AnyPlugin } from '@embedpdf/core';

import { measurementPlugin } from '../../../../packages/plugin/measurement/src/index';
import type { MeasurementConfig } from '../../../../packages/plugin/measurement/src/contract';
import { MeasurementToken } from '../../../../packages/plugin/measurement/src/host-contract';
import { initialMeasurementState, measurementReducer } from '../../../../packages/plugin/measurement/src/model';
import type { MeasurementAction, MeasurementState } from '../../../../packages/plugin/measurement/src/model';
import {
  DEFAULT_PRESETS,
  defaultMeasure,
  selectPageScale,
  withAreaUnit,
  withPrecision,
  withUnit,
} from '../../../../packages/plugin/measurement/src/scale';

import type { ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

import {
  CROP,
  CROP2,
  FLAGS,
  LOCAL,
  boot,
  created,
  hydrate,
  ix,
  layout,
  line,
  objRef,
  pr,
  settle,
  updated,
} from './annotation-fixtures';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "Pins the setScale preflight refusals, through the settled rejections. Its dispatched actions are compared; the state they build is not.",
} as const;
// Registers the `interaction` factory, so emit-one can run this suite alone.
import './interaction';

// ── the shim ──────────────────────────────────────────────────────────────────

let currentLog: PluginEnv['log'] | null = null;
onCaseEnd(() => {
  currentLog = null;
});
const previousError = console.error;
console.error = (...args: unknown[]) => {
  if (currentLog && typeof args[0] === 'string' && args[0].startsWith('[measurement]')) {
    currentLog('console.error', args);
    return;
  }
  previousError(...args);
};

registerPlugin('measurement', {
  factory: (config, env) => {
    currentLog = env.log;
    return measurementPlugin((config ?? {}) as MeasurementConfig) as AnyPlugin;
  },
  tokens: [MeasurementToken],
});

// ── fixtures ──────────────────────────────────────────────────────────────────

const rl = (unit: string, over: Record<string, unknown> = {}) => ({
  subtype: 'RL',
  ratio: `1 in = 1 ${unit}`,
  x: [{ unit, conversion: 1 / 72, precision: 100 }],
  distance: [{ unit, conversion: 1, precision: 100 }],
  area: [{ unit: `${unit}²`, conversion: 1, precision: 100 }],
  ...over,
});
const METRES = rl('m');
const FEET = rl('ft');
const GEO = { subtype: 'GEO' };
/** Feet and fractional inches, as an imported scale often is. */
const FEET_INCHES = rl('ft', {
  distance: [
    { unit: 'ft', conversion: 1, precision: 1 },
    { unit: 'in', conversion: 12, fraction: 'fraction', precision: 16 },
  ],
  area: [{ unit: 'ft²', conversion: 1, fraction: 'fraction', precision: 8 }],
});

const CROP3 = { left: 0, bottom: 0, right: 200, top: 200 };
const PAGES = [layout(0, 1), layout(1, 2, CROP2)];
const PAGES4 = [layout(0, 1), layout(1, 2, CROP2), layout(2, 3, CROP3), layout(3, 4, CROP3)];

type Dto = Record<string, unknown>;
const dimensionLine = (n: number, over: Dto = {}) =>
  line(n, {
    intent: 'LineDimension',
    measure: METRES,
    caption: { enabled: true, position: 'inline' },
    leader: { length: 12, extension: 5, offset: 0 },
    contents: '2 m',
    linePoints: { start: { x: 100, y: 700 }, end: { x: 244, y: 700 } },
    ...over,
  });

const docOf = (responses: ResponseSpec[], over: Dto = {}) => ({
  pages: PAGES,
  security: { allows: '*' as const },
  responses: [...responses, hydrate()],
  ...over,
});

const PLUGINS = [{ factory: 'interaction' }, { factory: 'annotation' }, { factory: 'measurement' }];
const withConfig = (config: MeasurementConfig, annotation: Dto = {}) => [
  { factory: 'interaction' },
  { factory: 'annotation', config: annotation },
  { factory: 'measurement', config },
];
const SNAP = ['measurement::a'];

// ── steps ─────────────────────────────────────────────────────────────────────

const m = (method: string, args: unknown[] = [], d = 'a'): Step => ({
  op: 'call',
  target: { token: 'measurement', doc: d },
  method,
  args,
});
const an = (method: string, args: unknown[] = [], d = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation', doc: d },
  method,
  args,
});
const emit = (event: Dto, d = 'a'): Step => ({ op: 'emit', doc: d, event });
const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
const fail = (label: string, message = 'refused', code = 'Refused'): Step => ({
  op: 'settle',
  label,
  error: { name: 'EngineError', message, code },
});
/** Subscribe every event hook to a named callback. */
const listen: Step[] = [
  m('onScaleChanged', [{ $fn: 'scaleChanged' }]),
  m('onCalibrationRequested', [{ $fn: 'calibrationRequested' }]),
  m('onCalibrationCompleted', [{ $fn: 'calibrationCompleted' }]),
  m('onCalibrationDismissed', [{ $fn: 'calibrationDismissed' }]),
];
const drag = (tool: string, from: [number, number], to: [number, number], page = pr(1)): Step[] => [
  an('createPointer', [tool, 'down', page, { x: from[0], y: from[1] }]),
  an('createPointer', [tool, 'move', page, { x: (from[0] + to[0]) / 2, y: (from[1] + to[1]) / 2 }]),
  an('createPointer', [tool, 'up', page, { x: to[0], y: to[1] }]),
];
const viewports = (pon: number, value: unknown, over: Dto = {}): ResponseSpec => ({
  path: 'page.measure.viewports',
  pon,
  value,
  ...over,
});
const vp = (bbox: Dto, measure: unknown, owned = false, name?: string) => ({
  bbox,
  ...(name === undefined ? {} : { name }),
  ...(measure === undefined ? {} : { measure }),
  owned,
});
const remeasured = (n: number, measure: unknown, pon = 1): ResponseSpec => ({
  path: 'page.annotations.update',
  pon,
  value: updated(dimensionLine(n, { measure, ...(pon === 1 ? {} : { pageObjectNumber: pon }) })),
});

// ── scenarios ─────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  {
    name: 'reads-without-a-measure-service',
    covers:
      'Without an engine measure service every page loads at once onto the metric default (1:1 in metres, not persistent). The reads: the scale (a page not in the document reads the loading scale), the twins, the busy flag, the last reports, the presets, the units and the area units, the pending request. measureDistance and measureArea convert page points through the page geometry and read the page scale; a page not in the document throws not-found. getReadout reads a dimension annotation and answers not-dimension for an unknown ref. ensureLoaded resolves at once for a loaded page and rejects not-found for a page not in the document.',
    plugins: PLUGINS,
    engine: { documents: { a: docOf([hydrate({ 1: [dimensionLine(1)] })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('getPageScale', [pr(2)]),
      m('getPageScale', [pr(9)]),
      m('canCalibrate'),
      m('canMeasure', [pr(1)]),
      m('canMeasure', [pr(9)]),
      m('isBusy'),
      m('listLastReports'),
      m('listPresets'),
      m('listUnits'),
      m('listAreaUnits'),
      m('getCalibrationRequest'),
      m('measureDistance', [pr(1), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('measureDistance', [pr(2), { x: 10, y: 10 }, { x: 10, y: 154 }]),
      m('measureArea', [pr(1), [{ x: 0, y: 0 }, { x: 72, y: 0 }, { x: 72, y: 72 }, { x: 0, y: 72 }]]),
      m('measureArea', [pr(1), [{ x: 0, y: 0 }, { x: 72, y: 0 }]]),
      m('measureDistance', [pr(9), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('measureArea', [pr(9), [{ x: 0, y: 0 }]]),
      m('getReadout', [objRef(1)]),
      m('getReadout', [objRef(77)]),
      m('ensureLoaded', [pr(1)]),
      m('ensureLoaded', [pr(9)]),
    ],
  },
  {
    name: 'imperial-default-and-custom-presets',
    covers:
      "defaultScale 'imperial' makes the fallback 1:1 in feet, and presets replaces the picker. A preset whose paper length is not 1 divides the real length by it.",
    plugins: withConfig({
      defaultScale: 'imperial',
      presets: [
        { id: 'site', label: '1:500', paper: 1, real: 500, unit: 'm' },
        { id: 'plan', label: '2 in = 300 ft', paper: 2, real: 300, unit: 'ft' },
      ],
    }),
    engine: { documents: { a: docOf([]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('listPresets'),
      m('measureDistance', [pr(1), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('setPreset', [pr(1), 'plan', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
    ],
  },
  {
    name: 'custom-default-measure',
    covers:
      'A PdfMeasure as defaultScale is the fallback as it is, whatever the page userUnit; setPreset scales the ratio by the page userUnit.',
    plugins: withConfig({ defaultScale: FEET_INCHES as never }),
    engine: {
      documents: {
        a: docOf([], { pages: [layout(0, 1), { ...layout(1, 2, CROP2), userUnit: 2 }] }),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('getPageScale', [pr(2)]),
      m('measureDistance', [pr(2), { x: 10, y: 10 }, { x: 10, y: 100 }]),
      m('setPreset', [pr(2), 'imperial-quarter']),
      m('getPageScale', [pr(2)]),
      m('clearScale', [pr(2), { recalculate: false }]),
      m('getPageScale', [pr(2)]),
    ],
  },
  {
    name: 'measure-service-viewports',
    covers:
      "With the engine's measure service each page reads its viewports. An owned viewport wins wherever it is; otherwise the viewport under the crop's centre (the last containing one) is a foreign scale, one without a measure reads null, and a page whose viewports miss the centre falls back to the default. Every page is persistent. canMeasure is ready on each; a foreign scale gives readouts of foreign-measure.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            hydrate({ 1: [dimensionLine(1)] }),
            viewports(1, [vp({ left: 0, bottom: 0, right: 50, top: 50 }, METRES), vp({ left: 0, bottom: 0, right: 10, top: 10 }, FEET, true, 'EmbedPDF')]),
            viewports(2, [vp(CROP2, METRES, false, 'outer'), vp({ left: 100, bottom: 200, right: 300, top: 400 }, GEO, false, 'inner')]),
            viewports(3, [vp(CROP3, undefined)]),
            viewports(4, [vp({ left: 0, bottom: 0, right: 10, top: 10 }, METRES)]),
          ],
          { pages: PAGES4, services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('getPageScale', [pr(2)]),
      m('getPageScale', [pr(3)]),
      m('getPageScale', [pr(4)]),
      m('canMeasure', [pr(3)]),
      m('measureDistance', [pr(1), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('measureDistance', [pr(2), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('measureDistance', [pr(3), { x: 0, y: 0 }, { x: 72, y: 0 }]),
      m('getReadout', [objRef(1)]),
    ],
  },
  {
    name: 'viewport-reads-fail-refresh-and-rehydrate',
    covers:
      "A refused viewport read leaves the page not ready with the serialized error; ensureLoaded retries it. A page.viewportsChanged event re-reads that page (one not in the document is refused quietly), stream.desynced and document.versioned re-read every page, and any other event is ignored. While a read is in flight the page reads not ready with its previous scale.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, undefined, { times: 1, error: { name: 'EngineError', message: 'offline', code: 'Unavailable' } }),
            viewports(1, [vp({ left: 0, bottom: 0, right: 600, top: 800 }, METRES, true)], { times: 1 }),
            viewports(1, undefined, { times: 1, defer: 'v1' }),
            viewports(1, [vp({ left: 0, bottom: 0, right: 600, top: 800 }, FEET, true)]),
            viewports(2, []),
          ],
          { services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('canMeasure', [pr(1)]),
      m('ensureLoaded', [pr(1)]),
      m('getPageScale', [pr(1)]),
      emit({ type: 'page.viewportsChanged', page: pr(1) }),
      m('getPageScale', [pr(1)]),
      m('isBusy'),
      settle('v1', [vp({ left: 0, bottom: 0, right: 600, top: 800 }, FEET, true)]),
      m('getPageScale', [pr(1)]),
      emit({ type: 'page.viewportsChanged', page: pr(9) }),
      emit({ type: 'stream.desynced' }),
      emit({ type: 'document.versioned' }),
      emit({ type: 'metadata.changed' }),
      m('getPageScale', [pr(2)]),
    ],
  },
  {
    name: 'refresh-epochs',
    covers:
      'Viewport reads carry an epoch per page: an older read that answers after a newer one began is dropped, whether it resolves or rejects, and ensureLoaded joins the read in flight rather than starting another.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, undefined, { times: 1, defer: 'first' }),
            viewports(1, undefined, { times: 1, defer: 'second' }),
            viewports(1, undefined, { times: 1, defer: 'third' }),
            viewports(1, undefined, { times: 1, defer: 'fourth' }),
            viewports(2, []),
          ],
          { services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('ensureLoaded', [pr(1)]),
      emit({ type: 'page.viewportsChanged', page: pr(1) }),
      m('ensureLoaded', [pr(1)]),
      settle('first', [vp(CROP, METRES, true)]),
      m('getPageScale', [pr(1)]),
      settle('second', [vp(CROP, FEET, true)]),
      m('getPageScale', [pr(1)]),
      emit({ type: 'page.viewportsChanged', page: pr(1) }),
      emit({ type: 'page.viewportsChanged', page: pr(1) }),
      fail('third', 'stale'),
      m('getPageScale', [pr(1)]),
      fail('fourth', 'gone'),
      m('getPageScale', [pr(1)]),
    ],
  },
  {
    name: 'set-scale-on-one-page',
    covers:
      "Without a measure service setScale publishes an owned 'EmbedPDF' viewport over the crop, keeping foreign ones, re-measures the page's dimensions through the annotation plugin and announces the change; isBusy is true while it runs and listLastReports keeps the reports. recalculate false skips the re-measure and reports the scale alone. clearScale drops the owned viewport and re-measures with the default. A single-page change rejects: a measure that is not writable, and a page not in the document.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          hydrate({ 1: [dimensionLine(1), dimensionLine(2, { flags: FLAGS({ locked: true }) })] }),
          { path: 'page.annotations.update', pon: 1, times: 1, defer: 'write' },
          remeasured(1, METRES),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listen,
      m('setScale', [pr(1), FEET]),
      m('isBusy'),
      settle('write', updated(dimensionLine(1, { measure: FEET }))),
      m('isBusy'),
      m('getPageScale', [pr(1)]),
      m('listLastReports'),
      m('setScale', [pr(1), METRES, { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('clearScale', [pr(1)]),
      m('getPageScale', [pr(1)]),
      m('setScale', [pr(1), GEO]),
      m('setScale', [pr(1), { ...FEET, x: 'nope' }]),
      m('setScale', [pr(9), FEET]),
      m('listLastReports'),
    ],
  },
  {
    name: 'set-scale-on-several-pages',
    covers:
      "'all' and a list of pages change each page and report per page: a page that fails (not in the document, a measure that is not writable, a scale without a rectilinear base) reports scaleError instead of rejecting. A list of one page is still strict.",
    plugins: PLUGINS,
    engine: { documents: { a: docOf([hydrate({ 1: [dimensionLine(1)], 2: [dimensionLine(20, { pageObjectNumber: 2 })] }), remeasured(1, FEET), remeasured(20, FEET, 2)]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listen,
      m('setScale', ['all', FEET]),
      m('setScale', [[pr(1), pr(9)], FEET, { recalculate: false }]),
      m('setScale', ['all', GEO]),
      m('setScale', [[pr(9)], FEET]),
      m('setUnit', [[pr(1), pr(2)], 'mm', { recalculate: false }]),
      m('listLastReports'),
    ],
  },
  {
    name: 'set-scale-preflight',
    covers:
      "A single-page setScale runs the write preflight and rejects with its message: a conversion that is not positive, a precision that is not a whole number or is past the int32 range, a format whose unit is not a string, a text member that is not a string, a missing distance list, and an origin outside the PDF float range. A decimal format without a precision defaults to 100, and a label position, a fixed flag, a finite origin and a positive CYX all pass. calibrate writes the ratio rounded to eight significant digits.",
    plugins: PLUGINS,
    engine: { documents: { a: docOf([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 0, precision: 100 }] }), { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 1.5 }] }), { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 1e10 }] }), { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 5, conversion: 1, precision: 100 }] }), { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 100, thousands: 5 }] }), { recalculate: false }]),
      m('setScale', [pr(1), { subtype: 'RL', ratio: '1:1', x: METRES.x, area: METRES.area }, { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { origin: { x: 1e39, y: 0 } }), { recalculate: false }]),
      m('setScale', [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 1 }] }), { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m(
        'setScale',
        [pr(1), rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 100, labelPosition: 'suffix', fixed: true }], origin: { x: 10, y: 20 }, cyx: 1 }), { recalculate: false }],
      ),
      m('getPageScale', [pr(1)]),
      m('calibrate', [{ page: pr(1), from: { x: 0, y: 0 }, to: { x: 7, y: 0 }, distance: { value: 1, unit: 'm' } }, { recalculate: false }]),
      m('getPageScale', [pr(1)]),
    ],
  },
  {
    name: 'units-precision-and-presets',
    covers:
      "setUnit, setAreaUnit and setPrecision edit the page's rectilinear scale (the default counts) and write it back; a fractional last format restarts at a precision of 100. An invalid precision, a base unit that is not a length unit, and an unknown preset reject; setPreset writes the preset's ratio.",
    plugins: PLUGINS,
    engine: { documents: { a: docOf([]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('setUnit', [pr(1), 'ft', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setAreaUnit', [pr(1), 'acre', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setPrecision', [pr(1), 1000, { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setPrecision', [pr(1), 3]),
      m('setScale', [pr(1), FEET_INCHES, { recalculate: false }]),
      m('setUnit', [pr(1), 'in', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setScale', [pr(1), FEET_INCHES, { recalculate: false }]),
      m('setAreaUnit', [pr(1), 'ha', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setScale', [pr(1), rl('furlong'), { recalculate: false }]),
      m('setUnit', [pr(1), 'm']),
      m('setAreaUnit', [pr(1), 'm2']),
      m('setPreset', [pr(1), 'metric-50', { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('setPreset', [pr(1), 'nope']),
    ],
  },
  {
    name: 'service-writes-and-foreign-scales',
    covers:
      "With a measure service a scale write goes to the engine, then the page re-reads its viewports. A refused engine write rejects a single-page change, and the next change on that page still runs. Unit and precision edits need a rectilinear scale: a foreign one, or a viewport without a measure, rejects not-ready. Two changes on one page queue behind each other.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, [vp(CROP, GEO)], { times: 1 }),
            viewports(1, [vp(CROP, FEET, true)], { times: 1 }),
            viewports(1, [vp(CROP, METRES, true)]),
            viewports(2, [vp(CROP2, undefined)]),
            { path: 'page.measure.setScale', pon: 1, times: 1, defer: 'set1' },
            { path: 'page.measure.setScale', pon: 1, times: 1, error: { name: 'EngineError', message: 'read-only', code: 'Refused' } },
            { path: 'page.measure.setScale', pon: 1, value: null },
          ],
          { services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('getPageScale', [pr(1)]),
      m('setUnit', [pr(1), 'ft']),
      m('setPrecision', [pr(2), 10]),
      m('setScale', [pr(1), FEET, { recalculate: false }]),
      m('setScale', [pr(1), METRES, { recalculate: false }]),
      m('setScale', [pr(1), METRES, { recalculate: false }]),
      settle('set1', null),
      m('getPageScale', [pr(1)]),
      m('listLastReports'),
    ],
  },
  {
    name: 'calibration-flow',
    covers:
      "startCalibration clears any request and arms the calibrate tool. The tool's captured line becomes the pending request in page space (the pointer tool comes back, onCalibrationRequested fires); calibrate derives the scale from the known length, writes it, clears the request for its page and fires onCalibrationCompleted. A request on another page survives a calibrate elsewhere; dismissCalibration drops it and fires onCalibrationDismissed, and is silent with nothing pending. applyTo widens the write. Two identical points reject invalid-input; a page not in the document rejects not-found. A capture tool other than calibrate is ignored.",
    plugins: withConfig({}, { tools: [{ id: 'ruler', extends: 'line', clickCreate: false, meta: { capture: true } }] }),
    engine: {
      documents: {
        a: docOf([hydrate({ 1: [dimensionLine(1)] }), remeasured(1, METRES)]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listen,
      m('startCalibration'),
      ix('getActiveToolId'),
      ...drag('calibrate', [100, 100], [244, 100]),
      ix('getActiveToolId'),
      m('getCalibrationRequest'),
      m('calibrate', [{ page: pr(1), from: { x: 100, y: 100 }, to: { x: 244, y: 100 }, distance: { value: 2, unit: 'm' } }]),
      m('getCalibrationRequest'),
      m('getPageScale', [pr(1)]),
      ...drag('calibrate', [10, 10], [10, 110], pr(2)),
      m('calibrate', [{ page: pr(1), from: { x: 0, y: 0 }, to: { x: 0, y: 72 }, distance: { value: 1, unit: 'ft' } }, { applyTo: [pr(1), pr(2)], recalculate: false }]),
      m('getCalibrationRequest'),
      m('startCalibration'),
      m('getCalibrationRequest'),
      ...drag('calibrate', [10, 10], [10, 110], pr(2)),
      m('dismissCalibration'),
      m('dismissCalibration'),
      m('getCalibrationRequest'),
      ...drag('ruler', [10, 10], [110, 10]),
      m('getCalibrationRequest'),
      m('calibrate', [{ page: pr(1), from: { x: 5, y: 5 }, to: { x: 5, y: 5 }, distance: { value: 1, unit: 'm' } }]),
      m('calibrate', [{ page: pr(1), from: { x: 5, y: 5 }, to: { x: 50, y: 5 }, distance: { value: -1, unit: 'm' } }]),
      m('calibrate', [{ page: pr(9), from: { x: 5, y: 5 }, to: { x: 50, y: 5 }, distance: { value: 1, unit: 'm' } }]),
    ],
  },
  {
    name: 'zero-length-capture',
    covers: 'A calibrate tool configured to click-create a zero-length line captures it, and a zero-length capture makes no request.',
    plugins: withConfig({}, { tools: [{ id: 'calibrate', clickCreate: { length: 0 } }] }),
    engine: { documents: { a: docOf([]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('onCalibrationRequested', [{ $fn: 'calibrationRequested' }]),
      an('onDraftCaptured', [{ $fn: 'captured' }]),
      an('createPointer', ['calibrate', 'down', pr(1), { x: 10, y: 10 }]),
      an('createPointer', ['calibrate', 'up', pr(1), { x: 10, y: 10 }]),
      m('getCalibrationRequest'),
    ],
  },
  {
    name: 'without-authority',
    covers:
      'Read-only: canCalibrate and canMeasure are false, every scale change rejects permission-denied before touching a page, startCalibration does nothing, and createMeasurement rejects permission-denied.',
    plugins: PLUGINS,
    engine: { documents: { a: docOf([], { security: { allows: ['doc.annotate.read'] } }) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('canCalibrate'),
      m('canMeasure', [pr(1)]),
      m('setScale', [pr(1), FEET]),
      m('clearScale', ['all']),
      m('calibrate', [{ page: pr(1), from: { x: 0, y: 0 }, to: { x: 72, y: 0 }, distance: { value: 1, unit: 'm' } }]),
      m('startCalibration'),
      ix('getActiveToolId'),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [{ x: 0, y: 0 }, { x: 10, y: 0 }] }]),
      m('isBusy'),
    ],
  },
  {
    name: 'create-measurement',
    covers:
      "createMeasurement creates a line (distance), a polyline (perimeter) or a polygon (area) through the annotation plugin, with the kind's own tool unless one is named. A distance needs exactly two points; a page whose scale is not known rejects not-ready.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(dimensionLine(40)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(line(41, { subtype: 'polyline', intent: 'PolyLineDimension', measure: METRES, vertices: [{ x: 1, y: 1 }, { x: 50, y: 1 }, { x: 50, y: 50 }] })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(line(42, { subtype: 'polygon', intent: 'PolygonDimension', measure: METRES, vertices: [{ x: 1, y: 1 }, { x: 50, y: 1 }, { x: 50, y: 50 }] })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [{ x: 100, y: 100 }, { x: 244, y: 100 }] }]),
      m('createMeasurement', [{ kind: 'perimeter', page: pr(1), points: [{ x: 1, y: 1 }, { x: 50, y: 1 }, { x: 50, y: 50 }] }]),
      m('createMeasurement', [{ kind: 'area', page: pr(1), points: [{ x: 1, y: 1 }, { x: 50, y: 1 }, { x: 50, y: 50 }], tool: 'area' }]),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [{ x: 1, y: 1 }] }]),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [] }]),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [{ x: 1, y: 1 }, { x: 2, y: 2 }, { x: 3, y: 3 }] }]),
      m('createMeasurement', [{ kind: 'distance', page: pr(9), points: [{ x: 1, y: 1 }, { x: 2, y: 2 }] }]),
      m('getReadout', [objRef(40)]),
    ],
  },
  {
    name: 'listener-throws',
    covers: "A throwing listener goes to console.error('[measurement] listener failed:', error); the change still resolves. An unsubscribed listener hears nothing more.",
    plugins: PLUGINS,
    engine: { documents: { a: docOf([]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('onScaleChanged', [{ $fn: 'loud', throws: { name: 'Error', message: 'listener broke' } }]),
      m('onCalibrationDismissed', [{ $fn: 'dismissed' }]),
      m('setScale', [pr(1), FEET, { recalculate: false }]),
      { op: 'call', target: { ref: 0 } },
      m('setScale', [pr(1), METRES, { recalculate: false }]),
    ],
  },
  {
    name: 'pages-inserted-and-removed',
    covers:
      "A page inserted after the plugin connected loads its viewports; a structural event that keeps the same pages loads nothing. A page removed while its viewport read is in flight is not published, and a later change on it rejects not-found.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, []),
            viewports(2, [], { times: 1 }),
            viewports(2, undefined, { times: 1, defer: 'late' }),
            viewports(3, [vp(CROP3, FEET, true)]),
            { path: 'page.annotations.list', pon: 3, value: { annotations: [] } },
            // The engine knows page 3 from the start; the registry learns it from the insert.
            { path: 'pages.list', value: { pageCount: 2, pages: PAGES } },
          ],
          { pages: PAGES4.slice(0, 3), services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'pages.inserted', insertedPages: [pr(3)], origin: LOCAL, layout: { pageCount: 3, pages: PAGES4.slice(0, 3) } }),
      m('getPageScale', [pr(3)]),
      emit({ type: 'pages.rotated', pages: [pr(1)], rotation: 0, origin: LOCAL, layout: { pageCount: 3, pages: PAGES4.slice(0, 3) } }),
      emit({ type: 'page.viewportsChanged', page: pr(2) }),
      emit({ type: 'pages.deleted', pages: [pr(2)], origin: LOCAL, layout: { pageCount: 2, pages: [PAGES4[0], PAGES4[2]] } }),
      settle('late', [vp(CROP2, FEET, true)]),
      m('getPageScale', [pr(2)]),
      m('setScale', [pr(2), FEET, { recalculate: false }]),
    ],
  },
  {
    name: 'page-removed-while-a-change-waits',
    covers:
      "Without a measure service, a preset change queued behind another on the same page finds the page gone once it runs: the ratio falls back to a userUnit of 1 and the write rejects not-found.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          hydrate({ 1: [dimensionLine(1)] }),
          { path: 'page.annotations.update', pon: 1, times: 1, defer: 'write' },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('setScale', [pr(1), FEET]),
      m('setPreset', [pr(1), 'metric-10', { recalculate: false }]),
      emit({ type: 'pages.deleted', pages: [pr(1)], origin: LOCAL, layout: { pageCount: 1, pages: [PAGES[1]] } }),
      settle('write', updated(dimensionLine(1, { measure: FEET }))),
      m('isBusy'),
    ],
  },
  {
    name: 'close-mid-flight',
    covers:
      "Closing the document while changes run: a single-page change rejects with the engine call's instance-closed, a multi-page change whose pages all reported rejects not-ready (the plugin is disposed), neither touches the busy counter again, and a viewport read in flight is not published.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, [], { times: 1 }),
            viewports(1, undefined, { defer: 'read' }),
            viewports(2, []),
            { path: 'page.measure.setScale', pon: 1, defer: 'set1' },
            { path: 'page.measure.setScale', pon: 2, defer: 'set2' },
          ],
          { services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('setScale', ['all', FEET, { recalculate: false }]),
      m('setScale', [pr(2), METRES, { recalculate: false }]),
      emit({ type: 'page.viewportsChanged', page: pr(1) }),
      m('isBusy'),
      close(),
    ],
  },
];

// ── functions ─────────────────────────────────────────────────────────────────

const functions = {
  initialMeasurementState,
  measurementReducer: (state: MeasurementState, action: MeasurementAction) => {
    const next = measurementReducer(state, action);
    return { changed: next !== state, state: next };
  },
  defaultPresets: () => DEFAULT_PRESETS,
  defaultMeasure,
  selectPageScale,
  withUnit,
  withAreaUnit,
  withPrecision,
};

const LOADING = { measure: null, source: 'default', ready: false, persistent: false };
const loaded: MeasurementState = {
  pages: { 1: { viewports: [], scale: { ...LOADING, ready: true } as never } },
  pending: 1,
  calibration: null,
  reports: [],
};
const fc = (name: string, fn: string, args: unknown[], covers?: string): FunctionCase => ({
  name,
  fn,
  args,
  ...(covers === undefined ? {} : { covers }),
});

const functionCases: FunctionCase[] = [
  fc('initial-state', 'initialMeasurementState', []),
  fc('reduce-page-scale', 'measurementReducer', [loaded, { type: 'PAGE_SCALE', page: pr(2), viewports: [vp(CROP, METRES, true)], scale: { measure: METRES, source: 'owned', ready: true, persistent: false } }]),
  fc('reduce-pending-up', 'measurementReducer', [loaded, { type: 'PENDING', delta: 1 }]),
  fc('reduce-pending-floors-at-zero', 'measurementReducer', [loaded, { type: 'PENDING', delta: -3 }], 'The busy counter never goes below zero.'),
  fc('reduce-calibration', 'measurementReducer', [loaded, { type: 'CALIBRATION', request: { page: pr(1), from: { x: 0, y: 0 }, to: { x: 1, y: 0 }, userSpaceLength: 1 } }]),
  fc('reduce-reports', 'measurementReducer', [loaded, { type: 'REPORTS', reports: [{ page: pr(1), updated: [], skipped: [], failed: [], scaleError: { name: 'EngineError', code: 'Unknown', message: 'x' } }] }]),
  fc('reduce-unknown', 'measurementReducer', [loaded, { type: 'NOPE' }], 'An unknown action returns the same state.'),
  fc('default-presets', 'defaultPresets', []),
  fc('default-measure-metric', 'defaultMeasure', [{}]),
  fc('default-measure-imperial-user-unit', 'defaultMeasure', [{ defaultScale: 'imperial' }, 2]),
  fc('default-measure-explicit-metric', 'defaultMeasure', [{ defaultScale: 'metric' }, 0.5]),
  fc('default-measure-object', 'defaultMeasure', [{ defaultScale: FEET }, 3]),
  fc('default-measure-bad-user-unit', 'defaultMeasure', [{}, 0]),
  fc('select-owned', 'selectPageScale', [[vp(CROP, METRES), vp({ left: 0, bottom: 0, right: 1, top: 1 }, FEET, true)], CROP, METRES, true]),
  fc('select-last-containing', 'selectPageScale', [[vp(CROP, METRES), vp(CROP, FEET), vp({ left: 0, bottom: 0, right: 1, top: 1 }, GEO)], CROP, METRES, false]),
  fc('select-foreign-without-measure', 'selectPageScale', [[vp(CROP, undefined)], CROP, METRES, false]),
  fc('select-default', 'selectPageScale', [[vp({ left: 0, bottom: 0, right: 1, top: 1 }, FEET)], CROP, METRES, false]),
  fc('select-none', 'selectPageScale', [[], CROP2, FEET, true]),
  fc('select-beside-the-centre', 'selectPageScale', [[vp({ left: 0, bottom: 0, right: 1, top: 800 }, FEET)], CROP, METRES, false], 'A viewport that spans the centre\'s height but not its x is not under it.'),
  fc('select-on-a-raised-crop', 'selectPageScale', [[vp({ left: 0, bottom: 450, right: 600, top: 900 }, FEET)], { left: 0, bottom: 100, right: 600, top: 900 }, METRES, false], 'The centre of a crop that does not start at 0 is the midpoint of its bottom and top.'),
  fc('with-unit', 'withUnit', [METRES, 'ft']),
  fc('with-unit-and-area', 'withUnit', [FEET, 'in', 'acre']),
  fc('with-unit-from-fraction', 'withUnit', [FEET_INCHES, 'yd']),
  fc('with-unit-keeps-precision', 'withUnit', [{ ...METRES, distance: [{ unit: 'm', conversion: 1, precision: 1000 }] }, 'ft'], 'A decimal last format keeps its precision.'),
  fc('with-unit-no-precision', 'withUnit', [{ ...METRES, distance: [{ unit: 'm', conversion: 1 }] }, 'km']),
  fc('with-unit-no-distance', 'withUnit', [{ ...METRES, distance: [] }, 'mi']),
  fc('with-unit-unknown-base', 'withUnit', [rl('furlong'), 'm']),
  fc('with-unit-no-x', 'withUnit', [{ ...METRES, x: [] }, 'm']),
  fc('with-unit-padded-base', 'withUnit', [{ ...METRES, x: [{ unit: ' m ', conversion: 1 / 72 }] }, 'cm']),
  fc('with-area-unit', 'withAreaUnit', [METRES, 'ha']),
  fc('with-area-unit-from-fraction', 'withAreaUnit', [FEET_INCHES, 'yd2']),
  fc('with-area-unit-no-precision', 'withAreaUnit', [{ ...METRES, area: [{ unit: 'm²', conversion: 1 }] }, 'pt2']),
  fc('with-area-unit-no-area', 'withAreaUnit', [{ ...METRES, area: [] }, 'mm2']),
  fc('with-area-unit-unknown-base', 'withAreaUnit', [rl('furlong'), 'acre']),
  fc('with-area-unit-no-x', 'withAreaUnit', [{ ...METRES, x: [] }, 'acre']),
  fc('with-precision', 'withPrecision', [FEET_INCHES, 1000]),
  fc('with-precision-one', 'withPrecision', [METRES, 1]),
  fc('with-precision-max', 'withPrecision', [METRES, 1e9]),
  fc('with-precision-too-big', 'withPrecision', [METRES, 1e10]),
  fc('with-precision-not-power', 'withPrecision', [METRES, 50]),
  fc('with-precision-fraction', 'withPrecision', [METRES, 0.1]),
  fc('with-precision-empty-lists', 'withPrecision', [{ ...METRES, distance: [], area: [] }, 10]),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/measurement.json',
    describes:
      'packages/plugin/measurement: page scale from the engine viewports or the default, scale writes and re-measurement, units, precision and presets, the calibration flow, readouts and measurement creation, run beside the real interaction and annotation plugins.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/measurement-functions.json',
    describes: 'packages/plugin/measurement: the slice reducer, the default presets and scale, page-scale selection, and the unit, area-unit and precision edits.',
    functions: functions as never,
    cases: functionCases,
  },
];
