/** Current annotation CRUD: engine drafts/patches and nested selection/text/tools APIs. */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';
import { annotationAfter } from '../../../../packages/core/annotation/src/record';
import { copiesOf, type AnnotationRecords } from '../../../../packages/plugin/annotation/src/sync/records';
import {
  annotationImportFacts,
  annotationKey,
  type Annotation,
  type ChangeOp,
  type DocumentEvent,
} from '@embedpdf/engine-core/runtime';
import type { ResponseSpec } from '../fake-engine';
import {
  scenario,
  call,
  hydrate,
  write,
  created,
  updated,
  deleted,
  refused,
  square,
  circle,
  line,
  polygon,
  freeText,
  callout,
  link,
  textNote,
  markup,
  pr,
  objRef,
  materialize,
  confirmation,
  settle,
  fail,
  advance,
  doc,
  PNG,
  operationAt,
  LOCAL,
} from './annotation-live-fixtures';
const box = { x: 100, y: 100, width: 100, height: 100 };
const seed = [
  square(1),
  circle(2, {
    box: { x: 300, y: 300, width: 100, height: 100 },
    rect: { x: 300, y: 300, width: 100, height: 100 },
  }),
  line(3),
];
const after = (n: number, patch: Parameters<typeof annotationAfter>[1]) =>
  annotationAfter(seed[n - 1]!, patch);
const select = (...refs: ReturnType<typeof objRef>[]) => call('selection.select', [refs]);
const draw = (tool: string, phase: string, x: number, y: number) =>
  call('createPointer', [tool, phase, pr(1), { x, y }]);
/**
 * The engine's answer to an `annotations.copy` op over `sources`: the copies
 * as the plugin's own rule reads them (`copiesOf`, the engine's
 * `planAnnotationCopies`), each with the fresh name the engine gives a copy,
 * and one `annotations.created` per copy, as an import publishes them.
 */
function copyWrite(
  n: number,
  sources: readonly Annotation[],
  op: Omit<Extract<ChangeOp, { type: 'annotations.copy' }>, 'type'>,
  options: { defer?: string } = {},
): ResponseSpec {
  const records: AnnotationRecords = {
    byKey: Object.fromEntries(
      sources.map((dto) => [annotationKey(dto.ref), { dto, pictureVersion: 0 }]),
    ),
    order: sources.map((dto) => annotationKey(dto.ref)),
  };
  const made = copiesOf(records, { type: 'annotations.copy', ...op });
  const annotations = made.map(({ copy }, at) => ({ ...copy!, nm: `copy-${n}-${at}` }));
  const opId = operationAt(n);
  const pages = [...new Map(annotations.map((a) => [a.page.objectNumber, a.page])).values()];
  const meta = { affectedPages: pages, cacheDelta: null, opId, undoable: true };
  const result = {
    annotations,
    refMap: made.map(({ of, ref }) => ({ from: of, to: ref })),
    dropped: [],
    meta,
  };
  // The fixture's meta names no changed pages beyond the copies'; the facts read it as an import's.
  const events = annotationImportFacts(result as unknown as Parameters<typeof annotationImportFacts>[0]).map(
    (fact, index): DocumentEvent => ({
      type: 'annotations.created',
      ...fact,
      origin: { ...LOCAL, transaction: { id: opId, index, count: annotations.length } },
    }),
  );
  return {
    path: 'apply',
    times: 1,
    value: { items: [{ type: 'annotations.copy', ...result }], meta },
    events,
    ...(options.defer ? { defer: options.defer } : {}),
  };
}
/** The deferred answer `duplicate-copies-as-one-change` settles with `settle('copy', …)`. */
const firstCopy = (): ResponseSpec =>
  copyWrite(
    1,
    copySeed(),
    {
      refs: [objRef(1), objRef(2), objRef(3), objRef(5)],
      offset: { x: 10, y: 20 },
      objectNumbers: [10001, 10002, 10003, 10004],
    },
    { defer: 'copy' },
  );
