/**
 * `packages/plugin/outline`. Replayed by
 * `crates/plugins/outline/tests/conformance.rs`.
 *
 * The outline is document-scoped and reads nothing when a document opens:
 * the scenarios ask (`ensureLoaded`, `refresh`), answer the read from the
 * fake engine's table, and then drive the session state (expanded and
 * selected items), the confirmed event stream (any origin) and the writes,
 * each one change on the document's queue through `doc.apply`. Every case
 * snapshots the plugin's slice and its mirror's store cell.
 *
 * The optional plugins are run for real: the stage (where a new item goes,
 * the reading position, a page target's reveal), actions (an item's `/A`),
 * link (websites and every other target), selection (a new item's title)
 * and i18n (the default title), each case with or without them, and the
 * history once, for what undoing a write shows.
 *
 * One shim, mirrored by `crates/plugins/outline/src/harness.rs`: an option
 * `signal: {"$abortSignal": name}` is the signal of a controller named
 * `name`, which the `outline-signals` test plugin's `abort(name)` aborts.
 *
 * A write's answer names the change's `opId`; ops ids come from one
 * deterministic UUID sequence per case (`installCaseEntropy`), so the n-th
 * id is `operationAt(n)`.
 */
import { createCapabilityToken, type AnyPlugin, type OperationOptions } from '@embedpdf/core';
import {
  actionTreeOf,
  changeEvents,
  pageGeometryOf,
  type ChangeItem,
  type DocumentEvent,
  type EventOrigin,
  type OutlineItem,
  type OutlineItemRef,
  type PageDestination,
  type PageGeometrySnapshot,
  type PdfCoordinates,
  type PdfLinkTarget,
  type PdfLinkTargetWritable,
} from '@embedpdf/engine-core/runtime';

import { outlinePlugin } from '../../../../packages/plugin/outline/src/outline.plugin';
import {
  OutlineToken,
  type OutlineCapability,
} from '../../../../packages/plugin/outline/src/contract';

