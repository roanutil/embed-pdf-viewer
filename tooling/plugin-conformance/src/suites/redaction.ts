/** Current redaction marks, annotation ownership, destructive applies and pure transitions. */
import type {
  Annotation,
  AnnotationRef,
  PageLayout,
  RedactionApplyScope,
} from '@embedpdf/engine-core/runtime';
import * as model from '../../../../packages/plugin/redaction/src/model';
import { markOf } from '../../../../packages/plugin/redaction/src/read/pending';
import type { FunctionCase, Suite } from '../vectors';
import type { ScenarioCase, Step } from '../scenario';
import {
  rd,
  PLUGINS,
  WITH_SELECTION,
  SNAP,
  docOf,
  doc,
  hydrate,
  square,
  textNote,
  markup,
  redact,
  objRef,
  baseRef,
  pr,
  quad,
  layout,
  boot,
  open,
  call as an,
  createdFact,
  created,
  updated,
  deleted,
  write,
  refused,
  confirmation,
  result,
  applyAnswer,
  applied,
  reload,
  REMOTE,
  LOCAL,
  OVERLAY,
  APPLY_CAPS,
  READS,
  load,
  sel,
  ix,
  sample,
  range,
  segment,
  hit,
  settle,
} from './redaction-fixtures';
const areaMark = redact(10, {
  rect: { x: 90, y: 90, width: 120, height: 120 },
  overlayText: 'CONFIDENTIAL',
});
const textMark = redact(11, {
  rect: { x: 50, y: 55, width: 150, height: 35 },
  quadPoints: [quad(50, 55, 150, 15), quad(50, 75, 60, 15)],
});
const secondMark = redact(12, {
  ref: baseRef(4, 2),
  page: pr(2),
  rect: { x: 10, y: 460, width: 40, height: 40 },
  fontFamily: 'courier',
  fontSize: 9,
});
const seed: Annotation[] = [
  square(1),
  textNote(2, { rect: { x: 205, y: 130, width: 20, height: 20 } }),
  markup('highlight', 3, {
    rect: { x: 60, y: 78, width: 30, height: 10 },
    quadPoints: [quad(60, 78, 30, 10)],
  }),
  areaMark,
  textMark,
  square(4, { rect: { x: 400, y: 600, width: 100, height: 100 } }),
  secondMark,
  square(5, { ref: objRef(5, 2), page: pr(2), rect: { x: 50, y: 460, width: 30, height: 40 } }),
];
const refsScope: RedactionApplyScope = { annotations: [objRef(10), baseRef(4, 2)] };
const pagesScope: RedactionApplyScope = { pages: [pr(1), pr(2)] };
const answer = result(pagesScope);
const scenario = (
  name: string,
  covers: string,
  steps: Step[],
  responses: Parameters<typeof docOf>[0] = [hydrate(seed)],
  over: Partial<Omit<ScenarioCase, 'snapshot' | 'reason'>> = {},
): ScenarioCase => ({
  name,
  covers,
  plugins: PLUGINS,
  engine: { documents: { a: docOf(responses) } },
  snapshot: SNAP,
  steps: [...boot, ...steps],
  ...over,
});
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
const ref = (n: number): Step => ({ op: 'call', target: { ref: n } });
const fresh = (
  n: number,
  rect: Extract<Annotation, { subtype: 'redact' }>['rect'],
  pon = 1,
  extra: Partial<Extract<Annotation, { subtype: 'redact' }>> = {},
) =>
  redact(n, {
    ref: objRef(n, pon),
    page: pr(pon),
    rect,
    color: '#e44234',
    textAlign: 'left',
    ...extra,
  });
