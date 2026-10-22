/** Current annotation CRUD: engine drafts/patches and nested selection/text/tools APIs. */
import type { ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';
import { annotationAfter } from '../../../../packages/core/annotation/src/record';
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
const select = (...refs: ReturnType<typeof objRef>[]) => call('selection.set', [refs]);
const draw = (tool: string, phase: string, x: number, y: number) =>
  call('createPointer', [tool, phase, pr(1), { x, y }]);
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
      call('selection.list'),
      call('selection.getProperties'),
      select(objRef(1)),
      call('selection.add', [[objRef(2)]]),
      call('selection.list'),
      call('selection.getProperties'),
      select(objRef(3)),
      call('selection.getProperties'),
      call('selection.add', [[objRef(5)]]),
      call('selection.getProperties'),
      call('selection.clear'),
      call('selection.list'),
      select(objRef(77)),
      call('selection.list'),
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
      call('selection.list'),
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
      call('selection.list'),
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
          snap: { rotationThreshold: 10 },
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
      call('selection.list'),
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