import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
import type { ErrorSpec, FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { ANNOTS_EMPTY, FORMS_EMPTY, catalog, tree } from './actions-current-fixtures';
import { layout, operationAt, pr } from './annotation-live-fixtures';
import { stageStep } from './stage-plugin';

// ── harness shims ─────────────────────────────────────────────────────────────

const controllers = new Map<string, AbortController>();
const controllerNamed = (name: string): AbortController => {
  let controller = controllers.get(name);
  if (!controller) {
    controller = new AbortController();
    controllers.set(name, controller);
  }
  return controller;
};
const withSignal = <T extends OperationOptions | undefined>(options: T): T => {
  const spec = (options as { signal?: { $abortSignal?: string } } | undefined)?.signal;
  return spec?.$abortSignal === undefined
    ? options
    : ({ ...options, signal: controllerNamed(spec.$abortSignal).signal } as T);
};

registerPlugin('outline', {
  factory: () => {
    controllers.clear();
    installCaseEntropy();
    const plugin = outlinePlugin() as AnyPlugin;
    return {
      ...plugin,
      create: (ctx) => {
        const { api, connect } = plugin.create!(ctx);
        const cap = api as OutlineCapability;
        const wrapped: OutlineCapability = {
          ...cap,
          ensureLoaded: (options) => cap.ensureLoaded(withSignal(options)),
          refresh: (options) => cap.refresh(withSignal(options)),
          create: (input, options) => cap.create(input, withSignal(options)),
          update: (ref, patch, options) => cap.update(ref, patch, withSignal(options)),
          move: (refs, parent, position, options) =>
            cap.move(refs, parent, position, withSignal(options)),
          delete: (refs, options) => cap.delete(refs, withSignal(options)),
        };
        return { api: wrapped, connect };
      },
    } as AnyPlugin;
  },
  tokens: [OutlineToken],
});
onCaseEnd(restoreCaseEntropy);

const SignalsToken = createCapabilityToken<{ abort(name: string): void }>('outline-test/signals');
registerPlugin('outline-signals', {
  factory: () =>
    ({
      id: 'outline-test/signals',
      token: SignalsToken,
      create: () => ({ api: { abort: (name: string) => controllerNamed(name).abort() } }),
    }) as AnyPlugin,
  tokens: [SignalsToken],
});

// ── the outline ───────────────────────────────────────────────────────────────

const ref = (objectNumber: number): OutlineItemRef => ({ kind: 'objectNumber', objectNumber });
const xyz = (pon: number, x: number | null, y: number | null): PageDestination => ({
  kind: 'xyz',
  page: pr(pon),
  x,
  y,
  zoom: null,
});
const goto = (destination: PageDestination): PdfLinkTarget => ({ kind: 'goto', destination });
const uri = (address: string): PdfLinkTarget => ({ kind: 'uri', uri: address });
const named = (name: string): PdfLinkTarget => ({ kind: 'named', name });

/** An item as a read returns it: its `/A` is the action its target is written as. */
const item = (
  n: number,
  parent: number | null,
  title: string,
  target: PdfLinkTarget | null,
  over: Partial<OutlineItem> = {},
): OutlineItem => ({
  ref: ref(n),
  parent: parent === null ? null : ref(parent),
  title,
  target,
  action: target ? actionTreeOf<PageDestination>(target as PdfLinkTargetWritable) : null,
  open: false,
  color: null,
  bold: false,
  italic: false,
  ...over,
});

/** A script the actions plugin runs; with no actions plugin the item has no target. */
const SCRIPT = tree({ type: 'uri', subtype: 'URI', uri: 'https://script.example', isMap: false, next: [] });

/**
 * 10 Chapter 1 (open)      page 1, top
 *   11 Section 1.1         page 1, y 400
 *   12 Section 1.2         page 2
 * 13 Chapter 2             a website
 *   14 Deep                a named verb
 * 15 Scripted              an /A and no target
 * 16 Lost                  a page the document doesn't have
 */
const ITEMS: OutlineItem[] = [
  item(10, null, 'Chapter 1', goto(xyz(1, 0, 0)), { open: true, color: '#ff0000', bold: true }),
  item(11, 10, 'Section 1.1', goto(xyz(1, 0, 400))),
  item(12, 10, 'Section 1.2', goto({ kind: 'fit', page: pr(2) })),
  item(13, null, 'Chapter 2', uri('https://example.com')),
  item(14, 13, 'Deep', named('NextPage'), { italic: true }),
  item(15, null, 'Scripted', null, { action: SCRIPT }),
  item(16, null, 'Lost', goto(xyz(99, 0, 0))),
];
const OUTLINE = { items: ITEMS, incomplete: false };

// ── the document ──────────────────────────────────────────────────────────────

const PAGES = [layout(0, 1), layout(1, 2)];
const read = (value: unknown = OUTLINE, times?: number): ResponseSpec => ({
  path: 'outline.get',
  value,
  ...(times === undefined ? {} : { times }),
});
const deferRead = (label: string): ResponseSpec => ({ path: 'outline.get', defer: label, times: 1 });
const failRead = (error: ErrorSpec): ResponseSpec => ({ path: 'outline.get', error, times: 1 });

const doc = (responses: ResponseSpec[] = [read()], over: Partial<FakeDocumentSpec> = {}) => ({
  documents: {
    a: {
      pages: PAGES,
      security: { allows: '*' },
      objectNumbers: [500, 501, 502, 503],
      responses,
      ...over,
    } satisfies FakeDocumentSpec,
  },
});

const LOCAL: EventOrigin = { kind: 'local', sessionId: 's1', sub: null, ts: 0, serverId: null };
const REMOTE: EventOrigin = { kind: 'remote', sessionId: 's2', sub: 'u-9', ts: 0, serverId: null };

const meta = (opId: string, changed: number[]) => ({
  affectedPages: [],
  cacheDelta: null,
  opId,
  undoable: true,
  changed: changed.map(ref),
});
const order = (parent: number | null, children: number[]) => ({
  parent: parent === null ? null : ref(parent),
  children: children.map(ref),
});

/** The engine's items for one change, built for the change named `opId`. */
type Answer = (opId: string) => ChangeItem;
const created =
  (made: OutlineItem, parent: number | null, children: number[]): Answer =>
  (opId) => ({
    type: 'outline.create',
    item: made,
    order: order(parent, children),
    meta: meta(opId, [made.ref.objectNumber]),
  });
const updated =
  (changed: OutlineItem): Answer =>
  (opId) => ({ type: 'outline.update', item: changed, meta: meta(opId, [changed.ref.objectNumber]) });
const moved =
  (orders: ReturnType<typeof order>[], changed: number[]): Answer =>
  (opId) => ({ type: 'outline.move', orders, meta: meta(opId, changed) });
const deleted =
  (gone: number[]): Answer =>
  (opId) => ({ type: 'outline.delete', meta: meta(opId, gone) });
const restored =
  (items: OutlineItem[], parent: number | null, children: number[]): Answer =>
  (opId) => ({
    type: 'outline.restore',
    items,
    order: order(parent, children),
    meta: meta(opId, items.map((i) => i.ref.objectNumber)),
  });

const resultOf = (opId: string, answers: Answer[]) => {
  const items = answers.map((answer) => answer(opId));
  return { items, meta: { affectedPages: [], cacheDelta: null, opId, undoable: true } };
};
/** The confirmed events of a result, as the engine publishes them for change `opId`. */
const factsOf = (opId: string, answers: Answer[], origin: EventOrigin = LOCAL): DocumentEvent[] => {
  const events = changeEvents(resultOf(opId, answers) as Parameters<typeof changeEvents>[0]);
  return events.map(
    (event, index) =>
      ({ ...event, origin: { ...origin, tx: { id: opId, index, count: events.length } } }) as DocumentEvent,
  );
};

/** The n-th change's answer: its events, then its result. */
const applied = (n: number, ...answers: Answer[]): ResponseSpec => ({
  path: 'apply',
  times: 1,
  value: resultOf(operationAt(n), answers),
  events: factsOf(operationAt(n), answers),
});
const deferApply = (label: string): ResponseSpec => ({ path: 'apply', times: 1, defer: label });
const refusedApply = (error: ErrorSpec): ResponseSpec => ({ path: 'apply', times: 1, error });

// ── steps ─────────────────────────────────────────────────────────────────────

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open: Step = {
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
};
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };

