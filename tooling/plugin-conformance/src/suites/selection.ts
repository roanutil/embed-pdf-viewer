/**
 * `packages/plugin/selection`. Replayed by
 * `crates/plugins/selection/tests/conformance.rs`.
 *
 * Scenarios drive the plugin through its capability and through the
 * interaction hub (its pointer handler), with page geometry and text answered
 * from the fake engine's table in the shapes `engine-core` defines
 * (`PageGeometrySnapshot`, `PageTextSnapshot`). Function suites pin the
 * coordinate seam, the engine-core text layout the plugin is built on (the
 * Rust crate carries its own port of it, since it links no engine), the
 * reducer and the handle policy.
 */
import {
  buildPageTextLayout,
  expandTextRangeToLine,
  expandTextRangeToWord,
  sliceTextByChars,
  textGlyphAt,
  textGlyphQuad,
  textSegmentsForRange,
  type PageGeometrySnapshot,
  type PageTextSnapshot,
  type PageRef,
  type PdfQuad,
  type PdfRect,
} from '../../../../packages/engine/core/src/runtime';
import {
  HANDLE_BAR,
  HANDLE_HEAD,
  HANDLE_PAD,
  createSelectionHandleDrag,
  selectionHandleGeom,
  selectionPlugin,
} from '../../../../packages/plugin/selection/src/index';
import type { SelectionHandleEndpoint, SelectionHandleView } from '../../../../packages/plugin/selection/src/index';
import {
  SelectionToken,
  buildSelectionPageGeometry,
  contentPointToPdf,
  initialSelectionState,
  reduceSelection,
  toContentSegment,
  toContentTextQuad,
} from '../../../../packages/plugin/selection/src/internal';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

registerPlugin('selection', { factory: () => selectionPlugin(), tokens: [SelectionToken] });

// ── page geometry fixtures ───────────────────────────────────────────────────

const W = 6;
const H = 12;
const ZERO = { left: 0, bottom: 0, right: 0, top: 0 };

type Glyph = { looseBox: PdfRect; flags: number; tightBox?: PdfRect };

/** Upright glyphs for `text` from `x`, one 6-wide cell per character;
 *  spaces carry the space flag and no tight box. */
const glyphs = (text: string, x: number, top: number): Glyph[] =>
  [...text].map((ch, i) => {
    const left = x + i * W;
    const looseBox = { left, bottom: top - H, right: left + W, top };
    if (ch === ' ') return { looseBox, flags: 1 };
    return { looseBox, flags: 0, tightBox: { left: left + 0.5, bottom: top - H + 1, right: left + W - 0.5, top: top - 1 } };
  });

/** A generated line break: PDFium's `\r\n`, two empty characters. */
const newline: Glyph[] = [
  { looseBox: ZERO, flags: 2 },
  { looseBox: ZERO, flags: 2 },
];

const run = (charStart: number, gs: Glyph[], fontSize = 12) => {
  const real = gs.filter((g) => !(g.flags & 2));
  return {
    rect: {
      left: Math.min(...real.map((g) => g.looseBox.left)),
      bottom: Math.min(...real.map((g) => g.looseBox.bottom)),
      right: Math.max(...real.map((g) => g.looseBox.right)),
      top: Math.max(...real.map((g) => g.looseBox.top)),
    },
    charStart,
    glyphs: gs,
    fontSize,
  };
};

/** Page 1: "Hello world\r\nSecond line", two lines. */
const geo1: PageGeometrySnapshot = {
  runs: [run(0, [...glyphs('Hello world', 50, 750), ...newline]), run(13, glyphs('Second line', 50, 730))],
};
const text1: PageTextSnapshot = { text: 'Hello world\r\nSecond line', charCount: 24 };

/** Page 5: one line, on a page whose crop box is offset. */
const geo5: PageGeometrySnapshot = { runs: [run(0, glyphs('Page two text', 40, 700))] };
const text5: PageTextSnapshot = { text: 'Page two text', charCount: 13 };

/** Page 9: "A😀B": three characters, four UTF-16 units, so a char map. */
const geo9: PageGeometrySnapshot = { runs: [run(0, glyphs('AxB', 100, 600))] };
const text9: PageTextSnapshot = { text: 'A😀B', charCount: 3, charMap: [[2, 3]] };

/** Page 13: a left-to-right line over a right-to-left one. */
const geo13: PageGeometrySnapshot = {
  runs: [run(0, glyphs('abc', 50, 750)), run(3, glyphs('fed', 50, 730).reverse())],
};

const pr = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const box = { left: 0, bottom: 0, right: 600, top: 800 };
const offset = { left: 10, bottom: 20, right: 610, top: 820 };
const layout = (index: number, pon: number, over: Record<string, unknown> = {}) => ({
  index,
  ref: pr(pon),
  label: null,
  size: { width: 600, height: 800 },
  rotation: 0,
  userUnit: 1,
  boxes: { media: box, crop: box },
  ...over,
});
const pages = [layout(0, 1), layout(1, 5, { boxes: { media: offset, crop: offset }, rotation: 90 }), layout(2, 9)];

const both = ['doc.text.select', 'doc.text.copy'];
const geometryReads: ResponseSpec[] = [
  { path: 'page.geometry.read', pon: 1, value: geo1 },
  { path: 'page.geometry.read', pon: 5, value: geo5 },
  { path: 'page.geometry.read', pon: 9, value: geo9 },
];
const textReads: ResponseSpec[] = [
  { path: 'page.text.read', pon: 1, value: text1 },
  { path: 'page.text.read', pon: 5, value: text5 },
  { path: 'page.text.read', pon: 9, value: text9 },
];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages,
  security: { allows: both },
  responses: [...geometryReads, ...textReads],
  ...over,
});

// ── steps ────────────────────────────────────────────────────────────────────

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const openDoc = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
/** v2 selection method → v3 name. Page-first verbs take a PageRef, and a
 *  range addresses pages by `page` rather than `pon`. */
const RENAMED: Readonly<Record<string, string>> = {
  beginAt: 'beginGestureAt',
  end: 'endGesture',
  ensurePage: 'ensureLoaded',
  isSelecting: 'isGestureActive',
  menuAnchor: 'getAnchor',
  onChange: 'onChanged',
  onCommit: 'onCommitted',
  rectsForPage: 'listRects',
  segmentsForPage: 'listSegments',
  selectedPages: 'listSelectedPages',
  snapshot: 'getSnapshot',
  highlightVisible: 'isHighlightVisible',
};
const PAGE_FIRST = new Set(['beginAt', 'ensurePage', 'isLoaded', 'isOverText', 'rectsForPage', 'segmentsForPage', 'selectWordAt', 'selectLineAt', 'extendTo']);
/** `{pon}` → `{page}` inside a v2 range. */
const rangeOf = (range: unknown): unknown => {
  if (!range || typeof range !== 'object') return range;
  const r = range as Record<string, unknown>;
  if (typeof r.pon === 'number') {
    const { pon, ...rest } = r;
    return { page: pr(pon as number), ...rest };
  }
  if (r.start && typeof r.start === 'object') return { ...r, start: rangeOf(r.start), end: rangeOf(r.end) };
  return range;
};
const selCall = (doc: string, method: string, args: readonly unknown[]): Step => {
  let out = [...args];
  if (PAGE_FIRST.has(method) && typeof out[0] === 'number') out = [pr(out[0] as number), ...out.slice(1)];
  if (method === 'select') out = [rangeOf(out[0]), ...out.slice(1)];
  return { op: 'call', target: { token: 'selection', doc }, method: RENAMED[method] ?? method, args: out };
};
const sel = (method: string, ...args: unknown[]): Step => selCall('a', method, args);
const HUB_RENAMED: Readonly<Record<string, string>> = { dispatch: 'dispatchPointer', cursor: 'getCursor' };
const hub = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'interaction', doc: 'a' },
  method: HUB_RENAMED[method] ?? method,
  args,
});
const ref = (n: number): Step => ({ op: 'call', target: { ref: n } });
const emit = (event: unknown): Step => ({ op: 'emit', doc: 'a', event });
const settle = (label: string, value?: unknown): Step => ({ op: 'settle', label, value });
const fail = (label: string): Step => ({ op: 'settle', label, error: { name: 'EngineError', message: 'aborted read', code: 'Aborted' } });

