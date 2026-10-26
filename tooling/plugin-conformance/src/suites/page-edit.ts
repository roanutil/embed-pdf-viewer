/** Current page-edit transactions over validated page refs and confirmed layouts. */
import { pageEditPlugin, PageEditToken } from '../../../../packages/plugin/page-edit/src/index';
import { registerPlugin, onCaseEnd, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  bindTransactions,
  pr,
  page,
  three,
  layout,
  meta,
  remote,
  rotated,
  reordered,
  deleted,
  inserted,
  result,
  fact,
  answer,
  doc,
  start,
  open,
  close,
  edit,
  copy,
  readPages,
  emit,
  settle,
  scenario,
} from './page-edit-current-fixtures';
import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
registerPlugin('page-edit', {
  factory: () => {
    installCaseEntropy();
    return pageEditPlugin();
  },
  tokens: [PageEditToken],
});
onCaseEnd(restoreCaseEntropy);
const p10 = (r: 0 | 90 | 180 | 270) => [page(10, 0, r), three[1], three[2]];
/** A step argument JSON can carry: NaN and ±Infinity travel as `$number`. */
const arg = (n: number): number =>
  Number.isFinite(n) ? n : ({ $number: String(n) } as unknown as number);

const r90 = rotated([pr(10)], 90, p10(90));
const r180 = rotated([pr(10)], 180, p10(180));
const d20 = deleted([pr(20)], [three[0], { ...three[2], index: 1 }]);
const add99 = inserted('pages.insertBlank', [99], [...three, page(99, 3, 0, 500, 600)]);
const ins77 = inserted('pages.insert', [77], [...three, page(77, 3, 0, 50, 60)]);
const denied = {
  name: 'PermissionDenied',
  message: 'source download refused',
  code: 'Forbidden',
  details: { required: 'doc.download', context: 'local' },
};
const cases: ScenarioCase[] = [
  scenario(
    'can-edit',
    'Assemble and download selectors are independent; wildcard grants both.',
    [],
    [
      edit('canEdit', []),
      edit('canExtract', []),
      open('b'),
      edit('canEdit', [], 'b'),
      edit('canExtract', [], 'b'),
      open('c'),
      edit('canEdit', [], 'c'),
      edit('canExtract', [], 'c'),
    ],
  ),
  scenario(
    'rotate-by',
    'Relative quarter/half turns use confirmed current rotations; unknown pages reject before staging.',
    [
      answer(r90),
      answer(rotated([pr(10)], 0, p10(0))),
      answer(rotated([pr(20)], 180, [three[0], page(20, 1, 180, 300, 400), three[2]])),
      answer(
        rotated([pr(30)], 0, [three[0], page(20, 1, 180, 300, 400), page(30, 2, 0, 500, 600)]),
      ),
      answer(rotated([pr(30)], 270, [three[0], page(20, 1, 180, 300, 400), three[2]])),
    ],
    [
      edit('rotateBy', [[pr(10)], 90]),
      readPages,
      edit('rotateBy', [[pr(10)], -90]),
      edit('rotateBy', [[pr(20)], 90]),
      edit('rotateBy', [[pr(30)], 90]),
      edit('rotateBy', [[pr(30)], -90]),
      edit('rotateBy', [[pr(99)], 90]),
      edit('rotateBy', [[pr(99)], -90]),
      readPages,
    ],
  ),
  scenario(
    'rotate-by-follows-the-registry',
    'A remote confirmed layout replaces the registry before the next relative turn.',
    [answer(rotated([pr(10)], 270, p10(270)))],
    [
      emit(fact(rotated([pr(10)], 180, p10(180)), remote)),
      edit('rotateBy', [[pr(10)], 90]),
      readPages,
    ],
  ),
  scenario(
    'passthroughs',
    'Absolute rotation, reorder and delete stage precise current ops, then expose confirmed order/count.',
    [
      answer(
        rotated([pr(10), pr(20)], 180, [page(10, 0, 180), page(20, 1, 180, 300, 400), three[2]]),
      ),
      answer(
        reordered(
          [pr(20), pr(30)],
          [page(20, 0, 180, 300, 400), { ...three[2], index: 1 }, page(10, 2, 180)],
        ),
      ),
      answer(deleted([pr(30)], [page(20, 0, 180, 300, 400), page(10, 1, 180)])),
    ],
    [
      edit('setRotation', [[pr(10), pr(20)], 180]),
      edit('reorder', [[pr(20), pr(30)], 'start']),
      readPages,
      edit('delete', [[pr(30)]]),
      readPages,
      edit('reorder', [[], 'start']),
    ],
  ),
  scenario(
    'refused-by-the-engine',
    'Local permission gates refuse all writes before engine work; an allowed verb preserves engine refusal.',
    [],
    [
      edit('canEdit', []),
      edit('rotateBy', [[pr(10)], 90]),
      edit('setRotation', [[pr(10)], 90]),
      edit('reorder', [[pr(10)], 'end']),
      edit('delete', [[pr(10)]]),
      edit('insertBlank', []),
      copy('insertFromBytes'),
      edit('extract', [[pr(10)]]),
      edit('duplicate', [[pr(10)]]),
      open('b'),
      edit('rotateBy', [[pr(10)], 90], 'b'),
    ],
    { security: { allows: ['doc.render'] } },
  ),
  scenario(
    'deferred-and-aborted',
    'Deferred staged edits are sent in staging order; pages stay confirmed until each event arrives.',
    [
      { path: 'apply', times: 1, defer: 'rotate', events: [fact(r90)] },
      {
        path: 'apply',
        times: 1,
        defer: 'delete',
        events: [fact(deleted([pr(20)], [page(10, 0, 90), { ...three[2], index: 1 }]))],
      },
    ],
    [
      edit('rotateBy', [[pr(10)], 90]),
      edit('delete', [[pr(20)]]),
      readPages,
      settle('rotate', result(r90)),
      readPages,
      settle('delete', result(deleted([pr(20)], [page(10, 0, 90), { ...three[2], index: 1 }]))),
      readPages,
    ],
  ),
  scenario(
    'add-blank',
    'Blank defaults and explicit size/count use end/start or validated neighbour positions.',
    [
      answer(add99),
      answer(
        inserted(
          'pages.insertBlank',
          [98],
          [...three, page(99, 3, 0, 500, 600), page(98, 4, 0, 500, 600)],
        ),
      ),
      answer(
        inserted(
          'pages.insertBlank',
          [97],
          [
            three[0],
            three[1],
            page(97, 2, 0, 300, 400),
            { ...three[2], index: 3 },
            page(99, 4, 0, 500, 600),
            page(98, 5, 0, 500, 600),
          ],
        ),
      ),
      answer(
        inserted(
          'pages.insertBlank',
          [96, 95, 94],
          [
            page(10, 0),
            page(96, 1, 0, 612, 792),
            page(95, 2, 0, 612, 792),
            page(94, 3, 0, 612, 792),
            page(20, 4, 90, 300, 400),
            page(97, 5, 0, 300, 400),
            page(30, 6, 270, 500, 600),
            page(99, 7, 0, 500, 600),
            page(98, 8, 0, 500, 600),
          ],
        ),
      ),
    ],
    [
      edit('insertBlank', []),
      edit('insertBlank', [{}]),
      edit('insertBlank', [{ position: { after: pr(20) } }]),
      edit('insertBlank', [
        { position: { after: pr(10) }, size: { width: 612, height: 792 }, count: 3 },
      ]),
      edit('insertBlank', [{ position: { after: pr(99 + 100) } }]),
      edit('insertBlank', [{ position: { before: pr(199) } }]),
      readPages,
    ],
  ),
  scenario(
    'add-blank-fractional-index',
    'Index positions retired; invalid numeric neighbour arguments now reject not-found without TypeErrors, explicit size does not bypass validation.',
    [],
    [
      ...[1.5, NaN, Infinity].map((n) =>
        edit('insertBlank', [{ position: { before: pr(arg(n)) } }]),
      ),
      edit('insertBlank', [{ position: { before: pr(1.5) }, size: { width: 3, height: 4 } }]),
    ],
  ),
  scenario(
    'insert',
    'Byte insertion retains source bytes and validated neighbour/end position; missing anchors refuse.',
    [
      answer(ins77),
      answer(
        inserted(
          'pages.insert',
          [78],
          [three[0], three[1], page(78, 2), { ...three[2], index: 3 }, page(77, 4, 0, 50, 60)],
        ),
      ),
    ],
    [
      copy('insertFromBytes'),
      copy('insertFromBytes', { $bytes: '' }, { position: { before: pr(30) } }),
      copy('insertFromBytes', undefined, { position: { after: pr(5) } }),
      readPages,
    ],
  ),
  scenario(
    'a-captured-capability-outlives-its-document',
    'Captured capability reads authority after close; staged verbs reject instance-closed while unresolved token lookup is absent.',
    [],
    [
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'page-edit' }, 'a'] },
      close,
      edit('canEdit', []),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 4 }, args: [[pr(20)], 90] },
      { op: 'call', target: { ref: 8 }, args: [] },
    ],
  ),
  scenario(
    'an-empty-registry',
    'Missing pages reject; an empty document inserts Letter blanks at start/end then exposes confirmed new pages.',
    [
      answer(inserted('pages.insertBlank', [99], [page(99, 0, 0, 612, 792)])),
      answer(inserted('pages.insert', [77], [page(77, 0), page(99, 1, 0, 612, 792)])),
    ],
    [
      edit('rotateBy', [[pr(10)], -90]),
      edit('insertBlank', []),
      copy('insertFromBytes', undefined, { position: 'start' }),
      edit('insertBlank', [{ position: { after: pr(10) } }]),
      readPages,
    ],
    { pages: [] },
  ),
  scenario(
    'rotate-by-groups-and-empty',
    'Different rotations group as three ops in one history transaction; empty pages reject.',
    [
      answer(
        rotated([pr(10), pr(40)], 90, [page(10, 0, 90), three[1], three[2], page(40, 3, 90)]),
        rotated([pr(20)], 180, [
          page(10, 0, 90),
          page(20, 1, 180, 300, 400),
          three[2],
          page(40, 3, 90),
        ]),
        rotated([pr(30)], 0, [
          page(10, 0, 90),
          page(20, 1, 180, 300, 400),
          page(30, 2, 0, 500, 600),
          page(40, 3, 90),
        ]),
      ),
    ],
    [
      edit('rotateBy', [[pr(10), pr(20), pr(30), pr(40)], 90]),
      edit('rotateBy', [[], 90]),
      readPages,
    ],
    { pages: [...three, page(40, 3)] },
  ),
  scenario(
    'move-to-the-end',
    'Reorder keeps end and validated ref positions; missing anchor refuses.',
    [
      answer(
        reordered([pr(10)], [{ ...three[1], index: 0 }, { ...three[2], index: 1 }, page(10, 2)]),
      ),
      answer(
        reordered([pr(30)], [{ ...three[1], index: 0 }, page(10, 1), { ...three[2], index: 2 }]),
      ),
      answer(
        reordered([pr(10)], [{ ...three[1], index: 0 }, page(10, 1), { ...three[2], index: 2 }]),
      ),
    ],
    [
      edit('reorder', [[pr(10)], 'end']),
      edit('reorder', [[pr(30)], { after: pr(10) }]),
      edit('reorder', [[pr(10)], { before: pr(30) }]),
      edit('reorder', [[pr(10)], { before: pr(99) }]),
      readPages,
    ],
  ),
  scenario(
    'insert-a-subset-of-another-pdf',
    'Scratch source opens with full scope, extracts selected refs in order and closes on success/refusal.',
    [
      answer(
        inserted(
          'pages.insert',
          [77, 78],
          [
            three[0],
            page(77, 1, 0, 50, 60),
            page(78, 2, 0, 10, 20),
            { ...three[1], index: 3 },
            { ...three[2], index: 4 },
          ],
        ),
      ),
    ],
    [
      copy('insertFromBytes', undefined, { pageIndexes: [2, 0], position: { after: pr(10) } }),
      copy('insertFromBytes', undefined, { pageIndexes: [] }),
      copy('insertFromBytes', undefined, { pageIndexes: [1, 3] }),
      copy('insertFromBytes', { $bytes: 'ERET' }, { pageIndexes: [0] }),
    ],
  ),
  scenario(
    'insert-a-subset-names-the-missing-index',
    'Source ids use the clock; invalid source indexes name their value in not-found and close the scratch handle.',
    [answer(inserted('pages.insert', [77], [...three, page(77, 3, 0, 30, 40)]))],
    [
      { op: 'advance', ms: 1234567890 },
      copy('insertFromBytes', undefined, { pageIndexes: [1] }),
      ...[-1, 1.5, -0.5, Infinity, -Infinity, NaN, 1e20, 1e21, 1e-7].map((n) =>
        copy('insertFromBytes', undefined, { pageIndexes: [arg(n)] }),
      ),
    ],
  ),
  scenario(
    'rotate-by-a-page-without-a-rotation',
    'Malformed undefined rotations retire; valid mixed registry rotations retain separate groups.',
    [
      answer(
        rotated([pr(10)], 90, [page(10, 0, 90), three[1], three[2]]),
        rotated([pr(20)], 180, [page(10, 0, 90), page(20, 1, 180, 300, 400), three[2]]),
        rotated([pr(30)], 0, [
          page(10, 0, 90),
          page(20, 1, 180, 300, 400),
          page(30, 2, 0, 500, 600),
        ]),
      ),
    ],
    [edit('rotateBy', [[pr(10), pr(20), pr(30)], 90]), readPages],
  ),
  scenario(
    'insert-from-another-document',
    'Open source resolves its own refs/indexes and extracts bytes; missing source/page and source download refusal propagate.',
    [
      answer(inserted('pages.insert', [77], [...three, page(77, 3, 90, 300, 400)])),
      answer(
        inserted(
          'pages.insert',
          [78],
          [
            page(78, 0, 90, 300, 400),
            ...three.map((p) => ({ ...p, index: p.index + 1 })),
            page(77, 4, 90, 300, 400),
          ],
        ),
      ),
    ],
    [
      open('b'),
      edit('insertFromDocument', ['b', [pr(20)]]),
      edit('insertFromDocument', ['b', [1], { position: 'start' }]),
      edit('insertFromDocument', ['zzz', [pr(20)]]),
      edit('insertFromDocument', ['b', [pr(99)]]),
      edit('insertFromDocument', ['b', [pr(30)]]),
    ],
  ),
  scenario(
    'duplicate',
    'Duplicate waits for staged work then extracts and inserts after last ref; explicit end, unknown/empty refusals and direct extraction survive.',
    [
      { path: 'pages.extract', value: { $bytes: 'CgsM' } },
      answer(
        inserted(
          'pages.insert',
          [77, 78],
          [three[0], three[1], page(77, 2), page(78, 3, 90, 300, 400), { ...three[2], index: 4 }],
        ),
      ),
      answer(
        inserted(
          'pages.insert',
          [79],
          [
            three[0],
            three[1],
            page(77, 2),
            page(78, 3, 90, 300, 400),
            { ...three[2], index: 4 },
            page(79, 5, 270, 500, 600),
          ],
        ),
      ),
    ],
    [
      edit('duplicate', [[pr(10), pr(20)]]),
      edit('duplicate', [[pr(30)], { position: 'end' }]),
      edit('duplicate', [[pr(99)]]),
      edit('duplicate', [[]]),
      edit('extract', [[pr(10)]]),
      readPages,
    ],
  ),
  {
    name: 'state-and-scope',
    covers:
      'Capability is absent before open; current selection is empty and selecting pages deduplicates resolved refs.',
    plugins: [{ factory: 'page-edit' }],
    engine: { documents: { a: doc() } },
    steps: [
      edit('canEdit', []),
      start,
      edit('canEdit', []),
      open(),
      edit('canEdit', []),
      edit('getSelected', []),
      edit('select', [[0, pr(20), pr(20), pr(99)]]),
      edit('getSelected', []),
      edit('select', [[]]),
      edit('getSelected', []),
    ],
  },
];
// Each document and scratch answer uses the existing current fake/engine ports.
const configured = cases.map((c) => {
  let engine = c.engine!;
  if (c.name === 'can-edit')
    engine = {
      ...engine,
      documents: {
        ...engine.documents,
        b: doc([], { security: { allows: ['doc.download'] } }),
        c: doc([], { security: { allows: '*' } }),
      },
    };
  if (c.name === 'refused-by-the-engine')
    engine = {
      ...engine,
      documents: {
        ...engine.documents,
        b: doc([
          {
            path: 'apply',
            error: {
              name: 'EngineError',
              message: 'write refused',
              code: 'Forbidden',
              details: { required: 'doc.pages.assemble', context: 'local' },
            },
          },
        ]),
      },
    };
  if (c.name.startsWith('insert-a-subset')) {
    engine = {
      ...engine,
      defaultDocument: 'src',
      documents: {
        ...engine.documents,
        src: doc([{ path: 'pages.extract', value: { $bytes: 'BAUG' } }], {
          pages: [page(5, 0, 0, 10, 20), page(6, 1, 0, 30, 40), page(7, 2, 0, 50, 60)],
        }),
      },
      responses:
        c.name === 'insert-a-subset-names-the-missing-index'
          ? []
          : [
              {
                path: 'open',
                args: [
                  { kind: 'bytes', id: 'page-edit-source-0', bytes: { $bytes: 'ERET' } },
                  { scope: ['*'] },
                ],
                error: {
                  name: 'EngineError',
                  message: 'source open refused',
                  code: 'Forbidden',
                  details: { required: 'doc.open', context: 'local' },
                },
              },
            ],
    };
  }
  if (c.name === 'insert-from-another-document')
    engine = {
      ...engine,
      documents: {
        ...engine.documents,
        b: doc([
          { path: 'pages.extract', args: [[pr(20)]], value: { $bytes: 'BwgJ' } },
          { path: 'pages.extract', error: denied },
        ]),
      },
    };
  return bindTransactions({ ...c, engine });
});
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/page-edit.json',
    describes: 'Current validated page edits, staged transactions and confirmed registry layouts.',
    cases: configured,
  },
];