function ol<M extends keyof OutlineCapability>(
  method: M,
  args: readonly unknown[] = [],
  meter?: number,
): Step {
  return {
    op: 'call',
    target: { token: 'outline', doc: 'a' },
    method,
    args,
    ...(meter === undefined ? {} : { meter }),
  };
}
const abort = (name: string): Step => ({
  op: 'call',
  target: { token: 'outline-test/signals' },
  method: 'abort',
  args: [name],
});
const signal = (name: string) => ({ signal: { $abortSignal: name } });
const emit = (event: unknown, meter?: number): Step => ({
  op: 'emit',
  doc: 'a',
  event,
  ...(meter === undefined ? {} : { meter }),
});
const settle = (label: string, value: unknown, meter?: number): Step => ({
  op: 'settle',
  label,
  value,
  ...(meter === undefined ? {} : { meter }),
});
const refuse = (label: string, error: ErrorSpec): Step => ({ op: 'settle', label, error });
const st = (method: string, args: unknown[] = []): Step => stageStep('stage', 'a', method, args);
const viewport: Step = st('setViewport', [{ width: 1000, height: 700 }]);

/** Every read, one step each. */
const look: Step[] = [
  ol('list'),
  ol('listRows'),
  ol('getSelected'),
  ol('getStatus'),
  ol('isIncomplete'),
];
const hooks: Step[] = [ol('onChanged', [{ $fn: 'changed' }]), ol('onResynced', [{ $fn: 'resynced' }])];

// ── plugins ───────────────────────────────────────────────────────────────────

const OUTLINE_ONLY = [{ factory: 'outline' }, { factory: 'outline-signals' }];
const STAGE = { factory: 'stage', config: { viewUnitsPerPoint: 1, scrollBehavior: 'instant' } };
const INTERACTION = { factory: 'interaction' };
const ACTIONS = { factory: 'actions-current', config: { openSequence: 'off' } };
const LINK = { factory: 'link' };
const SELECTION = { factory: 'selection' };
const I18N = {
  factory: 'i18n',
  config: {
    locale: 'de',
    locales: [{ code: 'de', name: 'Deutsch', translations: { outline: { untitled: 'Unbenannt' } } }],
  },
};
const HISTORY = { factory: 'history' };
const WITH_STAGE = [STAGE, ...OUTLINE_ONLY];
const EVERYTHING = [INTERACTION, ACTIONS, STAGE, LINK, ...OUTLINE_ONLY];
const SNAP = ['outline::a', 'outline::a/mirror/outline'];