const LOCAL = { kind: 'local', sessionId: 's1', sub: null };
const noMods = { shift: false, alt: false, ctrl: false, meta: false };
/** A pointer sample over page `pon` at content point (x, y); viewport = the
 *  point, so the drag threshold reads in the same units. */
const at = (phase: string, pon: number, x: number, y: number, over: Record<string, unknown> = {}) => ({
  phase,
  viewport: { x, y },
  page: { ref: pr(pon), point: { x, y } },
  modifiers: noMods,
  ...over,
});
const offPage = (phase: string, x: number, y: number) => ({ phase, viewport: { x, y }, modifiers: noMods });
const ptr = (method: string, pon: number, x: number, y: number, over: Record<string, unknown> = {}): Step =>
  hub('dispatch', at(method, pon, x, y, over));

// Content-space points on page 1 (crop top 800): "Hello" is y 50..62, the
// second line y 70..82; glyph i of line 1 spans x 50+6i..56+6i.
const H1 = { x: 53, y: 56 }; // 'H'
const O1 = { x: 77, y: 56 }; // 'o' of Hello (glyph 4)
const W1 = { x: 89, y: 56 }; // 'w' (glyph 6)
const L2 = { x: 83, y: 76 }; // 'd' of "Second" (glyph 13 + 5)
const BLANK = { x: 400, y: 400 };

const plugins = [{ factory: 'interaction' }, { factory: 'selection' }];
const withFeedback = [{ factory: 'feedback', config: { provider: true } }, ...plugins];
const opened = [start, openDoc()];
const slice = ['selection::a'];
const slices = ['selection::a', 'interaction::a'];
const loadAll = [sel('ensurePage', 1), sel('ensurePage', 5), sel('ensurePage', 9)];

/** A geometry read that waits for `settle(label)`. */
const deferGeo = (pon: number, label: string): ResponseSpec => ({ path: 'page.geometry.read', pon, defer: label, times: 1 });
const deferText = (pon: number, label: string): ResponseSpec => ({ path: 'page.text.read', pon, defer: label, times: 1 });

