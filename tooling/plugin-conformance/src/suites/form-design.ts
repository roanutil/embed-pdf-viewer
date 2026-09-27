/**
 * `packages/plugin/form` design mode: `init`'s tool and behavior
 * registration (with and without the annotation plugin), placeField and the
 * other field-design verbs, and the draw-to-place handler driven as
 * `PointerSample` sequences through `interaction.dispatch`. Replayed by
 * `crates/plugins/form/tests/conformance.rs`.
 */
import type { ResponseSpec } from '../fake-engine';
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  FORM_ONLY,
  SNAP,
  anno,
  annotations,
  boot,
  call,
  checkbox,
  doc,
  fnSpec,
  ix,
  list,
  pointer,
  probe,
  radio,
  sample,
  settle,
  text,
  tool,
  widgetAnnot,
  pr,
  v3Widget,
} from './form-fixtures';

const HYDRATE: ResponseSpec = { path: 'annotations.listRawAll', value: { pages: [] } };
const WITH_ANNOTATION = [{ factory: 'interaction' }, { factory: 'annotation' }, { factory: 'form' }];
const WITH_PROBE = [...WITH_ANNOTATION, { factory: 'form-probe' }];

const EXISTING = text(1, 'text_1', 'taken');
const created = (field: unknown) => ({ path: 'forms.createField', value: { field } });
const placed = (n: number, name: string, pon = 1) => text(n, name, '', {}, [[n + 100, pon]]);
const input = (over: Record<string, unknown> = {}) => ({
  family: 'text',
  pageObjectNumber: 1,
  box: { x: 10, y: 20, width: 100, height: 24 },
  ...over,
});

