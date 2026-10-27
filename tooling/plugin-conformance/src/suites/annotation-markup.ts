/** Text tools consume current selection snapshots in page space. */
import {
  pageGeometryOf,
  type PageGeometrySnapshot,
  type PdfCoordinates,
  type PdfRect,
} from '@embedpdf/engine-core/runtime';
import type { Step } from '../scenario';
import type { Suite } from '../vectors';
import { scenario, call, hydrate, pr, PAGES, SNAP, doc } from './annotation-live-fixtures';
import { ix, sel, tool, sample } from './annotation-live-inputs';
import { LIVE_ANSWERS } from './annotation-live-answers';
type Glyph = { loose: PdfRect; tight?: PdfRect; space?: true; empty?: true };
const glyphs = (text: string, x: number, top: number): Glyph[] =>
  [...text].map((ch, i) => {
    const left = x + i * 6,
      loose = { left, bottom: top - 12, right: left + 6, top };
    return ch === ' '
      ? { loose, space: true }
      : { loose, tight: { left: left + 0.5, bottom: top - 11, right: left + 5.5, top: top - 1 } };
  });
const newline: Glyph[] = [
  { loose: { left: 0, bottom: 0, right: 0, top: 0 }, empty: true },
  { loose: { left: 0, bottom: 0, right: 0, top: 0 }, empty: true },
];
const run = (start: number, glyphs: Glyph[]) => {
  const real = glyphs.filter((g) => !g.empty);
  return {
    start,
    glyphs,
    fontSize: 12,
    rect: {
      left: Math.min(...real.map((g) => g.loose.left)),
      bottom: Math.min(...real.map((g) => g.loose.bottom)),
      right: Math.max(...real.map((g) => g.loose.right)),
      top: Math.max(...real.map((g) => g.loose.top)),
    },
  };
};
const geometry = (page: number, snapshot: PageGeometrySnapshot<PdfCoordinates>) => ({
  path: 'page.text.layout',
  pon: page,
  value: { $textLayout: pageGeometryOf(snapshot, PAGES[page - 1]!.pdfCropBox) },
});
const reads = [
  geometry(1, {
    runs: [
      run(0, [...glyphs('Hello world', 50, 750), ...newline]),
      run(13, glyphs('Second line', 50, 730)),
    ],
  }),
  geometry(2, { runs: [run(0, glyphs('Page two', 40, 500))] }),
  { path: 'page.text.read', pon: 1, value: { text: 'Hello world\r\nSecond line', charCount: 24 } },
  { path: 'page.text.read', pon: 2, value: { text: 'Page two', charCount: 8 } },
];
const tall = [
  geometry(1, {
    runs: [
      run(0, [
        { loose: { left: 50, bottom: 726, right: 62, top: 750 } },
        { loose: { left: 62, bottom: 738, right: 68, top: 750 } },
        { loose: { left: 68, bottom: 738, right: 74, top: 750 } },
      ]),
    ],
  }),
  { path: 'page.text.read', pon: 1, value: { text: 'Abc', charCount: 3 } },
];
const plugins = [
  { factory: 'interaction' },
  { factory: 'annotation-live-selection' },
  { factory: 'annotation-live' },
];
const load = [sel('ensureLoaded', [pr(1)]), sel('ensureLoaded', [pr(2)])];
const range = (from: [number, number], to: [number, number]) => ({
  start: { page: pr(from[0]), index: from[1] },
  end: { page: pr(to[0]), index: to[1] },
});
const select = (from: [number, number], to: [number, number]) => sel('select', [range(from, to)]);
const ptr = (phase: string, x: number, y: number): Step =>
  ix('dispatchPointer', [sample(phase, x, y)]);
const drag = (from: [number, number], to: [number, number]) => [
  ptr('down', ...from),
  ptr('move', from[0] + 10, from[1]),
  ptr('move', ...to),
  ptr('up', ...to),
];
const markupCase = (name: string, covers: string, steps: Step[], geometryReads = reads) =>
  scenario(name, covers, steps, [hydrate(), ...geometryReads, ...LIVE_ANSWERS[name]], {
    plugins,
    snapshot: [...SNAP, 'selection::a'],
  });