const cases: ScenarioCase[] = [
  {
    name: 'requires-interaction',
    covers: 'Selection requires the interaction hub; without it the kernel refuses the plugin list with the token hint.',
    plugins: [{ factory: 'selection' }],
    engine: { documents: { a: doc() } },
    steps: [start],
  },
  {
    name: 'capability-and-initial-state',
    covers:
      'The capability object in literal member order (the public and host lenses are one object), the initial slice, and the handler registered with the hub at init.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      ...opened,
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'selection' }, 'a'] },
      sel('hasSelection'),
      sel('isSelecting'),
      sel('snapshot'),
      sel('menuAnchor'),
      sel('selectedPages'),
      sel('segmentsForPage', 1),
      sel('rectsForPage', 1),
      sel('highlightVisible'),
      sel('isLoaded', 1),
      hub('wouldClaimTouch', at('down', 1, H1.x, H1.y, { pointerType: 'touch' })),
    ],
  },
  {
    name: 'permission-twins',
    covers: 'canSelect is doc.text.select and canCopy doc.text.copy; neither implies the other.',
    plugins,
    engine: {
      documents: {
        a: doc({ security: { allows: ['doc.text.select'] } }),
        b: doc({ security: { allows: ['doc.text.copy'] } }),
        c: doc({ security: { allows: '*' } }),
        d: doc({ security: { allows: [] } }),
      },
    },
    snapshot: 'none',
    steps: [
      start,
      openDoc('a'),
      openDoc('b'),
      openDoc('c'),
      openDoc('d'),
      ...['a', 'b', 'c', 'd'].flatMap((d): Step[] => [
        { op: 'call', target: { token: 'selection', doc: d }, method: 'canSelect' },
        { op: 'call', target: { token: 'selection', doc: d }, method: 'canCopy' },
      ]),
    ],
  },
  {
    name: 'hover-warms-and-claims-the-ibeam',
    covers:
      'The first hover over a page warms its geometry (one read, deduplicated while in flight); once loaded, hover over text claims the I-beam at priority 10, off-text and over a gap release it.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(1, 'geo-1'), ...geometryReads, ...textReads] }) } },
    snapshot: slices,
    steps: [
      ...opened,
      ptr('move', 1, H1.x, H1.y),
      ptr('move', 1, H1.x, H1.y),
      sel('isLoaded', 1),
      sel('isOverText', 1, H1),
      settle('geo-1', geo1),
      sel('isLoaded', 1),
      ptr('move', 1, H1.x, H1.y),
      ptr('move', 1, BLANK.x, BLANK.y),
      ptr('move', 1, H1.x, H1.y),
      hub('dispatch', offPage('move', 0, 0)),
      hub('cursor'),
    ],
  },
  {
    name: 'ensure-page-guards',
    covers:
      'ensurePage issues nothing for a loaded or in-flight page, a page not in the registry, or without doc.text.select; a failed read clears in-flight so a later call retries; a second PAGE_LOADED for the same page is not a change.',
    plugins,
    engine: {
      documents: {
        a: doc({ responses: [deferGeo(5, 'geo-5'), { path: 'page.geometry.read', pon: 9, error: { name: 'EngineError', message: 'gone', code: 'DocClosed' }, times: 1 }, ...geometryReads] }),
        b: doc({ security: { allows: ['doc.text.copy'] } }),
      },
    },
    snapshot: slice,
    steps: [
      start,
      openDoc('a'),
      openDoc('b'),
      sel('ensurePage', 1),
      sel('ensurePage', 1),
      sel('ensurePage', 5),
      sel('ensurePage', 5),
      sel('ensurePage', 42),
      sel('ensurePage', 9),
      sel('ensurePage', 9),
      settle('geo-5', geo5),
      { op: 'call', target: { token: 'selection', doc: 'b' }, method: 'ensurePage', args: [1] },
      { op: 'call', target: { token: 'selection', doc: 'b' }, method: 'isOverText', args: [1, H1] },
    ],
  },
  {
    name: 'click-then-drag',
    covers:
      'A single-click down on text clears and captures but selects nothing; moves under the 4px threshold stay a click; past it the gesture opens at the anchor (selecting, change) and extends to the pointer; the up settles and commits. The snapshot, pages, segments, rects, menu anchor and text follow.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      sel('onChange', { $fn: 'change' }),
      sel('onCommit', { $fn: 'commit' }),
      ptr('down', 1, H1.x, H1.y),
      ptr('move', 1, H1.x + 2, H1.y + 2),
      sel('hasSelection'),
      ptr('move', 1, W1.x, W1.y),
      sel('isSelecting'),
      ptr('move', 1, BLANK.x, BLANK.y),
      ptr('move', 1, L2.x, L2.y),
      hub('dispatch', offPage('move', 1, 1)),
      ptr('up', 1, L2.x, L2.y),
      sel('isSelecting'),
      sel('snapshot'),
      sel('selectedPages'),
      sel('segmentsForPage', 1),
      sel('rectsForPage', 1),
      sel('segmentsForPage', 5),
      sel('menuAnchor'),
      sel('readText'),
    ],
  },
  {
    name: 'down-off-text-and-over-gaps',
    covers:
      'A down on blank page space clears the selection and declines, so a lower handler gets the gesture; a down over a gap declines without clearing; a move without an anchor does nothing.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      sel('select', { pon: 1, start: 0, count: 5 }),
      hub('registerHandler', {
        id: 'below',
        priority: 1,
        enabledFor: { $fn: 'below.enabledFor', returns: true },
        onDown: { $fn: 'below.onDown', returns: false },
      }),
      hub('dispatch', offPage('down', 5, 5)),
      sel('hasSelection'),
      ptr('down', 1, BLANK.x, BLANK.y),
      sel('hasSelection'),
      ptr('move', 1, H1.x, H1.y),
      ptr('up', 1, H1.x, H1.y),
    ],
  },
  {
    name: 'word-and-line-clicks',
    covers:
      'A double-click selects the word (capturing even off-text), a triple-click the visual line; a touch long-press that engages a word ticks the feedback provider, one on blank space or a mouse double-click does not.',
    plugins: withFeedback,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      ptr('down', 1, W1.x, W1.y, { clickCount: 2 }),
      ptr('up', 1, W1.x, W1.y, { clickCount: 2 }),
      sel('readText'),
      ptr('down', 1, O1.x, O1.y, { clickCount: 2, pointerType: 'touch', gesture: 'long-press' }),
      ptr('up', 1, O1.x, O1.y),
      ptr('down', 1, BLANK.x, BLANK.y, { clickCount: 2, pointerType: 'touch', gesture: 'long-press' }),
      ptr('up', 1, BLANK.x, BLANK.y),
      ptr('down', 1, L2.x, L2.y, { clickCount: 3 }),
      ptr('up', 1, L2.x, L2.y),
      sel('readText'),
      ptr('down', 1, BLANK.x, BLANK.y, { clickCount: 4 }),
      ptr('up', 1, BLANK.x, BLANK.y),
      sel('hasSelection'),
    ],
  },
  {
    name: 'word-click-before-geometry',
    covers: 'A double-click on a page whose geometry has not loaded selects nothing and no feedback fires.',
    plugins: withFeedback,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ptr('down', 1, W1.x, W1.y, { clickCount: 2, gesture: 'long-press' }),
      ptr('up', 1, W1.x, W1.y),
      sel('selectWordAt', 1, W1),
      sel('selectLineAt', 1, W1),
      sel('beginAt', 1, W1),
      sel('beginAt', 1, BLANK),
      sel('ensurePage', 1),
      sel('beginAt', 1, BLANK),
      sel('selectLineAt', 1, BLANK),
    ],
  },
  {
    name: 'cancel-drops-the-drag',
    covers: 'A cancelled drag clears the in-flight selection and settles the gesture (commit fires with nothing selected).',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      sel('onCommit', { $fn: 'commit' }),
      ptr('down', 1, H1.x, H1.y),
      ptr('move', 1, W1.x, W1.y),
      ptr('cancel', 1, W1.x, W1.y),
      sel('snapshot'),
    ],
  },
  {
    name: 'cross-page-drag',
    covers:
      'A drag onto a page that has not loaded warms it and keeps the last focus; when its geometry arrives the selection recomputes across the pages. A backward drag orders the ends by document position.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(5, 'geo-5'), ...geometryReads, ...textReads] }) } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('ensurePage', 1),
      sel('ensurePage', 9),
      ptr('down', 1, O1.x, O1.y),
      ptr('move', 1, W1.x, W1.y),
      ptr('move', 5, 70, 130),
      settle('geo-5', geo5),
      ptr('move', 5, 70, 130),
      ptr('move', 9, 107, 206),
      ptr('up', 9, 107, 206),
      sel('snapshot'),
      sel('readText'),
      ptr('down', 9, 101, 206),
      ptr('move', 9, 110, 206),
      ptr('move', 1, W1.x, W1.y),
      ptr('up', 1, W1.x, W1.y),
      sel('snapshot'),
      sel('menuAnchor'),
      sel('readText'),
    ],
  },
  {
    name: 'programmatic-select',
    covers:
      'select() takes a cross-page range or a single-page span, clamps a negative start, clears on an empty range, maps an end at index 0 to the previous page, refuses unknown pages, and is born settled. selectAll selects every character and settles its open end as geometry arrives.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(9, 'geo-9'), ...geometryReads, ...textReads] }) } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('ensurePage', 1),
      sel('select', { pon: 1, start: 6, count: 5 }),
      sel('readText'),
      sel('isSelecting'),
      sel('select', { start: { pon: 1, index: -3 }, end: { pon: 1, index: 2 } }),
      sel('snapshot'),
      sel('select', { pon: 1, start: 4, count: 0 }),
      sel('select', { start: { pon: 5, index: 3 }, end: { pon: 1, index: 9 } }),
      sel('select', { start: { pon: 1, index: 20 }, end: { pon: 5, index: 0 } }),
      sel('snapshot'),
      sel('select', { start: { pon: 1, index: 0 }, end: { pon: 1, index: 0 } }),
      sel('select', { start: { pon: 5, index: 0 }, end: { pon: 5, index: 0 } }),
      sel('select', { pon: 1, start: 0, count: 3 }),
      sel('select', { start: { pon: 1, index: -1 }, end: { pon: 1, index: 0 } }),
      sel('select', { start: { pon: 1, index: 0 }, end: { pon: 5, index: 0 } }),
      sel('select', { start: { pon: 5, index: 0 }, end: { pon: 9, index: 0 } }),
      sel('select', { pon: 77, start: 0, count: 1 }),
      sel('select', { start: { pon: 76, index: 0 }, end: { pon: 1, index: 1 } }),
      sel('select', { start: { pon: 1, index: 0 }, end: { pon: 78, index: 1 } }),
      sel('selectAll'),
      sel('snapshot'),
      settle('geo-9', geo9),
      sel('snapshot'),
      sel('readText'),
      sel('clear'),
      sel('clear'),
    ],
  },
  {
    name: 'select-refused',
    covers:
      'Without doc.text.select, select and selectAll throw PermissionDenied and the gesture functions are inert: no warming, no hit tests, the handler declines. clear is always allowed.',
    plugins,
    engine: { documents: { a: doc({ security: { allows: ['doc.text.copy'] } }) } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('select', { pon: 1, start: 0, count: 1 }),
      sel('selectAll'),
      sel('clear'),
      ptr('move', 1, H1.x, H1.y),
      ptr('down', 1, H1.x, H1.y),
      sel('readText'),
    ],
  },
  {
    name: 'select-all-empty-document',
    covers: 'selectAll on a document with no pages does nothing.',
    plugins,
    engine: { documents: { a: doc({ pages: [] }) } },
    snapshot: slice,
    steps: [...opened, sel('selectAll'), sel('menuAnchor')],
  },
  {
    name: 'read-text',
    covers:
      'readText resolves "" with nothing selected, rejects PermissionDenied without doc.text.copy, joins pages with a newline, slices through the char map, reads each page once (shared while in flight), and evicts a failed read so the next call retries.',
    plugins,
    engine: {
      documents: {
        a: doc({ responses: [...geometryReads, deferText(1, 'text-1'), { path: 'page.text.read', pon: 9, error: { name: 'EngineError', message: 'gone', code: 'DocClosed' }, times: 1 }, ...textReads] }),
        b: doc({ security: { allows: ['doc.text.select'] } }),
      },
    },
    snapshot: 'none',
    steps: [
      start,
      openDoc('a'),
      openDoc('b'),
      sel('readText'),
      sel('select', { start: { pon: 1, index: 6 }, end: { pon: 9, index: 2 } }),
      sel('readText'),
      sel('readText'),
      settle('text-1', text1),
      sel('readText'),
      sel('select', { pon: 9, start: 1, count: 1 }),
      sel('readText'),
      selCall('b', 'select', [{ pon: 1, start: 0, count: 2 }]),
      { op: 'call', target: { token: 'selection', doc: 'b' }, method: 'readText' },
    ],
  },
  {
    name: 'read-text-batches',
    covers: 'readText keeps at most eight page reads in flight: the ninth page is read after the first batch lands.',
    plugins,
    engine: {
      documents: {
        a: {
          pages: Array.from({ length: 10 }, (_, i) => layout(i, 100 + i)),
          security: { allows: both },
          responses: [
            deferText(100, 'first'),
            { path: 'page.geometry.read', value: { runs: [run(0, glyphs('Pn', 50, 750))] } },
            { path: 'page.text.read', value: { text: 'Pn', charCount: 2 } },
          ],
        },
      },
    },
    snapshot: 'none',
    steps: [...opened, sel('selectAll'), sel('readText'), settle('first', { text: 'P0', charCount: 2 })],
  },
  {
    name: 'endpoints',
    covers:
      'A snapshot endpoint sits on its boundary glyph cell, or on the segment when that glyph is degenerate (a generated line break); an endpoint on a page with no segments yet is null, and the menu anchor falls back to the last page with segments.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(5, 'geo-5'), ...geometryReads, ...textReads] }) } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('ensurePage', 1),
      sel('select', { pon: 1, start: 11, count: 4 }),
      sel('snapshot'),
      sel('select', { start: { pon: 1, index: 0 }, end: { pon: 5, index: 3 } }),
      sel('snapshot'),
      sel('menuAnchor'),
      sel('select', { start: { pon: 5, index: 0 }, end: { pon: 5, index: 3 } }),
      sel('snapshot'),
      sel('menuAnchor'),
      settle('geo-5', geo5),
      sel('menuAnchor'),
    ],
  },
  {
    name: 'endpoints-follow-their-segment',
    covers: 'The start endpoint takes the first segment\'s reading direction and the end the last\'s: a selection from a left-to-right line into a right-to-left one.',
    plugins,
    engine: {
      documents: {
        a: doc({ pages: [...pages, layout(3, 13)], responses: [...geometryReads, { path: 'page.geometry.read', pon: 13, value: geo13 }, ...textReads] }),
      },
    },
    snapshot: slice,
    steps: [...opened, sel('ensurePage', 13), sel('select', { pon: 13, start: 1, count: 4 }), sel('snapshot')],
  },
  {
    name: 'backward-drag-on-one-page',
    covers: 'A drag that ends before its anchor on the same page orders the ends backward.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [...opened, ...loadAll, ptr('down', 1, W1.x, W1.y), ptr('move', 1, H1.x, H1.y), ptr('up', 1, H1.x, H1.y), sel('snapshot'), sel('readText')],
  },
  {
    name: 'drag-threshold',
    covers: 'The drag threshold is 4 viewport px, measured from the down: 3.9 is still a click, exactly 4 begins the drag.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      hub('dispatch', { ...at('down', 1, H1.x, H1.y), viewport: { x: 1, y: 0 } }),
      hub('dispatch', { ...at('move', 1, W1.x, W1.y), viewport: { x: 4.9, y: 0 } }),
      sel('hasSelection'),
      hub('dispatch', { ...at('move', 1, W1.x, W1.y), viewport: { x: 5, y: 0 } }),
      sel('hasSelection'),
    ],
  },
  {
    name: 'handler-priority',
    covers: 'The text handler sits at priority 60: a capturing handler at 59 only gets the downs it declines.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      hub('registerHandler', {
        id: 'below',
        priority: 59,
        enabledFor: { $fn: 'below.enabledFor', returns: true },
        onDown: { $fn: 'below.onDown', returns: true },
        onUp: { $fn: 'below.onUp' },
      }),
      ptr('down', 1, H1.x, H1.y),
      ptr('up', 1, H1.x, H1.y),
      ptr('down', 1, BLANK.x, BLANK.y),
      ptr('up', 1, BLANK.x, BLANK.y),
    ],
  },
  {
    name: 'registry-changes',
    covers:
      'A page mutation (revision bump) recomputes the selection from cached geometry with no refetch (a rotation re-derives the transform), drops the caches of pages that left, and clears when an endpoint page is gone; events that change no structure are ignored.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      ...loadAll,
      sel('select', { start: { pon: 1, index: 13 }, end: { pon: 5, index: 4 } }),
      sel('readText'),
      emit({ type: 'annotations.created', page: pr(1), origin: LOCAL }),
      emit({
        type: 'pages.rotated',
        layout: { pageCount: 3, pages: [layout(0, 1, { rotation: 90 }), pages[1], pages[2]] },
      }),
      sel('snapshot'),
      emit({ type: 'pages.moved', layout: { pageCount: 3, pages: [pages[1], layout(1, 1), pages[2]] } }),
      sel('snapshot'),
      emit({ type: 'pages.deleted', layout: { pageCount: 2, pages: [layout(0, 1), layout(1, 9)] } }),
      sel('hasSelection'),
      sel('select', { pon: 9, start: 0, count: 2 }),
      emit({ type: 'pages.inserted', layout: { pageCount: 3, pages: [layout(0, 1), layout(1, 5), layout(2, 9)] } }),
      sel('snapshot'),
      sel('ensurePage', 5),
    ],
  },
  {
    name: 'content-invalidation',
    covers:
      'redaction.applied and pages.flattened drop every cache and clear the selection; a geometry read in flight from before is discarded when it lands, and the next warm refetches.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(5, 'geo-5-old'), ...geometryReads, ...textReads] }) } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('ensurePage', 1),
      sel('ensurePage', 5),
      sel('select', { pon: 1, start: 0, count: 5 }),
      sel('readText'),
      emit({ type: 'redaction.applied', page: pr(1), origin: LOCAL }),
      sel('hasSelection'),
      sel('isOverText', 1, H1),
      settle('geo-5-old', geo5),
      sel('isLoaded', 5),
      sel('ensurePage', 5),
      sel('ensurePage', 1),
      sel('select', { pon: 1, start: 0, count: 5 }),
      sel('readText'),
      emit({ type: 'pages.flattened' }),
      sel('snapshot'),
      emit({ type: 'redaction.applied' }),
    ],
  },
  {
    name: 'late-geometry-without-selection',
    covers: 'A page that loads while nothing is selected only marks itself loaded; a failed read is ignored.',
    plugins,
    engine: { documents: { a: doc({ responses: [deferGeo(1, 'geo-1'), deferGeo(5, 'geo-5'), ...geometryReads] }) } },
    snapshot: slice,
    steps: [...opened, sel('ensurePage', 1), sel('ensurePage', 5), settle('geo-1', geo1), fail('geo-5'), sel('isLoaded', 5)],
  },
  {
    name: 'gesture-verbs',
    covers:
      'The host verbs directly: beginAt opens a caret selection, extendTo moves the focus (keeping it off-text, ignoring a no-selection call), end settles and commits; highlight visibility round-trips; change and commit unsubscribes answer whether they were subscribed.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slice,
    steps: [
      ...opened,
      sel('extendTo', 1, W1),
      ...loadAll,
      sel('extendTo', 1, W1),
      sel('onChange', { $fn: 'change' }),
      sel('onCommit', { $fn: 'commit' }),
      sel('beginAt', 1, H1),
      sel('extendTo', 1, W1),
      sel('extendTo', 1, BLANK),
      sel('end'),
      sel('snapshot'),
      ref(0),
      ref(0),
      ref(1),
      sel('end'),
      sel('setHighlightVisible', false),
      sel('highlightVisible'),
      sel('setHighlightVisible', false),
      sel('setHighlightVisible', true),
      sel('highlightVisible'),
      sel('isOverText', 1, W1),
      sel('isOverText', 1, BLANK),
    ],
  },
  {
    name: 'pan-disables-selection',
    covers: 'Under the pan tool the text handler is not eligible: no warming, no capture.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: slices,
    steps: [...opened, hub('activateTool', 'pan'), ptr('move', 1, H1.x, H1.y), ptr('down', 1, H1.x, H1.y)],
  },
  {
    name: 'close-releases-subscriptions',
    covers: 'Closing the document releases the store and event subscriptions.',
    plugins,
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [...opened, sel('ensurePage', 1), { op: 'call', target: 'documents', method: 'close', args: ['a'] }],
  },
];