const initCases: ScenarioCase[] = [
  {
    name: 'init-without-annotation',
    covers:
      "Without the annotation plugin init registers form-fill, form-edit and the five palette tools as plain hub tools enabling 'form-place' and 'annotation-edit', plus the place handler.",
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list([])]) } },
    snapshot: 'none',
    steps: [...boot, ix('tools')],
  },
  {
    name: 'init-with-annotation',
    covers:
      "With the annotation plugin the palette tools join its registry (subtype, click policy, defaults) and a 'form-widgets' behavior is engaged for widget subtypes exactly while the active tool enables 'form-fill'.",
    plugins: WITH_PROBE,
    engine: { documents: { a: doc([list([]), HYDRATE]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      ix('tools'),
      anno('tool', ['form-text']),
      anno('tool', ['form-listbox']),
      anno('currentDefaults', ['form-checkbox']),
      probe('behaviorFor', ['widget']),
      probe('behaviorFor', ['widget-text']),
      probe('behaviorFor', ['square']),
      tool('form-edit'),
      probe('behaviorFor', ['widget']),
      tool('form-fill'),
      probe('behaviorFor', ['widget-toggle']),
      tool('pan'),
      probe('behaviorFor', ['widget']),
    ],
  },
];

const designCases: ScenarioCase[] = [
  {
    name: 'place-a-text-field',
    covers:
      'placeField converts the content box to a PDF rect on the page crop, auto-names against the current snapshot, sends the draft (appearance only when given), re-reads, clears that page’s geometry, and returns the field and its widget on the page.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([EXISTING, text(2, 'text_2', '')]),
          annotations(1, [widgetAnnot(1, 101)]),
          { path: 'forms.createField', times: 1, value: { field: placed(30, 'text_3') } },
          { path: 'forms.createField', value: { field: placed(31, 'text_3', 2) } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('ensureGeom', [1]),
      call('placeField', [input()]),
      call('placeField', [input({ pageObjectNumber: 2, appearance: { backgroundColor: '#ffffff', borderWidth: 1 } })]),
    ],
  },
  {
    name: 'place-each-family',
    covers:
      "A radio draft carries one widget with onState 'option1'; combobox and listbox drafts carry two starter options; checkbox takes the plain widget. A widget on another page is reported as null.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([]),
          { path: 'forms.createField', times: 1, value: { field: radio(40, 'radio_1', [v3Widget({ annotObjectNumber: 140, pageObjectNumber: 1, onState: 'option1', checked: false })]) } },
          { path: 'forms.createField', times: 1, value: { field: placed(41, 'combobox_1') } },
          { path: 'forms.createField', times: 1, value: { field: placed(42, 'listbox_1') } },
          { path: 'forms.createField', value: { field: checkbox(43, 'checkbox_1', false, {}, [v3Widget({ annotObjectNumber: 143, pageObjectNumber: 2, onState: 'Yes' })]) } },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('placeField', [input({ family: 'radio' })]),
      call('placeField', [input({ family: 'combobox' })]),
      call('placeField', [input({ family: 'listbox' })]),
      call('placeField', [input({ family: 'checkbox' })]),
    ],
  },
  {
    name: 'placement-before-the-snapshot',
    covers: 'Placing before the first snapshot lands names against an empty form.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([EXISTING], { defer: 'list' }), { path: 'forms.createField', value: { field: placed(50, 'text_1') } }]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('placeField', [input()]),
      settle('list', { formKind: 'acroform', needsAppearances: false, fields: [EXISTING], calculationOrder: [] }),
      settle('list', { formKind: 'acroform', needsAppearances: false, fields: [EXISTING], calculationOrder: [] }),
    ],
  },
  {
    name: 'placement-is-page-bound',
    covers:
      'An overshooting box is intersected with the page; a box under one point in either axis after that rejects (exactly one point is enough), as does a page the document does not have; neither reaches the engine.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([]), { path: 'forms.createField', value: { field: placed(50, 'text_1') } }]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('placeField', [input({ box: { x: -30, y: 790, width: 700, height: 40 } })]),
      call('placeField', [input({ box: { x: 10, y: 10, width: 0.5, height: 30 } })]),
      call('placeField', [input({ box: { x: 700, y: 10, width: 30, height: 30 } })]),
      call('placeField', [input({ pageObjectNumber: 99 })]),
      call('placeField', [input({ box: { x: 10, y: 10, width: 30, height: 0.25 } })]),
      call('placeField', [input({ box: { x: 599, y: 799, width: 5, height: 5 } })]),
    ],
  },
  {
    name: 'placement-refused-by-the-engine',
    covers: 'A createField refusal rejects placeField and changes nothing; the queue carries on.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([]),
          { path: 'forms.createField', times: 1, error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.modify', required: 'doc.forms.modify' } },
          { path: 'forms.createField', value: { field: placed(50, 'text_1') } },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('placeField', [input()]), call('placeField', [input()])],
  },
  {
    name: 'placement-reloads-the-annotation-page',
    covers: 'With the annotation plugin placeField awaits the page reload before resolving, so the widget is selectable.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([]),
          HYDRATE,
          { path: 'forms.createField', value: { field: placed(60, 'text_1') } },
          annotations(1, [widgetAnnot(1, 160)], { defer: 'reload' }),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('placeField', [input()]), settle('reload', { annotations: [widgetAnnot(1, 160)] }), anno('get', [{ kind: 'objectNumber', page: pr(1), annotObjectNumber: 160 }])],
  },
  {
    name: 'page-box',
    covers: "pageBox is the page's content box {0, 0, w, h}, or null for a page the document does not have.",
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list([])]) } },
    snapshot: 'none',
    steps: [...boot, call('pageBox', [1]), call('pageBox', [2]), call('pageBox', [99])],
  },
  {
    name: 'update-field',
    covers: 'updateField sends the patch against the key’s ref and re-reads; a refusal rejects.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([EXISTING]),
          { path: 'forms.updateField', times: 1, value: { field: { ...EXISTING, name: 'renamed' } } },
          { path: 'forms.updateField', error: { name: 'EngineError', message: 'family mismatch', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('updateField', ['obj:1', { family: 'text', name: 'renamed' }]), call('updateField', ['fqn:a.b', { family: 'checkbox' }])],
  },
  {
    name: 'delete-field',
    covers:
      "deleteField deletes by ref, re-reads, clears the geometry of each page the field's widgets were on (once each, direct widgets skipped) and nudges the annotation plane there; an unknown key still deletes, with no pages.",
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([text(7, 'spread', 'v', {}, [[107, 1], [108, 2], [109, 1], [0, 0]])], { times: 1 }),
          list([]),
          HYDRATE,
          annotations(1, [widgetAnnot(1, 107)]),
          annotations(2, []),
          { path: 'forms.deleteField', value: { deleted: true } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('ensureGeom', [1]), call('ensureGeom', [2]), call('deleteField', ['obj:7']), call('deleteField', ['obj:404'])],
  },
  {
    name: 'queued-verbs-interleave-with-page-reloads',
    covers:
      'The annotation reloads a delete nudges run unawaited beside the next queued verb: their page reads and model updates interleave with the update, turn for turn.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([text(7, 'spread', 'v', {}, [[107, 1], [108, 2]]), text(8, 'other', 'o')]),
          HYDRATE,
          annotations(1, [widgetAnnot(1, 107)]),
          annotations(2, [widgetAnnot(2, 108)]),
          { path: 'forms.deleteField', defer: 'delete' },
          { path: 'forms.updateField', value: { field: text(8, 'renamed', 'o') } },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('deleteField', ['obj:7']), call('updateField', ['obj:8', { family: 'text', name: 'renamed' }]), settle('delete', { deleted: true })],
  },
  {
    name: 'delete-field-without-annotation',
    covers: 'Without the annotation plugin there is no plane to nudge; a refusal rejects.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([text(7, 'spread', 'v', {}, [[107, 1]])]),
          { path: 'forms.deleteField', times: 1, value: { deleted: true } },
          { path: 'forms.deleteField', error: { name: 'EngineError', message: 'gone', code: 'NotFound' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('deleteField', ['obj:7']), call('deleteField', ['obj:7'])],
  },
  {
    name: 'detach-widget',
    covers:
      "detachWidget sends the widget's page with its object number, re-reads, then clears and nudges that page; an unknown widget detaches with page 0 and touches no page, and so does a direct widget.",
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([text(7, 'pair', 'v', {}, [[107, 1], [108, 0]])]),
          HYDRATE,
          annotations(1, [widgetAnnot(1, 107)]),
          { path: 'forms.detachWidget', times: 3, value: { field: text(7, 'pair', 'v', {}, []) } },
          { path: 'forms.detachWidget', error: { name: 'EngineError', message: 'not attached', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('ensureGeom', [1]),
      call('detachWidget', ['obj:7', 107]),
      call('detachWidget', ['obj:7', 999]),
      call('detachWidget', ['obj:7', 108]),
      call('detachWidget', ['obj:404', 1]),
    ],
  },
];

const HIT_ANNOTS = [
  widgetAnnot(1, 101, { left: 0, bottom: 500, right: 300, top: 800 }),
  widgetAnnot(1, 102, { left: 10, bottom: 700, right: 60, top: 790 }),
  widgetAnnot(1, 150, { left: 400, bottom: 0, right: 600, top: 200 }),
  { ...widgetAnnot(1, 103), subtype: 'square', color: { r: 1, g: 2, b: 3 }, interiorColor: null, opacity: 1, cloudyIntensity: null, rectDifferences: null },
];
const HIT_FIELDS = [text(1, 'big', '', {}, [[101, 1]]), text(2, 'small', '', {}, [[102, 1]]), text(3, 'boxy', '', {}, [[103, 1]])];

// Page 1 is 600 x 800, so a content y is 800 minus the PDF y.
const LIVE_ANNOTS = [
  widgetAnnot(1, 201, { left: 99, bottom: 300, right: 101, top: 500 }),
  widgetAnnot(1, 202, { left: 85, bottom: 385, right: 115, top: 415 }),
  widgetAnnot(1, 203, { left: 280, bottom: 395, right: 320, top: 405 }),
  widgetAnnot(1, 204, { left: 285, bottom: 385, right: 315, top: 415 }),
  widgetAnnot(1, 205, { left: 490, bottom: 390, right: 510, top: 410 }),
  widgetAnnot(1, 206, { left: 495, bottom: 385, right: 515, top: 405 }),
];
const LIVE_FIELDS = [201, 202, 203, 204, 205, 206].map((n) => text(n - 200, `live${n}`, '', {}, [[n, 1]]));

const v3DesignCases: ScenarioCase[] = [
  {
    name: 'widget-hit-test',
    covers:
      "getWidgetAt kicks the page's geometry read and, until it lands, asks the annotation plane's live boxes: the smallest widget with a field under the point wins, a widget no field owns and a non-widget annotation are skipped. Once the geometry lands the model answers; a page without either answers null.",
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list(HIT_FIELDS),
          { path: 'annotations.listRawAll', value: { pages: [{ pageState: { page: pr(1) }, annotations: HIT_ANNOTS }] } },
          annotations(1, HIT_ANNOTS, { defer: 'geom' }),
          annotations(2, []),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
      call('getWidgetAt', [pr(1), { x: 200, y: 200 }]),
      call('getWidgetAt', [pr(1), { x: 500, y: 700 }]),
      call('getWidgetAt', [pr(1), { x: 590, y: 20 }]),
      settle('geom', { annotations: HIT_ANNOTS }),
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
      call('getWidgetAt', [pr(1), { x: 500, y: 700 }]),
      call('getWidgetAt', [pr(2), { x: 1, y: 1 }]),
    ],
  },
  {
    name: 'widget-hit-test-live-boxes-compare-areas',
    covers:
      'The live-box fallback compares areas, not perimeters or aspect ratios: a 2 x 200 widget beats a 30 x 30 one and a 40 x 10 widget beats a 30 x 30 one; between equal areas the first listed wins.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list(LIVE_FIELDS),
          { path: 'annotations.listRawAll', value: { pages: [{ pageState: { page: pr(1) }, annotations: LIVE_ANNOTS }] } },
          annotations(1, LIVE_ANNOTS, { defer: 'geom' }),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('getWidgetAt', [pr(1), { x: 100, y: 400 }]),
      call('getWidgetAt', [pr(1), { x: 300, y: 400 }]),
      call('getWidgetAt', [pr(1), { x: 500, y: 400 }]),
    ],
  },
  {
    name: 'widget-hit-test-without-annotation',
    covers: 'Without the annotation plugin a page whose geometry has not landed answers null.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(HIT_FIELDS), annotations(1, HIT_ANNOTS)]) } },
    snapshot: 'none',
    steps: [...boot, call('getWidgetAt', [pr(1), { x: 20, y: 20 }]), call('getWidgetAt', [pr(1), { x: 20, y: 20 }])],
  },
  {
    name: 'attach-widget',
    covers: "attachWidget links an annotation to the field by ref, re-reads, then clears and nudges the widget's page; a refusal rejects.",
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([text(7, 'pair', 'v', {}, [[107, 1]])]),
          HYDRATE,
          annotations(1, [widgetAnnot(1, 107)]),
          { path: 'forms.attachWidget', times: 1, value: { field: text(7, 'pair', 'v', {}, [[107, 1], [150, 1]]) } },
          { path: 'forms.attachWidget', error: { name: 'EngineError', message: 'already attached', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('ensureGeom', [1]),
      call('attachWidget', [{ kind: 'objectNumber', fieldObjectNumber: 7 }, { kind: 'objectNumber', page: pr(1), annotObjectNumber: 150 }]),
      call('attachWidget', [{ kind: 'fqn', name: 'pair' }, { kind: 'objectNumber', page: pr(1), annotObjectNumber: 150 }]),
    ],
  },
  {
    name: 'place-with-a-name-and-options',
    covers: 'An explicit name skips auto-naming, and a choice field takes the options given instead of the two starters.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([]), { path: 'forms.createField', value: { field: placed(44, 'my.choice') } }]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('placeField', [input({ family: 'combobox', name: 'my.choice', options: [{ label: 'A', value: 'a' }] })]),
      call('placeField', [input({ family: 'signature' })]),
    ],
  },
];

