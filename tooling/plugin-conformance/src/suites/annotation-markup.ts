/**
 * `packages/plugin/annotation`: text-selection authoring (`markup.ts` and
 * `markupFromSelection`). The selection plugin is the producer: its
 * `text-select` gesture runs under the markup and text-edit tools, and this
 * plugin consumes its change and commit signals. Page geometry answers from
 * the fake engine in `engine-core`'s `PageGeometrySnapshot` shape. Replayed
 * by `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';

import { SNAP, boot, call, caret, created, doc, hydrate, ix, markup, objRef, sel, tool } from './annotation-fixtures';

const W = 6;
const H = 12;
const ZERO = { left: 0, bottom: 0, right: 0, top: 0 };
type Glyph = { looseBox: typeof ZERO; flags: number; tightBox?: typeof ZERO };
const glyphs = (text: string, x: number, top: number): Glyph[] =>
  [...text].map((ch, i) => {
    const left = x + i * W;
    const looseBox = { left, bottom: top - H, right: left + W, top };
    if (ch === ' ') return { looseBox, flags: 1 };
    return { looseBox, flags: 0, tightBox: { left: left + 0.5, bottom: top - H + 1, right: left + W - 0.5, top: top - 1 } };
  });
const newline: Glyph[] = [
  { looseBox: ZERO, flags: 2 },
  { looseBox: ZERO, flags: 2 },
];
const run = (charStart: number, gs: Glyph[]) => {
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
    fontSize: 12,
  };
};
/** Page 1: "Hello world\r\nSecond line" at y 50..62 and 70..82 (content). */
const geo1 = { runs: [run(0, [...glyphs('Hello world', 50, 750), ...newline]), run(13, glyphs('Second line', 50, 730))] };
/** Page 2 (crop 10,20..410,520): one line. */
const geo2 = { runs: [run(0, glyphs('Page two', 40, 500))] };
const reads = [
  { path: 'page.geometry.read', pon: 1, value: geo1 },
  { path: 'page.geometry.read', pon: 2, value: geo2 },
  { path: 'page.text.read', pon: 1, value: { text: 'Hello world\r\nSecond line', charCount: 24 } },
  { path: 'page.text.read', pon: 2, value: { text: 'Page two', charCount: 8 } },
];

const noMods = { shift: false, alt: false, ctrl: false, meta: false };
const at = (phase: string, pon: number, x: number, y: number) => ({
  phase,
  viewport: { x, y },
  page: { pon, point: { x, y } },
  modifiers: noMods,
});
const ptr = (phase: string, x: number, y: number, pon = 1): Step => ix('dispatch', [at(phase, pon, x, y)]);
const drag = (from: [number, number], to: [number, number]): Step[] => [
  ptr('down', from[0], from[1]),
  ptr('move', from[0] + 10, from[1]),
  ptr('move', to[0], to[1]),
  ptr('up', to[0], to[1]),
];

const plugins = [{ factory: 'interaction' }, { factory: 'selection' }, { factory: 'annotation' }];
const load: Step[] = [sel('ensurePage', [1]), sel('ensurePage', [2])];
const range = (from: [number, number], to: [number, number]) => ({ start: { pon: from[0], index: from[1] }, end: { pon: to[0], index: to[1] } });
const slices = [...SNAP, 'selection::a'];

