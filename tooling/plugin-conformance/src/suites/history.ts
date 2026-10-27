/**
 * `packages/plugin/history`. Replayed by
 * `crates/plugins/history/tests/conformance.rs`.
 *
 * The history is document-scoped and records what the document's change
 * queue stages, so every case opens document `a` with two plugins: the
 * history and `history-actor`, a sibling that stages changes on the same
 * queue as a feature plugin would (`stage`, a held change through `hold`,
 * `set`, `send`), and lists what the queue still holds (`pending`).
 *
 * The engine answers nothing by itself: the n-th `apply` call waits under
 * the label `c<n>` until a `settle` step answers it, so each case decides
 * when, and how, the engine answered the change, its undo and its redo. The
 * getters (`canUndo`, `canRedo`, `getUndoLabel`, `getRedoLabel`) are read
 * after the answer they depend on, and `onUndone`/`onUndoFailed` are scripted
 * listeners, so the events land in the trace with the answer that caused
 * them. The `history::a` slice is snapshotted after every step.
 */
import { definePlugin, type HeldChange } from '@embedpdf/core';
import type { ChangeOp } from '@embedpdf/engine-core/runtime';

import {
  HistoryToken,
  historyPlugin,
  type HistoryConfig,
} from '../../../../packages/plugin/history/src/index';

import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
import type { ErrorSpec, FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';

/** Change op ids come from one deterministic UUID sequence per case, as the
 *  Rust runner's (`install_case_entropy`), so states and engine calls that
 *  name them compare. */
registerPlugin('history', {
  factory: (config) => {
    installCaseEntropy();
    return historyPlugin(config as HistoryConfig | undefined);
  },
  tokens: [HistoryToken],
});
onCaseEnd(restoreCaseEntropy);

const page = { kind: 'objectNumber', objectNumber: 1 } as const;

/** Move the square `objectNumber` to `x`: the one op every change here makes. */
const move = (objectNumber: number, x: number): ChangeOp =>
  ({
    type: 'annotations.update',
    ref: { kind: 'objectNumber', page, objectNumber },
    patch: { subtype: 'square', box: { x, y: 0, width: 10, height: 10 } },
  }) as ChangeOp;

/**
 * The feature plugin the history records: it stages on `ctx.changes` under its
 * own capability. A held change is named by the index `hold` returns.
 */
registerPlugin('history-actor', {
  factory: (_config, env) =>
    definePlugin({
      id: 'history-actor',
      token: env.token('history-actor'),
      scope: 'document',
      create: (ctx) => {
        const holds: HeldChange[] = [];
        return {
          api: {
            /** One action: `objectNumber` moves to `x`; undoing it moves it to 0. */
            stage: (key: string, objectNumber: number, x: number) =>
              ctx.changes.stage({
                label: { key },
                ops: [move(objectNumber, x)],
                undo: [move(objectNumber, 0)],
              }).opId,
            /** A change that is not a step of the history. */
            stageUnrecorded: (key: string) =>
              ctx.changes.stage({ label: { key }, ops: [move(10, 1)], history: false }).opId,
            /** An undo staged by someone else than the history. */
            stageUndoOf: (key: string, undoOf: string) =>
              ctx.changes.stage({ label: { key }, undoOf, shows: [] }).opId,
            hold: (key: string, merge: string | null) =>
              holds.push(ctx.changes.hold({ key }, merge === null ? {} : { merge })) - 1,
            /** The hold's ops from now on: `x`, and undoing it shows `x - 1`. */
            set: (hold: number, x: number) => holds[hold]!.set([move(10, x)], [move(10, x - 1)]),
            send: (hold: number) => holds[hold]!.send()?.opId ?? null,
            pending: () =>
              ctx.changes.pending().map((change) => ({
                opId: change.opId,
                label: change.label,
                undoOf: change.undoOf,
                merge: change.merge,
                shows: change.shows,
                held: change.held,
                history: change.history,
              })),
          },
        };
      },
    }),
});

/** The n-th `apply` waits as `c<n>`. */
const applies = (count: number): ResponseSpec[] =>
  Array.from({ length: count }, (_, n) => ({ path: 'apply', times: 1, defer: `c${n}` }));

const doc: FakeDocumentSpec = {
  pages: [
    {
      index: 0,
      ref: page,
      label: null,
      size: { width: 600, height: 800 },
      rotation: 0,
      userUnit: 1,
      boxes: {
        media: { left: 0, bottom: 0, right: 600, top: 800 },
        crop: { left: 0, bottom: 0, right: 600, top: 800 },
      },
    },
  ],
  responses: applies(16),
};
const engine = { documents: { a: doc } };

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open: Step = {
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
};
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
const history = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'history', doc: 'a' },
  method,
  args,
});
const actor = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'history-actor', doc: 'a' },
  method,
  args,
});