// ── the place handler ─────────────────────────────────────────────────────────

const PLACE_ENGINE = (extra: ResponseSpec[] = [], over = {}) => ({
  documents: {
    a: doc(
      [
        list([]),
        HYDRATE,
        ...extra,
        { path: 'forms.createField', value: { field: placed(70, 'text_1') } },
        annotations(1, [widgetAnnot(1, 170)]),
      ],
      over,
    ),
  },
});
const gap = (phase: string, over: Record<string, unknown> = {}) =>
  ix('dispatch', [{ phase, viewport: { x: 900, y: 900 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false }, ...over }]);

const handlerCases: ScenarioCase[] = [
  {
    name: 'a-click-places-the-tool-default',
    covers:
      "A click (under the drag threshold in both axes) places the tool's click size centred on the down point through the shared placement layer, styled from the tool's live defaults, and selects the new widget.",
    plugins: WITH_ANNOTATION,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [...boot, tool('form-text'), pointer('down', 100, 100), pointer('up', 101, 101), anno('getSelection'), ix('activeToolId')],
  },
  {
    name: 'a-click-near-the-edge-is-clamped',
    covers: 'The click box is kept on the page by the shared placement layer.',
    plugins: WITH_ANNOTATION,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [...boot, tool('form-listbox'), pointer('down', 595, 5), pointer('up', 595, 5)],
  },
  {
    name: 'a-thin-drag-is-a-drag',
    covers: "Without the annotation plugin a 100 x 2 drag commits as the dragged rect, styled from the table's static defaults; nothing is selected.",
    plugins: FORM_ONLY,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [...boot, tool('form-text'), pointer('down', 50, 50), pointer('move', 150, 52), pointer('up', 150, 52)],
  },
  {
    name: 'the-home-page-tracks-through-the-projection',
    covers:
      'Moves over another page project back onto the home page; the up sample is final; a move that projects nowhere is ignored; an up over a gap falls back to the last resolved point.',
    plugins: FORM_ONLY,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [
      ...boot,
      tool('form-checkbox'),
      pointer('down', 10, 10),
      ix('dispatch', [sample('move', 1, 1, { project: fnSpec('project', { returns: { x: 90, y: 40 } }) }, 2)]),
      ix('dispatch', [sample('move', 1, 1, { project: fnSpec('project', { returns: null }) }, 2)]),
      ix('dispatch', [{ ...sample('up', 0, 0), page: undefined, project: fnSpec('project', { returns: { x: 110, y: 60 } }) }]),
      pointer('down', 10, 10),
      pointer('move', 60, 40),
      gap('up'),
    ],
  },
  {
    name: 'the-live-preview',
    covers:
      "With the annotation plugin a move past the drag threshold previews the dragged box through the placement ghost, a move back under it clears the preview, and the up clears it.",
    plugins: WITH_ANNOTATION,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [
      ...boot,
      tool('form-checkbox'),
      pointer('down', 10, 10),
      pointer('move', 11, 11),
      anno('toolGhost', [1]),
      pointer('move', 60, 40),
      anno('toolGhost', [1]),
      pointer('move', 12, 12),
      anno('toolGhost', [1]),
      pointer('move', 60, 40),
      pointer('up', 60, 40),
      anno('toolGhost', [1]),
    ],
  },
  {
    name: 'the-click-threshold-is-strict',
    covers: 'A drag exactly the drag threshold wide (or tall) is a drag, not a click: here a degenerate one, which placeField refuses.',
    plugins: FORM_ONLY,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [...boot, tool('form-text'), pointer('down', 50, 50), pointer('up', 53, 50), pointer('down', 50, 50), pointer('up', 50, 53)],
  },
  {
    name: 'the-handler-outranks-lower-priorities',
    covers: "At priority 95 the place handler takes a down before an eligible handler of priority 50 is asked; the gesture starts under the id 'form-place'.",
    plugins: FORM_ONLY,
    engine: PLACE_ENGINE(),
    snapshot: 'none',
    steps: [
      ...boot,
      ix('registerHandler', [
        {
          id: 'low',
          priority: 50,
          enabledFor: { $fn: 'low.enabledFor', returns: true },
          onDown: { $fn: 'low.onDown', returns: true },
          onUp: { $fn: 'low.onUp' },
        },
      ]),
      ix('onGestureStarted', [fnSpec('gestureStarted')]),
      tool('form-text'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
    ],
  },
  {
    name: 'the-handler-declines',
    covers:
      'No capture off a page, under a tool the palette does not know (even one enabling form-place), without the design grant, or under a palette id re-registered without form-place; the gesture falls through to other handlers.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([])]),
        b: doc([list([])], { security: { allows: ['doc.forms.read', 'doc.forms.fill'] } }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      tool('form-text'),
      gap('down'),
      gap('up'),
      ix('registerTool', [{ id: 'custom-place', cursor: 'crosshair', enables: { $set: ['form-place'] } }]),
      tool('custom-place'),
      pointer('down', 10, 10),
      pointer('up', 10, 10),
      ix('activateTool', ['form-text'], 'b'),
      ix('dispatch', [sample('down', 10, 10)], 'b'),
      ix('dispatch', [sample('up', 10, 10)], 'b'),
      ix('registerTool', [{ id: 'form-text', cursor: 'crosshair', enables: { $set: [] } }, { replace: true }]),
      tool('form-text'),
      pointer('down', 10, 10),
      pointer('up', 10, 10),
    ],
  },
  {
    name: 'auto-select-waits-for-a-stable-world',
    covers:
      'No selection when the tool changed while the engine wrote, when the engine reported no widget on the page, or when the widget has no object number.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: doc([
          list([]),
          HYDRATE,
          { path: 'forms.createField', times: 1, defer: 'slow' },
          { path: 'forms.createField', times: 1, value: { field: placed(71, 'text_1', 2) } },
          { path: 'forms.createField', value: { field: text(72, 'text_1', '', {}, [[0, 1]]) } },
          annotations(1, []),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      tool('form-text'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      tool('pointer'),
      settle('slow', { field: placed(70, 'text_1') }),
      tool('form-text'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      anno('getSelection'),
    ],
  },
  {
    name: 'a-failed-placement-is-logged',
    covers: "A placeField rejection is contained and reported to console.error: a drag that is thin in one axis after clamping, a click on a page the document does not have, an engine refusal.",
    plugins: FORM_ONLY,
    engine: PLACE_ENGINE([{ path: 'forms.createField', times: 1, error: { name: 'EngineError', message: 'refused', code: 'Refused' } }]),
    snapshot: 'none',
    steps: [
      ...boot,
      tool('form-text'),
      pointer('down', 50, 50),
      pointer('up', 150, 50.5),
      pointer('down', 50, 50, {}, 99),
      pointer('up', 50, 50, {}, 99),
      pointer('down', 50, 50),
      pointer('up', 50, 50),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-design.json',
    describes: 'packages/plugin/form design mode: tool and behavior registration, placeField and the design verbs, and the draw-to-place handler.',
    cases: [...initCases, ...designCases, ...v3DesignCases, ...handlerCases],
  },
];