// ── function suites ──────────────────────────────────────────────────────────

const fc = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

const crop = { left: 0, bottom: 0, right: 200, top: 100 };
const g8 = (left: number, bottom: number, flags = 0, w = 8, h = 10) => ({ looseBox: { left, bottom, right: left + w, top: bottom + h }, flags });
const simple = (count: number): PageGeometrySnapshot => ({
  runs: [{ rect: { left: 10, bottom: 90, right: 10 + count * 8, top: 100 }, charStart: 0, glyphs: Array.from({ length: count }, (_, i) => g8(10 + i * 8, 90)) }],
});
const columnGlyph = (yBottom: number, yTop: number, flags = 0) => ({
  looseQuad: { p1: { x: 88, y: yBottom }, p2: { x: 88, y: yTop }, p3: { x: 100, y: yBottom }, p4: { x: 100, y: yTop } },
  flags,
});
const mixed: PageGeometrySnapshot = {
  runs: [
    { rect: { left: 10, bottom: 90, right: 26, top: 100 }, charStart: 0, glyphs: [g8(10, 90), g8(18, 90)] },
    {
      rect: { left: 88, bottom: 20, right: 100, top: 44 },
      charStart: 2,
      rotation: Math.PI / 2,
      ascentFlip: false,
      glyphs: [columnGlyph(20, 28), columnGlyph(28, 36), columnGlyph(36, 44)],
    },
  ],
};
const quad = (x0: number, y0: number, x1: number, y1: number): PdfQuad => ({
  p1: { x: x0, y: y1 },
  p2: { x: x1, y: y1 },
  p3: { x: x0, y: y0 },
  p4: { x: x1, y: y0 },
});
/** Every run variant: upright with gaps and a font change, a rotated column,
 *  a mirrored run, a second column that shares the first one's frame, a
 *  run whose glyphs are all empty, a sheared run, and an RTL line. */