const cases: ScenarioCase[] = [
  {
    name: 'highlight-by-drag',
    covers:
      'Under the highlight tool the selection gesture drives a live markup preview (the blue highlight suppressed); the commit creates one highlight per selected page from the per-line quads, with the tool’s defaults, and clears the selection.',
    plugins,
    engine: {
      documents: {
        a: doc([hydrate(), ...reads, { path: 'page.annotations.create', pon: 1, value: created(markup('highlight', 30)) }]),
      },
    },
    snapshot: slices,
    steps: [...boot, ...load, tool('highlight'), ...drag([53, 56], [83, 76]), call('list', [1])],
  },
  {
    name: 'insert-and-replace-text',
    covers:
      'The insert-text tool creates a caret at the selection’s end anchor; the replace-text tool previews as a strikeout and creates one Caret + StrikeOut group per page (the true end anchor on the end page).',
    plugins,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          ...reads,
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(caret(40)) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(caret(41, { intent: 'replace' })) },
          { path: 'page.annotations.create', pon: 1, times: 1, value: created(markup('strikeout', 42, { intent: 'strikeout-text-edit', inReplyTo: objRef(41), replyType: 'group' })) },
        ]),
      },
    },
    snapshot: slices,
    steps: [...boot, ...load, tool('insert-text'), ...drag([53, 56], [77, 56]), tool('replace-text'), ...drag([53, 56], [83, 76]), call('list', [1])],
  },
  {
    name: 'replace-text-across-pages',
    covers: 'A selection spanning two pages under replace-text anchors each non-end page at its last segment’s trailing edge.',
    plugins,
    engine: {
      documents: {
        a: doc([
          hydrate(),
          ...reads,
          { path: 'page.annotations.create', pon: 1, value: created(caret(50)) },
          { path: 'page.annotations.create', pon: 2, value: created(caret(51, { pageObjectNumber: 2 })) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, ...load, tool('replace-text'), sel('select', [range([1, 13], [2, 4])]), sel('end'), call('list', [2])],
  },
  {
    name: 'preview-follows-the-tool',
    covers:
      'A selection made under the pointer tool keeps the blue highlight; switching to a markup tool previews it as that markup, back to the pointer tool clears the preview and restores the highlight. Without a selection the preview clears.',
    plugins,
    engine: { documents: { a: doc([hydrate(), ...reads]) } },
    snapshot: slices,
    steps: [...boot, ...load, sel('select', [range([1, 0], [1, 5])]), tool('underline'), tool('replace-text'), tool('pointer'), sel('clear'), tool('squiggly')],
  },
  {
    name: 'markup-from-selection',
    covers:
      'markupFromSelection turns the current selection into one markup per page with segments (the preset’s flags ride along), clears it, and returns true; with nothing selected it returns false.',
    plugins,
    engine: {
      documents: {
        a: doc([hydrate(), ...reads, { path: 'page.annotations.create', value: created(markup('underline', 60)) }]),
      },
    },
    snapshot: slices,
    steps: [
      ...boot,
      ...load,
      call('markupFromSelection', ['underline']),
      sel('select', [range([1, 0], [2, 4])]),
      call('markupFromSelection', ['underline', 'underline']),
      sel('hasSelection'),
      call('markupFromSelection', ['highlight']),
    ],
  },
  {
    name: 'markup-needs-a-selection-plugin-and-authority',
    covers:
      'Without a selection plugin markupFromSelection is false and the markup tools never register; without create authority it is false and createMarkup, createCaret and createReplaceText do nothing.',
    plugins: [{ factory: 'interaction' }, { factory: 'annotation' }],
    engine: {
      documents: {
        a: doc([hydrate()]),
        b: doc([hydrate()], { security: { allows: ['doc.annotate.read', 'doc.text.select'] } }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('markupFromSelection', ['highlight']),
      call('markupFromSelection', ['highlight'], 'b'),
      call('createMarkup', ['highlight', 1, [], 'highlight'], 'b'),
      call('createCaret', [1, { glyphQuad: { upperStart: { x: 0, y: 0 }, upperEnd: { x: 1, y: 0 }, lowerStart: { x: 0, y: 1 }, lowerEnd: { x: 1, y: 1 } }, advance: 1 }], 'b'),
      call('createReplaceText', [1, [], { glyphQuad: { upperStart: { x: 0, y: 0 }, upperEnd: { x: 1, y: 0 }, lowerStart: { x: 0, y: 1 }, lowerEnd: { x: 1, y: 1 } }, advance: 1 }], 'b'),
      call('previewMarkup', ['highlight', { 1: [{ upperStart: { x: 0, y: 0 }, upperEnd: { x: 1, y: 0 }, lowerStart: { x: 0, y: 1 }, lowerEnd: { x: 1, y: 1 } }] }, 'highlight']),
      call('pageItems', [1]),
      call('clearMarkupPreview'),
      call('pageItems', [1]),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-markup.json',
    describes: 'packages/plugin/annotation: markup, caret and replace-text from the selection plugin’s signals, and markupFromSelection.',
    cases,
  },
];