/** Stage one action labelled `key`. */
const act = (key: string, objectNumber = 10, x = 1): Step => actor('stage', key, objectNumber, x);
/** One typing pause under `merge`, sent at once: three steps on hold `index`. */
const type = (index: number, merge: string, x: number): Step[] => [
  actor('hold', 'annotation.text', merge),
  actor('set', index, x),
  actor('send', index),
];

/** A `ChangeResult` for an applied change. */
const result = (over: { items?: unknown[]; undoable?: boolean } = {}) => ({
  items: over.items ?? [],
  meta: { affectedPages: [], cacheDelta: null, opId: 'answered', undoable: over.undoable ?? true },
});
const applied = (label: string, over?: Parameters<typeof result>[0]): Step => ({
  op: 'settle',
  label,
  value: result(over),
});
const refused = (label: string, error: ErrorSpec): Step => ({ op: 'settle', label, error });

const engineError = (code: string, message: string, details?: unknown): ErrorSpec => ({
  name: 'EngineError',
  message,
  code,
  ...(details === undefined ? {} : { details }),
});
const conflict = engineError('ChangeConflict', 'no');
const notFound = engineError('NotFound', 'no such change');
const forbidden = engineError('Forbidden', 'no rights');
const unavailable = engineError('UndoUnavailable', 'can not be undone', { reason: 'final-change' });

/** Every getter, in one step each. */
const look: Step[] = [
  history('canUndo'),
  history('canRedo'),
  history('getUndoLabel'),
  history('getRedoLabel'),
];

const remote = { kind: 'remote', sessionId: 'other', sub: null, ts: 0, serverId: 1 };
const endsUndo = (type: string): Step => ({ op: 'emit', doc: 'a', event: { type, origin: remote } });

/** A case on document `a`, opened with the history and the actor, the
 *  history's events listened to, and the `history::a` slice snapshotted. */
const scenario = (
  name: string,
  covers: string,
  steps: Step[],
  config?: HistoryConfig,
): ScenarioCase => ({
  name,
  covers,
  plugins: [
    config === undefined ? { factory: 'history' } : { factory: 'history', config },
    { factory: 'history-actor' },
  ],
  engine,
  snapshot: ['history::a'],
  steps: [
    start,
    open,
    history('onUndone', { $fn: 'undone' }),
    history('onUndoFailed', { $fn: 'undoFailed' }),
    ...steps,
  ],
});