const varied: PageGeometrySnapshot = {
  runs: [
    { rect: { left: 10, bottom: 90, right: 90, top: 100 }, charStart: 0, fontSize: 10, glyphs: [g8(10, 90), g8(18, 90, 1), g8(26, 90), g8(70, 90), g8(78, 90, 2)] },
    { rect: { left: 10, bottom: 88, right: 40, top: 102 }, charStart: 5, fontSize: 30, glyphs: [g8(96, 88, 0, 8, 14), g8(104, 88, 0, 8, 14)] },
    { rect: { left: 10, bottom: 91, right: 30, top: 99 }, charStart: 7, fontSize: 11, glyphs: [g8(120, 91, 0, 8, 8), g8(128, 91, 0, 0, 8)] },
    { rect: { left: 88, bottom: 20, right: 100, top: 44 }, charStart: 9, rotation: Math.PI / 2, ascentFlip: false, glyphs: [columnGlyph(20, 28), columnGlyph(28, 36, 1), columnGlyph(36, 44)] },
    {
      rect: { left: 150, bottom: 20, right: 162, top: 44 },
      charStart: 12,
      rotation: Math.PI / 2,
      ascentFlip: true,
      glyphs: [
        { looseQuad: { p1: { x: 162, y: 20 }, p2: { x: 162, y: 28 }, p3: { x: 150, y: 20 }, p4: { x: 150, y: 28 } }, flags: 0, tightQuad: { p1: { x: 161, y: 21 }, p2: { x: 161, y: 27 }, p3: { x: 151, y: 21 }, p4: { x: 151, y: 27 } } },
      ],
    },
    { rect: { left: 60, bottom: 20, right: 72, top: 44 }, charStart: 13, rotation: Math.PI / 2 + 0.001, ascentFlip: false, glyphs: [{ looseQuad: { p1: { x: 60, y: 20 }, p2: { x: 60.008, y: 28 }, p3: { x: 72, y: 19.988 }, p4: { x: 72.008, y: 27.988 } }, flags: 0 }] },
    { rect: ZERO, charStart: 14, rotation: 1, ascentFlip: false, glyphs: [{ looseQuad: quad(0, 0, 0, 0), flags: 2 }] },
    { rect: { left: 0, bottom: 0, right: 1e-9, top: 1e-9 }, charStart: 15, rotation: 2, ascentFlip: false, glyphs: [{ looseQuad: quad(5, 5, 5, 5), flags: 0 }] },
    { rect: { left: 10, bottom: 60, right: 40, top: 70 }, charStart: 16, rotation: 0, ascentFlip: false, glyphs: [{ looseQuad: { p1: { x: 12, y: 70 }, p2: { x: 20, y: 70 }, p3: { x: 10, y: 60 }, p4: { x: 18, y: 60 } }, flags: 0 }, { looseQuad: { p1: { x: 22, y: 70 }, p2: { x: 30, y: 70 }, p3: { x: 20, y: 60 }, p4: { x: 28, y: 60 } }, flags: 0 }] },
    { rect: { left: 34, bottom: 40, right: 58, top: 50 }, charStart: 18, glyphs: [g8(50, 40), g8(42, 40), g8(34, 40)] },
  ],
};
const empty: PageGeometrySnapshot = { runs: [] };

/** A run rotated by `angle` (radians CCW): glyph cells 8 along the baseline,
 *  10 up the ascent, starting at `origin`; spaces where `spaces` says. */
const tiltedRun = (charStart: number, angle: number, origin: { x: number; y: number }, count: number, spaces: number[] = []) => {
  const b = { x: Math.cos(angle), y: Math.sin(angle) };
  const a = { x: -Math.sin(angle), y: Math.cos(angle) };
  const at = (u: number, v: number) => ({ x: origin.x + b.x * u + a.x * v, y: origin.y + b.y * u + a.y * v });
  const glyphs = Array.from({ length: count }, (_, i) => ({
    looseQuad: { p1: at(i * 8, 10), p2: at(i * 8 + 8, 10), p3: at(i * 8, 0), p4: at(i * 8 + 8, 0) },
    flags: spaces.includes(i) ? 1 : 0,
    ...(spaces.includes(i) ? {} : { tightQuad: { p1: at(i * 8 + 1, 9), p2: at(i * 8 + 7, 9), p3: at(i * 8 + 1, 1), p4: at(i * 8 + 7, 1) } }),
  }));
  const pts = glyphs.flatMap((g) => [g.looseQuad.p1, g.looseQuad.p2, g.looseQuad.p3, g.looseQuad.p4]);
  return {
    rect: {
      left: Math.min(...pts.map((p) => p.x)),
      bottom: Math.min(...pts.map((p) => p.y)),
      right: Math.max(...pts.map((p) => p.x)),
      top: Math.max(...pts.map((p) => p.y)),
    },
    charStart,
    rotation: angle,
    ascentFlip: false,
    glyphs,
  };
};
/** Text at 30 degrees: a word, a space, a word; then a far run in the same
 *  frame; then a run at 120 degrees, a frame of its own. */
const tilted: PageGeometrySnapshot = {
  runs: [
    tiltedRun(0, Math.PI / 6, { x: 100, y: 100 }, 5, [2]),
    tiltedRun(5, Math.PI / 6 + 0.001, { x: 300, y: 215 }, 2),
    tiltedRun(7, (2 * Math.PI) / 3, { x: 50, y: 300 }, 3),
  ],
};
/** Upright lines for the line-expansion walk: overlapping, stacked, a
 *  zero-width run, and an empty-rect run between them. */
const lines: PageGeometrySnapshot = {
  runs: [
    run(0, glyphs('ab', 0, 100)),
    { rect: { left: 13, bottom: 88, right: 13, top: 100 }, charStart: 2, glyphs: [{ looseBox: { left: 13, bottom: 88, right: 13, top: 100 }, flags: 0 }] },
    run(3, glyphs('cd', 20, 97)),
    { rect: ZERO, charStart: 5, glyphs: [{ looseBox: ZERO, flags: 2 }] },
    run(6, glyphs('ef', 40, 94)),
    run(8, glyphs('gh', 60, 80)),
    run(10, glyphs('ij', 0, 60)),
  ],
};
/** One upright run whose glyph gaps straddle the split threshold (2.5
 *  average widths, 15 here): 13, 15 and 16. */
const gaps: PageGeometrySnapshot = {
  runs: [
    run(0, [0, 19, 40, 62].map((left) => ({ looseBox: { left, bottom: 88, right: left + 6, top: 100 }, flags: 0 }))),
  ],
};
/** A zero-width run first, so a line walk ends on it. */
const zeroFirst: PageGeometrySnapshot = {
  runs: [
    { rect: { left: 13, bottom: 88, right: 13, top: 100 }, charStart: 0, glyphs: [{ looseBox: { left: 13, bottom: 88, right: 13, top: 100 }, flags: 0 }] },
    run(1, glyphs('cd', 20, 97)),
  ],
};
const sweepFixtures: Record<string, PageGeometrySnapshot> = { geo1, tilted, lines, gaps, zeroFirst };

/** Two upright runs, the second shifted by `dx` past the first's end,
 *  raised by `dy`, `h2` tall, `n2` glyphs, at font size `fs2`: the merge
 *  and line-overlap thresholds. */
