/**
 * `packages/plugin/form` with no actions and no annotation plugin: snapshot
 * hydration and refresh, widget geometry, the fused fill projection, value
 * writes and their queue, reset, interchange, the permission twins and the
 * reactive effect. Replayed by `crates/plugins/form/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  CROP2,
  FLAGS,
  FORM_ONLY,
  SNAP,
  annotations,
  boot,
  call,
  checkbox,
  close,
  combobox,
  doc,
  effectsOk,
  emit,
  fail,
  fnSpec,
  fqn,
  list,
  pushbutton,
  listbox,
  ref,
  settle,
  snapshot,
  text,
  widgetAnnot,
  v3Widget,
  pr,
} from './form-fixtures';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "Pins the authority twins, batch writes, resets and change events, through call outcomes, settled results, engine calls and listener callbacks. Its dispatched actions are compared; the state they build is not.",
} as const;

const NAME = text(1, 'name', 'Ada');
const AGREE = checkbox(2, 'agree', false);
const COLOUR = combobox(3, 'colour', 'r');
const COLOURS = listbox(4, 'colours', ['g']);
const FIELDS = [NAME, AGREE, COLOUR, COLOURS];
const bytes = { $bytes: 'AQID' };

const cases: ScenarioCase[] = [
  {
    name: 'hydrates-on-open',
    covers:
      'The capability reads the snapshot once when the document opens; reads return null and empty until it lands, then index by key and by widget. The twins follow the wildcard grant.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS, { defer: 'list' })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('snapshot'),
      call('fillItems', [1]),
      call('fillItem', [101]),
      call('field', ['obj:1']),
      call('canRead'),
      call('canFill'),
      call('canDesign'),
      settle('list', snapshot(FIELDS)),
      call('snapshot'),
      call('field', ['obj:1']),
      call('field', ['obj:404']),
      call('fieldForWidget', [102]),
      call('fieldForWidget', [404]),
      call('fillItem', [101]),
      call('fillItem', [404]),
    ],
  },
  {
    name: 'no-read-grant-never-lists',
    covers:
      'Without doc.forms.read the capability fires no list at open or on refresh(), the model stays empty, and the twins say which grants the session has.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)], { security: { allows: ['doc.forms.fill', 'doc.forms.modify'] } }) } },
    snapshot: SNAP,
    steps: [...boot, call('refresh'), call('snapshot'), call('canRead'), call('canFill'), call('canDesign')],
  },
  {
    name: 'refresh-coalesces-and-forces',
    covers:
      'A refresh while a read is in flight queues exactly one more read after it and resolves when that settles; any number of calls during the read coalesce into that one. (v2 forced a read per `refresh(true)`; v3 has no force argument.)',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([NAME], { defer: 'first', times: 1 }),
          list([{ ...NAME, value: 'Grace' }], { defer: 'second', times: 1 }),
          list([{ ...NAME, value: 'Hopper' }], { defer: 'third' }),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('refresh'),
      call('refresh'),
      call('refresh'),
      settle('first', snapshot([NAME])),
      settle('second', snapshot([{ ...NAME, value: 'Grace' }])),
      call('snapshot'),
      call('refresh'),
      settle('third', snapshot([{ ...NAME, value: 'Hopper' }])),
    ],
  },
  {
    name: 'a-failed-load-warns',
    covers: 'A snapshot read that rejects is reported to console.warn and leaves the model empty; the next refresh reads again.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          { path: 'forms.list', times: 1, error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.read', required: 'doc.forms.read' } },
          list([NAME]),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('snapshot'), call('refresh'), call('snapshot')],
  },
  {
    name: 'widget-geometry-loads-once-per-page',
    covers:
      'ensureGeom reads one page of annotations, keeps object-number widgets as crop-relative content boxes (skipping other subtypes and index refs), ignores a page it is loading or has loaded or does not know, and the projection (listWidgets included) joins it.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([NAME, AGREE, text(5, 'far', 'f', {}, [[105, 2]])]),
          annotations(
            1,
            [
              widgetAnnot(1, 101),
              widgetAnnot(1, 102, { left: 50, bottom: 700, right: 68, top: 718 }),
              { ref: aIndex(1, 3), subtype: 'widget', rect: { left: 0, bottom: 0, right: 1, top: 1 } },
              { ...widgetAnnot(1, 150), subtype: 'square' },
            ],
            { defer: 'page1' },
          ),
          annotations(2, [widgetAnnot(2, 105, { left: 30, bottom: 400, right: 130, top: 420 })]),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('fillItems', [1]),
      call('ensureGeom', [1]),
      call('ensureGeom', [1]),
      settle('page1', {
        annotations: [
          widgetAnnot(1, 101),
          widgetAnnot(1, 102, { left: 50, bottom: 700, right: 68, top: 718 }),
          { ref: aIndex(1, 3), subtype: 'widget', rect: { left: 0, bottom: 0, right: 1, top: 1 } },
          { ...widgetAnnot(1, 150), subtype: 'square' },
        ],
      }),
      call('ensureGeom', [1]),
      call('fillItems', [1]),
      call('fillItem', [102]),
      call('ensureGeom', [99]),
      call('ensureGeom', [2]),
      call('fillItems', [2]),
      call('fillItems', [1]),
      call('listWidgets', [pr(1)]),
    ],
  },
  {
    name: 'a-failed-geometry-read-can-retry',
    covers: 'A page read that rejects clears the loading latch, so the next ensureGeom reads again.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([NAME]),
          annotations(1, [], { times: 1, error: { name: 'EngineError', message: 'gone', code: 'Io' } }),
          annotations(1, [widgetAnnot(1, 101)]),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('ensureGeom', [1]), call('ensureGeom', [1]), call('fillItems', [1])],
  },
  {
    name: 'fill-authority-fuses-into-disabled',
    covers:
      'Without doc.forms.fill every projected item is disabled, read-only ones included; the grant is re-read on every projection, and a fillable session leaves the flag gate in charge.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([NAME, text(6, 'ro', 'r', { flags: FLAGS({ readOnly: true }) })]), annotations(1, [widgetAnnot(1, 101), widgetAnnot(1, 106)])], {
          security: { allows: ['doc.forms.read'] },
        }),
        b: doc([list([NAME, text(6, 'ro', 'r', { flags: FLAGS({ readOnly: true }) })]), annotations(1, [widgetAnnot(1, 101), widgetAnnot(1, 106)])]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('ensureGeom', [1]),
      call('ensureGeom', [1], 'b'),
      call('fillItems', [1]),
      call('fillItems', [1]),
      call('fillItem', [101]),
      call('fillItem', [106]),
      call('fillItems', [1], 'b'),
      call('fillItem', [101], 'b'),
      call('fillItem', [101], 'b'),
    ],
  },
  {
    name: 'typed-writes',
    covers:
      'setText, toggle and choose send the family value through setValue, mark the field writing, and re-read the snapshot (which clears it). A null toggle clears the group.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', value: { changedWidgets: [v3Widget({ annotObjectNumber: 101, pageObjectNumber: 1 })] } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('setText', ['obj:1', 'Grace']),
      call('toggle', ['obj:2', 'Yes']),
      call('toggle', ['obj:2', null]),
      call('choose', ['obj:3', ['g']]),
      call('setText', ['fqn:loose', 'x']),
    ],
  },
  {
    name: 'writes-serialize',
    covers: 'A second write waits for the first one to finish, its re-read included.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', times: 1, defer: 'first' },
          { path: 'forms.setValue', value: { changedWidgets: [] } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('setText', ['obj:1', 'a']), call('setText', ['obj:1', 'b']), settle('first', { changedWidgets: [] })],
  },
  {
    name: 'a-failed-write-reverts',
    covers: 'An engine refusal clears the writing mark and rejects with the engine error; the queue carries on.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', times: 1, error: { name: 'EngineError', message: 'invalid value', code: 'InvalidArg' } },
          { path: 'forms.setValue', value: { changedWidgets: [] } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('setText', ['obj:1', 'bad']), call('setText', ['obj:1', 'good'])],
  },
  {
    name: 'the-write-gate',
    covers:
      'Without doc.forms.fill, setText/toggle/choose and reset reject with PermissionDenied before any engine call or writing mark.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)], { security: { allows: ['doc.forms.read'] } }) } },
    snapshot: SNAP,
    steps: [...boot, call('setText', ['obj:1', 'x']), call('toggle', ['obj:2', 'Yes']), call('choose', ['obj:3', []]), call('reset', ['obj:1'])],
  },
  {
    name: 'commit-value-without-scripting',
    covers:
      'commitValue is setValue plus a forced re-read: applied when widgets changed, unchanged otherwise, rejected with the engine error.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', times: 1, value: { changedWidgets: [v3Widget({ annotObjectNumber: 101, pageObjectNumber: 1 })] } },
          { path: 'forms.setValue', times: 1, value: { changedWidgets: [] } },
          { path: 'forms.setValue', error: { name: 'EngineError', message: 'nope', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('commitValue', [ref(1), { type: 'text', value: 'x' }]),
      call('commitValue', [ref(1), { type: 'text', value: 'x' }]),
      call('commitValue', [ref(1), { type: 'text', value: 'x' }]),
    ],
  },
  {
    name: 'reset-rides-the-batch',
    covers:
      'reset(key) applies one reset effect, re-reads, and patches the field from the fresh snapshot; a key the snapshot no longer has just clears the writing mark.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS, { times: 1 }),
          list([{ ...NAME, value: '' }, AGREE, COLOUR, COLOURS]),
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('reset', ['obj:1']), call('reset', ['obj:404'])],
  },
  {
    name: 'a-failed-reset-batch-rejects',
    covers:
      "A failed or rejected reset entry rejects reset() with the entry's error message, or 'reset failed' without one, after the re-read; a thrown batch rejects with the engine error.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'failed', fields: [], changedWidgets: [], error: { code: 'Io', message: 'disk full' } }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'rejected', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', error: { name: 'EngineError', message: 'batch refused', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('reset', ['obj:1']), call('reset', ['obj:1']), call('reset', ['obj:1'])],
  },
  {
    name: 'reset-without-the-batch',
    covers: 'An engine with no applyEffects resets through forms.reset and patches the field it answers with; a refusal reverts and rejects.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc(
          [
            list(FIELDS),
            { path: 'forms.reset', times: 1, value: { field: { ...NAME, value: '' }, changedWidgets: [] } },
            { path: 'forms.reset', error: { name: 'EngineError', message: 'no default', code: 'InvalidArg' } },
          ],
          { lacks: ['forms.applyEffects'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('reset', ['obj:1']), call('reset', ['obj:1'])],
  },
  {
    name: 'set-value-passthrough',
    covers: 'setValue is the raw engine door: it resolves with the engine result after a forced re-read, and rejects with the engine error.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', times: 1, value: { changedWidgets: [v3Widget({ annotObjectNumber: 101, pageObjectNumber: 1 })] } },
          { path: 'forms.setValue', error: { name: 'EngineError', message: 'nope', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('setValue', [ref(1), { type: 'text', value: 'x' }]), call('setValue', [ref(1), { type: 'text', value: 'y' }])],
  },
  {
    name: 'interchange',
    covers:
      "exportData defaults to 'xfdf'; importData passes its format through (undefined included) and SETs the snapshot the engine answers, without a re-read; repair re-reads; each rejects with the engine error.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.exportData', args: ['xfdf'], value: { format: 'xfdf', data: bytes } },
          { path: 'forms.exportData', args: ['fdf'], error: { name: 'EngineError', message: 'no fdf', code: 'NotImplemented' } },
          { path: 'forms.importData', times: 1, value: { applied: 1, skipped: 0, snapshot: { formKind: 'acroform', needsAppearances: false, fields: [{ ...NAME, value: 'Imported' }], calculationOrder: [] } } },
          { path: 'forms.importData', error: { name: 'EngineError', message: 'garbled', code: 'InvalidArg' } },
          { path: 'forms.repair', times: 1, value: { fixed: 0 } },
          { path: 'forms.repair', error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.modify', required: 'doc.forms.modify' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('exportData'),
      call('exportData', ['fdf']),
      call('importData', [bytes]),
      call('importData', [bytes, 'fdf']),
      call('repair'),
      call('repair', [{ bakeAppearances: true }]),
    ],
  },
  {
    name: 'the-twins',
    covers: 'canRead, canFill and canDesign each mirror one grant, independently.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(FIELDS)], { security: { allows: ['doc.forms.read', 'doc.forms.fill'] } }),
        b: doc([list(FIELDS)], { security: { allows: ['doc.forms.modify'] } }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('canRead'),
      call('canFill'),
      call('canDesign'),
      call('canRead', [], 'b'),
      call('canFill', [], 'b'),
      call('canDesign', [], 'b'),
    ],
  },
  {
    name: 'the-effect-folds-foreign-events',
    covers:
      'A remote form event re-reads the snapshot; an own value event is ignored; an own structural event clears the widget geometry and re-reads; stream.desynced re-reads; events without an origin and non-form events are ignored.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(FIELDS), annotations(1, [widgetAnnot(1, 101)])]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('ensureGeom', [1]),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.valueChanged', origin: { kind: 'local' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.fieldDeleted', origin: { kind: 'local' }, deletedFieldObjectNumber: 1, removedWidgets: [] }),
      emit({ type: 'stream.desynced' }),
      emit({ type: 'form.imported' }),
      emit({ type: 'annotations.created', origin: { kind: 'remote' }, pageObjectNumber: 1 }),
      emit({ type: 'form.repaired', origin: { kind: 'remote' } }),
    ],
  },
  {
    name: 'the-effect-refresh-is-latched',
    covers:
      'While the effect is re-reading, further events coalesce into one more read queued after it (v2 dropped them); a failed re-read does not stop the queue.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS, { times: 1 }),
          list([NAME], { times: 1, defer: 'remote' }),
          { path: 'forms.list', times: 1, error: { name: 'EngineError', message: 'gone', code: 'Io' } },
          list([AGREE]),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      emit({ type: 'form.valueChanged', origin: { kind: 'remote' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'stream.desynced' }),
      settle('remote', snapshot([NAME])),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote' }, field: NAME, changedWidgets: [] }),
    ],
  },
  {
    name: 'a-reset-action-without-a-batch-door',
    covers:
      'resetFormAction on an engine with no applyEffects resolves unchanged with an unsupported-api diagnostic and never reaches the engine; with no actions plugin there is no port to surface it through.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)], { lacks: ['forms.applyEffects'] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, call('resetFormAction', [null, false])],
  },
  {
    name: 'close-unsubscribes',
    covers: 'Closing the document releases the event subscription.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, close()],
  },
  {
    name: 'geometry-on-an-offset-crop',
    covers: 'Content boxes subtract the crop origin: a page whose crop does not start at zero.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list([text(5, 'far', 'f', {}, [[105, 2]])]), annotations(2, [widgetAnnot(2, 105, { left: CROP2.left + 30, bottom: 400, right: 140, top: 420.5 })])]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, call('ensureGeom', [2]), call('fillItems', [2])],
  },
];

const READ_FIELDS = [
  NAME,
  AGREE,
  COLOUR,
  COLOURS,
  text(7, 'lines', '', { valueEntry: { kind: 'array', values: ['l1', 'l2'] } }),
  checkbox(8, 'multi', false, { valueEntry: { kind: 'array', values: [] } }, [{ annotObjectNumber: 108, pageObjectNumber: 2, onState: 'Yes' }]),
  checkbox(9, 'multi2', false, { valueEntry: { kind: 'array', values: ['On'] } }),
  combobox(10, 'pick', '', { valueEntry: { kind: 'array', values: ['r', 'g'] } }),
  listbox(11, 'one', [], { valueEntry: { kind: 'scalar', value: 'r' } }),
  pushbutton(12, 'go'),
  text(13, 'odd', '', { valueEntry: { kind: 'unsupported' } }),
  text(14, 'loose', 'l', {}, [[114, 0]]),
  checkbox(15, 'ticked', true),
];

const v3Cases: ScenarioCase[] = [
  {
    name: 'reads-over-the-snapshot',
    covers:
      'getStatus walks loading to ready; listFields filters by family, exact name and widget page (an unplaced widget matches no page); getValue maps each value entry to the write vocabulary and answers null for a valueless, unsupported or unknown field; exportValues keys the valued ones by name; getFieldForWidget takes a ref or an object number, and a non-object-number ref finds nothing.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(READ_FIELDS, { defer: 'list' })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('getStatus'),
      call('listFields'),
      call('exportValues'),
      settle('list', snapshot(READ_FIELDS)),
      call('getStatus'),
      call('listFields'),
      call('listFields', [{ family: 'checkbox' }]),
      call('listFields', [{ name: 'colour' }]),
      call('listFields', [{ name: 'nope' }]),
      call('listFields', [{ page: pr(2) }]),
      call('listFields', [{ family: 'text', page: pr(1) }]),
      call('listFields', [{}]),
      ...[1, 2, 3, 4, 7, 8, 9, 10, 11, 12, 13, 15, 404].map((n) => call('getValue', [ref(n)])),
      call('getValue', [fqn('loose')]),
      call('exportValues'),
      call('getFieldForWidget', [{ kind: 'objectNumber', page: pr(1), annotObjectNumber: 102 }]),
      call('getFieldForWidget', [{ kind: 'nm', page: pr(1), nm: 'n1' }]),
      call('getFieldForWidget', [{ annotObjectNumber: 104 }]),
      call('listWidgets', [pr(1)]),
    ],
  },
  {
    name: 'status-without-read-and-after-a-failed-load',
    covers: "Without doc.forms.read the status is 'forbidden'; a load that rejects leaves it 'error'.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(FIELDS)], { security: { allows: ['doc.forms.fill'] } }),
        b: doc([{ path: 'forms.list', error: { name: 'EngineError', message: 'gone', code: 'Io' } }]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('getStatus'),
      call('getStatus', [], 'b'),
    ],
  },
  {
    name: 'batch-writes',
    covers:
      'setValues writes each entry through the validated path in order and reports the refs that landed and the ones that threw, with the error info; importValues does the same keyed by name. Without doc.forms.fill every entry fails permission-denied.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.setValue', times: 1, value: { changedWidgets: [] } },
          { path: 'forms.setValue', times: 1, error: { name: 'EngineError', message: 'invalid value', code: 'InvalidArg' } },
          { path: 'forms.setValue', value: { changedWidgets: [] } },
        ]),
        b: doc([list(FIELDS)], { security: { allows: ['doc.forms.read'] } }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('setValues', [[
        { ref: ref(1), value: { type: 'text', value: 'Grace' } },
        { ref: ref(2), value: { type: 'toggle', state: 'Yes' } },
      ]]),
      call('importValues', [{ name: { type: 'text', value: 'Ada' }, colour: { type: 'choice', values: ['g'] } }]),
      call('setValues', [[]]),
      call('setValues', [[{ ref: ref(1), value: { type: 'text', value: 'x' } }]], 'b'),
      call('importValues', [{ name: { type: 'text', value: 'x' } }], 'b'),
    ],
  },
  {
    name: 'values-keyed-by-integer-like-names',
    covers:
      "exportValues and importValues follow JavaScript object key order: integer-like names ('2', '10') first in ascending numeric order, then the rest in insertion order, whatever the field order.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([text(1, 'b', 'x'), text(2, '10', 'y'), text(3, '2', 'z'), text(4, 'a', 'w'), text(5, '02', 'v')]),
          { path: 'forms.setValue', value: { changedWidgets: [] } },
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      call('exportValues'),
      call('importValues', [
        {
          b: { type: 'text', value: 'B' },
          10: { type: 'text', value: 'Ten' },
          2: { type: 'text', value: 'Two' },
          a: { type: 'text', value: 'A' },
          '02': { type: 'text', value: 'Zero-two' },
        },
      ]),
    ],
  },
  {
    name: 'reset-all',
    covers:
      'resetAll resets every resettable field (buttons and signatures skipped), a listed subset by object number or name, or its complement with exclude, through the one reset batch; it answers the refs it resolved from the current snapshot.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list([...FIELDS, pushbutton(12, 'go')]),
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      call('resetAll'),
      call('resetAll', [{ fields: [ref(1), fqn('agree')] }]),
      call('resetAll', [{ fields: [ref(1)], exclude: true }]),
      call('resetAll', [{ fields: [] }]),
    ],
  },
  {
    name: 'reset-all-failures',
    covers:
      "A failed or rejected reset entry makes resetAll report its fields as failed 'operation-failed' and the rest as applied; without doc.forms.fill it rejects permission-denied before any call; before the snapshot lands it reads one itself.",
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FIELDS, { defer: 'first', times: 1 }),
          list(FIELDS),
          {
            path: 'forms.applyEffects',
            times: 1,
            value: {
              results: [{ index: 0, status: 'failed', fields: [NAME], changedWidgets: [] }, { index: 1, status: 'applied', fields: [AGREE], changedWidgets: [] }],
              changedWidgets: [],
              meta: null,
            },
          },
          {
            path: 'forms.applyEffects',
            times: 1,
            value: { results: [{ index: 0, status: 'rejected', fields: [AGREE], changedWidgets: [] }], changedWidgets: [], meta: null },
          },
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
        b: doc([list(FIELDS)], { security: { allows: ['doc.forms.read'] } }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('resetAll', [], 'b'),
      call('resetAll'),
      settle('first', snapshot(FIELDS)),
      call('resetAll'),
      call('resetAll'),
    ],
  },
  {
    name: 'confirmed-change-events',
    covers:
      'Every form event with an origin is announced before it is folded: a value change with its field, a created field, an updated, attached or detached one as updated, a deleted one by object number with no field; each carries the origin, whose sub becomes the actorId. onResynced fires when a read lands. document.versioned and signature.completed re-read too. A throwing listener is reported to console.error and the rest still run; an unsubscribed one hears nothing.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      call('onValueChanged', [fnSpec('valueChanged.bad', { throws: { message: 'listener broke' } })]),
      call('onValueChanged', [fnSpec('valueChanged')]),
      call('onFieldCreated', [fnSpec('fieldCreated')]),
      call('onFieldUpdated', [fnSpec('fieldUpdated')]),
      call('onFieldDeleted', [fnSpec('fieldDeleted')]),
      call('onValidationRejected', [fnSpec('validationRejected')]),
      call('onResynced', [fnSpec('resynced')]),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote', actor: 'u2' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.valueChanged', origin: { kind: 'local' }, field: AGREE, changedWidgets: [] }),
      emit({ type: 'form.valueChanged', origin: { kind: 'remote', sub: 'u9', sessionId: 's9' }, field: NAME, changedWidgets: [] }),
      emit({ type: 'form.fieldCreated', origin: { kind: 'local' }, field: COLOUR, changedWidgets: [] }),
      emit({ type: 'form.fieldUpdated', origin: { kind: 'local' }, field: COLOUR, changedWidgets: [] }),
      emit({ type: 'form.widgetAttached', origin: { kind: 'local' }, field: COLOUR, changedWidgets: [] }),
      emit({ type: 'form.widgetDetached', origin: { kind: 'remote' }, field: COLOUR, changedWidgets: [] }),
      emit({ type: 'form.fieldDeleted', origin: { kind: 'local' }, deletedFieldObjectNumber: 3, removedWidgets: [] }),
      emit({ type: 'form.repaired', origin: { kind: 'local' } }),
      emit({ type: 'document.versioned', version: 2 }),
      emit({ type: 'signature.completed' }),
    ],
  },
  {
    name: 'an-unsubscribed-listener-hears-nothing',
    covers: 'The unsubscribe an event hook returns detaches that listener.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      call('onResynced', [fnSpec('resynced')]),
      { op: 'call', target: { ref: 0 } },
      call('refresh'),
    ],
  },
];

function aIndex(pon: number, index: number) {
  return { kind: 'index', page: pr(pon), index, revision: 1 };
}

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-fill.json',
    describes: 'packages/plugin/form without actions or annotation: hydration, geometry, the fill projection, value writes, reset, interchange, the twins and the reactive effect.',
    cases: [...cases, ...v3Cases],
  },
];
