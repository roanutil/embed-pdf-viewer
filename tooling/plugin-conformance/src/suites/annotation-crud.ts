/**
 * `packages/plugin/annotation`: the data API (create, update, delete), the
 * optimistic effects and their rollback, selection, the props and flags
 * pipelines, text editing, settings and the render projections. Replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';

import {
  PLUGINS,
  SNAP,
  advance,
  boot,
  call,
  callout,
  circle,
  created,
  doc,
  fail,
  freeText,
  hydrate,
  ink,
  line,
  link,
  markup,
  objRef,
  polygon,
  settle,
  square,
  stamp,
  textNote,
  updated,
} from './annotation-fixtures';

const shapes = { 1: [square(1), circle(2, { rect: { left: 300, bottom: 300, right: 400, top: 400 } }), line(3)] };

const cases: ScenarioCase[] = [
  {
    name: 'create-programmatically',
    covers:
      'create() defaults an absent /F to print and sends an explicit one verbatim, then syncs the created DTO (vector, or baked for a stamp) and resolves its ref. An engine refusal rejects; a page the document does not have rejects with the NotFound page() throws.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(square(10)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(stamp(11)) },
          { path: 'page.annotations.create', pon: 1, times: 1, error: { name: 'EngineError', message: 'nope', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('create', [1, { subtype: 'square', rect: { left: 1, bottom: 2, right: 3, top: 4 } }]),
      call('create', [1, { subtype: 'stamp', rect: { left: 1, bottom: 2, right: 3, top: 4 }, flags: { hidden: true } }]),
      call('create', [1, { subtype: 'square', rect: { left: 1, bottom: 2, right: 3, top: 4 } }]),
      call('create', [99, { subtype: 'square' }]),
      call('list', [1]),
    ],
  },
  {
    name: 'update-and-delete-by-ref',
    covers:
      'update() and delete() resolve the page from the model (or an index ref’s own pon), await the engine, and sync or remove. An unknown ref rejects with the page error; an engine refusal rejects and changes nothing.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(shapes),
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(square(1, { contents: 'patched' })) },
          { path: 'page.annotations.update', pon: 1, times: 1, error: { name: 'EngineError', message: 'stale', code: 'Conflict' } },
          { path: 'page.annotations.update', pon: 1, value: updated(square(1, { contents: 'by index' })) },
          { path: 'page.annotations.delete', pon: 1, times: 1, value: {} },
          { path: 'page.annotations.delete', pon: 1, error: { name: 'EngineError', message: 'locked', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('update', [objRef(1), { subtype: 'square', contents: 'patched' }]),
      call('get', [objRef(1)]),
      call('update', [objRef(1), { subtype: 'square', contents: 'again' }]),
      call('update', [objRef(42), { subtype: 'square' }]),
      call('update', [{ kind: 'index', pageObjectNumber: 1, index: 0, revision: { docSessionId: 's', pageObjectNumber: 1, generation: 1 } }, { subtype: 'square' }]),
      call('delete', [objRef(2)]),
      call('delete', [objRef(3)]),
      call('delete', [objRef(42)]),
      call('list', [1]),
    ],
  },
  {
    name: 'selection-reads',
    covers:
      'select (a group member takes its group, add extends), selection, getSelection and getSelected (drafts skipped), deselect, and the selection flags and props (shared specs, first values, mixed keys, the derived link value).',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              square(1, { flags: { ...FLAGS_LOCKED() } }),
              square(2, { color: { r: 0, g: 0, b: 255 }, strokeWidth: 5 }),
              circle(3),
              link(4, { inReplyTo: objRef(3), replyType: 'group', target: { kind: 'uri', uri: 'https://child' } }),
              markup('highlight', 5),
            ],
          }),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('getSelectionFlags'),
      call('getSelectionProps'),
      call('select', [objRef(1)]),
      call('select', [objRef(2), { add: true }]),
      call('selection'),
      call('getSelection'),
      call('getSelected'),
      call('getSelectionFlags'),
      call('getSelectionProps'),
      call('select', [objRef(3), { add: false }]),
      call('getSelectionProps'),
      call('select', [objRef(5), { add: true }]),
      call('getSelectionProps'),
      call('getSelectionFlags'),
      call('deselect'),
      call('selection'),
      call('select', [objRef(77)]),
      call('selection'),
    ],
  },
  {
    name: 'restyle-writes-and-rolls-back',
    covers:
      'updateSelection is optimistic: each selected member takes the keys its kind declares, and one scoped engine write per changed member re-syncs from the returned DTO (bumping the appearance only on the engine’s say-so). A refused write rolls the member back to its last committed DTO.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(shapes),
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(square(1, { color: { r: 0, g: 255, b: 0 } }), true) },
          { path: 'page.annotations.update', pon: 1, times: 1, defer: 'circle' },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(line(3, { strokeWidth: 9 })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('select', [objRef(1)]),
      call('select', [objRef(2), { add: true }]),
      call('updateSelection', [{ color: '#00ff00', fontSize: 30 }]),
      call('getSelectionProps'),
      fail('circle', 'stale revision'),
      call('get', [objRef(2)]),
      call('select', [objRef(3)]),
      call('updateSelection', [{ strokeWidth: 9, lineEndings: { start: 'circle' } }]),
      call('appearanceEpoch', [1]),
    ],
  },
  {
    name: 'flags-writes',
    covers:
      'updateSelectionFlags merges /F over the selection and writes the full flag set per changed member, re-syncing with the member’s own render source; a refused write reports to console.error. It works on a locked annotation (that is how one unlocks).',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1, { flags: FLAGS_LOCKED() }), square(2)] }),
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated(square(1)) },
          { path: 'page.annotations.update', pon: 1, error: { name: 'EngineError', message: 'denied', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('select', [objRef(1)]),
      call('select', [objRef(2), { add: true }]),
      call('updateSelectionFlags', [{ locked: false, print: false }]),
      call('getSelectionFlags'),
      call('list', [1]),
    ],
  },
  {
    name: 'delete-and-rotate-the-selection',
    covers:
      'deleteSelection removes optimistically and fires one engine delete per committed member, whatever it answers; rotateSelection90 and resetSelectionRotation commit one geometry patch per rotatable member.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), square(2), polygon(3)] }),
          { path: 'page.annotations.update', pon: 1, value: updated(square(1)) },
          { path: 'page.annotations.delete', pon: 1, times: 1, value: {} },
          { path: 'page.annotations.delete', pon: 1, error: { name: 'EngineError', message: 'x', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('select', [objRef(1)]),
      call('rotateSelection90'),
      call('resetSelectionRotation'),
      call('select', [objRef(3)]),
      call('rotateSelection90'),
      call('select', [objRef(2), { add: true }]),
      call('deleteSelection'),
      call('list', [1]),
    ],
  },
  {
    name: 'drawn-shape-commits',
    covers:
      'A createPointer drag commits a draft: the create effect sends the repository draft (with the seeded /NM and /F), reconciles the temp id to the durable ref, and syncs the DTO as vector. A refused create drops the draft.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(square(20)) },
          { path: 'page.annotations.create', pon: 1, error: { name: 'EngineError', message: 'full', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('createPointer', ['square', 'down', 1, { x: 100, y: 100 }]),
      call('createPointer', ['square', 'move', 1, { x: 150, y: 140 }]),
      call('createPointer', ['square', 'up', 1, { x: 180, y: 160 }]),
      call('list', [1]),
      call('createPointer', ['circle', 'down', 1, { x: 10, y: 10 }]),
      call('createPointer', ['circle', 'up', 1, { x: 60, y: 50 }]),
      call('selection'),
      call('createPointer', ['no-such-tool', 'down', 1, { x: 10, y: 10 }, false, 90]),
      call('creationDraftAnchor'),
      call('cancelCreationDraft'),
    ],
  },
  {
    name: 'free-text-editing',
    covers:
      'beginTextEdit and beginTextEditAt (a hit on a free-text body or handle opens it; anything else reports false), setContents (optimistic, debounced 250ms per annotation, restarted by the next keystroke), endTextEdit (flushes the committed contents at once and cancels the timers), textItems and currentEditing.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [freeText(1, { rect: { left: 100, bottom: 600, right: 300, top: 700 } }), square(2, { rect: { left: 400, bottom: 100, right: 500, top: 200 } }), callout(3)] }),
          { path: 'page.annotations.update', pon: 1, value: updated(freeText(1, { contents: 'Hello world' })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('textItems', [1]),
      call('beginTextEditAt', [1, { x: 150, y: 150 }]),
      call('currentEditing'),
      call('beginTextEditAt', [1, { x: 450, y: 650 }, 2, 0, 1]),
      call('beginTextEditAt', [1, { x: 5, y: 5 }]),
      call('setContents', [objRef(1), 'Hello w']),
      advance(100),
      call('setContents', [objRef(1), 'Hello world']),
      advance(249),
      advance(1),
      call('setContents', [objRef(1), 'Hello world!']),
      call('endTextEdit'),
      advance(500),
      call('currentEditing'),
      call('beginTextEdit', [objRef(1)]),
      call('textItems', [1, { zoom: 2, rotation: 0 }]),
      call('endTextEdit'),
      call('beginTextEdit', [objRef(3)]),
      call('setContents', [objRef(3), 'lead']),
      call('endTextEdit'),
    ],
  },
  {
    name: 'settings',
    covers:
      'setDefaults and currentDefaults through a tool’s preset (arrow reads its own bag; a bare subtype falls back), propsForTool through the tool’s props kind, setSnap and snapSettings, setChrome (deep merge) and chromeSettings, and chrome() dropping the rotate guides when they are disabled.',
    plugins: [{ factory: 'interaction' }, { factory: 'annotation', config: { snap: { guides: false }, chrome: { accent: '#e91e63' }, tools: [{ id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'closed-arrow' } } }] } }],
    engine: { documents: { a: doc([hydrate({ 1: [square(1)] })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('currentDefaults', ['arrow']),
      call('currentDefaults', ['line']),
      call('setDefaults', ['arrow', { strokeWidth: 3 }]),
      call('currentDefaults', ['arrow']),
      call('currentDefaults', ['square']),
      call('currentDefaults', ['widget-text']),
      call('propsForTool', ['free-text-callout']),
      call('propsForTool', ['insert-text']),
      call('propsForTool', ['nothing']),
      call('snapSettings'),
      call('setSnap', [{ rotationThreshold: 10 }]),
      call('snapSettings'),
      call('chromeSettings'),
      call('setChrome', [{ handles: { hitSize: 40 }, guides: { enabled: false } }]),
      call('chromeSettings'),
      call('select', [objRef(1)]),
      call('chrome', [1]),
      call('chrome', [1, 2, 90, 1.5]),
      call('chrome', [1, 2, 90, 1.5]),
      call('selectionAnchor'),
      call('selectionAnchor', [2, 0, 1]),
      call('selectionAnchor', [2, 0, 1]),
    ],
  },
  {
    name: 'render-projections',
    covers:
      'pageItems (memoized by model, ghost and view), appearanceEpoch (baked, committed, painting annotations only), bakeScale (identity under the continuous policy), toContentBox, linkItemsOn (standalone and attached links, hidden ones excluded, the /AA hover flags), creationDraftAnchor.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              square(1),
              textNote(2, { inReplyTo: objRef(1), contents: 'reply' }),
              link(3, { actions: { activate: { root: { kind: 'uri' } }, cursorEnter: { root: { kind: 'js' } } } }),
              link(4, { inReplyTo: objRef(1), replyType: 'group', actions: { cursorExit: { root: { kind: 'js' } } } }),
              link(5, { flags: { ...FLAGS_HIDDEN() } }),
              link(6, { target: null }),
              ink(7),
            ],
          }),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('pageItems', [1]),
      call('pageItems', [1, { zoom: 2, rotation: 90 }]),
      call('appearanceEpoch', [1]),
      call('appearanceEpoch', [2]),
      call('bakeScale', [1.37]),
      call('linkItemsOn', [1]),
      call('linkItemsOn', [2]),
      call('createPointer', ['polygon', 'down', 1, { x: 10, y: 10 }]),
      call('createPointer', ['polygon', 'down', 1, { x: 50, y: 10 }]),
      call('creationDraftAnchor'),
      call('createPointer', ['polygon', 'down', 1, { x: 30, y: 40 }]),
      call('creationDraftAnchor'),
      call('cancelCreationDraft'),
    ],
  },
];

function FLAGS_LOCKED() {
  return { invisible: false, hidden: false, print: true, noZoom: false, noRotate: false, noView: false, readOnly: false, locked: true, toggleNoView: false, lockedContents: false };
}
function FLAGS_HIDDEN() {
  return { ...FLAGS_LOCKED(), locked: false, hidden: true };
}

void settle;

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-crud.json',
    describes:
      'packages/plugin/annotation: create/update/delete, the optimistic effects and their rollback, selection, props and flags, text editing, settings and projections.',
    cases,
  },
];
