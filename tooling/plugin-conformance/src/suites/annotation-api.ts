/**
 * `packages/plugin/annotation`: the v3 page-space surface the older suites,
 * written against v2 names, never reached. The page-space reads (`get`,
 * `list`, `hitTestAt`, `getStatus`), `create` / `createMany` from page-space
 * input, `update` / `updateMany` / `setRotation` / `rotateBy` / `deleteMany`,
 * the confirmed-change events, the selection verbs (`selectAll`,
 * `selectInRect`, `toggleTextFormat`, run restyles over an editor range),
 * rich-text editing, `createAttachment`, `placeStamp`, `readAttachment` on an
 * engine without `downloadFile`, the measurement seam (viewports, distance
 * drafts, the calibration capture, `remeasurePage`) and the v3 document
 * events. Replayed by `crates/plugins/annotation/tests/conformance.rs`.
 *
 * Every call here names its v3 method directly (`api`), bypassing the v2
 * mapping table in `annotation-fixtures.ts`.
 */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';

import {
  PLUGINS,
  SNAP,
  advance,
  boot,
  circle,
  created,
  doc,
  emit,

  FLAGS,
  formWidget,
  freeText,
  callout,
  hydrate,
  ink,
  ix,
  line,
  link,
  markup,
  objRef,
  pr,
  polygon,
  polyline,
  settle,
  caret,
  redact,
  square,
  stamp,
  textNote,
  tool,
  updated,
  fileAttachment,
  widget,
} from './annotation-fixtures';

/** A call on the annotation capability by its v3 name. */
const api = (method: string, args: unknown[] = [], d = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation', doc: d },
  method,
  args,
});
/** A text quad over one line, and one glyph cell of it. */
const TQ = { upperStart: { x: 10, y: 10 }, upperEnd: { x: 60, y: 10 }, lowerStart: { x: 10, y: 22 }, lowerEnd: { x: 60, y: 22 } };
const GLYPH = { upperStart: { x: 10, y: 10 }, upperEnd: { x: 16, y: 10 }, lowerStart: { x: 10, y: 22 }, lowerEnd: { x: 16, y: 22 } };
/** Subscribe every confirmed-change event hook to a named callback. */
const listenAll: Step[] = [
  api('onCreated', [{ $fn: 'created' }]),
  api('onUpdated', [{ $fn: 'updated' }]),
  api('onDeleted', [{ $fn: 'deleted' }]),
  api('onResynced', [{ $fn: 'resynced' }]),
  api('onSelectionChanged', [{ $fn: 'selection' }]),
  api('onDraftChanged', [{ $fn: 'draft' }]),
  api('onEditingChanged', [{ $fn: 'editing' }]),
];

const RICH_BODY = {
  family: 'Helvetica',
  weight: 400,
  italic: false,
  size: 12,
  color: '#EF4444',
  decoration: [],
  script: 'normal',
  letterSpacing: 0,
  horizontalScale: 1,
  align: 'left',
  dir: 'ltr',
};
const richFree = (n: number, over: Record<string, unknown> = {}) =>
  freeText(n, {
    richText: { body: RICH_BODY, paragraphs: [{ runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }] }] },
    ...over,
  });

/** 72 pt to the metre. */
const METRES = {
  subtype: 'RL',
  ratio: '1 in = 1 m',
  x: [{ unit: 'm', conversion: 1 / 72 }],
  distance: [{ unit: 'm', conversion: 1 }],
  area: [{ unit: 'm²', conversion: 1 }],
};
const FEET = { ...METRES, ratio: '1 in = 1 ft', x: [{ unit: 'ft', conversion: 1 / 72 }], distance: [{ unit: 'ft', conversion: 1 }], area: [{ unit: 'ft²', conversion: 1 }] };
const dimensionLine = (n: number, over: Record<string, unknown> = {}) =>
  line(n, {
    intent: 'LineDimension',
    measure: METRES,
    caption: { enabled: true, position: 'inline' },
    leader: { length: 12, extension: 5, offset: 0 },
    contents: '2 m',
    linePoints: { start: { x: 100, y: 700 }, end: { x: 244, y: 700 } },
    ...over,
  });