const pair = (dx: number, dy: number, h2: number, n2: number, fs2: number | undefined, fs1 = 12): PageGeometrySnapshot => {
  const a = run(0, glyphs('ab', 0, 100), fs1);
  const g2 = Array.from({ length: n2 }, (_, i) => {
    const left = 12 + dx + i * 6;
    return { looseBox: { left, bottom: 100 + dy - h2, right: left + 6, top: 100 + dy }, flags: 0 };
  });
  const b: Record<string, unknown> = { ...run(2, g2 as Glyph[]), fontSize: fs2 };
  if (fs2 === undefined) delete b.fontSize;
  return { runs: [a, b as never] };
};
const pairs: [string, PageGeometrySnapshot][] = [];
const addPair = (dx: number, dy: number, h2: number, n2: number, fs2: number | undefined, fs1 = 12) =>
  pairs.push([`dx${dx}-dy${dy}-h${h2}-n${n2}-fs${fs2}${fs1 === 12 ? '' : `-first${fs1}`}`, pair(dx, dy, h2, n2, fs2, fs1)]);
// Offset against overlap: where lines merge and where a triple-click stops.
for (const dx of [0, 5, 9, 11, 12, 13, 30])
  for (const [dy, h2] of [[0, 12], [2, 12], [2.4, 12], [3, 12], [6, 12], [0, 9], [-1, 14], [-6, 24], [0, 0]]) addPair(dx, dy, h2, 1, 12);
// Font size ratio, either side of 1.5, and the sizes that skip the test.
for (const dx of [0, 13]) for (const fs2 of [18, 18.5, 7.9, 8.1, 0, undefined]) addPair(dx, 0, 12, 1, fs2);
// The second run left of the first (a reversed line), either side of the
// padded adjacency edge, and a first run with no usable font size.
for (const dx of [-31, -30, -29, -20]) for (const n2 of [1, 3]) addPair(dx, 0, 12, n2, 12);
addPair(0, 0, 12, 1, 12, 0);
// Vertical overlap exactly at the 0.8 merge threshold (10 / 12.5).
addPair(0, 0.5, 10.5, 1, 12);
// Glyph count, which sets the per-character width the adjacency test pads by.
for (const dx of [9, 11, 12, 13, 18, 19]) addPair(dx, 0, 12, 3, 12);

/** Text snapshots with several char-map anchors: supplementary characters
 *  and dropped ones. */
const maps: Record<string, PageTextSnapshot> = {
  emoji: { text: 'a😀b😀c😀d', charCount: 7, charMap: [[2, 3], [4, 6], [6, 9]] },
  dropped: { text: 'abcd', charCount: 7, charMap: [[2, 1], [4, 2], [6, 3]] },
  mixed: { text: 'x😀yz', charCount: 6, charMap: [[1, 1], [2, 3], [3, 3], [5, 4]] },
};

/** `textSegmentsForRange` from every start in [-1, n] for a spread of
 *  counts (short, long, to the end, past it), plus word, line and quad per
 *  glyph. */
const layoutSweep = (snap: PageGeometrySnapshot) => {
  const layout = buildPageTextLayout(snap);
  const n = layout.glyphs.length;
  const segments: unknown[] = [];
  for (let from = -1; from <= n; from++)
    for (const count of new Set([0, 1, 2, 3, 5, 8, n - from, n + 1 - from])) segments.push(textSegmentsForRange(layout, from, count));
  const glyphsIn = Array.from({ length: n + 2 }, (_, i) => i - 1);
  return {
    segments,
    words: glyphsIn.map((g) => expandTextRangeToWord(layout, g)),
    lines: glyphsIn.map((g) => expandTextRangeToLine(layout, g)),
    quads: glyphsIn.map((g) => textGlyphQuad(layout, g)),
  };
};

/** `textGlyphAt` on a grid, row-major. */
const glyphAtGrid = (snap: PageGeometrySnapshot, grid: { x0: number; x1: number; y0: number; y1: number; step: number }, tolerance?: number) => {
  const layout = buildPageTextLayout(snap);
  const out: (number | null)[] = [];
  for (let y = grid.y0; y <= grid.y1; y += grid.step)
    for (let x = grid.x0; x <= grid.x1; x += grid.step) out.push(textGlyphAt(layout, { x, y }, tolerance));
  return out;
};

/** The two runs of a pair, whole: segments, and the line around each end. */
const pairProbe = (snap: PageGeometrySnapshot) => {
  const layout = buildPageTextLayout(snap);
  const n = layout.glyphs.length;
  return { segments: textSegmentsForRange(layout, 0, n), first: expandTextRangeToLine(layout, 0), last: expandTextRangeToLine(layout, n - 1) };
};

/** `sliceTextByChars` over every (from, to) in [-1, charCount + 1]. */
const sliceSweep = (snap: PageTextSnapshot) => {
  const out: string[] = [];
  for (let from = -1; from <= snap.charCount + 1; from++)
    for (let to = from; to <= snap.charCount + 1; to++) out.push(sliceTextByChars(snap, from, to));
  return out;
};

const geomOf = (spec: { snapshot: PageGeometrySnapshot; crop: PdfRect; rotation: 0 | 90 | 180 | 270; userUnit: number }) =>
  buildSelectionPageGeometry(spec.snapshot, spec.crop, spec.rotation, spec.userUnit);
type GeomSpec = Parameters<typeof geomOf>[0];
const spec = (snapshot: PageGeometrySnapshot, c = crop, rotation: 0 | 90 | 180 | 270 = 0, userUnit = 1) => ({
  snapshot,
  crop: c,
  rotation,
  userUnit,
});

const geometryFns = {
  buildSelectionPageGeometry,
  contentPointToPdf: (s: GeomSpec, p: { x: number; y: number }) => contentPointToPdf(geomOf(s), p),
  toContentTextQuad: (s: GeomSpec, q: PdfQuad) => toContentTextQuad(geomOf(s), q),
  toContentSegment: (s: GeomSpec, segment: Parameters<typeof toContentSegment>[1]) => toContentSegment(geomOf(s), segment),
  buildPageTextLayout,
  textGlyphAt: (snap: PageGeometrySnapshot, p: { x: number; y: number }, tolerance?: number) => textGlyphAt(buildPageTextLayout(snap), p, tolerance),
  expandTextRangeToWord: (snap: PageGeometrySnapshot, glyph: number) => expandTextRangeToWord(buildPageTextLayout(snap), glyph),
  expandTextRangeToLine: (snap: PageGeometrySnapshot, glyph: number) => expandTextRangeToLine(buildPageTextLayout(snap), glyph),
  textGlyphQuad: (snap: PageGeometrySnapshot, glyph: number) => textGlyphQuad(buildPageTextLayout(snap), glyph),
  textSegmentsForRange: (snap: PageGeometrySnapshot, from: number, count: number) => textSegmentsForRange(buildPageTextLayout(snap), from, count),
  sliceTextByChars,
  layoutSweep,
  glyphAtGrid,
  pairProbe,
  sliceSweep,
  initialSelectionState: () => initialSelectionState(),
  reduceSelection,
};

const allGlyphs = (snap: PageGeometrySnapshot) => {
  let n = 0;
  for (const r of snap.runs) n += r.glyphs.length;
  return n;
};

const state = (over: Record<string, unknown> = {}) => ({ ...initialSelectionState(), ...over });
const segmentsOf = (s: GeomSpec, from: number, to: number) =>
  textSegmentsForRange(buildPageTextLayout(s.snapshot), from, to - from + 1).map((seg) => toContentSegment(geomOf(s), seg));