const quad = {
  upperLeft: { x: 0, y: 0 },
  upperRight: { x: 1, y: 0 },
  lowerLeft: { x: 0, y: 1 },
  lowerRight: { x: 1, y: 1 },
};
const anchor = { glyphQuad: quad, advance: 1 };
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-markup.json',
    describes: 'Current text-selection markup, caret, replacement and preview.',
    cases: [
      markupCase(
        'highlight-by-drag',
        'A real text-select drag commits current per-line quads with tool defaults, clears selection and can author squiggly from a later selection.',
        [
          ...load,
          ...drag([53, 56], [77, 56]),
          sel('hasSelection'),
          sel('clear'),
          tool('highlight'),
          ...drag([53, 56], [83, 76]),
          call('list'),
          select([1, 0], [1, 4]),
          call('applyToolToSelection', ['squiggly']),
          call('createMarkup', ['underline', pr(1), []]),
          call('list'),
        ],
      ),
      markupCase(
        'insert-and-replace-text',
        'Insertion anchors to the end glyph; replacement previews strikeout and atomically creates a grouped Caret+StrikeOut pair.',
        [
          ...load,
          tool('insert-text'),
          ...drag([53, 56], [77, 56]),
          tool('replace-text'),
          ...drag([53, 56], [83, 76]),
          call('list'),
        ],
      ),
      markupCase(
        'replace-text-anchors-at-the-end-glyph',
        'The replacement caret uses the final glyph cell instead of a taller first glyph segment.',
        [
          sel('ensureLoaded', [pr(1)]),
          tool('replace-text'),
          select([1, 0], [1, 2]),
          sel('endGesture'),
          call('list'),
        ],
        tall,
      ),
      markupCase(
        'create-from-selection-anchors-at-the-end-glyph',
        'The public text-selection create uses the same final glyph anchor and resolves the primary record.',
        [
          sel('ensureLoaded', [pr(1)]),
          select([1, 0], [1, 2]),
          call('createFromSelection', ['replace-text']),
          call('list'),
        ],
        tall,
      ),
      markupCase(
        'replace-text-across-pages',
        'Replacement authors a separate atomic group per page, anchoring non-end pages to the final segment and end page to its glyph.',
        [...load, tool('replace-text'), select([1, 13], [2, 4]), sel('endGesture'), call('list')],
      ),
      markupCase(
        'preview-follows-the-tool',
        'During an active text gesture, tool switches change markup preview and highlight visibility; switching after the gesture instead authors the selected text.',
        [
          ...load,
          sel('beginGesture'),
          select([1, 0], [1, 5]),
          tool('underline'),
          call('listPageItems', [pr(1)]),
          sel('isHighlightVisible'),
          tool('replace-text'),
          call('listPageItems', [pr(1)]),
          sel('isHighlightVisible'),
          tool('pointer'),
          call('listPageItems', [pr(1)]),
          sel('isHighlightVisible'),
          sel('clear'),
          sel('endGesture'),
          tool('squiggly'),
          call('listPageItems', [pr(1)]),
          sel('isHighlightVisible'),
        ],
      ),
      markupCase(
        'markup-from-selection',
        'The current host bridge returns false for no selection and true after creating one markup per selected page; it clears the text selection.',
        [
          ...load,
          call('applyToolToSelection', ['underline']),
          select([1, 0], [2, 4]),
          call('applyToolToSelection', ['underline']),
          sel('hasSelection'),
          call('applyToolToSelection', ['highlight']),
          call('list'),
        ],
      ),
      markupCase(
        'create-from-selection-text-edits',
        'The public authoring door resolves records, supports clear:false and creates replacement groups independently on both selected pages.',
        [
          ...load,
          call('createFromSelection', ['insert-text']),
          select([1, 0], [1, 4]),
          call('createFromSelection', ['insert-text', { clear: false }]),
          sel('hasSelection'),
          select([1, 13], [2, 4]),
          call('createFromSelection', ['replace-text']),
          sel('hasSelection'),
          select([1, 0], [2, 4]),
          call('createFromSelection', ['highlight', { clear: false }]),
          call('list'),
          sel('hasSelection'),
        ],
      ),
      scenario(
        'markup-needs-a-selection-plugin-and-authority',
        'The host bridge is inert without selection or create authority; direct host markup creation is gated while pure previews remain available.',
        [
          {
            op: 'call',
            target: 'documents',
            method: 'open',
            args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
          },
          call('applyToolToSelection', ['highlight']),
          call('applyToolToSelection', ['highlight'], 'b'),
          call('createMarkup', ['highlight', pr(1), [], 'highlight'], 'b'),
          call('createCaret', [pr(1), anchor], 'b'),
          call('createReplaceText', [pr(1), [], anchor], 'b'),
          call('previewMarkup', ['highlight', { 1: [quad] }, 'highlight']),
          call('listPageItems', [pr(1)]),
          call('clearMarkupPreview'),
          call('listPageItems', [pr(1)]),
        ],
        [],
        {
          engine: {
            documents: {
              a: doc([hydrate()]),
              b: doc([hydrate()], {
                security: { allows: ['doc.annotate.read', 'doc.text.select'] },
              }),
            },
          },
        },
      ),
    ],
  },
];