const scenario = (
  name: string,
  covers: string,
  engine: ReturnType<typeof doc>,
  steps: Step[],
  plugins: readonly { factory: string; config?: unknown }[] = OUTLINE_ONLY,
): ScenarioCase => ({
  name,
  covers,
  plugins,
  engine,
  snapshot: SNAP,
  steps: [start, open, ...hooks, ...steps],
});

const denied = (required: string) => ({
  name: 'PermissionDenied',
  message: `permission denied (local): ${required}`,
  required,
  context: 'local',
  code: 'Forbidden',
  details: { required, context: 'local' },
});
const refusedByEngine = { name: 'EngineError', message: 'no', code: 'Refused' };

// ── selection text (page 1: "Hello world") ────────────────────────────────────

const W = 6;
const H = 12;
const glyphs = (text: string, x: number, top: number) =>
  [...text].map((ch, i) => {
    const left = x + i * W;
    const loose = { left, bottom: top - H, right: left + W, top };
    return ch === ' '
      ? { loose, space: true as const }
      : { loose, tight: { left: left + 0.5, bottom: top - H + 1, right: left + W - 0.5, top: top - 1 } };
  });
const helloRun = glyphs('Hello  world', 50, 750);
const helloGeometry: PageGeometrySnapshot<PdfCoordinates> = {
  runs: [
    {
      rect: { left: 50, bottom: 738, right: 50 + 12 * W, top: 750 },
      start: 0,
      glyphs: helloRun,
      fontSize: 12,
    },
  ],
};
const TEXT_READS: ResponseSpec[] = [
  {
    path: 'page.text.layout',
    pon: 1,
    value: { $textLayout: pageGeometryOf(helloGeometry, { left: 0, bottom: 0, right: 600, top: 800 }) },
  },
  { path: 'page.text.get', pon: 1, value: { text: 'Hello  world', charCount: 12 } },
];
const sel = (method: string, args: unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'selection', doc: 'a' },
  method,
  args,
});