const geometryCases: FunctionCase[] = [
  fc('buildSelectionPageGeometry', 'simple', simple(3), crop, 0, 1),
  fc('buildSelectionPageGeometry', 'mixed-offset-crop', mixed, { left: 10, bottom: 20, right: 210, top: 120 }, 90, 2),
  fc('buildSelectionPageGeometry', 'varied', varied, crop, 0, 1),
  fc('buildSelectionPageGeometry', 'empty', empty, crop, 180, 1),
  fc('contentPointToPdf', 'identity-crop', spec(simple(1)), { x: 12, y: 5 }),
  fc('contentPointToPdf', 'offset-crop-user-unit', spec(simple(1), { left: 10, bottom: 20, right: 210, top: 120 }, 270, 2), { x: 12, y: 5 }),
  fc('toContentTextQuad', 'corners-map-to-names', spec(simple(1)), quad(10, 90, 18, 100)),
  fc('toContentSegment', 'rect-follows-the-quad', spec(simple(1), { left: 5, bottom: 0, right: 205, top: 150 }), { quad: quad(10, 90, 18, 100), rect: { left: 10, bottom: 90, right: 18, top: 100 }, advance: -1 }),
  fc('buildPageTextLayout', 'varied', varied),
  fc('buildPageTextLayout', 'mixed', mixed),
  fc('buildPageTextLayout', 'empty', empty),
  fc('buildPageTextLayout', 'rotated-all-empty', { runs: [varied.runs[6]] }),
  ...[
    ['inside-tight', { x: 14, y: 95 }],
    ['gap-between-glyphs-tolerance', { x: 45, y: 95 }],
    ['far-away', { x: 500, y: 500 }],
    ['rotated-column', { x: 94, y: 30 }],
    ['near-rotated', { x: 102, y: 30 }],
    ['mirrored-tight', { x: 156, y: 24 }],
    ['sheared', { x: 20, y: 65 }],
    ['rtl', { x: 45, y: 45 }],
    ['tolerance-picks-closest', { x: 10, y: 103 }],
  ].map(([label, p]) => fc('textGlyphAt', label as string, varied, p)),
  fc('textGlyphAt', 'no-tolerance-miss', varied, { x: 45, y: 95 }, 0),
  fc('textGlyphAt', 'wider-tolerance', varied, { x: 10, y: 110 }, 4),
  fc('textGlyphAt', 'empty-layout', empty, { x: 0, y: 0 }),
  fc('textGlyphAt', 'only-empty-glyphs', { runs: [{ rect: { left: 0, bottom: 0, right: 10, top: 10 }, charStart: 0, glyphs: [{ looseBox: ZERO, flags: 2 }] }] }, { x: 20, y: 5 }),
  fc('textGlyphAt', 'page-1-line-1', geo1, { x: 53, y: 744 }),
  ...Array.from({ length: allGlyphs(varied) + 2 }, (_, i) => fc('expandTextRangeToWord', `varied-${i - 1}`, varied, i - 1)),
  ...Array.from({ length: allGlyphs(varied) + 2 }, (_, i) => fc('expandTextRangeToLine', `varied-${i - 1}`, varied, i - 1)),
  fc('expandTextRangeToWord', 'page-1', geo1, 8),
  fc('expandTextRangeToLine', 'page-1', geo1, 15),
  fc('expandTextRangeToLine', 'mixed-column', mixed, 3),
  fc('expandTextRangeToLine', 'stacked-lines', { runs: [run(0, glyphs('ab', 0, 100)), run(2, glyphs('cd', 20, 100)), run(4, glyphs('ef', 0, 80)), run(6, glyphs('gh', 20, 80))] }, 5),
  fc('expandTextRangeToLine', 'zero-rect-neighbours', { runs: [run(0, glyphs('ab', 0, 100)), { rect: ZERO, charStart: 2, glyphs: [{ looseBox: ZERO, flags: 2 }] }, run(3, glyphs('cd', 20, 100))] }, 0),
  fc('expandTextRangeToLine', 'two-flat-runs', { runs: [0, 1].map((i) => ({ rect: { left: i * 20, bottom: 5, right: i * 20 + 10, top: 5 }, charStart: i, glyphs: [g8(i * 20, 5, 0, 8, 0)] })) }, 1),
  fc('expandTextRangeToLine', 'zero-height-run',{ runs: [{ rect: { left: 0, bottom: 5, right: 10, top: 5 }, charStart: 0, glyphs: [g8(0, 5, 0, 8, 0)] }, run(1, glyphs('ab', 20, 100))] }, 1),
  ...Array.from({ length: allGlyphs(varied) + 2 }, (_, i) => fc('textGlyphQuad', `varied-${i - 1}`, varied, i - 1)),
  ...[
    [0, 5],
    [0, 7],
    [0, 20],
    [2, 1],
    [3, 4],
    [5, 4],
    [7, 2],
    [9, 3],
    [9, 7],
    [12, 2],
    [14, 2],
    [16, 2],
    [18, 3],
    [-5, 8],
    [0, 0],
    [4, -1],
    [30, 5],
  ].map(([from, count]) => fc('textSegmentsForRange', `varied-${from}-${count}`, varied, from, count)),
  fc('textSegmentsForRange', 'empty-layout', empty, 0, 3),
  fc('textSegmentsForRange', 'page-1-both-lines', geo1, 6, 12),
  fc('textSegmentsForRange', 'far-glyph-splits-the-run', { runs: [run(0, [...glyphs('ab', 0, 100), ...glyphs('c', 200, 100)])] }, 0, 3),
  fc('textSegmentsForRange', 'zero-width-glyphs', { runs: [{ rect: { left: 0, bottom: 0, right: 0, top: 10 }, charStart: 0, glyphs: [g8(0, 0, 0, 0, 10), g8(0, 0, 0, 0, 10)] }] }, 0, 2),
  fc('textSegmentsForRange', 'mixed-all', mixed, 0, 5),
  fc('textSegmentsForRange', 'rtl', { runs: [{ rect: { left: 34, bottom: 90, right: 58, top: 100 }, charStart: 0, glyphs: [g8(50, 90), g8(42, 90), g8(34, 90)] }] }, 0, 3),
  fc('textSegmentsForRange', 'same-height-lines-merge', { runs: [run(0, glyphs('ab', 0, 100)), run(2, glyphs('cd', 13, 100), 0)] }, 0, 4),
  fc('textSegmentsForRange', 'contained-line-merges', { runs: [run(0, glyphs('ab', 0, 100), 12), { rect: { left: 13, bottom: 90, right: 19, top: 98 }, charStart: 2, fontSize: 12, glyphs: [g8(13, 90, 0, 6, 8)] }] }, 0, 3),
  fc('textSegmentsForRange', 'distant-lines-split', { runs: [run(0, glyphs('ab', 0, 100)), run(2, glyphs('cd', 100, 100))] }, 0, 4),
  ...[
    [0, 24],
    [6, 11],
    [11, 13],
    [13, 999],
    [-4, 3],
    [5, 5],
    [7, 3],
  ].map(([from, to]) => fc('sliceTextByChars', `page-1-${from}-${to}`, text1, from, to)),
  ...[
    [0, 3],
    [0, 1],
    [1, 2],
    [2, 3],
    [0, 9007199254740991],
    [1.5, 2.5],
  ].map(([from, to]) => fc('sliceTextByChars', `char-map-${from}-${to}`, text9, from, to)),
  fc('sliceTextByChars', 'dropped-char', { text: 'AB', charCount: 3, charMap: [[2, 1]] }, 0, 3),
  fc('sliceTextByChars', 'empty-map-is-identity', { text: 'xyz', charCount: 3, charMap: [] }, 1, 3),
  fc('sliceTextByChars', 'short-text', { text: 'ab', charCount: 5 }, 0, 5),
  fc('initialSelectionState', 'empty'),
  fc('reduceSelection', 'page-loaded', state(), { type: 'pageLoaded', page: pr(3) }),
  fc('reduceSelection', 'page-loaded-again', state({ loaded: { 3: true } }), { type: 'pageLoaded', page: pr(3) }),
  fc('reduceSelection', 'set', state({ selecting: true }), {
    type: 'set',
    selection: { anchor: { page: pr(1), glyph: 0 }, focus: { page: pr(1), glyph: 2 } },
    segments: { 1: segmentsOf(spec(simple(3)), 0, 2) },
  }),
  fc('reduceSelection', 'clear', state({ selection: { anchor: { page: pr(1), glyph: 0 }, focus: { page: pr(1), glyph: 0 } }, segments: {} }), { type: 'clear' }),
  fc('reduceSelection', 'clear-segments-only', state({ segments: { 2: [] } }), { type: 'clear' }),
  fc('reduceSelection', 'clear-empty', state(), { type: 'clear' }),
  fc('reduceSelection', 'hide', state(), { type: 'setHighlightHidden', hidden: true }),
  fc('reduceSelection', 'hide-again', state({ highlightHidden: true }), { type: 'setHighlightHidden', hidden: true }),
  fc('reduceSelection', 'selecting', state(), { type: 'setGestureActive', active: true }),
  fc('reduceSelection', 'selecting-again', state(), { type: 'setGestureActive', active: false }),
  ...Object.entries({ ...sweepFixtures, varied, mixed }).map(([name, snap]) => fc('layoutSweep', name, snap)),
  fc('glyphAtGrid', 'varied', varied, { x0: 0, x1: 170, y0: 10, y1: 110, step: 2.5 }),
  fc('glyphAtGrid', 'varied-tight-tolerance', varied, { x0: 0, x1: 170, y0: 10, y1: 110, step: 2.5 }, 0.4),
  fc('glyphAtGrid', 'tilted', tilted, { x0: 30, x1: 330, y0: 90, y1: 330, step: 4 }),
  fc('glyphAtGrid', 'tilted-wide', tilted, { x0: 30, x1: 330, y0: 90, y1: 330, step: 4 }, 3),
  fc('glyphAtGrid', 'lines', lines, { x0: -10, x1: 80, y0: 40, y1: 110, step: 1.5 }),
  ...pairs.map(([name, snap]) => fc('pairProbe', name, snap)),
  ...Object.entries({ ...maps, page1: text1, page9: text9 }).map(([name, snap]) => fc('sliceSweep', name, snap)),
];