const selection1 = fresh(10001, { x: 50, y: 50, width: 66, height: 32 }, 1, {
  quadPoints: [quad(50, 50, 66, 12), quad(50, 70, 66, 12)],
});
const selection2 = fresh(10002, { x: 30, y: 20, width: 24, height: 12 }, 2, {
  quadPoints: [quad(30, 20, 24, 12)],
});
const overlayFields = {
  interiorColor: '#ff0000',
  fontColor: '#00ff00',
  fontFamily: 'courier' as const,
  fontSize: 0,
};
const cases: ScenarioCase[] = [
  scenario(
    'initial-state-and-twins',
    'A fresh slice is idle. Marking reads annotation create authority; apply requires the redaction service and all three engine grants.',
    [
      ...['b', 'c', 'd', 'e', 'f'].map((id) => open(id)),
      rd('isApplying'),
      rd('getLastResult'),
      rd('canMark'),
      rd('canApply'),
      rd('canMark', [], 'b'),
      rd('canApply', [], 'b'),
      rd('canMark', [], 'c'),
      rd('canApply', [], 'c'),
      rd('canApply', [], 'd'),
      rd('canApply', [], 'e'),
      rd('canMark', [], 'f'),
      rd('canApply', [], 'f'),
    ],
    [],
    {
      engine: {
        documents: {
          a: docOf(),
          b: docOf([hydrate()], { security: { allows: ['doc.annotate.read', ...APPLY_CAPS] } }),
          c: docOf([hydrate()], {
            security: {
              allows: ['doc.annotate.read', 'doc.annotate.create', 'doc.redact', 'doc.pages.modify'],
            },
          }),
          d: docOf([hydrate()], {
            security: { allows: ['doc.annotate.read', 'doc.redact', 'doc.annotate.modify'] },
          }),
          e: docOf([hydrate()], {
            security: { allows: ['doc.annotate.read', 'doc.pages.modify', 'doc.annotate.modify'] },
          }),
          f: docOf(),
        },
      },
    },
  ),
  scenario(
    'mark-the-text-selection',
    'The current selection creates a mark per selected page from page-space segments, clears selection, and reads the confirmed marks.',
    [
      ...load,
      sel('select', [range([1, 0], [2, 4])]),
      rd('markSelection'),
      sel('hasSelection'),
      rd('listPending'),
      rd('getPendingCount'),
      rd('markSelection'),
    ],
    [
      hydrate(),
      ...READS,
      write(1, [{ item: created(selection1) }]),
      write(2, [{ item: created(selection2) }]),
    ],
    { plugins: WITH_SELECTION },
  ),
  scenario(
    'marking-needs-create-authority',
    'Marking rejects permission-denied without create authority and leaves the text selection intact.',
    [
      ...load,
      sel('select', [range([1, 0], [1, 4])]),
      rd('canMark'),
      rd('markSelection'),
      sel('hasSelection'),
    ],
    [hydrate(), ...READS],
    {
      plugins: WITH_SELECTION,
      engine: {
        documents: {
          a: docOf([hydrate(), ...READS], {
            security: { allows: ['doc.annotate.read', 'doc.text.select', ...APPLY_CAPS] },
          }),
        },
      },
    },
  ),
  scenario(
    'the-redact-tool',
    'The actual interaction tool activates redact, draws an area through annotation createPointer, and switches to pointer and back.',
    [
      ix('getActiveToolId'),
      ix('activateTool', ['redact']),
      ix('getActiveToolId'),
      an('createPointer', ['redact', 'down', pr(1), { x: 300, y: 300 }]),
      an('createPointer', ['redact', 'move', pr(1), { x: 320, y: 280 }]),
      an('createPointer', ['redact', 'move', pr(1), { x: 400, y: 200 }]),
      an('createPointer', ['redact', 'up', pr(1), { x: 400, y: 200 }]),
      rd('listPending'),
      ix('activateDefaultTool'),
      ix('getActiveToolId'),
      ix('activateTool', ['redact']),
      ix('getActiveToolId'),
    ],
    [
      hydrate(),
      write(1, [{ item: created(fresh(10001, { x: 300, y: 200, width: 100, height: 100 })) }]),
    ],
    { plugins: WITH_SELECTION },
  ),
  scenario(
    'without-a-selection-plugin',
    'Without selection the composed redact tool is absent: activating it rejects not-found; markSelection rejects unsupported.',
    [
      ix('getActiveToolId'),
      ix('activateTool', ['redact']),
      ix('getActiveToolId'),
      ix('activateDefaultTool'),
      rd('markSelection'),
    ],
    [hydrate()],
  ),
  scenario(
    'pending-projection',
    'Pending is a synchronous projection of current loaded annotations, in page order, with current refs and page indices.',
    [an('getStatus'), rd('listPending'), rd('getPendingCount')],
  ),
  scenario(
    'estimate-collateral',
    'Positive area overlap uses quad boxes in preference to the mark rect and counts each non-mark once.',
    [
      rd('estimateCollateral'),
      rd('estimateCollateral', [[objRef(10)]]),
      rd('estimateCollateral', [[objRef(11)]]),
      rd('estimateCollateral', [[baseRef(4, 2)]]),
      rd('estimateCollateral', [[]]),
      rd('estimateCollateral', [[objRef(1), objRef(99)]]),
    ],
  ),
  scenario(
    'apply-named-marks',
    'Apply filters explicit current refs. Busy begins before settlement; the confirmed event records the result and callback, and unsubscribe removes its listener.',
    [
      rd('onApplied', [{ $fn: 'applied' }]),
      rd('apply', [[objRef(10), baseRef(4, 2), objRef(1), objRef(99)]]),
      rd('isApplying'),
      settle('apply', result(refsScope)),
      rd('isApplying'),
      rd('getLastResult'),
      ref(0),
      ref(0),
      rd('apply', [[objRef(11)]]),
    ],
    [
      { ...hydrate(seed), times: 1 },
      applyAnswer(result(refsScope), { defer: 'apply' }),
      reload([textMark]),
      applyAnswer(result({ annotations: [objRef(11)] }, 0, 'redaction-2')),
      reload([], [pr(1)]),
    ],
  ),
  scenario(
    'apply-all-pages',
    'ApplyAll sends every current page, including unloaded pages, with fact provenance and both listeners.',
    [
      rd('onApplied', [{ $fn: 'first' }]),
      rd('onApplied', [{ $fn: 'second' }]),
      rd('applyAll'),
      rd('getLastResult'),
      rd('listPending'),
    ],
    [{ ...hydrate(seed), times: 1 }, applyAnswer(result(pagesScope, 3)), reload([seed[5]!])],
  ),
  scenario(
    'apply-refused',
    'A refused second apply clears busy, preserves the previous confirmed result and pending annotations, and publishes no false applied fact.',
    [
      rd('onApplied', [{ $fn: 'applied' }]),
      rd('applyAll'),
      rd('apply', [[objRef(10)]]),
      rd('getLastResult'),
      rd('isApplying'),
      rd('listPending'),
    ],
    [
      { ...hydrate(seed), times: 1 },
      applyAnswer(result(pagesScope, 1)),
      reload([areaMark]),
      {
        path: 'redaction.apply',
        error: { name: 'EngineError', message: 'permission denied', code: 'Refused' },
      },
    ],
  ),
  scenario(
    'apply-refusals-before-the-engine',
    'Empty and unknown refs reject not-found; an empty document rejects not-ready; the required redaction service can refuse with EngineError Unsupported, then recover. Historical absent-service semantics are archived.',
    [
      open('b'),
      open('c'),
      rd('apply', [[]]),
      rd('apply', [[objRef(99)]]),
      rd('applyAll', [], 'b'),
      rd('getPendingCount', [], 'b'),
      an('getStatus', [], 'b'),
      rd('apply', [[objRef(10)]], 'c'),
      rd('getPendingCount', [], 'c'),
      rd('isApplying', [], 'c'),
      rd('applyAll', [], 'c'),
      rd('getLastResult', [], 'c'),
    ],
    [],
    {
      engine: {
        documents: {
          a: docOf([hydrate(seed)]),
          b: docOf([hydrate()], { pages: [] }),
          c: docOf([
            { ...hydrate(seed), times: 1 },
            {
              path: 'redaction.apply',
              times: 1,
              error: { name: 'EngineError', message: 'redaction unsupported', code: 'Unsupported' },
            },
            applyAnswer(result(pagesScope, 0, 'recovered-apply')),
            reload([]),
          ]),
        },
      },
    },
  ),
  scenario(
    'remote-applies-arrive-as-events',
    'Both local and remote confirmed redaction events update lastResult; unrelated events do not. Closing releases listeners.',
    [
      rd('onApplied', [{ $fn: 'applied' }]),
      { op: 'emit', doc: 'a', event: applied(answer) },
      rd('getLastResult'),
      { op: 'emit', doc: 'a', event: createdFact(square(70)) },
      rd('getLastResult'),
      { op: 'emit', doc: 'a', event: applied(result(pagesScope, 4, 'remote-apply'), REMOTE) },
      rd('getLastResult'),
      rd('listPending'),
      close,
      { op: 'emit', doc: 'a', event: applied(answer, REMOTE) },
    ],
    [{ ...hydrate(seed), times: 1 }, reload([])],
  ),
  scenario(
    'a-throwing-listener-is-isolated',
    'A throwing onApplied callback reaches the kernel report; the next callback still runs and the apply succeeds.',
    [
      rd('onApplied', [{ $fn: 'bad', throws: { name: 'Error', message: 'listener broke' } }]),
      rd('onApplied', [{ $fn: 'good' }]),
      rd('applyAll'),
    ],
    [{ ...hydrate(seed), times: 1 }, applyAnswer(answer), reload([])],
  ),
  scenario(
    'set-label',
    'updateLabel writes explicit patch members and preserves the current font styling; non-mark and missing refs reject.',
    [
      rd('updateLabel', [objRef(10), { overlayText: 'SECRET' }]),
      rd('listPending'),
      rd('updateLabel', [objRef(10), { overlayText: null, repeat: true }]),
      rd('updateLabel', [objRef(10), {}]),
      rd('updateLabel', [baseRef(4, 2), { overlayText: 'B', repeat: false }]),
      rd('updateLabel', [objRef(1), { overlayText: 'x' }]),
      rd('updateLabel', [objRef(77), { overlayText: 'x' }]),
    ],
    [
      hydrate(seed),
      write(1, [{ item: updated({ ...areaMark, overlayText: 'SECRET' }) }]),
      write(2, [{ item: updated({ ...areaMark, overlayText: null, repeat: true }) }]),
      write(3, [{ item: updated({ ...areaMark, overlayText: null, repeat: true }) }]),
      write(4, [{ item: updated({ ...secondMark, overlayText: 'B', repeat: false }) }]),
    ],
  ),
  scenario(
    'mark-area-and-page',
    'Page-space rectangles and full visible page sizes take configured overlay defaults. Missing pages and absent create authority reject; pending events omit non-mark creations.',
    [
      open('b'),
      rd('onPendingChanged', [{ $fn: 'pending' }]),
      rd('markArea', [pr(1), { x: 10, y: 20, width: 100, height: 50 }]),
      rd('markPage', [pr(2)]),
      an('create', [pr(1), { subtype: 'square', box: { x: 300, y: 300, width: 50, height: 50 } }]),
      rd('markPage', [pr(9)]),
      rd('listPending'),
      rd('canMark', [], 'b'),
      rd('markArea', [pr(1), { x: 0, y: 0, width: 10, height: 10 }], 'b'),
      rd('markPage', [pr(1)], 'b'),
    ],
    [],
    {
      plugins: [
        { factory: 'interaction' },
        { factory: 'annotation-live' },
        { factory: 'redaction-current', config: OVERLAY },
      ],
      engine: {
        documents: {
          a: docOf([
            hydrate(),
            write(1, [
              {
                item: created(
                  fresh(10001, { x: 10, y: 20, width: 100, height: 50 }, 1, overlayFields),
                ),
              },
            ]),
            write(2, [
              {
                item: created(
                  fresh(10002, { x: 0, y: 0, width: 400, height: 500 }, 2, overlayFields),
                ),
              },
            ]),
            write(3, [
              {
                item: created(
                  square(10003, {
                    rect: { x: 300, y: 300, width: 50, height: 50 },
                    box: { x: 300, y: 300, width: 50, height: 50 },
                  }),
                ),
              },
            ]),
          ]),
          b: docOf([hydrate()], { security: { allows: ['doc.annotate.read', ...APPLY_CAPS] } }),
        },
      },
    },
  ),
  scenario(
    'an-overlay-without-values-adds-nothing',
    'A partial empty overlay is merged with current settings defaults; marking still receives the default fill and text styling.',
    [
      rd('listPending'),
      rd('markArea', [pr(1), { x: 10, y: 20, width: 100, height: 50 }]),
      rd('getPendingCount'),
    ],
    [
      hydrate(),
      write(1, [{ item: created(fresh(10001, { x: 10, y: 20, width: 100, height: 50 })) }]),
    ],
    {
      plugins: [
        { factory: 'interaction' },
        { factory: 'annotation-live' },
        { factory: 'redaction-current', config: { overlay: { text: {} } } },
      ],
    },
  ),
  scenario(
    'mark-search-matches',
    'Actual search results yield one mark per segmented hit in hit order; the page filter selects only page two.',
    [
      rd('markMatches', [{ text: 'the' }]),
      rd('markMatches', [{ text: 'the' }, { pages: [pr(2)] }]),
      rd('listPending'),
    ],
    [
      hydrate(),
      {
        path: 'search.query',
        value: {
          matches: [
            hit(1, 0, [segment(50, 50, 66, 12), segment(50, 70, 30, 12)]),
            hit(1, 20, []),
            hit(2, 3, [segment(30, 20, 24, 12)]),
          ],
          nextCursor: null,
          pagesSearched: 2,
          pageCount: 2,
        },
      },
      write(1, [
        {
          item: created(
            fresh(10001, { x: 50, y: 50, width: 66, height: 32 }, 1, {
              quadPoints: [quad(50, 50, 66, 12), quad(50, 70, 30, 12)],
            }),
          ),
        },
      ]),
      write(2, [
        {
          item: created(
            fresh(10002, { x: 30, y: 20, width: 24, height: 12 }, 2, {
              quadPoints: [quad(30, 20, 24, 12)],
            }),
          ),
        },
      ]),
      write(3, [
        {
          item: created(
            fresh(10003, { x: 30, y: 20, width: 24, height: 12 }, 2, {
              quadPoints: [quad(30, 20, 24, 12)],
            }),
          ),
        },
      ]),
    ],
    {
      plugins: [
        { factory: 'interaction' },
        { factory: 'annotation-live' },
        { factory: 'redaction-search' },
        { factory: 'redaction-current' },
      ],
    },
  ),
  scenario(
    'marking-refusals',
    'Missing search or selection rejects unsupported only after authority passes; denied create rejects permission-denied.',
    [
      open('b'),
      rd('markMatches', [{ text: 'x' }]),
      rd('markMatches', [{ text: 'x' }], 'b'),
      rd('markSelection'),
      rd('markSelection', [], 'b'),
    ],
    [],
    {
      engine: {
        documents: {
          a: docOf(),
          b: docOf([hydrate()], { security: { allows: ['doc.annotate.read'] } }),
        },
      },
    },
  ),
  scenario(
    'unmark-and-clear',
    'Unmark applies pending refs, skips others, and reports per-mark refused deletes while retaining the refused mark. Confirmed delete events publish pending page changes.',
    [
      rd('onPendingChanged', [{ $fn: 'pending' }]),
      rd('unmark', [[objRef(10), objRef(1), objRef(99)]]),
      rd('unmark', [[objRef(1)]]),
      rd('clearPending'),
      rd('listPending'),
    ],
    [
      hydrate(seed),
      write(1, [{ item: deleted(objRef(10)), changed: [objRef(10)] }]),
      refused('locked'),
      write(3, [{ item: deleted(baseRef(4, 2)), changed: [baseRef(4, 2)] }]),
    ],
  ),
  scenario(
    'apply-pages',
    'Empty pages reject invalid-input; current page refs resolve to a pages scope and lastResult reflects the engine fact.',
    [rd('applyPages', [[]]), rd('isApplying'), rd('applyPages', [[pr(2)]]), rd('getLastResult')],
    [
      { ...hydrate(seed), times: 1 },
      applyAnswer(result({ pages: [pr(2)] }, 1)),
      reload([], [pr(2)]),
    ],
  ),
  scenario(
    'pending-reads',
    'Current objectNumber and baseIndex refs resolve pending marks; page filters are stable, absent pages are empty, and page counts agree.',
    [
      rd('getPending', [objRef(10)]),
      rd('getPending', [baseRef(4, 2)]),
      rd('getPending', [objRef(1)]),
      rd('listPending', [{ page: pr(2) }]),
      rd('listPending', [{ page: pr(2) }]),
      rd('listPending', [{ page: pr(9) }]),
      rd('listPending', [{}]),
      rd('listPending'),
      rd('getPendingCount', [pr(1)]),
      rd('getPendingCount', [pr(9)]),
      rd('getPendingCount'),
    ],
  ),
  scenario(
    'update-label-refused',
    'An engine label refusal rejects and rolls back. A later successful edit carries the unchanged overlay label and fires pending changed.',
    [
      rd('onPendingChanged', [{ $fn: 'pending' }]),
      rd('updateLabel', [objRef(10), { overlayText: 'X' }]),
      rd('getPending', [objRef(10)]),
      rd('updateLabel', [objRef(10), { repeat: true }]),
      rd('getPending', [objRef(10)]),
    ],
    [
      hydrate(seed),
      refused('locked'),
      write(2, [{ item: updated({ ...areaMark, repeat: true }) }]),
    ],
  ),
  scenario(
    'mark-ownership-and-flags',
    'Self-scoped authority allows the owned mark only; foreign, unstamped and locked marks refuse without dispatch. Owned update and removal publish facts.',
    [
      rd('canUpdateLabel', [objRef(60)]),
      rd('canUnmark', [objRef(60)]),
      rd('canUpdateLabel', [objRef(61)]),
      rd('canUnmark', [objRef(61)]),
      rd('canUpdateLabel', [objRef(62)]),
      rd('canUnmark', [objRef(63)]),
      rd('updateLabel', [objRef(61), { overlayText: 'forbidden' }]),
      rd('updateLabel', [objRef(60), { overlayText: 'mine' }]),
      rd('unmark', [[objRef(60), objRef(61), objRef(62), objRef(63)]]),
      rd('listPending'),
    ],
    [
      hydrate([
        redact(60, { userId: 'me' }),
        redact(61, { userId: 'other' }),
        redact(62),
        redact(63, { userId: 'me', locked: true }),
      ]),
      write(1, [{ item: updated(redact(60, { userId: 'me', overlayText: 'mine' })) }]),
      write(2, [{ item: deleted(objRef(60)), changed: [objRef(60)] }]),
    ],
    {
      engine: {
        documents: {
          a: docOf(
            [
              hydrate([
                redact(60, { userId: 'me' }),
                redact(61, { userId: 'other' }),
                redact(62),
                redact(63, { userId: 'me', locked: true }),
              ]),
              write(1, [{ item: updated(redact(60, { userId: 'me', overlayText: 'mine' })) }]),
              write(2, [{ item: deleted(objRef(60)), changed: [objRef(60)] }]),
            ],
            {
              security: {
                rawScope: [
                  'doc.annotate.read',
                  'doc.annotate.modify',
                  'annotations:update:self',
                  'annotations:delete:self',
                ],
                identity: { userId: 'me' },
              },
            },
          ),
        },
      },
    },
  ),
  scenario(
    'apply-queue-and-unchanged-page',
    'Queued applies compute refs when their turn begins. Each confirmed single-page apply removes its mark while page two survives unchanged.',
    [
      rd('onApplied', [{ $fn: 'applied' }]),
      rd('apply', [[objRef(10)]]),
      rd('apply', [[objRef(11)]]),
      rd('isApplying'),
      settle('first', result({ annotations: [objRef(10)] }, 1, 'first')),
      rd('listPending'),
      rd('getLastResult'),
      rd('isApplying'),
    ],
    [
      { ...hydrate(seed), times: 1 },
      applyAnswer(result({ annotations: [objRef(10)] }, 1, 'first'), { defer: 'first' }),
      { ...reload([textMark], [pr(1)]), times: 1 },
      applyAnswer(result({ annotations: [objRef(11)] }, 0, 'second')),
      reload([], [pr(1)]),
    ],
  ),
  scenario(
    'close-during-apply',
    'Closing a document cancels its pending apply and releases the event listener; a later engine settlement cannot publish an applied callback into the closed instance.',
    [
      rd('onApplied', [{ $fn: 'applied' }]),
      rd('applyAll'),
      rd('isApplying'),
      close,
      settle('closing', answer),
      open('b'),
      rd('isApplying', [], 'b'),
      rd('getLastResult', [], 'b'),
    ],
    [],
    {
      engine: {
        documents: {
          a: docOf([hydrate(seed), applyAnswer(answer, { defer: 'closing' })]),
          b: docOf(),
        },
      },
    },
  ),
];

