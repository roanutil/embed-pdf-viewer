import { measureFromRatio, measureFromKnownLength } from '@embedpdf/engine-core/runtime';
/** Measurement oracle: current page mirrors, scale writes, calibration and engine annotations. */
import * as model from '../../../../packages/plugin/measurement/src/model';
import {
  DEFAULT_PRESETS,
  defaultMeasure,
  withAreaUnit,
  withPrecision,
  withUnit,
} from '../../../../packages/plugin/measurement/src/scale';
import type { FunctionCase, Suite } from '../vectors';
import type { ScenarioCase } from '../scenario';
import {
  open,
  refused,
  scaleSequence,
  layout,
  creation,
  created,
  CROP,
  CROP2,
  METRES,
  FEET,
  GEO,
  FEET_INCHES,
  rl,
  malformedMeasure,
  dimensionLine,
  PAGES,
  PAGES4,
  docOf,
  PLUGINS,
  withConfig,
  SNAP,
  m,
  an,
  emit,
  close,
  fail,
  listen,
  drag,
  vp,
  viewports,
  remeasured,
  boot,
  hydrate,
  objRef,
  pr,
  settle,
  LOCAL,
  updated,
  write,
  confirmation,
  meta,
  scaleFact,
  scaleAnswer,
  ix,
} from './measurement-fixtures';
const CROP3 = { x: 0, y: 0, width: 200, height: 200 };
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason:
    'Pins exact preflight refusals and successful scale recovery through calls and settlements.',
} as const;
const cases: ScenarioCase[] = [
  {
    name: 'reads-without-a-measure-service',
    covers:
      'The required current measure service returns empty viewports; pages load onto the metric default and are persistent. The historical optional-service contract is archived. The reads: the scale (a page not in the document reads the loading scale), the twins, the busy flag, the last reports, the presets, the units and the area units, the pending request. measureDistance and measureArea convert page points through the page geometry and read the page scale; a page not in the document returns an unavailable readout. getReadout reads a dimension annotation and answers not-dimension for an unknown ref. ensureLoaded resolves at once for a loaded page and rejects not-found for a page not in the document.',
    plugins: PLUGINS,
    engine: { documents: { a: docOf([hydrate([dimensionLine(1)])]) } },
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
      m('measureArea', [
        pr(1),
        [
          { x: 0, y: 0 },
          { x: 72, y: 0 },
          { x: 72, y: 72 },
          { x: 0, y: 72 },
        ],
      ]),
      m('measureArea', [
        pr(1),
        [
          { x: 0, y: 0 },
          { x: 72, y: 0 },
        ],
      ]),
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
    engine: { documents: { a: docOf([...scaleSequence(1, [measureFromRatio(2, 300, 'ft')])]) } },
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
    plugins: withConfig({ defaultScale: FEET_INCHES }),
    engine: {
      documents: {
        a: docOf([...scaleSequence(2, [measureFromRatio(1, 48, 'ft', { userUnit: 2 }), null])], {
          pages: [layout(0, 1), { ...layout(1, 2, CROP2), userUnit: 2 }],
        }),
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
      "With the engine's measure service each page reads its viewports. An owned viewport wins wherever it is; otherwise the viewport under the page-space centre (the last containing one) is a foreign scale, one without a measure reads null, and a page whose viewports miss the centre falls back to the default. Every page is persistent. canMeasure is ready on each; a foreign scale gives readouts of foreign-measure.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            hydrate([dimensionLine(1)]),
            viewports(1, [
              vp({ x: 0, y: 0, width: 50, height: 50 }, METRES),
              vp({ x: 0, y: 0, width: 10, height: 10 }, FEET, true, 'EmbedPDF'),
            ]),
            viewports(2, [
              vp({ x: 0, y: 0, width: 400, height: 500 }, METRES, false, 'outer'),
              vp({ x: 100, y: 200, width: 200, height: 200 }, GEO, false, 'inner'),
            ]),
            viewports(3, [vp(CROP3, undefined)]),
            viewports(4, [vp({ x: 0, y: 0, width: 10, height: 10 }, METRES)]),
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
      'A refused viewport read leaves the page not ready with the serialized error; ensureLoaded retries it. A pages.scaleSet event re-reads that page (one not in the document is refused quietly), stream.desynced and document.versioned re-read every page, and any other event is ignored. While a read is in flight the page reads not ready with its previous scale.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, undefined, {
              times: 1,
              error: { name: 'EngineError', message: 'offline', code: 'RuntimeUnavailable' },
            }),
            viewports(1, [vp({ x: 0, y: 0, width: 600, height: 800 }, METRES, true)], { times: 1 }),
            viewports(1, undefined, { times: 1, defer: 'v1' }),
            viewports(1, [vp({ x: 0, y: 0, width: 600, height: 800 }, FEET, true)]),
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
      emit(scaleFact(pr(1))),
      m('getPageScale', [pr(1)]),
      m('isBusy'),
      settle('v1', { viewports: [vp({ x: 0, y: 0, width: 600, height: 800 }, FEET, true)] }),
      m('getPageScale', [pr(1)]),
      emit(scaleFact(pr(9))),
      emit({ type: 'stream.desynced', reason: 'backlog-overflow', ts: 0 }),
      emit({
        type: 'document.versioned',
        version: { sha256: 'version2', byteLength: 100 },
        origin: LOCAL,
      }),
      emit({
        type: 'pages.named',
        name: 'unrelated',
        page: pr(1),
        layout: { pageCount: 2, pages: PAGES, namedPages: [] },
        meta: meta(),
        origin: LOCAL,
      }),
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
      emit(scaleFact(pr(1))),
      m('ensureLoaded', [pr(1)]),
      settle('first', { viewports: [vp({ x: 0, y: 0, width: 600, height: 800 }, METRES, true)] }),
      m('getPageScale', [pr(1)]),
      settle('second', { viewports: [vp({ x: 0, y: 0, width: 600, height: 800 }, FEET, true)] }),
      m('getPageScale', [pr(1)]),
      emit(scaleFact(pr(1))),
      emit(scaleFact(pr(1))),
      fail('third', 'stale'),
      m('getPageScale', [pr(1)]),
      fail('fourth', 'gone'),
      m('getPageScale', [pr(1)]),
    ],
  },
  {
    name: 'set-scale-on-one-page',
    covers:
      "The required engine measure service confirms an owned page-space 'EmbedPDF' viewport through pages.scaleSet and a mirror reload, then re-measures the page's dimensions through the annotation plugin and announces the change; isBusy is true while it runs and listLastReports keeps the reports. recalculate false skips the re-measure and reports the scale alone. clearScale drops the owned viewport and re-measures with the default. A single-page change rejects: a measure that is not writable, and a page not in the document.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [FEET, METRES, null]),
          hydrate([dimensionLine(1), dimensionLine(2, { locked: true })]),
          write(1, [{ item: updated(dimensionLine(1, { measure: FEET }), true) }], {
            defer: 'write',
          }),
          remeasured(2, 1, defaultMeasure('metric')),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listen,
      m('setScale', [pr(1), FEET]),
      m('isBusy'),
      settle(
        'write',
        confirmation(1, [{ item: updated(dimensionLine(1, { measure: FEET }), true) }]).value,
      ),
      m('isBusy'),
      m('getPageScale', [pr(1)]),
      m('listLastReports'),
      m('setScale', [pr(1), METRES, { recalculate: false }]),
      m('getPageScale', [pr(1)]),
      m('clearScale', [pr(1)]),
      m('getPageScale', [pr(1)]),
      m('setScale', [pr(1), malformedMeasure(GEO)]),
      m('setScale', [pr(1), malformedMeasure({ ...FEET, x: 'nope' })]),
      m('setScale', [pr(9), FEET]),
      m('listLastReports'),
    ],
  },
  {
    name: 'set-scale-on-several-pages',
    covers:
      "'all' and a list of pages change each page and report per page: a page that fails (not in the document, a measure that is not writable, a scale without a rectilinear base) reports scaleError instead of rejecting. A list of one page is still strict.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [FEET, withUnit(FEET, 'mm')]),
          ...scaleSequence(2, [FEET, withUnit(FEET, 'mm')]),
          hydrate([dimensionLine(1), dimensionLine(20, { page: pr(2), ref: objRef(20, 2) })]),
          remeasured(1, 1, FEET),
          remeasured(2, 20, FEET, 2),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listen,
      m('setScale', ['all', FEET]),
      m('setScale', [[pr(1), pr(9)], FEET, { recalculate: false }]),
      m('setScale', ['all', malformedMeasure(GEO)]),
      m('setScale', [[pr(9)], FEET]),
      m('setUnit', [[pr(1), pr(2)], 'mm', { recalculate: false }]),
      m('listLastReports'),
    ],
  },
  {
    name: 'set-scale-preflight',
    covers:
      'A single-page setScale runs the write preflight and rejects with its message: a conversion that is not positive, a precision that is not a whole number or is past the int32 range, a format whose unit is not a string, a text member that is not a string, a missing distance list, and an origin outside the PDF float range. A decimal format without a precision defaults to 100, and a label position, a fixed flag, a finite origin and a positive CYX all pass. calibrate writes the ratio rounded to eight significant digits.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [
            rl('m', { distance: [{ unit: 'm', conversion: 1 }] }),
            rl('m', {
              distance: [
                { unit: 'm', conversion: 1, precision: 100, labelPosition: 'suffix', fixed: true },
              ],
              origin: { x: 10, y: 20 },
              cyx: 1,
            }),
            measureFromKnownLength(7, { value: 1, unit: 'm' }),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      m('setScale', [
        pr(1),
        rl('m', { distance: [{ unit: 'm', conversion: 0, precision: 100 }] }),
        { recalculate: false },
      ]),
      m('setScale', [
        pr(1),
        rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 1.5 }] }),
        { recalculate: false },
      ]),
      m('setScale', [
        pr(1),
        rl('m', { distance: [{ unit: 'm', conversion: 1, precision: 1e10 }] }),
        { recalculate: false },
      ]),
      m('setScale', [
        pr(1),
        malformedMeasure({ ...METRES, distance: [{ unit: 5, conversion: 1, precision: 100 }] }),
        { recalculate: false },
      ]),
      m('setScale', [
        pr(1),
        malformedMeasure({
          ...METRES,
          distance: [{ unit: 'm', conversion: 1, precision: 100, thousands: 5 }],
        }),
        { recalculate: false },
      ]),
      m('setScale', [
        pr(1),
        malformedMeasure({ subtype: 'rectilinear', ratio: '1:1', x: METRES.x, area: METRES.area }),
        { recalculate: false },
      ]),
      m('setScale', [pr(1), rl('m', { origin: { x: 1e39, y: 0 } }), { recalculate: false }]),
      m('setScale', [
        pr(1),
        rl('m', { distance: [{ unit: 'm', conversion: 1 }] }),
        { recalculate: false },
      ]),
      m('getPageScale', [pr(1)]),
      m('setScale', [
        pr(1),
        rl('m', {
          distance: [
            { unit: 'm', conversion: 1, precision: 100, labelPosition: 'suffix', fixed: true },
          ],
          origin: { x: 10, y: 20 },
          cyx: 1,
        }),
        { recalculate: false },
      ]),
      m('getPageScale', [pr(1)]),
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 0, y: 0 },
          to: { x: 7, y: 0 },
          distance: { value: 1, unit: 'm' },
        },
        { recalculate: false },
      ]),
      m('getPageScale', [pr(1)]),
    ],
  },
  {
    name: 'units-precision-and-presets',
    covers:
      "setUnit, setAreaUnit and setPrecision edit the page's rectilinear scale (the default counts) and write it back; a fractional last format restarts at a precision of 100. An invalid precision, a base unit that is not a length unit, and an unknown preset reject; setPreset writes the preset's ratio.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [
            withUnit(defaultMeasure('metric'), 'ft'),
            withAreaUnit(withUnit(defaultMeasure('metric'), 'ft'), 'acre'),
            withPrecision(withAreaUnit(withUnit(defaultMeasure('metric'), 'ft'), 'acre'), 1000),
            FEET_INCHES,
            withUnit(FEET_INCHES, 'in'),
            FEET_INCHES,
            withAreaUnit(FEET_INCHES, 'ha'),
            rl('furlong'),
            measureFromRatio(1, 50, 'm'),
          ]),
        ]),
      },
    },
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
      'With a measure service a scale write goes to the engine, then the page re-reads its viewports. A refused engine write rejects a single-page change, and the next change on that page still runs. Unit and precision edits need a rectilinear scale: a foreign one, or a viewport without a measure, rejects not-ready. Two changes on one page queue behind each other.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, [vp({ x: 0, y: 0, width: 600, height: 800 }, GEO)], { times: 1 }),
            viewports(1, [vp({ x: 0, y: 0, width: 600, height: 800 }, FEET, true)], { times: 1 }),
            viewports(1, [vp({ x: 0, y: 0, width: 600, height: 800 }, METRES, true)]),
            viewports(2, [vp({ x: 0, y: 0, width: 400, height: 500 }, undefined)]),
            scaleAnswer(1, { times: 1, defer: 'set1' }, 'engine-scale-first'),
            {
              path: 'page.measure.setScale',
              pon: 1,
              times: 1,
              error: { name: 'EngineError', message: 'read-only', code: 'Refused' },
            },
            scaleAnswer(1),
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
      settle('set1', { page: pr(1), meta: meta(pr(1), 'engine-scale-first') }),
      m('getPageScale', [pr(1)]),
      m('listLastReports'),
    ],
  },
  {
    name: 'calibration-flow',
    covers:
      "startCalibration clears any request and arms the calibrate tool. The tool's captured line becomes the pending request in page space (the pointer tool comes back, onCalibrationRequested fires); calibrate derives the scale from the known length, writes it, clears the request for its page and fires onCalibrationCompleted. A request on another page survives a calibrate elsewhere; dismissCalibration drops it and fires onCalibrationDismissed, and is silent with nothing pending. applyTo widens the write. Two identical points reject invalid-input; a page not in the document rejects not-found. A capture tool other than calibrate is ignored.",
    plugins: withConfig({}, { tools: [{ id: 'ruler', extends: 'calibrate', clickCreate: false }] }),
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [
            measureFromKnownLength(144, { value: 2, unit: 'm' }),
            measureFromKnownLength(72, { value: 1, unit: 'ft' }),
          ]),
          ...scaleSequence(2, [measureFromKnownLength(72, { value: 1, unit: 'ft' })]),
          hydrate([dimensionLine(1)]),
          remeasured(1, 1, measureFromKnownLength(144, { value: 2, unit: 'm' })),
        ]),
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
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 100, y: 100 },
          to: { x: 244, y: 100 },
          distance: { value: 2, unit: 'm' },
        },
      ]),
      m('getCalibrationRequest'),
      m('getPageScale', [pr(1)]),
      ...drag('calibrate', [10, 10], [10, 110], pr(2)),
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 0, y: 0 },
          to: { x: 0, y: 72 },
          distance: { value: 1, unit: 'ft' },
        },
        { applyTo: [pr(1), pr(2)], recalculate: false },
      ]),
      m('getCalibrationRequest'),
      m('startCalibration'),
      m('getCalibrationRequest'),
      ...drag('calibrate', [10, 10], [10, 110], pr(2)),
      m('dismissCalibration'),
      m('dismissCalibration'),
      m('getCalibrationRequest'),
      ...drag('ruler', [10, 10], [110, 10]),
      m('getCalibrationRequest'),
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 5, y: 5 },
          to: { x: 5, y: 5 },
          distance: { value: 1, unit: 'm' },
        },
      ]),
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 5, y: 5 },
          to: { x: 50, y: 5 },
          distance: { value: -1, unit: 'm' },
        },
      ]),
      m('calibrate', [
        {
          page: pr(9),
          from: { x: 5, y: 5 },
          to: { x: 50, y: 5 },
          distance: { value: 1, unit: 'm' },
        },
      ]),
    ],
  },
  {
    name: 'zero-length-capture',
    covers:
      'A calibrate tool configured to click-create a zero-length line captures it, and a zero-length capture makes no request.',
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
      'Read-only: canCalibrate and canMeasure are false, every scale change rejects permission-denied before touching a page, startCalibration throws permission-denied, and createMeasurement rejects permission-denied.',
    plugins: PLUGINS,
    engine: { documents: { a: docOf([], { security: { allows: ['doc.annotate.read'] } }) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('canCalibrate'),
      m('canMeasure', [pr(1)]),
      m('setScale', [pr(1), FEET]),
      m('clearScale', ['all']),
      m('calibrate', [
        {
          page: pr(1),
          from: { x: 0, y: 0 },
          to: { x: 72, y: 0 },
          distance: { value: 1, unit: 'm' },
        },
      ]),
      m('startCalibration'),
      ix('getActiveToolId'),
      m('createMeasurement', [
        {
          kind: 'distance',
          page: pr(1),
          points: [
            { x: 0, y: 0 },
            { x: 10, y: 0 },
          ],
        },
      ]),
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
          write(1, [{ item: created(creation('distance', 10001)) }]),
          write(2, [{ item: created(creation('perimeter', 10002)) }]),
          write(3, [{ item: created(creation('area', 10003)) }]),
          refused('measurement create refused'),
        ]),
        b: docOf([viewports(1, undefined, { times: 1, defer: 'loading' })]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      an('onCreated', [{ $fn: 'created' }]),
      m('createMeasurement', [
        {
          kind: 'distance',
          page: pr(1),
          points: [
            { x: 100, y: 100 },
            { x: 244, y: 100 },
          ],
        },
      ]),
      m('createMeasurement', [
        {
          kind: 'perimeter',
          page: pr(1),
          points: [
            { x: 1, y: 1 },
            { x: 50, y: 1 },
            { x: 50, y: 50 },
          ],
        },
      ]),
      m('createMeasurement', [
        {
          kind: 'area',
          page: pr(1),
          points: [
            { x: 1, y: 1 },
            { x: 50, y: 1 },
            { x: 50, y: 50 },
          ],
          tool: 'area',
        },
      ]),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [{ x: 1, y: 1 }] }]),
      m('createMeasurement', [{ kind: 'distance', page: pr(1), points: [] }]),
      m('createMeasurement', [
        {
          kind: 'distance',
          page: pr(1),
          points: [
            { x: 1, y: 1 },
            { x: 2, y: 2 },
            { x: 3, y: 3 },
          ],
        },
      ]),
      m('createMeasurement', [
        {
          kind: 'distance',
          page: pr(9),
          points: [
            { x: 1, y: 1 },
            { x: 2, y: 2 },
          ],
        },
      ]),
      m('getReadout', [objRef(10001)]),
      m('createMeasurement', [
        {
          kind: 'distance',
          page: pr(1),
          points: [
            { x: 100, y: 100 },
            { x: 244, y: 100 },
          ],
        },
      ]),
      an('get', [objRef(10004)]),
      open('b'),
      m(
        'createMeasurement',
        [
          {
            kind: 'distance',
            page: pr(1),
            points: [
              { x: 0, y: 0 },
              { x: 72, y: 0 },
            ],
          },
        ],
        'b',
      ),
      settle('loading', { viewports: [] }),
      m('canMeasure', [pr(1)], 'b'),
    ],
  },
  {
    name: 'listener-throws',
    covers:
      'A throwing listener goes to the kernel report channel with its exact Error; the change still resolves. An unsubscribed listener hears nothing more.',
    plugins: PLUGINS,
    engine: { documents: { a: docOf([...scaleSequence(1, [FEET, METRES])]) } },
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
      'A page inserted after the plugin connected loads its viewports; a structural event that keeps the same pages loads nothing. A page removed while its viewport read is in flight is not published, and a later change on it rejects not-found.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, []),
            viewports(2, [], { times: 1 }),
            viewports(2, undefined, { times: 1, defer: 'late' }),
            viewports(3, [vp(CROP3, FEET, true)]),
            hydrate(),
            // The engine knows page 3 from the start; the registry learns it from the insert.
            { path: 'pages.list', value: { pageCount: 2, pages: PAGES, namedPages: [] } },
          ],
          { pages: PAGES4.slice(0, 3), services: ['page.measure'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({
        type: 'pages.inserted',
        insertedPages: [pr(3)],
        origin: LOCAL,
        layout: { pageCount: 3, pages: PAGES4.slice(0, 3), namedPages: [] },
        meta: meta(),
      }),
      m('getPageScale', [pr(3)]),
      emit({
        type: 'pages.rotated',
        pages: [pr(1)],
        rotation: 0,
        origin: LOCAL,
        layout: { pageCount: 3, pages: PAGES4.slice(0, 3), namedPages: [] },
        meta: meta(),
      }),
      emit(scaleFact(pr(2))),
      emit({
        type: 'pages.deleted',
        pages: [pr(2)],
        origin: LOCAL,
        layout: { pageCount: 2, pages: [PAGES4[0], PAGES4[2]], namedPages: [] },
        meta: meta(),
      }),
      settle('late', { viewports: [vp({ x: 0, y: 0, width: 400, height: 500 }, FEET, true)] }),
      m('getPageScale', [pr(2)]),
      m('setScale', [pr(2), FEET, { recalculate: false }]),
    ],
  },
  {
    name: 'page-removed-while-a-change-waits',
    covers:
      'With the required current measure service, a preset change queued behind another on the same page finds the page gone once it runs: the ratio falls back to a userUnit of 1 and the write rejects not-found.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf([
          ...scaleSequence(1, [FEET]),
          hydrate([dimensionLine(1)]),
          write(1, [{ item: updated(dimensionLine(1, { measure: FEET }), true) }], {
            defer: 'write',
          }),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      m('setScale', [pr(1), FEET]),
      m('setPreset', [pr(1), 'metric-10', { recalculate: false }]),
      emit({
        type: 'pages.deleted',
        pages: [pr(1)],
        origin: LOCAL,
        layout: { pageCount: 1, pages: [PAGES[1]], namedPages: [] },
        meta: meta(),
      }),
      settle(
        'write',
        confirmation(1, [{ item: updated(dimensionLine(1, { measure: FEET }), true) }]).value,
      ),
      m('isBusy'),
    ],
  },
  {
    name: 'close-mid-flight',
    covers:
      "Closing the document while changes run: a single-page change rejects with the engine call's instance-closed, the all-pages change resolves two serialized per-page scale errors and the attempted closed-state update reports instance-closed, and a viewport read in flight is not published.",
    plugins: PLUGINS,
    engine: {
      documents: {
        a: docOf(
          [
            viewports(1, [], { times: 1 }),
            viewports(1, undefined, { defer: 'read' }),
            viewports(2, []),
            { path: 'page.measure.setScale', pon: 1, defer: 'set1' },
            scaleAnswer(2, { defer: 'set2' }),
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
      emit(scaleFact(pr(1))),
      m('isBusy'),
      close(),
    ],
  },
];

const functions = {
  initialMeasurementState: model.initialMeasurementState,
  beginScaleChange: model.beginScaleChange,
  endScaleChange: model.endScaleChange,
  setCalibration: model.setCalibration,
  setReports: model.setReports,
  setLocalViewports: model.setLocalViewports,
  clearLoadError: model.clearLoadError,
  DEFAULT_PRESETS: () => DEFAULT_PRESETS,
  defaultMeasure,
  selectPageScale: model.selectPageScale,
  pageScaleOf: model.pageScaleOf,
  withUnit,
  withAreaUnit,
  withPrecision,
};
const loaded = { ...model.initialMeasurementState(), pending: 1 };
function fc<K extends keyof typeof functions>(
  name: string,
  fn: K,
  args: Parameters<(typeof functions)[K]>,
  covers?: string,
): FunctionCase {
  return { name, fn, args, ...(covers ? { covers } : {}) };
}
const functionCases: FunctionCase[] = [
  fc('initial-state', 'initialMeasurementState', []),
  fc(
    'reduce-page-scale',
    'setLocalViewports',
    [loaded, 2, [vp({ x: 0, y: 0, width: 400, height: 500 }, METRES, true)]],
    'Surviving fallback viewport storage uses setLocalViewports; removed independently cached public PAGE_SCALE state is archived. Current mirror-backed public projection is tested independently.',
  ),
  fc('reduce-pending-up', 'beginScaleChange', [loaded]),
  fc(
    'reduce-pending-floors-at-zero',
    'endScaleChange',
    [{ ...loaded, pending: 0 }],
    'The busy counter never goes below zero.',
  ),
  fc('reduce-calibration', 'setCalibration', [
    loaded,
    { page: pr(1), from: { x: 0, y: 0 }, to: { x: 1, y: 0 }, userSpaceLength: 1 },
  ]),
  fc('reduce-reports', 'setReports', [
    loaded,
    [
      {
        page: pr(1),
        updated: [],
        skipped: [],
        failed: [],
        scaleError: { name: 'EngineError', code: 'Unknown', message: 'x' },
      },
    ],
  ]),
  fc(
    'reduce-unknown',
    'clearLoadError',
    [loaded, 99],
    'Unknown-action dispatch is archived; current absent-error clearing preserves identity without copied dispatch logic.',
  ),
  fc('default-presets', 'DEFAULT_PRESETS', []),
  fc('default-measure-metric', 'defaultMeasure', ['metric']),
  fc('default-measure-imperial-user-unit', 'defaultMeasure', ['imperial', 2]),
  fc('default-measure-explicit-metric', 'defaultMeasure', ['metric', 0.5]),
  fc('default-measure-object', 'defaultMeasure', [FEET, 3]),
  fc('default-measure-bad-user-unit', 'defaultMeasure', ['metric', 0]),
  fc('select-owned', 'selectPageScale', [
    [
      vp({ x: 0, y: 0, width: 600, height: 800 }, METRES),
      vp({ x: 0, y: 0, width: 1, height: 1 }, FEET, true),
    ],
    { width: 600, height: 800 },
    METRES,
    true,
  ]),
  fc('select-last-containing', 'selectPageScale', [
    [
      vp({ x: 0, y: 0, width: 600, height: 800 }, METRES),
      vp({ x: 0, y: 0, width: 600, height: 800 }, FEET),
      vp({ x: 0, y: 0, width: 1, height: 1 }, GEO),
    ],
    { width: 600, height: 800 },
    METRES,
    false,
  ]),
  fc('select-foreign-without-measure', 'selectPageScale', [
    [vp({ x: 0, y: 0, width: 600, height: 800 }, null)],
    { width: 600, height: 800 },
    METRES,
    false,
  ]),
  fc('select-default', 'selectPageScale', [
    [vp({ x: 0, y: 0, width: 1, height: 1 }, FEET)],
    { width: 600, height: 800 },
    METRES,
    false,
  ]),
  fc('select-none', 'selectPageScale', [[], { width: 400, height: 500 }, FEET, true]),
  fc(
    'select-beside-the-centre',
    'selectPageScale',
    [[vp({ x: 0, y: 0, width: 1, height: 800 }, FEET)], { width: 600, height: 800 }, METRES, false],
    "A viewport that spans the centre's height but not its x is not under it.",
  ),
  fc(
    'select-on-a-raised-crop',
    'selectPageScale',
    [
      [vp({ x: 0, y: 0, width: 600, height: 450 }, FEET)],
      { width: 600, height: 800 },
      METRES,
      false,
    ],
    'Raised PDF crop geometry is recorded in page space before invocation; its center is size/2.',
  ),
  fc('with-unit', 'withUnit', [METRES, 'ft']),
  fc('with-unit-and-area', 'withUnit', [FEET, 'in', 'acre']),
  fc('with-unit-from-fraction', 'withUnit', [FEET_INCHES, 'yd']),
  fc(
    'with-unit-keeps-precision',
    'withUnit',
    [{ ...METRES, distance: [{ unit: 'm', conversion: 1, precision: 1000 }] }, 'ft'],
    'A decimal last format keeps its precision.',
  ),
  fc('with-unit-no-precision', 'withUnit', [
    { ...METRES, distance: [{ unit: 'm', conversion: 1 }] },
    'km',
  ]),
  fc('with-unit-no-distance', 'withUnit', [{ ...METRES, distance: [] }, 'mi']),
  fc('with-unit-unknown-base', 'withUnit', [rl('furlong'), 'm']),
  fc('with-unit-no-x', 'withUnit', [{ ...METRES, x: [] }, 'm']),
  fc('with-unit-padded-base', 'withUnit', [
    { ...METRES, x: [{ unit: ' m ', conversion: 1 / 72 }] },
    'cm',
  ]),
  fc('with-area-unit', 'withAreaUnit', [METRES, 'ha']),
  fc('with-area-unit-from-fraction', 'withAreaUnit', [FEET_INCHES, 'yd2']),
  fc('with-area-unit-no-precision', 'withAreaUnit', [
    { ...METRES, area: [{ unit: 'm²', conversion: 1 }] },
    'pt2',
  ]),
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
  fc('page-scale-loading', 'pageScaleOf', [
    {
      viewports: undefined,
      status: 'loading',
      error: undefined,
      size: { width: 600, height: 800 },
      fallback: METRES,
      persistent: true,
    },
  ]),
  fc('page-scale-failed-known', 'pageScaleOf', [
    {
      viewports: [vp({ x: 0, y: 0, width: 600, height: 800 }, FEET, true)],
      status: 'error',
      error: { name: 'EngineError', code: 'RuntimeUnavailable', message: 'offline' },
      size: { width: 600, height: 800 },
      fallback: METRES,
      persistent: true,
    },
  ]),
  fc('page-scale-failed-empty', 'pageScaleOf', [
    {
      viewports: undefined,
      status: 'error',
      error: { name: 'EngineError', code: 'RuntimeUnavailable', message: 'offline' },
      size: undefined,
      fallback: METRES,
      persistent: false,
    },
  ]),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/measurement.json',
    describes: 'Current measurement page scales, calibration, mutations, readouts and lifecycle.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/measurement-functions.json',
    describes: 'Current named measurement transitions, scale selection/projection and arithmetic.',
    functions,
    cases: functionCases,
  },
];