// ── cases ─────────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  scenario(
    'empty-until-loaded',
    'controller.ts:94-112 on-demand mirror; model.ts:45-69,203-208 the first read seeds the open items; reads empty until it lands, onResynced on load, a second ensureLoaded resolves at once',
    doc([deferRead('r'), read()]),
    [
      ...look,
      ol('getChildren', [null]),
      ol('get', [ref(10)]),
      ol('ensureLoaded'),
      ol('getStatus'),
      ol('list'),
      settle('r', OUTLINE, 8),
      ...look,
      ol('getChildren', [null]),
      ol('getChildren', [ref(10)]),
      ol('getChildren', [ref(11)]),
      ol('getChildren', [ref(99)]),
      ol('get', [ref(14)]),
      ol('get', [ref(99)]),
      ol('isExpanded', [ref(10)]),
      ol('isExpanded', [ref(13)]),
      ol('ensureLoaded'),
    ],
  ),
  scenario(
    'an-empty-outline',
    'model.ts:45-54 a document with no outline lists nothing and is ready',
    doc([read({ items: [], incomplete: false })]),
    [ol('ensureLoaded'), ...look, ol('getChildren', [null]), ol('expandAll'), ol('getCurrentItem')],
  ),
  scenario(
    'an-incomplete-tree',
    'contract.ts:86 isIncomplete; model.ts:45-69 an item whose parent the read lost is not listed',
    doc([
      read({
        items: [
          item(10, null, 'Kept', null, { open: true }),
          item(11, 10, 'Child', null),
          item(21, 20, 'Orphan', null),
        ],
        incomplete: true,
      }),
    ]),
    [ol('ensureLoaded'), ...look, ol('get', [ref(21)]), ol('getChildren', [ref(20)])],
  ),
  scenario(
    'ensure-loaded-refresh-and-failure',
    'controller.ts:356-358 ensureLoaded/refresh; a failed read rejects and reads again next time; refresh keeps what the user expanded, drops what the outline lost (model.ts:236-242), fires onResynced and no onChanged; a refresh during a read joins it and reads once more',
    doc([
      failRead({ name: 'EngineError', message: 'broken', code: 'Unknown' }),
      read(OUTLINE, 1),
      deferRead('again'),
      deferRead('third'),
      read(OUTLINE),
    ]),
    [
      ol('ensureLoaded'),
      ol('getStatus'),
      ol('ensureLoaded'),
      ol('getStatus'),
      ol('collapse', [ref(10)]),
      ol('expand', [ref(13)]),
      ol('select', [[ref(14), ref(16)]]),
      ol('refresh'),
      ol('refresh'),
      ol('getStatus'),
      settle('again', { items: ITEMS.filter((i) => i.ref.objectNumber !== 14), incomplete: false }),
      ...look,
      settle('third', { items: ITEMS.slice(0, 3), incomplete: false }, 12),
      ...look,
      ol('isExpanded', [ref(10)]),
    ],
  ),
  scenario(
    'a-forbidden-read',
    'mirror status forbidden for a permission-denied read; the outline stays empty',
    doc([failRead(denied('doc.outline.read'))]),
    [ol('ensureLoaded'), ...look],
  ),
  scenario(
    'confirmed-events-fold-whoever-made-them',
    'sync/tree.ts:107-175 foldOutline for created, updated, moved, deleted and restored; controller.ts:61-77,109-110 onChanged with the event origin; an event before the first read changes nothing; a page delete reads the outline again',
    doc([read(OUTLINE, 1), read({ items: ITEMS.slice(0, 2), incomplete: false })]),
    [
      emit(factsOf('early', [created(item(30, null, 'Early', null), null, [10, 13, 15, 16, 30])], REMOTE)[0]),
      ol('list'),
      ol('ensureLoaded'),
      emit(factsOf('r1', [created(item(30, 10, 'Remote', null), 10, [11, 30, 12])], REMOTE)[0]),
      ol('getChildren', [ref(10)]),
      emit(factsOf('r2', [updated(item(13, null, 'Renamed', uri('https://example.com')))], REMOTE)[0]),
      emit(
        factsOf(
          'r3',
          [
            moved(
              [order(null, [10, 14, 13, 15, 16]), order(13, [])],
              [14],
            ),
          ],
          REMOTE,
        )[0],
      ),
      ...look,
      emit(factsOf('r4', [deleted([10, 11, 30, 12])], REMOTE)[0]),
      ...look,
      emit(
        factsOf(
          'r5',
          [
            restored(
              [ITEMS[0]!, ITEMS[1]!, ITEMS[2]!],
              null,
              [10, 14, 13, 15, 16],
            ),
          ],
          LOCAL,
        )[0],
      ),
      ...look,
      emit({ type: 'metadata.updated', metadata: {}, previous: null, origin: REMOTE }),
      emit({
        type: 'pages.deleted',
        pages: [pr(2)],
        layout: { pageCount: 1, pages: [layout(0, 1)], namedPages: [] },
        meta: { affectedPages: [], cacheDelta: null, opId: 'r6', undoable: true },
        origin: REMOTE,
      }),
      ...look,
    ],
  ),
  scenario(
    'expanded-and-selected',
    'controller.ts:173-186,360-373 isExpanded, expand, collapse, toggle, expandAll, collapseAll, reveal, select; model.ts:210-233 unknown refs and leaves change nothing, a selection keeps its order and drops repeats; getSelected drops refs the outline lost',
    doc(),
    [
      ol('expand', [ref(13)]),
      ol('ensureLoaded'),
      ol('isExpanded', [ref(10)]),
      ol('collapse', [ref(10)]),
      ol('toggle', [ref(10)]),
      ol('toggle', [ref(10)]),
      ol('toggle', [ref(11)]),
      ol('expand', [ref(99)]),
      ol('collapse', [ref(99)]),
      ol('expand', [ref(11)]),
      ol('isExpanded', [ref(11)]),
      ol('expandAll'),
      ol('listRows'),
      ol('collapseAll'),
      ol('listRows'),
      ol('reveal', [ref(14)]),
      ol('listRows'),
      ol('reveal', [ref(99)]),
      ol('select', [[ref(14), ref(99), ref(11), ref(14)]]),
      ol('getSelected'),
      ol('select', [[ref(14), ref(11)]]),
      ol('select', [[]]),
      ol('select', [[ref(12)]]),
      emit(factsOf('gone', [deleted([10, 11, 12])], REMOTE)[0]),
      ol('getSelected'),
      ol('listRows'),
    ],
  ),
  scenario(
    'activation-with-no-optional-plugin',
    'controller.ts:190-225: an /A without the actions plugin falls to the target; without link and stage a page is handed back, a website and a named verb are handed on, a page the document lacks is reported, no target is none; not-found throws; no reading position',
    doc(),
    [
      ol('activate', [ref(10)]),
      ol('ensureLoaded'),
      ol('activate', [ref(10)]),
      ol('activate', [ref(12)]),
      ol('activate', [ref(13)]),
      ol('activate', [ref(14)]),
      ol('activate', [ref(15)]),
      ol('activate', [ref(16)]),
      ol('activate', [ref(99)]),
      ol('getCurrentItem'),
      ol('getCurrentViewTarget'),
    ],
  ),
  scenario(
    'activation-with-the-stage',
    'controller.ts:212-216 a page target moves the stage; controller.ts:148-169 the current item is the last one at or above the top of the view; getCurrentViewTarget is where the view is',
    doc(),
    [
      ol('ensureLoaded'),
      ol('getCurrentItem'),
      ol('getCurrentViewTarget'),
      viewport,
      ol('getCurrentItem'),
      ol('getCurrentViewTarget'),
      ol('activate', [ref(11)]),
      ol('getCurrentItem'),
      ol('activate', [ref(12)]),
      ol('getCurrentItem'),
      ol('getCurrentViewTarget'),
      ol('activate', [ref(13)]),
      ol('activate', [ref(16)]),
    ],
    WITH_STAGE,
  ),
  scenario(
    'activation-through-link-and-actions',
    'controller.ts:196-209 an /A runs through the actions plugin as a user dispatch from the outline item; every other target goes through link.activate',
    doc([read(), FORMS_EMPTY, ANNOTS_EMPTY, catalog()]),
    [
      ol('ensureLoaded'),
      viewport,
      ol('activate', [ref(15)]),
      { op: 'drain' },
      ol('activate', [ref(12)]),
      ol('getCurrentItem'),
      ol('activate', [ref(14)]),
      ol('activate', [ref(16)]),
    ],
    EVERYTHING,
  ),
  scenario(
    'activation-through-link',
    'controller.ts:205-209 with link and no actions plugin every target goes through link.activate: a page reveals through the stage, a website and a named verb are handed on, a page the document lacks is reported; an /A with no target is none',
    doc(),
    [
      ol('ensureLoaded'),
      viewport,
      ol('activate', [ref(12)]),
      ol('getCurrentItem'),
      ol('activate', [ref(13)]),
      ol('activate', [ref(14)]),
      ol('activate', [ref(15)]),
      ol('activate', [ref(16)]),
    ],
    [INTERACTION, STAGE, LINK, ...OUTLINE_ONLY],
  ),
  scenario(
    'create-shows-at-once-and-confirms',
    'controller.ts:293-316 create: an object number from the pool, the item shown before the answer (sync/tree.ts:185-202), the confirmed event and onChanged before the promise settles; select reveals and selects; positions before, after, start and end; not-found for a parent the outline lacks',
    doc([
      read(),
      deferApply('c1'),
      applied(
        2,
        created(item(501, 10, 'First', null), 10, [501, 11, 12]),
      ),
      applied(3, created(item(502, 13, 'After', null, { color: '#00aa00' }), 13, [14, 502])),
      applied(4, created(item(503, null, 'Before', null), null, [10, 13, 503, 15, 16])),
    ]),
    [
      ol('ensureLoaded'),
      ol('collapse', [ref(10)]),
      ol('create', [{ title: 'New', parent: ref(11) }, { select: true }]),
      ...look,
      ol('getCurrentViewTarget'),
      emit(factsOf(operationAt(1), [created(item(500, 11, 'New', null), 11, [500])])[0]),
      ol('list'),
      settle('c1', resultOf(operationAt(1), [created(item(500, 11, 'New', null), 11, [500])]), 6),
      ol('create', [{ title: 'First', parent: ref(10), position: 'start' }], 12),
      ol('getChildren', [ref(10)]),
      ol('create', [{ title: 'After', parent: ref(13), position: { after: ref(14) }, color: '#00AA00' }]),
      ol('create', [{ title: 'Before', position: { before: ref(15) } }]),
      ...look,
      ol('create', [{ title: 'Nowhere', parent: ref(99) }]),
    ],
  ),
  scenario(
    'create-takes-defaults-from-the-optional-plugins',
    'controller.ts:278-305: without them a new item is "Untitled" with no target; with the stage it goes where the view is, with i18n its title is the viewer language\'s, with a selection it is the selected text on one line; an empty pool reserves a number',
    doc(
      [
        read(),
        ...TEXT_READS,
        { path: 'objectNumbers.reserve', args: [1], value: undefined },
        applied(1, created(item(500, null, 'Hello world', goto(xyz(1, 0, 0))), null, [10, 13, 15, 16, 500])),
        applied(2, created(item(501, null, 'Unbenannt', goto(xyz(1, 0, 0))), null, [10, 13, 15, 16, 500, 501])),
      ],
      { objectNumbers: [500] },
    ),
    [
      ol('ensureLoaded'),
      viewport,
      sel('ensureLoaded', [pr(1)]),
      sel('select', [{ page: pr(1), start: 0, count: 12 }]),
      ol('create', [], 20),
      ...look,
      sel('clear'),
      ol('create', []),
      ol('list'),
    ],
    [INTERACTION, I18N, SELECTION, STAGE, ...OUTLINE_ONLY],
  ),
  scenario(
    'create-without-the-optional-plugins',
    'controller.ts:290,297: "Untitled" and no target with no stage and no i18n',
    doc([read(), applied(1, created(item(500, null, 'Untitled', null), null, [10, 13, 15, 16, 500]))]),
    [ol('ensureLoaded'), ol('create', []), ol('list')],
  ),
  scenario(
    'update-move-and-delete',
    'controller.ts:318-347 and sync/tree.ts:203-263: update shows the patch (a lowercase colour, open only with children); move takes items with their children together; delete names each topmost item once, one change; each confirmed before its promise settles; an empty move or delete does nothing',
    doc([
      read(),
      deferApply('u'),
      applied(2, updated({ ...ITEMS[0]!, open: false })),
      applied(3, moved([order(13, [14, 11]), order(10, [12])], [11])),
      applied(4, deleted([13, 14, 11]), deleted([16])),
    ]),
    [
      ol('ensureLoaded'),
      ol('update', [ref(12), { title: 'Renamed', color: '#AABBCC', open: true, bold: true }]),
      ol('get', [ref(12)]),
      ol('update', [ref(10), { open: false }]),
      ol('listRows'),
      emit(
        factsOf(operationAt(1), [
          updated(item(12, 10, 'Renamed', goto({ kind: 'fit', page: pr(2) }), { color: '#aabbcc', bold: true })),
        ])[0],
      ),
      settle(
        'u',
        resultOf(operationAt(1), [
          updated(item(12, 10, 'Renamed', goto({ kind: 'fit', page: pr(2) }), { color: '#aabbcc', bold: true })),
        ]),
      ),
      ol('move', [[ref(11)], ref(13), { after: ref(14) }], 8),
      ...look,
      ol('delete', [[ref(14), ref(13), ref(11), ref(13), ref(16)]], 8),
      ...look,
      ol('move', [[], null, 'start']),
      ol('delete', [[]]),
    ],
  ),
  scenario(
    'undo-shows-the-outline-as-it-was',
    'sync/tree.ts:299-356 undoOf: undoing a delete restores the item and its children in place, undoing a move puts each item back, undoing a create deletes it',
    doc([
      read(),
      applied(1, deleted([10, 11, 12])),
      deferApply('undo1'),
      applied(3, moved([order(null, [14, 13, 15, 16]), order(13, [])], [14])),
      deferApply('undo2'),
      applied(5, created(item(500, null, 'Made', null), null, [13, 15, 16, 500])),
      deferApply('undo3'),
    ]),
    [
      ol('ensureLoaded'),
      ol('delete', [[ref(10)]]),
      ol('list'),
      { op: 'call', target: { token: 'history', doc: 'a' }, method: 'undo', args: [] },
      ...look,
      refuse('undo1', refusedByEngine),
      ol('list'),
      ol('move', [[ref(14)], null, 'start']),
      ol('list'),
      { op: 'call', target: { token: 'history', doc: 'a' }, method: 'undo', args: [] },
      ol('list'),
      ol('create', [{ title: 'Made' }]),
      ol('list'),
      { op: 'call', target: { token: 'history', doc: 'a' }, method: 'undo', args: [] },
      ol('list'),
    ],
    [HISTORY, ...OUTLINE_ONLY],
  ),
  scenario(
    'refused-writes',
    'controller.ts:229-246: without doc.outline.modify every write rejects permission-denied before anything is staged; canEdit; a ref the outline lacks rejects not-found; an engine refusal rejects and the view drops the change with no onChanged',
    doc([read(), refusedApply(refusedByEngine)], { security: { allows: ['doc.outline.read'] } }),
    [
      ol('canEdit'),
      ol('ensureLoaded'),
      ol('create', [{ title: 'No' }]),
      ol('update', [ref(10), { title: 'No' }]),
      ol('move', [[ref(11)], null, 'end']),
      ol('delete', [[ref(10)]]),
      ol('list'),
    ],
  ),
  scenario(
    'an-engine-refusal',
    'controller.ts:257-267 the change\'s refusal, mapped to a PluginError; the view drops the prediction; not-found for refs the outline lacks before anything is staged',
    doc([read(), deferApply('x')]),
    [
      ol('canEdit'),
      ol('ensureLoaded'),
      ol('update', [ref(99), { title: 'No' }]),
      ol('move', [[ref(11)], ref(99), 'end']),
      ol('delete', [[ref(99)]]),
      ol('update', [ref(10), { title: 'Pending' }]),
      ol('get', [ref(10)]),
      refuse('x', refusedByEngine),
      ol('get', [ref(10)]),
    ],
  ),
  scenario(
    'cancellation',
    'controller.ts:230-235 an aborted signal refuses before anything starts; controller.ts:262 a signal aborted while the answer is on its way rejects operation-cancelled and the change still lands; ensureLoaded races the signal and the read lands anyway',
    doc([deferRead('r'), deferApply('c')]),
    [
      ol('ensureLoaded', [signal('load')]),
      abort('load'),
      ol('getStatus'),
      settle('r', OUTLINE),
      ol('getStatus'),
      abort('gone'),
      ol('create', [{ title: 'No' }, signal('gone')]),
      ol('update', [ref(10), { title: 'No' }, signal('gone')]),
      ol('move', [[ref(11)], null, 'end', signal('gone')]),
      ol('delete', [[ref(10)], signal('gone')]),
      ol('ensureLoaded', [signal('gone')]),
      ol('update', [ref(10), { title: 'Late' }, signal('late')]),
      abort('late'),
      ol('get', [ref(10)]),
      emit(factsOf(operationAt(1), [updated({ ...ITEMS[0]!, title: 'Late' })])[0]),
      settle('c', resultOf(operationAt(1), [updated({ ...ITEMS[0]!, title: 'Late' })])),
      ol('get', [ref(10)]),
    ],
  ),
  scenario(
    'close',
    'a read and a write on their way when the document closes reject instance-closed; reopening starts idle',
    doc([deferRead('r'), read(), deferApply('c')]),
    [
      ol('ensureLoaded'),
      close,
      open,
      ...hooks,
      ...look,
      ol('ensureLoaded'),
      ol('update', [ref(10), { title: 'Closing' }]),
      close,
    ],
  ),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/outline.json',
    describes:
      'packages/plugin/outline: the on-demand outline mirror, session state, activation with and without the optional plugins, and writes through doc.apply.',
    cases,
  },
];