import { pending } from './redaction-read-inputs';

const functions = {
  initialRedactionState: model.initialRedactionState,
  startApply: model.startApply,
  finishApply: model.finishApply,
  setLastResult: model.setLastResult,
  markOf,
  estimateCollateral: (
    pages: readonly PageLayout[] | null,
    records: readonly Annotation[],
    refs?: readonly AnnotationRef[],
  ) => pending(pages, records).api.estimateCollateral(refs),
};
const initial = model.initialRedactionState(),
  busy = { applying: true, lastResult: null },
  done = { applying: false, lastResult: answer };
const f = <K extends keyof typeof functions>(
  name: string,
  fn: K,
  args: Parameters<(typeof functions)[K]>,
  covers?: string,
): FunctionCase => ({ name, fn, args, ...(covers ? { covers } : {}) });
const box = (x: number, y: number, width: number, height: number) => ({ x, y, width, height });
const m = (n: number, over: Partial<Extract<Annotation, { subtype: 'redact' }>> = {}) =>
  redact(n, { rect: box(100, 600, 100, 100), ...over });
const other = (
  n: number,
  rect: Annotation['rect'],
  over: Partial<Extract<Annotation, { subtype: 'square' }>> = {},
) => square(n, { rect, ...over });
const c = (
  name: string,
  records: Annotation[],
  refs?: AnnotationRef[],
  pages: PageLayout[] | null = [layout(0, 1)],
  covers?: string,
) =>
  f(
    name,
    'estimateCollateral',
    refs === undefined ? [pages, records] : [pages, records, refs],
    covers,
  );