const copySeed = (): Annotation[] => [
  square(1),
  textNote(2),
  textNote(3, { reply: { to: objRef(2), type: 'group' } }),
  square(4, { locked: true }),
  link(5, { reply: { to: objRef(1), type: 'group' } }),
];
const cases: ScenarioCase[] = [
  scenario(
    'create-programmatically',
    'Current complete draft creates preserve birth refs, flags and resources; engine and missing-page refusals reject.',
    [
      call('create', [
        pr(1),
        { subtype: 'square', box: { x: 1, y: 2, width: 3, height: 4 }, print: true },
      ]),
      call('create', [
        pr(1),
        { subtype: 'stamp', box: { x: 1, y: 2, width: 3, height: 4 }, hidden: true },
        { appearance: PNG },
      ]),
      call('create', [pr(1), { subtype: 'square', box }]),
      call('create', [pr(99), { subtype: 'square', box }]),
      call('list'),
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            materialize(10001, {
              subtype: 'square',
              box: { x: 1, y: 2, width: 3, height: 4 },
              print: true,
            }),
          ),
        },
      ]),
      write(2, [
        {
          item: created(
            materialize(10002, {
              subtype: 'stamp',
              box: { x: 1, y: 2, width: 3, height: 4 },
              hidden: true,
            }),
          ),
        },
      ]),
      refused('nope'),
    ],
  ),
  scenario(
    'update-and-delete-by-ref',
    'Current patches preserve identity; absent/base-index refs and refused writes remain explicit, deletion is confirmed through doc.apply.',
    [
      call('update', [objRef(1), { contents: 'patched' }]),
      call('get', [objRef(1)]),
      call('update', [objRef(1), { contents: 'again' }]),
      call('update', [objRef(42), { contents: 'missing' }]),
      call('update', [
        { kind: 'baseIndex', page: pr(1), baseIndex: 0 },
        { contents: 'not-loaded' },
      ]),
      call('delete', [objRef(2)]),
      call('delete', [objRef(3)]),
      call('delete', [objRef(42)]),
      call('list'),
    ],
    [
      hydrate(seed),
      write(1, [{ item: updated(after(1, { contents: 'patched' })) }]),
      refused('stale', 'Conflict'),
      write(3, [{ item: deleted(objRef(2)), changed: [objRef(2)] }]),
      refused('locked'),
    ],
  ),
  scenario(
    'selection-reads',
    'Nested selection set/add/list and properties read complete current annotations; grouped parts select with their parent.',
    [
      call('selection.getSelected'),
      call('selection.getProperties'),
      select(objRef(1)),
      call('selection.add', [[objRef(2)]]),
      call('selection.getSelected'),
      call('selection.getProperties'),
      select(objRef(3)),
      call('selection.getProperties'),
      call('selection.add', [[objRef(5)]]),
      call('selection.getProperties'),
      call('selection.clear'),
      call('selection.getSelected'),
      select(objRef(77)),
      call('selection.getSelected'),
    ],
    [
      hydrate([
        square(1, { locked: true }),
        square(2, { color: '#0000ff', strokeWidth: 5 }),
        circle(3),
        link(4, { reply: { to: objRef(3), type: 'group' } }),
        markup('highlight', 5),
      ]),
    ],
  ),
  scenario(
    'restyle-writes-and-rolls-back',
    'A multi-selection update is one atomic current Change; pending fields show before refusal and roll back as one.',
    [
      select(objRef(1), objRef(2)),
      call('selection.update', [{ color: '#00ff00', fontSize: 30 }]),
      call('list'),
      fail('restyle', 'intended restyle refusal'),
      call('list'),
      select(objRef(3)),
      call('selection.update', [{ strokeWidth: 9 }]),
      call('get', [objRef(3)]),
    ],
    [
      hydrate(seed),
      { path: 'apply', times: 1, defer: 'restyle' },
      write(2, [{ item: updated(after(3, { strokeWidth: 9 })) }]),
    ],
  ),
  scenario(
    'flags-writes',
    'Flattened flags use the real selection patch, including an explicit unlock; a later refused atomic flag write rolls back both members.',
    [
      select(objRef(1), objRef(2)),
      call('selection.update', [{ locked: false, print: false }]),
      call('selection.getSelected'),
      call('selection.update', [{ hidden: true }]),
      call('list'),
    ],
    [
      hydrate([square(1, { locked: true }), square(2)]),
      write(1, [
        { item: updated(square(1, { locked: false, print: false })) },
        { item: updated(square(2, { print: false })) },
      ]),
      refused('denied'),
    ],
  ),
  scenario(
    'delete-and-rotate-the-selection',
    'Nested quarter-turn and reset write current patches; deleteSelection is one transaction and restores its members on refusal.',
    [
      select(objRef(1)),
      call('selection.rotateBy', [90]),
      call('get', [objRef(1)]),
      call('selection.resetRotation'),
      select(objRef(3), objRef(2)),
      call('selection.delete'),
      call('list'),
    ],
    [
      hydrate(seed),
      write(1, [{ item: updated(after(1, { rotation: 90 })) }]),
      write(2, [{ item: updated(seed[0]!) }]),
      refused('intended delete refusal'),
    ],
  ),
  scenario(
    'drawn-shape-commits',
    'Pointer creation names the drawing at birth; confirmed creation and refused creation follow the real pending queue.',
    [
      draw('square', 'down', 100, 100),
      draw('square', 'move', 150, 140),
      draw('square', 'move', 180, 160),
      draw('square', 'up', 180, 160),
      call('list'),
      draw('circle', 'down', 10, 10),
      draw('circle', 'up', 60, 50),
      call('selection.getSelected'),
      call('draft.get'),
      call('draft.cancel'),
    ],
    [
      hydrate(),
      write(1, [
        {
          item: created(
            materialize(10001, {
              subtype: 'square',
              box: { x: 100, y: 100, width: 80, height: 60 },
              color: '#e5484d',
              strokeWidth: 6,
            }),
          ),
        },
      ]),
      refused('full'),
    ],
  ),
  scenario(
    'free-text-editing',
    'Typing stages current rich text in a hold; the 250ms pause and text.end send the latest contents, and current editing/text projections follow.',
    [
      call('listTextItems', [pr(1)]),
      call('beginTextEditAt', [pr(1), { x: 150, y: 150 }]),
      call('text.getEditing'),
      call('beginTextEditAt', [pr(1), { x: 5, y: 5 }]),
      call('draftContents', [objRef(1), 'Hello w']),
      advance(100),
      call('draftContents', [objRef(1), 'Hello world']),
      advance(249),
      advance(1),
      call('draftContents', [objRef(1), 'Hello world!']),
      call('text.end'),
      call('text.getEditing'),
      call('text.begin', [objRef(3)]),
      call('draftContents', [objRef(3), 'lead']),
      call('text.end'),
    ],
    [
      hydrate([
        freeText(1),
        square(2, {
          box: { x: 400, y: 600, width: 100, height: 100 },
          rect: { x: 400, y: 600, width: 100, height: 100 },
        }),
        callout(3),
      ]),
      write(1, [{ item: updated(freeText(1, { contents: 'Hello world' })) }], { ms: 0 }),
      write(2, [{ item: updated(freeText(1, { contents: 'Hello world!' })) }], { ms: 350 }),
      write(3, [{ item: updated(callout(3, { contents: 'lead' })) }], { ms: 350 }),
    ],
  ),
  scenario(
    'text-commits-flush-when-switching-annotation',
    'The current single typing hold flushes when typing switches records; each pause writes the latest words.',
    [
      call('draftContents', [objRef(1), 'a1']),
      call('draftContents', [objRef(3), 'b1']),
      call('draftContents', [objRef(1), 'a2']),
      advance(300),
      call('get', [objRef(1)]),
      call('get', [objRef(3)]),
    ],
    [
      hydrate([freeText(1), callout(3)]),
      write(1, [{ item: updated(freeText(1, { contents: 'a1' })) }]),
      write(2, [{ item: updated(callout(3, { contents: 'b1' })) }]),
      write(3, [{ item: updated(freeText(1, { contents: 'a2' })) }], { ms: 0 }),
    ],
  ),
  scenario(
    'live-link-targets-and-finishing-a-draft',
    'A standalone link target is an actual engine field, pending retarget shows immediately; a polygon finishes through draft.finish.',
    [
      select(objRef(3)),
      call('selection.update', [{ target: { kind: 'uri', uri: 'https://new' } }]),
      call('listLinkItems', [pr(1)]),
      settle(
        'retarget',
        confirmation(1, [
          { item: updated(link(3, { target: { kind: 'uri', uri: 'https://new' } })) },
        ]).value,
      ),
      draw('polygon', 'down', 10, 10),
      draw('polygon', 'down', 50, 10),
      draw('polygon', 'down', 30, 40),
      call('draft.finish'),
      call('list'),
    ],
    [
      hydrate([link(3)]),
      write(1, [{ item: updated(link(3, { target: { kind: 'uri', uri: 'https://new' } })) }], {
        defer: 'retarget',
      }),
      write(2, [
        {
          item: created(
            materialize(10001, {
              subtype: 'polygon',
              vertices: [
                { x: 10, y: 10 },
                { x: 50, y: 10 },
                { x: 30, y: 40 },
              ],
              color: '#e5484d',
              strokeWidth: 6,
            }),
          ),
        },
      ]),
    ],
  ),
  scenario(
    'settings',
    'Current tools defaults and shared settings preserve preset isolation and merge chrome/snap fields.',
    [
      call('tools.register', [
        { id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'closed-arrow' } } },
      ]),
      call('tools.getDefaults', ['arrow']),
      call('tools.getDefaults', ['line']),
      call('tools.updateDefaults', ['arrow', { strokeWidth: 3 }]),
      call('tools.getDefaults', ['arrow']),
      call('tools.getProperties', ['free-text-callout']),
      call('getSettings'),
      call('updateSettings', [
        {
          snap: { rotationThresholdDeg: 10 },
          chrome: { handles: { hitSize: 40 }, rotationHandle: { enabled: false } },
        },
      ]),
      call('getSettings'),
      select(objRef(1)),
      call('listChromeNodes', [pr(1)]),
      call('selection.getAnchor'),
      call('getSelectionAnchorIn', [{ scale: 2, rotation: 0, zoom: 1 }]),
    ],
    [hydrate([square(1)])],
  ),
  scenario(
    'render-projections',
    'Current page/link projections retain attached refs and hide excluded links; creation anchor uses draft.get in page space.',
    [
      call('listPageItems', [pr(1)]),
      call('listPageItems', [pr(1), { zoom: 2, rotation: 90 }]),
      call('getAppearanceEpoch', [pr(1)]),
      call('listLinkItems', [pr(1)]),
      call('listLinkItems', [pr(2)]),
      draw('polygon', 'down', 10, 10),
      draw('polygon', 'down', 50, 10),
      call('draft.get'),
      draw('polygon', 'down', 30, 40),
      call('draft.get'),
      call('draft.cancel'),
    ],
    [
      hydrate([
        square(1),
        textNote(2, { reply: { to: objRef(1), type: 'reply' }, contents: 'reply' }),
        link(3),
        link(4, { reply: { to: objRef(1), type: 'group' } }),
        link(5, { hidden: true }),
        link(6, { target: null }),
      ]),
    ],
  ),
  scenario(
    'memoized-selectors-and-edges',
    'Current selectors cache by actual model/view; pending creations already carry stable refs and accept a follow-up update before confirmation.',
    [
      call('listPageItems', [pr(1)]),
      call('listPageItems', [pr(1)]),
      call('listTextItems', [pr(1)]),
      call('listTextItems', [pr(1)]),
      call('selection.getProperties'),
      call('selection.getProperties'),
      call('getHitKind', [pr(7), { x: 1, y: 1 }]),
      call('text.end'),
      draw('square', 'down', 100, 500),
      draw('square', 'move', 180, 560),
      draw('square', 'up', 180, 560),
      call('selection.getSelected'),
      call('isPending', [objRef(10001)]),
      call('selection.update', [{ color: '#000000' }]),
      settle(
        'draft',
        confirmation(1, [
          {
            item: created(
              materialize(10001, {
                subtype: 'square',
                box: { x: 100, y: 500, width: 80, height: 60 },
                color: '#e5484d',
                strokeWidth: 6,
              }),
            ),
          },
        ]).value,
      ),
      call('get', [objRef(10001)]),
      settle(
        'followup',
        confirmation(2, [
          {
            item: updated(
              materialize(10001, {
                subtype: 'square',
                box: { x: 100, y: 500, width: 80, height: 60 },
                color: '#000000',
                strokeWidth: 6,
              }),
            ),
          },
        ]).value,
      ),
      call('get', [objRef(10001)]),
      call('isPending', [objRef(10001)]),
    ],
    [
      hydrate([square(1), freeText(2)]),
      write(
        1,
        [
          {
            item: created(
              materialize(10001, {
                subtype: 'square',
                box: { x: 100, y: 500, width: 80, height: 60 },
                color: '#e5484d',
                strokeWidth: 6,
              }),
            ),
          },
        ],
        { defer: 'draft' },
      ),
      write(
        2,
        [
          {
            item: updated(
              materialize(10001, {
                subtype: 'square',
                box: { x: 100, y: 500, width: 80, height: 60 },
                color: '#000000',
                strokeWidth: 6,
              }),
            ),
          },
        ],
        { defer: 'followup' },
      ),
    ],
  ),
  scenario(
    'duplicate-copies-as-one-change',
    "duplicate (1481e4556): canDuplicate needs the right to create, an annotation that is here and nothing locked; isMovable is false for one that isn't here. A copy is one annotations.copy op under the object numbers its sources' places in refs are given, an attached link coming along (its copy isn't among the ones named); the copies show at once, read as creates (a reply to a copied note replies to its copy), are selected, and the engine's answer, with each copy's new name, replaces them. An empty duplicate resolves with nothing, an unknown ref rejects not-found, and code may copy a locked annotation onto another page by index.",
    [
      call('canDuplicate', [[objRef(1), objRef(2)]]),
      call('canDuplicate', [[objRef(4)]]),
      call('canDuplicate', [[]]),
      call('isMovable', [objRef(1)]),
      call('isMovable', [objRef(9)]),
      call('duplicate', [[objRef(1), objRef(2), objRef(3)], { offset: { x: 10, y: 20 }, select: true }]),
      call('list'),
      call('selection.getSelected'),
      settle('copy', firstCopy().value),
      call('list'),
      call('duplicate', [[objRef(9)]]),
      call('duplicate', [[]]),
      call('duplicate', [[objRef(4)], { page: 1 }]),
      call('get', [objRef(10006, 2)]),
    ],
    [
      hydrate(copySeed()),
      firstCopy(),
      // The unknown ref took 10005 before it was refused.
      copyWrite(2, copySeed(), { refs: [objRef(4)], page: pr(2), objectNumbers: [10006] }),
    ],
  ),
  scenario(
    'selection-duplicate-lands-beside',
    "selection.duplicate (1481e4556) copies the selection 10 points right and down, or left and up where a copy would leave its page, and selects the copies; selection.canDuplicate is false with nothing selected and with a locked annotation selected. A refused copy takes its copies away again.",
    [
      call('selection.canDuplicate'),
      call('selection.select', [[objRef(1)]]),
      call('selection.canDuplicate'),
      call('selection.duplicate'),
      call('selection.getSelected'),
      call('selection.select', [[objRef(6)]]),
      call('selection.duplicate'),
      call('list'),
      call('selection.select', [[objRef(4)]]),
      call('selection.canDuplicate'),
    ],
    [
      hydrate([
        square(1),
        square(4, { locked: true }),
        square(6, {
          box: { x: 565, y: 775, width: 30, height: 20 },
          rect: { x: 565, y: 775, width: 30, height: 20 },
        }),
      ]),
      copyWrite(1, [square(1)], {
        refs: [objRef(1)],
        offset: { x: 10, y: 10 },
        objectNumbers: [10001],
      }),
      refused('copy refused'),
    ],
  ),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-crud.json',
    describes:
      'Current live annotation CRUD, pending records, nested selection/text/tools, settings and projections.',
    cases,
  },
];