const READS = {
  1: [
    square(1, { author: 'ann', rotation: 30, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } }),
    line(2, { author: 'bob' }),
    polygon(3, { rect: { left: 300, bottom: 300, right: 400, top: 400 }, vertices: [{ x: 300, y: 400 }, { x: 400, y: 400 }, { x: 350, y: 300 }] }),
    ink(4, { rect: { left: 400, bottom: 100, right: 500, top: 200 }, inkList: [[{ x: 400, y: 200 }, { x: 450, y: 150 }]] }),
    markup('highlight', 5),
    caret(6),
    callout(7, { author: 'ann', created: '2024-01-01T00:00:00Z', modified: '2024-01-02T00:00:00Z' }),
    link(8, { inReplyTo: objRef(7), replyType: 'group', rect: { left: 60, bottom: 510, right: 120, top: 560 } }),
    textNote(9, { inReplyTo: objRef(1), replyType: 'reply', contents: 'a reply' }),
    polyline(10, { rect: { left: 500, bottom: 500, right: 580, top: 580 }, vertices: [{ x: 500, y: 580 }, { x: 580, y: 500 }] }),
  ],
  2: [circle(20, { pageObjectNumber: 2, rect: { left: 60, bottom: 100, right: 160, top: 200 } })],
};

const cases: ScenarioCase[] = [
  {
    name: 'page-space-reads',
    covers:
      'get projects a loaded record into page space (bounds, geometry per kind, props, flags, text, author, dates, group, reply, authority, raw) once per model entry; list filters by page, subtype, author and group, memoizing the whole and per-page lists; listRaw and getRaw read the DTOs; hitTestAt answers the annotation under a point, the selection for a handle, null over empty space; getStatus is loading until hydration lands; listTools and getTool project the resolved tools.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([{ path: 'annotations.listRawAll', defer: 'hydrate' }]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      api('getStatus'),
      api('list'),
      settle('hydrate', {
        pages: Object.entries(READS).map(([pon, annotations]) => ({ pageState: { page: pr(Number(pon)) }, annotations })),
      }),
      api('getStatus'),
      ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 10].map((n) => api('get', [objRef(n)])),
      api('get', [objRef(20, 2)]),
      api('get', [objRef(99)]),
      api('list'),
      api('list'),
      api('list', [{ page: pr(2) }]),
      api('list', [{ page: pr(2) }]),
      api('list', [{ page: pr(1), subtype: 'square' }]),
      api('list', [{ author: 'ann' }]),
      api('list', [{ group: objRef(7) }]),
      api('listRaw', [{ group: objRef(7) }]),
      api('listRaw', [{ author: 'nobody' }]),
      api('getRaw', [objRef(2)]),
      api('getRaw', [objRef(99)]),
      api('hitTestAt', [pr(1), { x: 150, y: 150 }]),
      api('hitTestAt', [pr(1), { x: 5, y: 5 }]),
      api('select', [objRef(3)]),
      api('hitTestAt', [pr(1), { x: 300, y: 400 }]),
      api('listSelected'),
      api('listSelected'),
      api('getSelection'),
      api('listTools'),
      api('getTool', ['arrow']),
      api('getTool', ['square']),
      api('getTool', ['square']),
      api('registerTool', [{ id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'open-arrow' } } }]),
      api('getTool', ['arrow']),
      { op: 'call', target: { ref: 0 } },
      api('getTool', ['arrow']),
    ],
  },
  {
    name: 'create-from-page-space',
    covers:
      'create stages page-space input through the one createAnnot path a draw tool takes: tool defaults, the tool’s /F seed under the input’s, props over defaults, optional selection; it resolves the engine’s ref once confirmed and announces onCreated with an api origin; an engine refusal rolls the entry back and rejects. Each geometry kind lowers to its draft. A page the document lacks, invalid geometry, an unknown tool and a subtype create cannot build reject without staging. createMany folds per-input outcomes into a batch result.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(square(30)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(circle(31)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(line(32)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(polygon(33)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(polyline(34)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(ink(35)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(markup('highlight', 36)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(redact(37)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(freeText(38)) },
          { path: 'page.annotations.create', pon: 1, times: 1, error: { name: 'EngineError', message: 'full', code: 'Refused' } },
          { path: 'page.annotations.create', pon: 2, times: 1, value: created(square(40, { pageObjectNumber: 2 })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(square(41)) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listenAll,
      api('create', [{ page: pr(1), subtype: 'square', bounds: { x: 10, y: 20, width: 30, height: 40 }, props: { color: '#00ff00' }, select: true }]),
      api('create', [{ page: pr(1), subtype: 'circle', bounds: { x: 10, y: 20, width: 30, height: 40 }, rotation: 45, flags: { locked: true } }]),
      api('create', [{ page: pr(1), subtype: 'line', from: { x: 1, y: 2 }, to: { x: 50, y: 60 }, tool: 'line' }]),
      api('create', [{ page: pr(1), subtype: 'polygon', vertices: [{ x: 0, y: 0 }, { x: 50, y: 0 }, { x: 25, y: 40 }], tool: 'area' }]),
      api('create', [{ page: pr(1), subtype: 'polyline', vertices: [{ x: 0, y: 0 }, { x: 50, y: 10 }] }]),
      api('create', [{ page: pr(1), subtype: 'ink', strokes: [[{ x: 0, y: 0 }, { x: 5, y: 5 }]], tool: 'ink-highlight' }]),
      api('create', [{ page: pr(1), subtype: 'highlight', quads: [TQ] }]),
      api('create', [{ page: pr(1), subtype: 'redact', bounds: { x: 10, y: 20, width: 30, height: 40 } }]),
      api('create', [{ page: pr(1), subtype: 'free-text', bounds: { x: 10, y: 20, width: 100, height: 40 }, callout: { tip: { x: 0, y: 0 }, ending: 'open-arrow' }, tool: 'free-text-callout' }]),
      api('create', [{ page: pr(1), subtype: 'free-text', bounds: { x: 10, y: 20, width: 100, height: 40 } }]),
      api('create', [{ page: pr(3), subtype: 'square', bounds: { x: 0, y: 0, width: 1, height: 1 } }]),
      api('create', [{ page: pr(1), subtype: 'square', bounds: { x: 0, y: 0, width: -1, height: 1 } }]),
      api('create', [{ page: pr(1), subtype: 'square', bounds: { x: 0, y: 0, width: 1, height: 1 }, tool: 'nope' }]),
      api('create', [{ page: pr(1), subtype: 'stamp', bounds: { x: 0, y: 0, width: 1, height: 1 } }]),
      api('createMany', [
        [
          { page: pr(2), subtype: 'square', bounds: { x: 1, y: 2, width: 3, height: 4 } },
          { page: pr(1), subtype: 'polygon', vertices: [] },
          { page: pr(1), subtype: 'square', bounds: { x: 1, y: 2, width: 3, height: 4 } },
        ],
      ]),
      api('list'),
    ],
  },
  {
    name: 'create-without-authority',
    covers:
      'create, createFromSelection and createAttachment refuse with permission-denied when the session cannot create; the pointer, markup, caret and replace-text doors stay inert; canCreate answers false.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate({ 1: [square(1)] })], { security: { allows: ['doc.annotate.read'] } }) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('canCreate'),
      api('create', [{ page: pr(1), subtype: 'square', bounds: { x: 0, y: 0, width: 1, height: 1 } }]),
      api('createFromSelection', ['highlight']),
      api('createAttachment', [pr(1), { x: 10, y: 10 }, { data: { $bytes: 'AAEC' }, name: 'a.bin' }]),
      api('createMarkup', ['highlight', pr(1), [TQ]]),
      api('createCaret', [pr(1), { glyphQuad: GLYPH, advance: 6 }]),
      api('createReplaceText', [pr(1), [], { glyphQuad: GLYPH, advance: 6 }]),
      api('createPointer', ['square', 'down', pr(1), { x: 10, y: 10 }]),
      api('getCreationDraft'),
      api('hasCreationDraft'),
      api('canEdit', [objRef(1)]),
      api('canDelete', [objRef(1)]),
      api('canEdit', [objRef(99)]),
    ],
  },
  {
    name: 'update-in-page-space',
    covers:
      'update lowers page-space bounds, geometry per kind, props, flags, contents and rich text through the repository projection and one engine write, announcing onUpdated; rich text commits through the text door. A geometry of the wrong kind, bounds on a vertex kind, a props patch the annotation refuses, and an unloaded ref reject. setRotation and rotateBy write the geometry scope (a markup does not rotate); updateRaw sends a PDF-space patch as is; updateMany and deleteMany fold per-ref outcomes; delete announces onDeleted.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              square(1),
              line(2),
              polygon(3),
              ink(4),
              markup('highlight', 5),
              callout(6),
              richFree(7),
              square(8, { flags: FLAGS({ locked: true }) }),
              polyline(9),
              circle(10, { rotation: 30 }),
            ],
          }),
          { path: 'page.annotations.update', pon: 1, args: [objRef(99), { subtype: 'square', contents: 'x' }], error: { name: 'EngineError', message: 'gone', code: 'NotFound' } },
          { path: 'page.annotations.update', pon: 1, value: updated(square(1, { contents: 'patched' }), true) },
          { path: 'page.annotations.delete', pon: 1, args: [objRef(4)], error: { name: 'EngineError', message: 'locked', code: 'Refused' } },
          { path: 'page.annotations.delete', pon: 1, value: {} },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listenAll,
      api('update', [objRef(1), { bounds: { x: 10, y: 20, width: 30, height: 40 }, flags: { hidden: true }, contents: 'new' }]),
      api('update', [objRef(1), { geometry: { kind: 'rect', bounds: { x: 1, y: 2, width: 3, height: 4 }, rotation: 15 } }]),
      api('update', [objRef(10), { geometry: { kind: 'rect', bounds: { x: 1, y: 2, width: 3, height: 4 } } }]),
      api('update', [objRef(2), { geometry: { kind: 'line', from: { x: 1, y: 2 }, to: { x: 30, y: 40 } } }]),
      api('update', [objRef(3), { geometry: { kind: 'polygon', vertices: [{ x: 0, y: 0 }, { x: 5, y: 0 }, { x: 5, y: 5 }] } }]),
      api('update', [objRef(9), { geometry: { kind: 'polyline', vertices: [{ x: 0, y: 0 }, { x: 5, y: 5 }] } }]),
      api('update', [objRef(4), { geometry: { kind: 'ink', strokes: [[{ x: 0, y: 0 }, { x: 9, y: 9 }]] } }]),
      api('update', [objRef(5), { geometry: { kind: 'markup', quads: [TQ] } }]),
      api('update', [objRef(6), { geometry: { kind: 'text', bounds: { x: 100, y: 100, width: 150, height: 50 }, rotation: 90, callout: null } }]),
      api('update', [objRef(6), { geometry: { kind: 'text', bounds: { x: 100, y: 100, width: 150, height: 50 }, callout: { tip: { x: 10, y: 10 }, ending: 'none' } } }]),
      api('update', [objRef(6), { bounds: { x: 100, y: 100, width: 150, height: 50 }, props: { fontSize: 20, bold: true } }]),
      api('update', [objRef(7), { richText: { paragraphs: [{ runs: [{ text: 'rich' }] }] } }]),
      api('update', [objRef(1), { props: { color: '#123456' }, richText: undefined }]),
      api('update', [objRef(1), { geometry: { kind: 'line', from: { x: 0, y: 0 }, to: { x: 1, y: 1 } } }]),
      ...['rect', 'polygon', 'ink', 'markup', 'text'].map((kind) =>
        api('update', [objRef(2), { geometry: { kind, bounds: { x: 0, y: 0, width: 1, height: 1 }, vertices: [], strokes: [], quads: [] } }]),
      ),
      api('update', [objRef(1), { geometry: { kind: 'line', from: { x: 0, y: 0 }, to: { x: 1, y: 1 } } }]),
      api('update', [objRef(2), { bounds: { x: 0, y: 0, width: 1, height: 1 } }]),
      api('update', [objRef(8), { props: { color: '#000000' } }]),
      api('update', [objRef(99), { contents: 'x' }]),
      api('update', [objRef(1), {}]),
      api('setRotation', [objRef(1), 45]),
      api('rotateBy', [objRef(10), 90]),
      api('rotateBy', [objRef(6), 10]),
      api('setRotation', [objRef(5), 90]),
      api('setRotation', [objRef(99), 90]),
      api('updateRaw', [objRef(1), { subtype: 'square', opacity: 0.5 }]),
      api('updateMany', [[objRef(1), objRef(99), objRef(2)], { contents: 'many' }]),
      api('deleteMany', [[objRef(3), objRef(4)]]),
      api('delete', [objRef(9)]),
      api('list', [{ page: pr(1) }]),
    ],
  },
  {
    name: 'selection-verbs',
    covers:
      'selectAll selects every selectable annotation, or one page’s; selectInRect selects what intersects the rect, and an additive empty hit leaves the selection; rotateSelectionBy(-90) rotates each member through setRotation; resetSelectionRotation awaits its writes; updateSelection, updateSelectionFlags and deleteSelection settle into a batch result that names each refused member once.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1, { rotation: 90 }), circle(2, { rect: { left: 300, bottom: 300, right: 400, top: 400 } }), markup('highlight', 3)], 2: [square(20, { pageObjectNumber: 2 })] }),
          { path: 'page.annotations.update', pon: 1, args: [objRef(2), { subtype: 'circle', color: { r: 0, g: 0, b: 0 } }], error: { name: 'EngineError', message: 'no', code: 'Refused' } },
          { path: 'page.annotations.update', pon: 1, value: updated(square(1)) },
          { path: 'page.annotations.update', pon: 2, value: updated(square(20, { pageObjectNumber: 2 })) },
          { path: 'page.annotations.delete', pon: 1, value: {} },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('selectAll'),
      api('getSelection'),
      api('selectAll', [pr(2)]),
      api('getSelection'),
      api('selectInRect', [pr(1), { x: 0, y: 0, width: 250, height: 250 }]),
      api('getSelection'),
      api('selectInRect', [pr(1), { x: 590, y: 790, width: 5, height: 5 }, { add: true }]),
      api('getSelection'),
      api('selectInRect', [pr(1), { x: 290, y: 390, width: 20, height: 20 }, { add: true }]),
      api('selectInRect', [pr(1), { x: 590, y: 790, width: 5, height: 5 }]),
      api('getSelection'),
      api('select', [[objRef(1), objRef(2)]]),
      api('rotateSelectionBy', [-90]),
      api('resetSelectionRotation'),
      api('updateSelection', [{ color: '#000000' }]),
      api('updateSelectionFlags', [{ print: false }]),
      api('select', [objRef(20, 2)]),
      api('rotateSelectionBy', [90]),
      api('select', [[objRef(1), objRef(3)]]),
      api('deleteSelection'),
      api('getSelection'),
    ],
  },
  {
    name: 'rich-text-editing',
    covers:
      'setContents and setRichText commit through the one debounced text write immediately; draftRichText stages a paragraph edit and schedules the write; setTextSelection stores the editor range (a repeat is a no-op, null clears) and the range keys of getSelectionProps then read the runs it covers; updateSelection restyles those runs as a delta (flushing first) while the other keys restyle the body; toggleTextFormat flips a format from the current value; endTextEdit flushes and drops the range; getEditingRef and getCssFontFamily read back.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [richFree(1), freeText(2, { rect: { left: 300, bottom: 300, right: 400, top: 400 } }), square(3, { rect: { left: 400, bottom: 400, right: 500, top: 500 } })] }),
          { path: 'page.annotations.update', pon: 1, value: updated(richFree(1)) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listenAll,
      api('setContents', [objRef(2), 'plain']),
      api('setRichText', [objRef(2), { paragraphs: [{ runs: [{ text: 'a' }, { text: 'b', style: { italic: true } }] }] }]),
      api('setContents', [objRef(99), 'x']),
      api('select', [objRef(1)]),
      api('beginTextEdit', [objRef(1)]),
      api('getEditingRef'),
      api('getEditingId'),
      api('setTextSelection', [objRef(1), { start: 1, end: 4 }]),
      api('setTextSelection', [objRef(1), { start: 1, end: 4 }]),
      api('getSelectionProps'),
      api('updateSelection', [{ fontSize: 18, fontColor: '#00ff00', bold: false, opacity: 0.5 }]),
      api('getSelectionProps'),
      api('toggleTextFormat', ['italic']),
      api('draftRichText', [objRef(1), { paragraphs: [{ runs: [{ text: 'Hello there' }] }] }]),
      advance(100),
      api('updateSelection', [{ fontSize: 9 }]),
      api('setTextSelection', [objRef(1), { start: 2, end: 2 }]),
      api('getSelectionProps'),
      api('setTextSelection', [objRef(1), null]),
      api('setTextSelection', [objRef(1), null]),
      api('draftRichText', [objRef(1), { paragraphs: [{ runs: [{ text: 'Typing' }] }] }]),
      api('updateSelection', [{ fontColor: '#111111' }]),
      api('setTextSelection', [objRef(1), { start: 0, end: 3 }]),
      api('endTextEdit'),
      api('getSelectionProps'),
      api('getEditingRef'),
      api('select', [[objRef(1), objRef(2)]]),
      api('toggleTextFormat', ['underline']),
      api('select', [objRef(3)]),
      api('toggleTextFormat', ['bold']),
      api('getCssFontFamily', ['Arial']),
      api('getCssFontFamily', ['Fancy']),
      api('draftContents', [objRef(1), 'draft']),
      advance(300),
    ],
  },
  {
    name: 'attachments-stamps-and-downloads',
    covers:
      'createAttachment places the attachment tool’s icon at a point with its defaults and the picked file, selects and announces it; a page the document lacks rejects not-found; an engine refusal rejects as a PluginError. placeStamp sniffs the bytes and places them exactly as a click does, rejecting junk and a page with no crop. readAttachment rejects on an engine whose page service has no downloadFile.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc(
          [
            hydrate({ 1: [fileAttachment(1)] }),
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(fileAttachment(30)) },
            { path: 'page.annotations.create', pon: 1, times: 1, error: { name: 'EngineError', message: 'too big', code: 'Refused' } },
            { path: 'page.annotations.create', pon: 1, times: 1, value: created(stamp(31)) },
          ],
          { lacks: ['page.annotations.downloadFile'] },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...listenAll,
      api('createAttachment', [pr(1), { x: 50, y: 60 }, { data: { $bytes: 'AAEC' }, name: 'a.bin', mimeType: 'application/octet-stream' }]),
      api('createAttachment', [pr(7), { x: 50, y: 60 }, { data: { $bytes: 'AAEC' }, name: 'a.bin' }]),
      api('createAttachment', [pr(1), { x: 50, y: 60 }, { data: { $bytes: 'AAEC' }, name: 'b.bin' }]),
      api('placeStamp', [
        { source: { $bytes: 'iVBORw0KGgoAAAANSUhEUgAAAGQAAAAy' }, name: 'Approved', subject: 'Stamp' },
        { page: pr(1), at: { x: 300, y: 400 }, rotation: 90 },
      ]),
      api('placeStamp', [{ source: { $bytes: 'AAEC' } }, { page: pr(1), at: { x: 300, y: 400 } }]),
      api('placeStamp', [{ source: { $bytes: 'iVBORw0KGgoAAAANSUhEUgAAAGQAAAAy' }, targetWidth: 40 }, { page: pr(7), at: { x: 1, y: 1 }, targetWidth: 20 }]),
      api('readAttachment', [objRef(1)]),
      api('getSelection'),
    ],
  },
  {
    name: 'measurement-distance-drafts',
    covers:
      'A dimension tool is inert on a page with no viewports; setPageViewports supplies them, and the scale comes from the viewport under the first point (the last containing one) or the fallback, a foreign scale refusing the draft. The distance tool drags its endpoints, then places the offset between clicks (distanceCreationPage names the page, the edit handler stands down and the hover projects onto it), and the next click commits a LineDimension draft with its caption and leader. The perimeter tool builds a PolyLineDimension. The calibrate tool captures the line in PDF space through onDraftCaptured instead of creating, and needs doc.annotate.modify.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(dimensionLine(40)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(polyline(41, { intent: 'PolyLineDimension', measure: METRES, caption: { enabled: true } })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('onDraftCaptured', [{ $fn: 'captured' }]),
      api('onDraftChanged', [{ $fn: 'draft' }]),
      tool('distance'),
      ix('dispatch', [{ phase: 'down', viewport: { x: 100, y: 100 }, page: { ref: pr(1), point: { x: 100, y: 100 }, scale: 1 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      api('hasCreationDraft'),
      api('setPageViewports', [pr(1), [{ bbox: { left: 0, bottom: 0, right: 300, top: 800 }, measure: METRES, owned: true }, { bbox: { left: 0, bottom: 0, right: 100, top: 100 }, measure: { subtype: 'GEO' }, owned: false }], FEET]),
      api('createPointer', ['distance', 'down', pr(1), { x: 50, y: 750 }]),
      api('hasCreationDraft'),
      ...[
        ['down', 100, 100],
        ['move', 150, 100],
        ['up', 244, 100],
      ].map(([phase, x, y]) =>
        ix('dispatch', [{ phase, viewport: { x, y }, page: { ref: pr(1), point: { x, y }, scale: 1 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      ),
      api('distanceCreationPage'),
      api('getCreationDraft'),
      ix('dispatch', [{ phase: 'move', viewport: { x: 170, y: 80 }, page: { ref: pr(1), point: { x: 170, y: 80 }, scale: 1 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      ix('dispatch', [{ phase: 'move', viewport: { x: 170, y: 80 }, page: null, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      ix('dispatch', [{ phase: 'down', viewport: { x: 170, y: 70 }, page: { ref: pr(1), point: { x: 170, y: 70 }, scale: 1 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      ix('dispatch', [{ phase: 'up', viewport: { x: 170, y: 70 }, page: { ref: pr(1), point: { x: 170, y: 70 }, scale: 1 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      api('distanceCreationPage'),
      api('list'),
      api('setPageViewports', [pr(1), undefined, METRES]),
      api('createPointer', ['perimeter', 'down', pr(1), { x: 400, y: 400 }]),
      api('setPageViewports', [pr(1), [], METRES]),
      api('createPointer', ['perimeter', 'down', pr(1), { x: 400, y: 400 }]),
      api('createPointer', ['perimeter', 'down', pr(1), { x: 500, y: 400 }]),
      api('createPointer', ['perimeter', 'down', pr(1), { x: 500, y: 500 }, true]),
      api('setPageViewports', [pr(1), [], { subtype: 'GEO' }]),
      api('createPointer', ['area', 'down', pr(1), { x: 400, y: 400 }]),
      api('hasCreationDraft'),
      api('createPointer', ['calibrate', 'down', pr(1), { x: 10, y: 10 }]),
      api('createPointer', ['calibrate', 'move', pr(1), { x: 60, y: 10 }]),
      api('createPointer', ['calibrate', 'up', pr(1), { x: 110, y: 10 }]),
      api('list'),
    ],
  },
  {
    name: 'calibration-needs-modify',
    covers: 'The calibrate capture tool is inert without doc.annotate.modify, even with create authority.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate()], { security: { allows: ['doc.annotate.read', 'doc.annotate.create'] } }) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('onDraftCaptured', [{ $fn: 'captured' }]),
      api('createPointer', ['calibrate', 'down', pr(1), { x: 10, y: 10 }]),
      api('createPointer', ['calibrate', 'up', pr(1), { x: 110, y: 10 }]),
      api('hasCreationDraft'),
      api('createPointer', ['square', 'down', pr(1), { x: 10, y: 10 }]),
      api('hasCreationDraft'),
    ],
  },
  {
    name: 'remeasure-page',
    covers:
      'remeasurePage rehydrates, then rewrites the scale of every dimension on the page it can, reporting each updated ref and each one skipped: no update authority, locked, a foreign measure, or a scale that yields no readout; a refused write lands in failed. A hydration that ends in error reports it instead.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate({
            1: [
              dimensionLine(1),
              dimensionLine(2, { flags: FLAGS({ locked: true }) }),
              dimensionLine(3, { measure: { subtype: 'GEO' } }),
              polygon(4, { intent: 'PolygonDimension', measure: METRES, caption: { enabled: true } }),
              polyline(5, { intent: 'PolyLineDimension', measure: METRES, caption: { enabled: true }, vertices: [{ x: 1, y: 1 }] }),
              dimensionLine(6),
              square(7),
            ],
            2: [dimensionLine(20, { pageObjectNumber: 2 })],
          }),
          { path: 'page.annotations.update', pon: 1, args: [objRef(6), { subtype: 'line', measure: FEET }], error: { name: 'EngineError', message: 'stale', code: 'Conflict' } },
          { path: 'page.annotations.update', pon: 1, value: updated(dimensionLine(1, { measure: FEET })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [...boot, api('remeasurePage', [pr(1), FEET]), api('remeasurePage', [pr(2), { ...FEET, distance: [] }])],
  },
  {
    name: 'remeasure-without-authority-and-after-errors',
    covers:
      'A dimension this session may not update is skipped as no-authority; a remeasure whose rehydration fails reports the error and changes nothing.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc(
          [
            { path: 'annotations.listRawAll', times: 1, value: { pages: [{ pageState: { page: pr(1) }, annotations: [dimensionLine(1)] }] } },
            { path: 'annotations.listRawAll', times: 1, value: { pages: [{ pageState: { page: pr(1) }, annotations: [dimensionLine(1)] }] } },
            { path: 'annotations.listRawAll', error: { name: 'EngineError', message: 'offline', code: 'Unavailable' } },
          ],
          { security: { allows: '*', allowsAnnotationMutation: { update: false } } },
        ),
      },
    },
    snapshot: SNAP,
    steps: [...boot, api('remeasurePage', [pr(1), FEET]), api('remeasurePage', [pr(1), FEET])],
  },
  {
    name: 'v3-document-events',
    covers:
      'A form field update and a completed signature bump the widgets they repaint (a widget without an annotation address is skipped, a signature without a widget bumps nothing); document.versioned rehydrates; a failed page reload keeps the previous view.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          { path: 'annotations.listRawAll', times: 1, value: { pages: [{ pageState: { page: pr(1) }, annotations: [widget(1, 'text'), widget(2, 'signature')] }] } },
          { path: 'annotations.listRawAll', value: { pages: [{ pageState: { page: pr(1) }, annotations: [widget(1, 'text')] }] } },
          { path: 'page.annotations.list', pon: 1, error: { name: 'EngineError', message: 'busy', code: 'Unavailable' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('onResynced', [{ $fn: 'resynced' }]),
      emit({ type: 'form.fieldUpdated', field: { widgets: [formWidget(1), formWidget(0)] } }),
      emit({ type: 'signature.completed', signature: { widget: formWidget(2) } }),
      emit({ type: 'signature.completed', signature: { widget: null } }),
      emit({ type: 'form.valueChanged', changedWidgets: [formWidget(1, null)] }),
      emit({ type: 'document.versioned' }),
      api('reloadPage', [pr(1)]),
      api('listRaw'),
    ],
  },
  {
    name: 'finish-drafts-and-the-edit-hub',
    covers:
      'finishCreationDraft commits a polygon draft and resolves its ref, resolves null with no draft, and finishes a buffered ink draft through its own door; cancel drops a draft.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(polygon(30)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(ink(31)) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('finishCreationDraft'),
      api('createPointer', ['polygon', 'down', pr(1), { x: 10, y: 10 }]),
      api('createPointer', ['polygon', 'down', pr(1), { x: 100, y: 10 }]),
      api('createPointer', ['polygon', 'down', pr(1), { x: 100, y: 100 }]),
      api('finishCreationDraft'),
      api('createPointer', ['ink', 'down', pr(1), { x: 10, y: 10 }]),
      api('createPointer', ['ink', 'move', pr(1), { x: 20, y: 20 }]),
      api('createPointer', ['ink', 'up', pr(1), { x: 30, y: 10 }]),
      api('hasCreationDraft'),
      api('finishCreationDraft'),
      api('createPointer', ['polyline', 'down', pr(1), { x: 10, y: 10 }]),
      api('cancel'),
      api('hasCreationDraft'),
    ],
  },
  {
    name: 'awaited-writes-and-pending-text',
    covers:
      'A quarter-turn whose write the engine refuses rejects rotateSelectionBy with the lowered error; a debounced text write whose annotation disappeared before the flush is dropped; the file-picker remover clears only the provider it installed; createAttachment refuses when the attachment tool no longer authors an icon kind.',
    plugins: [{ factory: 'interaction' }, { factory: 'annotation', config: { tools: [{ id: 'attachment', subtype: 'custom-pin' }] } }],
    engine: {
      documents: {
        a: doc([
          hydrate({ 1: [square(1), freeText(2, { rect: { left: 300, bottom: 300, right: 400, top: 400 } })] }),
          { path: 'page.annotations.update', pon: 1, error: { name: 'EngineError', message: 'locked', code: 'Refused' } },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('select', [objRef(1)]),
      api('rotateSelectionBy', [90]),
      api('resetSelectionRotation'),
      api('draftContents', [objRef(2), 'typing']),
      emit({ type: 'annotation.deleted', page: pr(1), deleted: { kind: 'objectNumber', value: 2 }, origin: { kind: 'remote', sessionId: 's2', sub: 'u-9', serverId: 5 } }),
      api('endTextEdit'),
      advance(300),
      api('setFilePickerProvider', [{ $fn: 'first', resolves: null }]),
      api('setFilePickerProvider', [{ $fn: 'second', resolves: null }]),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 } },
      api('createAttachment', [pr(1), { x: 50, y: 60 }, { data: { $bytes: 'AAEC' }, name: 'a.bin' }]),
    ],
  },
  {
    name: 'stamp-preview-providers',
    covers:
      'A resolution-aware preview provider renders once per size bucket for the arm (powers of two from 128 px, capped at 4096); a provider that rejects is logged and reads null, and the null is cached for that bucket too.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate()]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      api('armStamp', [
        {
          source: { $bytes: 'JVBERi0x' },
          preview: { $fn: 'render', seq: [{ resolves: { bytes: { $bytes: 'AAEC' }, mimeType: 'image/png' } }, { rejects: { name: 'Error', message: 'raster failed' } }] },
        },
      ]),
      api('getArmedStampPreview', [200]),
      api('getArmedStampPreview', [250]),
      api('getArmedStampPreview', [9000]),
      api('getArmedStampPreview', [5000]),
      api('getArmedStampPreview'),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-api.json',
    describes:
      'packages/plugin/annotation: the v3 page-space reads and writes, the change events, the selection and rich-text verbs, attachments and stamps without a pointer, the measurement seam and the v3 document events.',
    cases,
  },
];