const functionCases: FunctionCase[] = [
  f('initial-state', 'initialRedactionState', []),
  f('started-from-idle', 'startApply', [initial]),
  f('started-keeps-the-last-result', 'startApply', [done]),
  f(
    'finished-with-a-result',
    'finishApply',
    [busy],
    'finishApply clears busy; setLastResult is separately exercised.',
  ),
  f('finished-replaces-the-result', 'setLastResult', [
    { applying: false, lastResult: answer },
    result(pagesScope, 9),
  ]),
  f('finished-with-null-keeps-the-result', 'finishApply', [{ applying: true, lastResult: answer }]),
  f('finished-without-a-result-member', 'finishApply', [{ applying: true, lastResult: answer }]),
  f('finished-when-idle', 'finishApply', [initial]),
  f(
    'already-started-is-stable',
    'startApply',
    [busy],
    'Replaces removed unknown-action dispatcher behavior with current transition identity.',
  ),
  c('no-document', [m(1), other(2, box(100, 600, 100, 100))], undefined, null),
  c('no-pages', [m(1)], undefined, []),
  c('no-marks', [other(1, box(0, 0, 10, 10)), other(2, box(0, 0, 10, 10))]),
  c('rect-mark-overlaps', [
    m(1),
    other(2, box(150, 650, 100, 100)),
    other(3, box(300, 300, 100, 100)),
  ]),
  c('touching-edges-do-not-count', [
    m(1),
    other(2, box(200, 600, 100, 100)),
    other(3, box(100, 700, 100, 100)),
    other(4, box(0, 500, 100, 100)),
  ]),
  c('contained-and-containing', [
    m(1),
    other(2, box(120, 620, 10, 10)),
    other(3, box(0, 0, 600, 800)),
  ]),
  c('sliver-overlap', [
    m(1),
    other(2, box(199.999, 600, 100.001, 100)),
    other(3, box(100, 699.5, 100, 1.5)),
  ]),
  c('quads-replace-the-rect', [
    m(1, {
      rect: box(0, 0, 600, 800),
      quadPoints: [quad(100, 100, 50, 10), quad(300, 100, 50, 10)],
    }),
    other(2, box(120, 105, 10, 3)),
    other(3, box(200, 100, 50, 10)),
    other(4, box(340, 90, 20, 30)),
  ]),
  c('tilted-quad-box', [
    m(1, {
      quadPoints: [
        {
          upperLeft: { x: 150, y: 100 },
          upperRight: { x: 200, y: 150 },
          lowerLeft: { x: 100, y: 150 },
          lowerRight: { x: 150, y: 200 },
        },
      ],
    }),
    other(2, box(195, 195, 15, 15)),
    other(3, box(100, 100, 1, 1)),
    other(4, box(201, 150, 9, 10)),
  ]),
  c('one-annotation-counts-once', [
    m(1),
    m(2, { rect: box(150, 650, 10, 10) }),
    other(3, box(140, 640, 30, 30)),
  ]),
  c('marks-are-never-collateral', [m(1), m(2, { rect: box(150, 650, 100, 100) })]),
  c(
    'filter-by-id',
    [
      m(1),
      m(2, { rect: box(300, 300, 100, 100) }),
      other(3, box(150, 650, 10, 10)),
      other(4, box(350, 350, 10, 10)),
    ],
    [objRef(2)],
  ),
  c('filter-matches-nothing', [m(1), other(3, box(150, 650, 10, 10))], [objRef(3), baseRef(9, 2)]),
  c('empty-filter', [m(1), other(3, box(150, 650, 10, 10))], []),
  c(
    'null-filter-is-all',
    [m(1), other(3, box(150, 650, 10, 10))],
    undefined,
    undefined,
    'The current optional refs contract records omitted refs; null is outside the public contract.',
  ),
  c(
    'filter-by-nm-and-index',
    [
      m(1, { ref: baseRef(1) }),
      m(2, { ref: baseRef(4), rect: box(300, 300, 100, 100) }),
      other(3, box(150, 650, 10, 10)),
      other(4, box(350, 350, 10, 10)),
    ],
    [baseRef(4)],
    undefined,
    'Current baseIndex refs replace removed nm/index revision refs; exact originals remain archived.',
  ),
  c(
    'pages-in-document-order',
    [
      m(1),
      other(2, box(150, 650, 10, 10)),
      m(3, { page: pr(2), ref: objRef(3, 2) }),
      other(4, box(150, 650, 10, 10), { page: pr(2), ref: objRef(4, 2) }),
      other(5, box(0, 0, 1, 1), { page: pr(2), ref: objRef(5, 2) }),
      m(6, { page: pr(3), ref: objRef(6, 3) }),
      other(7, box(150, 650, 10, 10), { page: pr(3), ref: objRef(7, 3) }),
    ],
    undefined,
    [layout(0, 2), layout(1, 1)],
  ),
  c('nan-geometry-never-intersects', [
    m(1),
    other(2, box(NaN, 600, 100, 100)),
    m(3, { quadPoints: [{ ...quad(100, 600, 10, 10), upperLeft: { x: NaN, y: 610 } }] }),
    other(4, box(100, 600, 5, 5)),
  ]),
  c('negative-and-fractional', [
    m(1, { rect: box(-50.5, -50.5, 40.25, 40.25) }),
    other(2, box(-20, -20, 20, 20)),
    other(3, box(-10.25, -60, 10.25, 60)),
  ]),
  f('set-first-result', 'setLastResult', [initial, answer]),
  f('set-same-result-is-stable', 'setLastResult', [done, answer]),
  f('mark-of-text', 'markOf', [textMark, 0]),
  f('mark-of-area', 'markOf', [areaMark, 0]),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/redaction.json',
    describes:
      'Current redaction selection/search/area marks, pending ownership and apply lifecycle.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/redaction-functions.json',
    describes:
      'Current named session transitions, markOf and pending collateral over explicit current read inputs.',
    functions,
    cases: functionCases,
  },
];
