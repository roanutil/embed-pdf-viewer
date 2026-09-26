/**
 * `packages/plugin/redaction`: the destructive apply stage over `redact`
 * annotation marks. Marks live on the annotation plane, so every scenario
 * runs the real interaction, selection and annotation plugins beside this
 * one, with the annotation fixtures' shims. Replayed by
 * `crates/plugins/redaction/tests/conformance.rs`.
 *
 * One more shim, mirrored by `crates/plugins/redaction/src/harness.rs`:
 * while a `redaction` factory is current, a `console.error` whose message
 * starts `[redaction]` logs as the callback `console.error` (the Rust port
 * takes it as `RedactionHost::console_error`).
 */
import { createCapabilityToken, definePlugin, type AnyPlugin } from '@embedpdf/core';

import { initialRedactionState, redactionReducer } from '../../../../packages/plugin/redaction/src/model';
import { createPendingReads } from '../../../../packages/plugin/redaction/src/read/pending';
import { redactionPlugin } from '../../../../packages/plugin/redaction/src/index';
import { RedactionToken } from '../../../../packages/plugin/redaction/src/host-contract';
import type { RedactionAction, RedactionState } from '../../../../packages/plugin/redaction/src/model';
import { InteractionToken } from '../../../../packages/plugin/interaction/src/host-contract';

import { registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

import {
  base,
  boot,
  call as anno,
  created,
  doc,
  hydrate,
  ix,
  markup,
  objRef,
  open,
  pr,
  redact,
  sel,
  settle,
  square,
  textNote,
  updated,
  v3Event,
} from './annotation-fixtures';

// ── the console.error shim ────────────────────────────────────────────────────

let redactionLog: PluginEnv['log'] | null = null;
const previousError = console.error;
console.error = (...args: unknown[]) => {
  if (redactionLog && typeof args[0] === 'string' && args[0].startsWith('[redaction]')) {
    redactionLog('console.error', args);
    return;
  }
  previousError(...args);
};

registerPlugin('redaction', {
  factory: (_config, env) => {
    redactionLog = env.log;
    return redactionPlugin() as AnyPlugin;
  },
  tokens: [RedactionToken],
});

/**
 * v2's redact-tool verbs (enableRedact, toggleRedact, isRedactActive) left
 * the plugin: the `redact` tool is the annotation plugin's, armed through the
 * interaction hub. This lens keeps those three verbs over the hub, so the
 * cases read the same; `crates/plugins/redaction/src/harness.rs` mirrors it.
 */
const ToolLensToken = createCapabilityToken<Record<string, () => unknown>>('redaction-test/tool');
registerPlugin('redaction-tool', {
  factory: () =>
    definePlugin({
      id: 'redaction-test/tool',
      token: ToolLensToken,
      scope: 'document',
      requires: [InteractionToken],
      capability: (ctx) => {
        const hub = () => ctx.get(InteractionToken);
        const active = () => hub().getActiveToolId() === 'redact';
        return {
          enableRedact: () => hub().activateTool('redact'),
          toggleRedact: () => (active() ? hub().activateDefaultTool() : hub().activateTool('redact')),
          isRedactActive: active,
        };
      },
    }) as AnyPlugin,
  tokens: [ToolLensToken],
});

// ── text geometry (the annotation-markup suite's page 1) ─────────────────────

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
const geo1 = { runs: [run(0, [...glyphs('Hello world', 50, 750), ...newline]), run(13, glyphs('Second line', 50, 730))] };
const geo2 = { runs: [run(0, glyphs('Page two', 40, 500))] };
const reads = [
  { path: 'page.geometry.read', pon: 1, value: geo1 },
  { path: 'page.geometry.read', pon: 2, value: geo2 },
  { path: 'page.text.read', pon: 1, value: { text: 'Hello world\r\nSecond line', charCount: 24 } },
  { path: 'page.text.read', pon: 2, value: { text: 'Page two', charCount: 8 } },
];
const load: Step[] = [sel('ensurePage', [1]), sel('ensurePage', [2])];
const range = (from: [number, number], to: [number, number]) => ({
  start: { pon: from[0], index: from[1] },
  end: { pon: to[0], index: to[1] },
});

// ── steps ─────────────────────────────────────────────────────────────────────

/** A v2 mark id (`obj:N`, `nm:X` on page 2, anything else unknown) as a ref. */
const idRef = (id: unknown): unknown => {
  if (typeof id !== 'string') return id;
  if (id.startsWith('obj:')) return objRef(Number(id.slice(4)));
  if (id.startsWith('nm:')) return { kind: 'nm', page: pr(2), nm: id.slice(3) };
  if (id.startsWith('idx:')) {
    const [, pon, index] = id.split(':');
    return { kind: 'index', page: pr(Number(pon)), index: Number(index), revision: { docSessionId: 's', page: pr(Number(pon)), generation: 1 } };
  }
  return { kind: 'nm', page: pr(1), nm: id };
};
const RENAMED: Readonly<Record<string, string>> = {
  lastResult: 'getLastResult',
  queueCurrentSelection: 'markSelection',
  pendingCount: 'getPendingCount',
  getPending: 'listPending',
  setLabel: 'updateLabel',
};
const TOOL = new Set(['enableRedact', 'toggleRedact', 'isRedactActive']);
const call = (method: string, args: unknown[] = [], docId = 'a'): Step => {
  if (TOOL.has(method)) return { op: 'call', target: { token: 'redaction-test/tool', doc: docId }, method, args };
  // The pending list is read off the annotation plane, which hydrates on
  // its own; v2's explicit preparePending is its ensureHydrated.
  if (method === 'preparePending') return anno('ensurePage', [], docId);
  const out = (method === 'apply' || method === 'estimateCollateral') && Array.isArray(args[0]) ? [(args[0] as unknown[]).map(idRef), ...args.slice(1)] : args;
  return { op: 'call', target: { token: 'redaction', doc: docId }, method: RENAMED[method] ?? method, args: out };
};
const ref = (n: number): Step => ({ op: 'call', target: { ref: n } });
const emit = (event: Record<string, unknown>, docId = 'a'): Step => ({ op: 'emit', doc: docId, event: v3Event(event) });
const openDoc = (id: string): Step => open(id);

const noMods = { shift: false, alt: false, ctrl: false, meta: false };
const at = (phase: string, x: number, y: number, pon = 1) => ({
  phase,
  viewport: { x, y },
  page: { ref: pr(pon), point: { x, y }, scale: 1 },
  modifiers: noMods,
});
const ptr = (phase: string, x: number, y: number): Step => ix('dispatch', [at(phase, x, y)]);

const REMOTE = { kind: 'remote', serverId: 'srv-7' };
const LOCAL = { kind: 'local', serverId: null };
const APPLY_CAPS = ['doc.redact', 'doc.pages.modify', 'doc.annotate.modify'];

const WITH_SELECTION = [{ factory: 'interaction' }, { factory: 'selection' }, { factory: 'annotation' }, { factory: 'redaction' }, { factory: 'redaction-tool' }];
const PLUGINS = [{ factory: 'interaction' }, { factory: 'annotation' }, { factory: 'redaction' }, { factory: 'redaction-tool' }];
const SNAP = ['redaction::a', 'annotation::a'];
const withService = (responses: Parameters<typeof doc>[0], over: Parameters<typeof doc>[1] = {}) =>
  doc(responses, { services: ['redaction'], ...over });

// ── fixtures ──────────────────────────────────────────────────────────────────

const quad = (x: number, y: number, w: number, h: number) => ({
  p1: { x, y: y + h },
  p2: { x: x + w, y: y + h },
  p3: { x, y },
  p4: { x: x + w, y },
});
/** Page 1: an area mark over a square and a note, a text mark, a stray. */
const areaMark = redact(10, { rect: { left: 90, bottom: 590, right: 210, top: 710 }, overlayText: 'CONFIDENTIAL' });
const textMark = redact(11, {
  rect: { left: 50, bottom: 730, right: 200, top: 745 },
  quadPoints: [quad(50, 730, 150, 15), quad(50, 710, 60, 15)],
});
const nmMark = redact(12, {
  ref: { kind: 'nm', page: pr(2), nm: 'mark-b' },
  pageObjectNumber: 2,
  rect: { left: 20, bottom: 20, right: 60, top: 60 },
});
const seed = {
  1: [
    square(1),
    textNote(2, { rect: { left: 205, bottom: 650, right: 225, top: 670 } }),
    markup('highlight', 3, { rect: { left: 60, bottom: 712, right: 90, top: 722 }, quadPoints: [quad(60, 712, 30, 10)] }),
    areaMark,
    textMark,
    square(4, { rect: { left: 400, bottom: 100, right: 500, top: 200 } }),
  ],
  2: [nmMark, square(5, { pageObjectNumber: 2, rect: { left: 60, bottom: 20, right: 90, top: 60 } })],
};
const applyResult = (scope: unknown, removed = 2) => ({
  scope,
  results: [
    { page: pr(1), status: 'applied', removedAnnotationCount: removed },
    { page: pr(2), status: 'unchanged', removedAnnotationCount: 0 },
  ],
  removedAnnotationCount: removed,
  meta: { revision: 3 },
});
const refsScope = { kind: 'annotations', refs: [objRef(10), { kind: 'nm', page: pr(2), nm: 'mark-b' }] };
const pagesScope = { kind: 'pages', pages: [pr(1), pr(2)] };
const reloadPages = [
  { path: 'page.annotations.list', pon: 1, value: { annotations: [square(4, { rect: { left: 400, bottom: 100, right: 500, top: 200 } })] } },
  { path: 'page.annotations.list', pon: 2, value: { annotations: [] } },
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  {
    name: 'initial-state-and-twins',
    covers:
      'A fresh slice is idle with no result. canMark is annotation create authority; canApply needs the engine service and all three of doc.redact, doc.pages.modify and doc.annotate.modify.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([hydrate()]),
        b: withService([hydrate()], { security: { allows: ['doc.annotate.read', ...APPLY_CAPS] } }),
        c: withService([hydrate()], { security: { allows: ['doc.annotate.read', 'doc.annotate.create', 'doc.redact', 'doc.pages.modify'] } }),
        d: withService([hydrate()], { security: { allows: ['doc.annotate.read', 'doc.redact', 'doc.annotate.modify'] } }),
        e: withService([hydrate()], { security: { allows: ['doc.annotate.read', 'doc.pages.modify', 'doc.annotate.modify'] } }),
        f: doc([hydrate()]),
      },
    },
    snapshot: ['redaction::a'],
    steps: [
      ...boot,
      openDoc('b'),
      openDoc('c'),
      openDoc('d'),
      openDoc('e'),
      openDoc('f'),
      call('isApplying'),
      call('lastResult'),
      call('canMark'),
      call('canApply'),
      call('canMark', [], 'b'),
      call('canApply', [], 'b'),
      call('canMark', [], 'c'),
      call('canApply', [], 'c'),
      call('canApply', [], 'd'),
      call('canApply', [], 'e'),
      call('canMark', [], 'f'),
      call('canApply', [], 'f'),
    ],
  },
  {
    name: 'mark-the-text-selection',
    covers:
      'queueCurrentSelection creates one redact markup per selected page from the segment quads, with the redact preset, clears the selection and resolves true; the marks then read back as text-kind pending items.',
    plugins: WITH_SELECTION,
    engine: {
      documents: {
        a: withService([
          hydrate(),
          ...reads,
          { path: 'page.annotations.create', pon: 1, value: created(redact(20, { quadPoints: [quad(50, 738, 66, 12)] })) },
          { path: 'page.annotations.create', pon: 2, value: created(redact(21, { pageObjectNumber: 2, quadPoints: [quad(40, 488, 24, 12)] })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...load,
      sel('select', [range([1, 0], [2, 4])]),
      call('queueCurrentSelection'),
      sel('hasSelection'),
      call('getPending'),
      call('pendingCount'),
      call('queueCurrentSelection'),
    ],
  },
  {
    name: 'marking-needs-create-authority',
    covers: 'Without annotation create authority canMark is false and queueCurrentSelection resolves false without creating or clearing anything.',
    plugins: WITH_SELECTION,
    engine: {
      documents: {
        a: withService([hydrate(), ...reads], { security: { allows: ['doc.annotate.read', 'doc.text.select', ...APPLY_CAPS] } }),
      },
    },
    snapshot: 'none',
    steps: [...boot, ...load, sel('select', [range([1, 0], [1, 4])]), call('canMark'), call('queueCurrentSelection'), sel('hasSelection')],
  },
  {
    name: 'the-redact-tool',
    covers:
      'enableRedact activates the composed redact tool; isRedactActive follows it; toggleRedact switches between redact and pointer. A drag off text under the tool draws an area mark, pending as area-kind.',
    plugins: WITH_SELECTION,
    engine: {
      documents: {
        a: withService([
          hydrate(),
          ...reads,
          { path: 'page.annotations.create', pon: 1, value: created(redact(30, { rect: { left: 300, bottom: 200, right: 400, top: 300 } })) },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...load,
      call('isRedactActive'),
      call('enableRedact'),
      call('isRedactActive'),
      ix('activeToolId'),
      ptr('down', 300, 300),
      ptr('move', 320, 280),
      ptr('move', 400, 200),
      ptr('up', 400, 200),
      call('getPending'),
      call('toggleRedact'),
      call('isRedactActive'),
      ix('activeToolId'),
      call('toggleRedact'),
      call('isRedactActive'),
    ],
  },
  {
    name: 'without-a-selection-plugin',
    covers:
      'Without the selection plugin the composed redact tool never registers: enableRedact and toggleRedact throw the hub’s unknown-tool error, and queueCurrentSelection resolves false.',
    plugins: PLUGINS,
    engine: { documents: { a: withService([hydrate()]) } },
    snapshot: 'none',
    steps: [...boot, call('isRedactActive'), call('enableRedact'), call('toggleRedact'), call('queueCurrentSelection')],
  },
  {
    name: 'pending-projection',
    covers:
      'getPending projects every loaded redact annotation in page order: its model id (obj: and nm:), ref, page, display index, area or text kind, and overlay label. preparePending asks the annotation plane for every page and resolves.',
    plugins: PLUGINS,
    engine: { documents: { a: withService([hydrate(seed)]) } },
    snapshot: 'none',
    steps: [...boot, call('preparePending'), call('getPending'), call('pendingCount')],
  },
  {
    name: 'estimate-collateral',
    covers:
      'estimateCollateral counts the non-redact annotations whose rect has positive-area overlap with a mark’s quads (else its rect), for all marks, for the named ones, and for none.',
    plugins: PLUGINS,
    engine: { documents: { a: withService([hydrate(seed)]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      call('estimateCollateral'),
      call('estimateCollateral', [['obj:10']]),
      call('estimateCollateral', [['obj:11']]),
      call('estimateCollateral', [['nm:mark-b']]),
      call('estimateCollateral', [[]]),
      call('estimateCollateral', [['obj:1', 'nope']]),
    ],
  },
  {
    name: 'apply-named-marks',
    covers:
      'apply(ids) sends the matching pending refs as an annotations scope; the slice is applying until the engine answers, then records the result, fires onApplied and resolves with it. The unsubscribe stops later notifications and answers whether it removed anything.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([hydrate(seed), { path: 'redaction.apply', args: [refsScope], defer: 'apply' }, { path: 'redaction.apply', value: applyResult({ kind: 'annotations', refs: [objRef(11)] }, 0) }]),
      },
    },
    snapshot: ['redaction::a'],
    steps: [
      ...boot,
      call('onApplied', [{ $fn: 'applied' }]),
      call('apply', [['obj:10', 'nm:mark-b', 'obj:1', 'missing']]),
      call('isApplying'),
      settle('apply', applyResult(refsScope)),
      call('isApplying'),
      call('lastResult'),
      ref(0),
      ref(0),
      call('apply', [['obj:11']]),
    ],
  },
  {
    name: 'apply-all-pages',
    covers: 'applyAll sends every page of the document as a pages scope, including pages without marks.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([hydrate(seed), { path: 'redaction.apply', args: [pagesScope], value: applyResult(pagesScope, 3) }]),
      },
    },
    snapshot: ['redaction::a'],
    steps: [...boot, call('onApplied', [{ $fn: 'first' }]), call('onApplied', [{ $fn: 'second' }]), call('applyAll')],
  },
  {
    name: 'apply-refused',
    covers:
      'An engine refusal ends the apply with a null result, which keeps the last result, and rejects with the engine’s error.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([
          hydrate(seed),
          { path: 'redaction.apply', times: 1, value: applyResult(pagesScope, 1) },
          {
            path: 'redaction.apply',
            error: { name: 'PermissionDenied', message: 'permission denied (local): doc.redact', required: 'doc.redact', context: 'local' },
          },
        ]),
      },
    },
    snapshot: ['redaction::a'],
    steps: [...boot, call('onApplied', [{ $fn: 'applied' }]), call('applyAll'), call('apply', [['obj:10']]), call('lastResult'), call('isApplying')],
  },
  {
    name: 'apply-refusals-before-the-engine',
    covers:
      'apply rejects when no pending id matches (an empty list, no argument, an unknown id); applyAll rejects for a document with no pages; both reject without the engine redaction service. None of them dispatches.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([hydrate(seed)]),
        b: withService([hydrate()], { pages: [] }),
        c: doc([hydrate(seed)]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      openDoc('b'),
      openDoc('c'),
      call('apply', [[]]),
      call('apply'),
      call('apply', [['obj:99']]),
      call('applyAll', [], 'b'),
      call('pendingCount', [], 'b'),
      call('preparePending', [], 'b'),
      call('apply', [['obj:10']], 'c'),
      call('applyAll', [], 'c'),
    ],
  },
  {
    name: 'remote-applies-arrive-as-events',
    covers:
      'A redaction.applied event from a remote origin is recorded as the last result and fires onApplied; a local one (own applies flow through apply) and other event types are ignored. Closing the document releases the subscription.',
    plugins: PLUGINS,
    engine: { documents: { a: withService([hydrate(seed), ...reloadPages]) } },
    snapshot: ['redaction::a'],
    steps: [
      ...boot,
      call('onApplied', [{ $fn: 'applied' }]),
      emit({ type: 'redaction.applied', origin: LOCAL, ...applyResult(pagesScope) }),
      emit({ type: 'annotations.created', origin: REMOTE, pageObjectNumber: 1 }),
      call('lastResult'),
      emit({ type: 'redaction.applied', origin: REMOTE, ...applyResult(pagesScope, 4) }),
      call('lastResult'),
      call('getPending'),
      { op: 'call', target: 'documents', method: 'close', args: ['a'] },
    ],
  },
  {
    name: 'a-throwing-listener-is-isolated',
    covers: 'A throwing onApplied callback is reported to console.error and the next callback still runs.',
    plugins: PLUGINS,
    engine: { documents: { a: withService([hydrate(seed), { path: 'redaction.apply', value: applyResult(pagesScope) }]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      call('onApplied', [{ $fn: 'bad', throws: { name: 'Error', message: 'listener broke' } }]),
      call('onApplied', [{ $fn: 'good' }]),
      call('applyAll'),
    ],
  },
  {
    name: 'set-label',
    covers:
      'setLabel writes the label fields the patch names (null clears the text) and always carries the mark’s current font family, size, colour and alignment; it rejects for a non-redact annotation and for an unknown ref.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: withService([
          hydrate(seed),
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated({ ...areaMark, overlayText: 'SECRET' }) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated({ ...areaMark, overlayText: null, repeat: true }) },
          { path: 'page.annotations.update', pon: 1, times: 1, value: updated({ ...areaMark, overlayText: null, repeat: true }) },
          {
            path: 'page.annotations.update',
            pon: 2,
            value: updated({ ...nmMark, overlayText: 'B', fontFamily: 'courier', fontSize: 9 }),
          },
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('setLabel', [objRef(10), { overlayText: 'SECRET' }]),
      call('getPending'),
      call('setLabel', [objRef(10), { overlayText: null, repeat: true }]),
      call('setLabel', [objRef(10), {}]),
      call('setLabel', [{ kind: 'nm', page: pr(2), nm: 'mark-b' }, { overlayText: 'B', repeat: false }]),
      call('setLabel', [objRef(1), { overlayText: 'x' }]),
      call('setLabel', [objRef(77), { overlayText: 'x' }]),
    ],
  },
];

// ── functions ─────────────────────────────────────────────────────────────────

/**
 * `estimateCollateral` reads only the document's page list and the
 * annotation plane's `listRaw({page})`, so a function case drives the real
 * pending reads over a stub: `pages` is the document's page list (`null` for
 * no document) and `byPage` the annotations `listRaw` answers per object
 * number. A filter lists v2 mark ids, turned into refs.
 */
const stubReads = (pages: { pageObjectNumber: number }[] | null, byPage: Record<string, unknown[]>) =>
  createPendingReads({
    store: {
      pages: () => (pages ?? []).map((p) => pr(p.pageObjectNumber)),
      pageIndexOf: () => -1,
    },
    siblings: {
      annotation: { listRaw: ({ page }: { page: { pageObjectNumber: number } }) => byPage[String(page.pageObjectNumber)] ?? [] },
    },
  } as never).api;

const functions = {
  initialRedactionState,
  redactionReducer: (state: RedactionState, action: RedactionAction) => {
    const next = redactionReducer(state, action);
    return { changed: next !== state, state: next };
  },
  estimateCollateral: (pages: { pageObjectNumber: number }[] | null, byPage: Record<string, unknown[]>, ids?: unknown[] | null) =>
    stubReads(pages, byPage).estimateCollateral(ids ? (ids.map(idRef) as never) : undefined),
};

const RESULT = applyResult(pagesScope);
const busy: RedactionState = { applying: true, lastResult: null };
const done: RedactionState = { applying: false, lastResult: RESULT as never };

const r = (name: string, state: RedactionState, action: unknown, covers?: string): FunctionCase => ({
  name,
  fn: 'redactionReducer',
  args: [state, action],
  ...(covers === undefined ? {} : { covers }),
});

const page = (index: number, pon: number) => ({ index, pageObjectNumber: pon });
const rect = (left: number, bottom: number, right: number, top: number) => ({ left, bottom, right, top });
const mark = (n: number, over: Record<string, unknown> = {}) => redact(n, over);
const other = (n: number, box: ReturnType<typeof rect>, over: Record<string, unknown> = {}) => square(n, { rect: box, ...over });
/** A mark's region is its quads' axis-aligned boxes, however the quad is wound. */
const tilted = { p1: { x: 150, y: 100 }, p2: { x: 200, y: 150 }, p3: { x: 100, y: 150 }, p4: { x: 150, y: 200 } };
const NAN = { $number: 'NaN' };

const c = (name: string, byPage: Record<string, unknown[]>, ids?: unknown, pages: unknown[] | null = [page(0, 1)], covers?: string): FunctionCase => ({
  name,
  fn: 'estimateCollateral',
  args: ids === undefined ? [pages, byPage] : [pages, byPage, ids],
  ...(covers === undefined ? {} : { covers }),
});

const functionCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialRedactionState', args: [] },
  r('started-from-idle', initialRedactionState(), { type: 'APPLY_STARTED' }),
  r('started-keeps-the-last-result', done, { type: 'APPLY_STARTED' }, 'APPLY_STARTED sets applying and keeps lastResult.'),
  r('finished-with-a-result', busy, { type: 'APPLY_FINISHED', result: RESULT }),
  r('finished-replaces-the-result', { applying: true, lastResult: RESULT as never }, { type: 'APPLY_FINISHED', result: applyResult(pagesScope, 9) }),
  r('finished-with-null-keeps-the-result', { applying: true, lastResult: RESULT as never }, { type: 'APPLY_FINISHED', result: null }, 'A failed apply (result null) clears applying and keeps the previous result.'),
  r('finished-without-a-result-member', { applying: true, lastResult: RESULT as never }, { type: 'APPLY_FINISHED' }),
  r('finished-when-idle', initialRedactionState(), { type: 'APPLY_FINISHED', result: null }, 'APPLY_FINISHED always makes a new state, even when nothing changes.'),
  r('unknown-action', done, { type: 'OTHER' }, 'An action outside the union returns the same state.'),

  c('no-document', { 1: [mark(1), other(2, rect(100, 600, 200, 700))] }, undefined, null, 'Without a document there are no pages, so nothing is counted.'),
  c('no-pages', { 1: [mark(1)] }, undefined, []),
  c('no-marks', { 1: [other(1, rect(0, 0, 10, 10)), other(2, rect(0, 0, 10, 10))] }),
  c('rect-mark-overlaps', { 1: [mark(1), other(2, rect(150, 650, 250, 750)), other(3, rect(300, 300, 400, 400))] }),
  c('touching-edges-do-not-count', { 1: [mark(1), other(2, rect(200, 600, 300, 700)), other(3, rect(100, 700, 200, 800)), other(4, rect(0, 500, 100, 600))] }, undefined, undefined, 'Intersection needs positive area: shared edges and corners are not collateral.'),
  c('contained-and-containing', { 1: [mark(1), other(2, rect(120, 620, 130, 630)), other(3, rect(0, 0, 600, 800))] }),
  c('sliver-overlap', { 1: [mark(1), other(2, rect(199.999, 600, 300, 700)), other(3, rect(100, 699.5, 200, 701))] }),
  c('quads-replace-the-rect', { 1: [mark(1, { rect: rect(0, 0, 600, 800), quadPoints: [quad(100, 100, 50, 10), quad(300, 100, 50, 10)] }), other(2, rect(120, 105, 130, 108)), other(3, rect(200, 100, 250, 110)), other(4, rect(340, 90, 360, 120))] }, undefined, undefined, 'A mark with quads targets the quads’ boxes only, not its /Rect.'),
  c('tilted-quad-box', { 1: [mark(1, { quadPoints: [tilted] }), other(2, rect(195, 195, 210, 210)), other(3, rect(100, 100, 101, 101)), other(4, rect(201, 150, 210, 160))] }),
  c('one-annotation-counts-once', { 1: [mark(1), mark(2, { rect: rect(150, 650, 160, 660) }), other(3, rect(140, 640, 170, 670))] }, undefined, undefined, 'An annotation under several marks is one piece of collateral.'),
  c('marks-are-never-collateral', { 1: [mark(1), mark(2, { rect: rect(150, 650, 250, 750) })] }),
  c('filter-by-id', { 1: [mark(1), mark(2, { rect: rect(300, 300, 400, 400) }), other(3, rect(150, 650, 160, 660)), other(4, rect(350, 350, 360, 360))] }, ['obj:2']),
  c('filter-matches-nothing', { 1: [mark(1), other(3, rect(150, 650, 160, 660))] }, ['obj:3', 'nm:x']),
  c('empty-filter', { 1: [mark(1), other(3, rect(150, 650, 160, 660))] }, []),
  c('null-filter-is-all', { 1: [mark(1), other(3, rect(150, 650, 160, 660))] }, null),
  c('filter-by-nm-and-index', {
    1: [
      mark(1, { ref: { kind: 'nm', page: pr(1), nm: 'n1' } }),
      mark(2, { ref: { kind: 'index', page: pr(1), index: 4, revision: { docSessionId: 's', page: pr(1), generation: 2 } }, rect: rect(300, 300, 400, 400) }),
      other(3, rect(150, 650, 160, 660)),
      other(4, rect(350, 350, 360, 360)),
    ],
  }, ['idx:1:4']),
  c('pages-in-document-order', {
    1: [mark(1), other(2, rect(150, 650, 160, 660))],
    2: [mark(3, { pageObjectNumber: 2 }), other(4, rect(150, 650, 160, 660), { pageObjectNumber: 2 }), other(5, rect(0, 0, 1, 1), { pageObjectNumber: 2 })],
    3: [mark(6, { pageObjectNumber: 3 }), other(7, rect(150, 650, 160, 660), { pageObjectNumber: 3 })],
  }, undefined, [page(0, 2), page(1, 1)], 'Only pages in the document are counted, each against its own marks.'),
  c('nan-geometry-never-intersects', { 1: [mark(1), other(2, rect(NAN as never, 600, 200, 700)), mark(3, { quadPoints: [{ ...quad(100, 600, 10, 10), p1: { x: NAN, y: 610 } }] }), other(4, rect(100, 600, 105, 605))] }),
  c('negative-and-fractional', { 1: [mark(1, { rect: rect(-50.5, -50.5, -10.25, -10.25) }), other(2, rect(-20, -20, 0, 0)), other(3, rect(-10.25, -60, 0, 0))] }),

];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/redaction.json',
    describes: 'packages/plugin/redaction: marking from a selection and by area, the pending view, collateral, labels, the apply flow over the engine’s redaction service, remote applies, and the permission twins.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/redaction-functions.json',
    describes: 'packages/plugin/redaction: the reducer, and estimateCollateral over a stub annotation plane.',
    functions,
    cases: functionCases,
  },
];