const cases: ScenarioCase[] = [
  scenario('records-as-staged', 'controller.ts:203-206 onStaged records; model.ts:80-109 recorded', [
    ...look,
    act('annotation.move'),
    ...look,
    act('annotation.update'),
    ...look,
    applied('c0'),
    applied('c1'),
    ...look,
  ]),
  scenario('leaves-out-unrecorded-and-undos', 'controller.ts:204,209 history:false and undoOf skip', [
    actor('stageUnrecorded', 'form.calculate'),
    ...look,
    actor('stageUndoOf', 'history.undo', 'elsewhere'),
    ...look,
    applied('c0'),
    refused('c1', notFound),
    ...look,
  ]),
  scenario('undo-then-redo', 'controller.ts:124-177 step; model.ts:115-134 stepped opId replacement', [
    act('annotation.move', 10, 5),
    applied('c0'),
    history('undo'),
    ...look,
    actor('pending'),
    applied('c1'),
    ...look,
    history('redo'),
    actor('pending'),
    ...look,
    applied('c2'),
    ...look,
    history('undo'),
    actor('pending'),
    applied('c3'),
    ...look,
  ]),
  scenario('undo-before-the-answer', 'controller.ts:2-7 undo of a change still on its way', [
    act('annotation.move'),
    history('undo'),
    ...look,
    actor('pending'),
    applied('c0'),
    applied('c1'),
    ...look,
  ]),
  scenario('nothing-to-step-over', 'controller.ts:127-128 nextOf null', [
    history('undo'),
    history('redo'),
    ...look,
  ]),
  scenario('skipped-parts', 'controller.ts:51-54 skippedIn, :172-176 onUndone', [
    act('annotation.move'),
    applied('c0'),
    history('undo'),
    applied('c1', {
      items: [
        { type: 'skipped', op: 'annotations.update', meta: {} },
        { type: 'annotations.update', skipped: ['color'] },
        { type: 'annotations.update', skipped: [] },
        { type: 'annotations.update' },
      ],
    }),
    ...look,
  ]),
  scenario('a-new-change-forgets-redo', 'model.ts:103-108 redo: []', [
    act('a'),
    applied('c0'),
    history('undo'),
    applied('c1'),
    ...look,
    act('b'),
    ...look,
  ]),
  scenario(
    'limit-keeps-the-newest',
    'model.ts:104 slice(-max(1, limit)); history.plugin.ts:19 registered settings',
    [
      history('getSettings'),
      act('a'),
      act('b'),
      act('c'),
      applied('c0'),
      applied('c1'),
      applied('c2'),
      history('undo'),
      history('undo'),
      ...look,
    ],
    { limit: 2 },
  ),
  scenario('limit-from-updated-settings', 'controller.ts:205 settings.get().limit read per change', [
    history('getSettings'),
    history('onSettingsChanged', { $fn: 'settingsChanged' }),
    history('updateSettings', { limit: 1 }),
    act('a'),
    act('b'),
    ...look,
    history('undo'),
    ...look,
    history('updateSettings', { limit: 0 }),
    act('c'),
    act('d'),
    ...look,
    history('resetSettings'),
    history('getSettings'),
    act('e'),
    ...look,
  ]),
  scenario('typing-merges-into-one-step', 'model.ts:89-96 merged parts; controller.ts:129 newest first', [
    ...type(0, 'text:10', 1),
    ...type(1, 'text:10', 2),
    ...type(2, 'text:10', 3),
    ...look,
    history('undo'),
    actor('pending'),
    ...look,
    history('redo'),
    actor('pending'),
    ...look,
  ]),
  scenario('typing-breaks-on-another-change-or-key', 'model.ts:89 merge key; stepped merging null', [
    ...type(0, 'text:10', 1),
    act('annotation.move'),
    ...type(1, 'text:10', 2),
    ...type(2, 'text:11', 3),
    history('undo'),
    history('canUndo'),
    history('undo'),
    history('canUndo'),
    history('undo'),
    history('canUndo'),
    history('undo'),
    ...look,
  ]),
  scenario('an-open-hold-is-undone-first', 'controller.ts:126 sendHolds, :181-192 openHold getters', [
    act('annotation.move'),
    applied('c0'),
    history('undo'),
    applied('c1'),
    ...look,
    actor('hold', 'annotation.text', 'text:10'),
    ...look,
    actor('set', 0, 3),
    ...look,
    history('undo'),
    actor('pending'),
    ...look,
  ]),
  scenario('a-refused-change-leaves-the-history', 'controller.ts:207-216 refused original; :104 moot undo', [
    act('a'),
    act('b'),
    history('undo'),
    refused('c1', conflict),
    refused('c2', notFound),
    applied('c0'),
    ...look,
  ]),
  scenario('a-change-that-did-nothing-leaves', 'controller.ts:211 meta.undoable false', [
    act('a'),
    act('b'),
    applied('c0'),
    applied('c1', { undoable: false }),
    ...look,
  ]),
  scenario('a-refused-undo-is-put-back', 'controller.ts:111-115 stepRefused; model.ts:171-193', [
    act('a'),
    applied('c0'),
    history('undo'),
    ...look,
    refused('c1', forbidden),
    ...look,
    history('undo'),
    actor('pending'),
    applied('c2'),
    ...look,
  ]),
  scenario('a-refused-undo-returns-under-newer-steps', 'model.ts:186-191 reinsertion by seq', [
    act('a'),
    act('b'),
    history('undo'),
    act('c'),
    refused('c2', forbidden),
    ...look,
    history('undo'),
    ...look,
  ]),
  scenario('a-refused-redo-is-put-back', 'model.ts:186-188 redo stack newest first', [
    act('a'),
    applied('c0'),
    history('undo'),
    applied('c1'),
    history('redo'),
    refused('c2', forbidden),
    ...look,
    history('redo'),
    actor('pending'),
    applied('c3'),
    ...look,
  ]),
  scenario('a-redo-behind-a-refused-undo', 'controller.ts:63-65 dead, :104 moot', [
    act('a'),
    history('undo'),
    history('redo'),
    refused('c1', forbidden),
    refused('c2', notFound),
    applied('c0'),
    ...look,
    history('undo'),
    actor('pending'),
  ]),
  scenario('an-unavailable-step-goes-with-everything-before', 'controller.ts:105-111; model.ts:199-207', [
    act('a'),
    act('b'),
    act('c'),
    history('undo'),
    history('undo'),
    act('d'),
    applied('c0'),
    applied('c1'),
    applied('c2'),
    applied('c3'),
    refused('c4', unavailable),
    ...look,
    history('undo'),
    ...look,
  ]),
  scenario('a-merged-step-refused-in-part', 'model.ts:182-184 part rejoins its entry', [
    ...type(0, 'text:10', 1),
    ...type(1, 'text:10', 2),
    applied('c0'),
    applied('c1'),
    history('undo'),
    applied('c2'),
    refused('c3', forbidden),
    ...look,
    history('undo'),
    actor('pending'),
  ]),
  scenario('interleaved-steps-answered-out-of-order', 'controller.ts:68-73,155-159 inFlight', [
    act('a'),
    act('b'),
    applied('c0'),
    applied('c1'),
    history('undo'),
    history('undo'),
    ...look,
    refused('c3', forbidden),
    ...look,
    applied('c2'),
    ...look,
    history('redo'),
    actor('pending'),
  ]),
  scenario('a-change-staged-while-its-undo-waits', 'controller.ts:2-7 interleaved pending changes', [
    act('a'),
    history('undo'),
    act('b'),
    ...look,
    applied('c0'),
    applied('c1'),
    ...look,
    refused('c2', conflict),
    ...look,
  ]),
  scenario('final-changes-end-undo', 'controller.ts:38-44,217-219 ENDS_UNDO', [
    act('a'),
    act('b'),
    history('undo'),
    endsUndo('redaction.applied'),
    ...look,
    act('c'),
    history('undo'),
    endsUndo('pages.flattened'),
    ...look,
    act('d'),
    history('undo'),
    endsUndo('signatures.completed'),
    ...look,
    act('e'),
    history('undo'),
    endsUndo('forms.repaired'),
    ...look,
    act('f'),
    history('undo'),
    endsUndo('document.versioned'),
    ...look,
    act('g'),
    endsUndo('annotations.updated'),
    ...look,
  ]),
  scenario('a-step-on-its-way-when-cleared-puts-nothing-back', 'controller.ts:66-73 gone', [
    act('a'),
    history('undo'),
    endsUndo('redaction.applied'),
    refused('c1', unavailable),
    applied('c0'),
    ...look,
  ]),
  scenario('clear-forgets-every-step', 'controller.ts:77-81; model.ts:210-213', [
    act('a'),
    history('undo'),
    act('b'),
    history('clear'),
    ...look,
    history('clear'),
  ]),
  scenario('close-with-a-step-on-its-way', 'changes.ts close rejects instance-closed', [
    act('a'),
    applied('c0'),
    history('undo'),
    close,
    history('canUndo'),
  ]),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/history.json',
    describes:
      'packages/plugin/history: recording staged changes, undo and redo by opId, merges, limits, refusals, what ends undo.',
    cases,
  },
];
