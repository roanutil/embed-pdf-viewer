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
          { path: 'forms.createField', times: 1, value: { field: radio(40, 'radio_1', [{ annotObjectNumber: 140, pageObjectNumber: 1, onState: 'option1', checked: false }]) } },
          { path: 'forms.createField', times: 1, value: { field: placed(41, 'combobox_1') } },
          { path: 'forms.createField', times: 1, value: { field: placed(42, 'listbox_1') } },
          { path: 'forms.createField', value: { field: checkbox(43, 'checkbox_1', false, {}, [{ annotObjectNumber: 143, pageObjectNumber: 2, onState: 'Yes' }]) } },
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
      'An overshooting box is intersected with the page; a box under one point in either axis after that rejects, as does a page the document does not have; neither reaches the engine.',
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
    steps: [...boot, call('placeField', [input()]), settle('reload', { annotations: [widgetAnnot(1, 160)] }), anno('get', [{ kind: 'objectNumber', pageObjectNumber: 1, annotObjectNumber: 160 }])],
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
    name: 'the-handler-declines',
    covers:
      'No capture off a page, under a tool the palette does not know (even one enabling form-place), or without the design grant; the gesture falls through to other handlers.',
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
    cases: [...initCases, ...designCases, ...handlerCases],
  },
];