// ── handles ──────────────────────────────────────────────────────────────────

/** A projector from a table: `toOverlay` applies `matrices[pon]`
 *  (`[a, b, c, d, e, f]`), null for a page not in it. `pages` lists overlay
 *  rects for `pageAt`; `pointOnPage` subtracts the page's rect origin. */
interface ViewSpec {
  matrices?: Record<string, [number, number, number, number, number, number]>;
  pages?: { pon: number; rect: { x: number; y: number; width: number; height: number } }[];
}
const viewOf = (v: ViewSpec): SelectionHandleView => ({
  toOverlay: (page, p) => {
    const m = v.matrices?.[String(page.pageObjectNumber)];
    return m ? { x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5] } : null;
  },
  pageAt: (o) => {
    const hit = (v.pages ?? []).find((pg) => o.x >= pg.rect.x && o.x < pg.rect.x + pg.rect.width && o.y >= pg.rect.y && o.y < pg.rect.y + pg.rect.height);
    return hit ? { ref: { kind: 'objectNumber', pageObjectNumber: hit.pon }, point: { x: o.x - hit.rect.x, y: o.y - hit.rect.y } } : null;
  },
  pointOnPage: (page, o) => {
    const pg = (v.pages ?? []).find((p) => p.pon === page.pageObjectNumber);
    return pg ? { x: o.x - pg.rect.x, y: o.y - pg.rect.y } : null;
  },
});

/** A drag session against a recording target: `begins` answers
 *  `beginGestureAt` per call (the last repeats); returns every target call in order. */
const handleDrag = (
  view: ViewSpec,
  opposite: SelectionHandleEndpoint,
  draggedPon: number,
  begins: boolean[],
  moves: ({ x: number; y: number } | 'end')[],
) => {
  const log: unknown[] = [];
  let calls = 0;
  const target = {
    beginGestureAt: (page: PageRef, point: { x: number; y: number }) => {
      const answer = begins[Math.min(calls++, begins.length - 1)];
      log.push({ beginGestureAt: [page, point], answer });
      return answer;
    },
    extendTo: (page: PageRef, point: { x: number; y: number }) => void log.push({ extendTo: [page, point] }),
    endGesture: () => void log.push({ endGesture: [] }),
  };
  const session = createSelectionHandleDrag(target, viewOf(view), opposite, { kind: 'objectNumber', pageObjectNumber: draggedPon });
  for (const m of moves) {
    if (m === 'end') session.end();
    else session.move(m);
  }
  return log;
};

const handleFns = {
  HANDLE_HEAD: () => HANDLE_HEAD,
  HANDLE_BAR: () => HANDLE_BAR,
  HANDLE_PAD: () => HANDLE_PAD,
  selectionHandleGeom: (view: ViewSpec, ep: SelectionHandleEndpoint, role: 'start' | 'end') => selectionHandleGeom(viewOf(view), ep, role),
  handleDrag,
};

const upright = { upperStart: { x: 10, y: 20 }, upperEnd: { x: 16, y: 20 }, lowerStart: { x: 10, y: 32 }, lowerEnd: { x: 16, y: 32 } };
const turned = { upperStart: { x: 20, y: 40 }, upperEnd: { x: 20, y: 34 }, lowerStart: { x: 32, y: 40 }, lowerEnd: { x: 32, y: 34 } };
const degenerate = { upperStart: { x: 5, y: 5 }, upperEnd: { x: 5, y: 5 }, lowerStart: { x: 5, y: 5 }, lowerEnd: { x: 5, y: 5 } };
const id2: [number, number, number, number, number, number] = [2, 0, 0, 2, 100, 50];
const tilt: [number, number, number, number, number, number] = [Math.cos(0.0005), Math.sin(0.0005), -Math.sin(0.0005), Math.cos(0.0005), 0, 0];
const views: Record<string, ViewSpec> = {
  scaled: { matrices: { 1: id2 } },
  tilted: { matrices: { 1: tilt } },
  rotated: { matrices: { 1: [0, 1, -1, 0, 500, 0] } },
  none: { matrices: {} },
};
const ep = (glyphQuad: typeof upright, advance: 1 | -1, pon = 1) => ({ page: { kind: 'objectNumber' as const, pageObjectNumber: pon }, glyphQuad, advance });

const handleCases: FunctionCase[] = [
  fc('HANDLE_HEAD', 'px'),
  fc('HANDLE_BAR', 'px'),
  fc('HANDLE_PAD', 'px'),
  ...(['start', 'end'] as const).flatMap((role) =>
    ([1, -1] as const).flatMap((advance) =>
      Object.entries(views).flatMap(([name, view]) => [
        fc('selectionHandleGeom', `${name}-upright-${role}-${advance}`, view, ep(upright, advance), role),
        fc('selectionHandleGeom', `${name}-turned-${role}-${advance}`, view, ep(turned, advance), role),
      ]),
    ),
  ),
  fc('selectionHandleGeom', 'degenerate-cell', views.scaled, ep(degenerate, 1), 'start'),
  fc('selectionHandleGeom', 'no-advance-start', views.scaled, ep(upright, 0 as never), 'start'),
  fc('selectionHandleGeom', 'no-advance-end', views.scaled, ep(upright, 0 as never), 'end'),
  fc('selectionHandleGeom', 'half-projected', { matrices: { 1: id2 } }, ep(upright, 1, 2), 'end'),
  fc('handleDrag', 'untouched-press-settles-nothing', { pages: [{ pon: 1, rect: { x: 0, y: 0, width: 100, height: 100 } }] }, ep(upright, 1), 1, [true], ['end']),
  fc('handleDrag', 'moves-extend-and-end-commits', { pages: [{ pon: 1, rect: { x: 0, y: 0, width: 100, height: 100 } }, { pon: 2, rect: { x: 0, y: 110, width: 100, height: 100 } }] }, ep(turned, -1), 1, [true], [{ x: 40, y: 30 }, { x: 50, y: 120 }, { x: 50, y: 105 }, 'end']),
  fc('handleDrag', 'begin-refused-then-accepted', { pages: [{ pon: 1, rect: { x: 0, y: 0, width: 100, height: 100 } }] }, ep(upright, 1), 1, [false, true], [{ x: 1, y: 1 }, 'end', { x: 2, y: 2 }, { x: 3, y: 3 }, 'end']),
  fc('handleDrag', 'gap-with-no-home-page', { pages: [] }, ep(upright, 1), 3, [true], [{ x: 1, y: 1 }, 'end']),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/selection.json',
    describes: 'packages/plugin/selection: text selection over engine geometry, the pointer handler, copy, permissions.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/selection-geometry.json',
    describes: 'packages/plugin/selection: the coordinate seam, the engine-core text layout it builds on, the reducer.',
    functions: geometryFns,
    cases: geometryCases,
  },
  {
    kind: 'function',
    file: 'plugins/selection-handles.json',
    describes: 'packages/plugin/selection: selection handle geometry and the handle drag session.',
    functions: handleFns,
    cases: handleCases,
  },
];
