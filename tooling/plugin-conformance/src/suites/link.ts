/**
 * `packages/plugin/link`: the navigation plane of link annotations. The
 * scenarios run the real interaction, annotation, actions and stage plugins
 * beside this one, in the combinations a deployment ships: viewer-only (the
 * engine-backed page cache), annotation-backed (the folded model and the
 * `link-nav` behavior), with a stage (reveal), and with the action engine
 * (dispatch). Replayed by `crates/plugins/link/tests/conformance.rs`.
 *
 * One shim, mirrored by `crates/plugins/link/src/harness.rs`: the `link`
 * factory's `onActivate` config is a scripted callback spec. The event it
 * receives can carry the in-flight dispatch promise, which a callback's
 * logged arguments cannot print, so the shim logs that member as the string
 * `[promise]`.
 */
import type { AnyPlugin } from '@embedpdf/core';

import { linkPlugin } from '../../../../packages/plugin/link/src/link.plugin';
import { initialLinkState, linkReducer } from '../../../../packages/plugin/link/src/reducer';
import { LinkToken, type LinkActivateEvent, type LinkAction, type LinkState } from '../../../../packages/plugin/link/src/types';

import type { ResponseSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';
import { scriptedCallback, type AnswerSpec } from './acrojs';

import {
  CROP2,
  FLAGS,
  boot,
  call as anno,
  created,
  doc,
  emit,
  hydrate,
  ix,
  layout,
  link,
  objRef,
  settle,
  square,
  updated,
} from './annotation-fixtures';

// ── the factory ───────────────────────────────────────────────────────────────

type Scripted = AnswerSpec & { readonly $fn: string };
interface HarnessConfig {
  readonly onActivate?: Scripted;
}

const printable = (event: LinkActivateEvent) => ({
  target: event.target,
  activation: event.activation.outcome === 'dispatched' ? { outcome: 'dispatched', dispatch: '[promise]' } : event.activation,
});

registerPlugin('link', {
  factory: (config, env) => {
    const { onActivate } = (config ?? {}) as HarnessConfig;
    if (!onActivate) return linkPlugin() as AnyPlugin;
    const callback = scriptedCallback(onActivate.$fn, onActivate, env);
    return linkPlugin({ onActivate: (event) => void callback(printable(event)) }) as AnyPlugin;
  },
  tokens: [LinkToken],
});

// ── steps ─────────────────────────────────────────────────────────────────────

const lk = (method: string, args: unknown[] = [], docId = 'a'): Step => ({
  op: 'call',
  target: { token: 'link', doc: docId },
  method,
  args,
});
const st = (method: string, args: unknown[] = []): Step => ({ op: 'call', target: { token: 'stage', doc: 'a' }, method, args });
const ref = (n: number, args: unknown[] = []): Step => ({ op: 'call', target: { ref: n }, args });
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
const viewport: Step = st('setViewport', [{ width: 1000, height: 700 }]);

// ── plugins ───────────────────────────────────────────────────────────────────

const LOGGED = { factory: 'link', config: { onActivate: { $fn: 'onActivate' } } };
const INTERACTION = { factory: 'interaction' };
const STAGE = { factory: 'stage', config: { viewUnitsPerPoint: 1, scrollBehavior: 'instant' } };
const ACTIONS = { factory: 'actions', config: { openSequence: 'off' } };
const ANNOTATION = { factory: 'annotation' };

const VIEWER = [INTERACTION, LOGGED];
const WITH_STAGE = [INTERACTION, STAGE, LOGGED];
const WITH_ANNOTATION = [INTERACTION, ANNOTATION, LOGGED];
const FULL = [INTERACTION, ACTIONS, STAGE, ANNOTATION, LOGGED];
const SNAP = ['link::a'];

// ── fixtures ──────────────────────────────────────────────────────────────────

const goto = (destination: Record<string, unknown>) => ({ kind: 'goto', destination });
const xyz = (pon: number, left: unknown, top: unknown, zoom: unknown) => ({ kind: 'xyz', pageObjectNumber: pon, left, top, zoom });
const uri = (u: string) => ({ kind: 'uri', uri: u });
const named = (name: string) => ({ kind: 'named', name });
const tree = (root: unknown) => ({ root, incomplete: false, warningFlags: 0, warnings: [] });
const gotoNode = (destination: Record<string, unknown>) => ({ type: 'goto', subtype: 'GoTo', destination, next: [] });
const uriNode = (u: string) => ({ type: 'uri', subtype: 'URI', uri: u, next: [] });

const list = (annotations: unknown[]) => ({ annotations });
const omit = (dto: Record<string, unknown>, key: string) => {
  const { [key]: _omitted, ...rest } = dto;
  return rest;
};
const pageList = (pon: number, annotations: unknown[], times?: number): ResponseSpec => ({
  path: 'page.annotations.list',
  pon,
  value: list(annotations),
  ...(times === undefined ? {} : { times }),
});

/** Page 1 of the viewer-only suite: every shape `loadLinksPage` sorts. */
const PAGE1 = [
  link(1, { rect: { left: 100, bottom: 700, right: 200, top: 720 } }),
  link(2, {
    ref: { kind: 'nm', pageObjectNumber: 1, nm: 'n-2' },
    target: goto(xyz(2, 20, 500, 0)),
    rect: { left: 0, bottom: 0, right: 50, top: 10 },
  }),
  link(3, {
    ref: { kind: 'index', pageObjectNumber: 1, index: 3, revision: { docSessionId: 's', pageObjectNumber: 1, generation: 1 } },
    target: named('NextPage'),
  }),
  square(4),
  link(5, { target: null }),
  (() => {
    const { target: _omitted, ...rest } = link(6);
    return rest;
  })(),
  link(7, { flags: FLAGS({ hidden: true }) }),
  link(8, { flags: FLAGS({ noView: true }) }),
  link(9, { flags: FLAGS({ invisible: true }), target: { kind: 'javascript' } }),
  link(10, { actions: { activate: tree(uriNode('https://a.example')) } }),
  link(11, { actions: { cursorEnter: { root: uriNode('https://enter.example') } } }),
  link(12, { actions: { cursorExit: { root: uriNode('https://exit.example') }, activate: null } }),
  link(13, { actions: { cursorEnter: { root: null }, cursorExit: {} } }),
  link(14, { actions: {} }),
  link(15, { replyType: 'group', inReplyTo: objRef(4) }),
  link(16, { replyType: 'group', inReplyTo: null }),
  link(17, { replyType: 'reply', inReplyTo: objRef(4) }),
];
/** Page 2 sits on an offset crop, so its rects shift. */
const PAGE2 = [link(20, { pageObjectNumber: 2, ref: objRef(20, 2), rect: { left: 30, bottom: 40, right: 130, top: 90 } })];

const annotationDoc = (byPage: Record<number, unknown[]>, extra: ResponseSpec[] = []) =>
  doc([hydrate(byPage as never), ...extra]);

const FORMS_EMPTY: ResponseSpec = { path: 'forms.list', value: { formKind: 'none', needsAppearances: false, fields: [], calculationOrder: [] } };

// ── scenarios ─────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  {
    name: 'viewer-only-page-cache',
    covers:
      'Without the annotation plugin linksOn reads the engine-backed cache (empty until loaded). ensurePage lists the page once, keeps visible links with a target (hidden and noView dropped, invisible kept), ids obj: refs by object number and every other ref by page and index, converts the rect through the page crop, copies a truthy activate tree, flags hover events from either cursor root, and marks group children attached. A loaded page, or one the document does not have, asks nothing.',
    plugins: VIEWER,
    engine: { documents: { a: doc([pageList(1, PAGE1), pageList(2, PAGE2)]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('linksOn', [1]),
      lk('ensurePage', [1]),
      lk('linksOn', [1]),
      lk('ensurePage', [1]),
      lk('ensurePage', [2]),
      lk('linksOn', [2]),
      lk('ensurePage', [99]),
      lk('linksOn', [99]),
      lk('engaged'),
    ],
  },
  {
    name: 'viewer-only-malformed-lists',
    covers:
      'A list the fulfilled handler throws on (no annotations member, a null entry, a link without flags, ref or rect) caches nothing; a non-link without flags is skipped before flags are read. A missing index prints as undefined in the id and a rect without edges converts to NaN.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc(
          [
            { path: 'page.annotations.list', pon: 1, value: {} },
            pageList(2, [link(1), null]),
            pageList(3, [link(1), omit(link(2), 'flags')]),
            pageList(4, [omit(link(1), 'ref')]),
            pageList(5, [omit(link(1), 'rect')]),
            pageList(6, [
              omit(square(1), 'flags'),
              omit(link(3, { ref: { kind: 'nm', pageObjectNumber: 6, nm: 'x' } }), 'index'),
              link(4, { rect: {} }),
              omit(link(5, { replyType: 'group' }), 'inReplyTo'),
              link(6, { actions: { activate: 0, cursorEnter: { root: false }, cursorExit: null } }),
            ]),
          ],
          { pages: [1, 2, 3, 4, 5, 6].map((pon, i) => layout(i, pon)) },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('ensurePage', [1]),
      lk('ensurePage', [2]),
      lk('ensurePage', [3]),
      lk('ensurePage', [4]),
      lk('ensurePage', [5]),
      lk('ensurePage', [6]),
    ],
  },
  {
    name: 'viewer-only-loads-in-flight',
    covers:
      'Until a list answers the page is not cached, so a second ensurePage lists it again; each answer replaces the page. A refused list is swallowed and leaves the page unloaded, so the next ensurePage retries.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          { path: 'page.annotations.list', pon: 1, times: 1, defer: 'first' },
          { path: 'page.annotations.list', pon: 1, times: 1, defer: 'second' },
          { path: 'page.annotations.list', pon: 2, times: 1, error: { name: 'EngineError', message: 'refused', code: 'Refused' } },
          pageList(2, PAGE2),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('ensurePage', [1]),
      lk('ensurePage', [1]),
      settle('second', list([link(1)])),
      settle('first', list([link(1), link(2)])),
      lk('linksOn', [1]),
      lk('ensurePage', [2]),
      lk('linksOn', [2]),
      lk('ensurePage', [2]),
      lk('linksOn', [2]),
    ],
  },
  {
    name: 'viewer-only-refetch-on-events',
    covers:
      'Created, updated, moved (by its first entry) and deleted annotation events refetch a cached page; an empty move, an uncached page and any other event ask nothing. Closing the document releases the subscription.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          pageList(1, [link(1)], 1),
          pageList(1, [link(1), link(2)], 1),
          pageList(1, [link(2)], 1),
          pageList(1, [link(2), link(3)], 1),
          pageList(1, [], 1),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('ensurePage', [1]),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: { kind: 'remote', serverId: 's' }, ...created(link(2)) }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: { kind: 'local', serverId: null }, ...updated(link(2)) }),
      emit({ type: 'annotation.moved', pageObjectNumber: 1, origin: { kind: 'local', serverId: null }, moved: [link(3), link(2)] }),
      emit({ type: 'annotation.moved', pageObjectNumber: 1, origin: { kind: 'local', serverId: null }, moved: [] }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 1, origin: { kind: 'local', serverId: null }, deleted: objRef(3) }),
      emit({ type: 'annotation.created', pageObjectNumber: 2, origin: { kind: 'local', serverId: null }, ...created(link(9, { pageObjectNumber: 2 })) }),
      emit({ type: 'annotation.updated', pageObjectNumber: 1, origin: { kind: 'local', serverId: null }, updated: link(4, { pageObjectNumber: 2 }) }),
      emit({ type: 'pages.rotated', pageObjectNumbers: [1], origin: { kind: 'local', serverId: null } }),
      lk('linksOn', [1]),
      close,
    ],
  },
  {
    name: 'annotation-backed-source',
    covers:
      'With the annotation plugin linksOn is its folded model (hidden links excluded, no engine read), ensurePage defers to it, and annotation events never make this plugin fetch: the cache stays empty.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: annotationDoc({
          1: [link(1), link(2, { flags: FLAGS({ hidden: true }) }), link(3, { actions: { activate: tree(uriNode('https://a.example')) } }), square(4)],
          2: PAGE2,
        }),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('linksOn', [1]),
      lk('ensurePage', [1]),
      lk('linksOn', [2]),
      lk('ensurePage', [3]),
      emit({ type: 'annotation.created', pageObjectNumber: 1, origin: { kind: 'remote', serverId: 's' }, ...created(link(5)) }),
      lk('linksOn', [1]),
    ],
  },
  {
    name: 'link-nav-behavior',
    covers:
      'With the annotation plugin init registers the link-nav behavior: it matches link annotations only and is engaged while the active tool enables link-nav, as engaged() is. The link tool (no link-nav) hands links to the annotation plane; pan takes them back.',
    plugins: WITH_ANNOTATION,
    engine: { documents: { a: annotationDoc({ 1: [link(1), square(2)] }) } },
    snapshot: 'none',
    steps: [
      ...boot,
      lk('engaged'),
      anno('behaviorFor', [{ subtype: 'link', ref: objRef(1) }]),
      ref(0, [{ subtype: 'square', ref: objRef(2) }]),
      ref(0, [{ subtype: 'link', ref: null }]),
      ref(1),
      anno('behaviorFor', [{ subtype: 'square', ref: objRef(2) }]),
      ix('activateTool', ['link']),
      lk('engaged'),
      ref(1),
      anno('behaviorFor', [{ subtype: 'link', ref: objRef(1) }]),
      ix('activateTool', ['pan']),
      lk('engaged'),
      ref(1),
    ],
  },
  {
    name: 'engaged-follows-the-active-tool',
    covers: 'Without the annotation plugin engaged() still reads the hub: true under pointer, false under a tool without link-nav.',
    plugins: VIEWER,
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      lk('engaged'),
      ix('registerTool', [{ id: 'plain', cursor: 'default', enables: { $set: ['text-select'] } }]),
      ix('activateTool', ['plain']),
      lk('engaged'),
      ix('activateTool', ['pan']),
      lk('engaged'),
    ],
  },
  {
    name: 'activate-without-a-stage',
    covers:
      'With no stage a goto hands back its explicit destination; uri and named report their member; javascript, goto-remote, launch and unsupported are reported whole. An activate tree without the actions plugin takes the same classic path. onActivate sees every activation.',
    plugins: VIEWER,
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      lk('activate', [goto(xyz(2, 10, 20, 3))]),
      lk('activate', [uri('https://example.com/x')]),
      lk('activate', [named('LastPage')]),
      lk('activate', [{ kind: 'javascript' }]),
      lk('activate', [{ kind: 'goto-remote', file: 'other.pdf' }]),
      lk('activate', [{ kind: 'launch', path: 'calc.exe' }]),
      lk('activate', [{ kind: 'unsupported' }]),
      lk('activate', [goto({ kind: 'fit', pageObjectNumber: 1 }), { activate: tree(gotoNode({ kind: 'fit', pageObjectNumber: 1 })), ref: objRef(1), pon: 1 }]),
    ],
  },
  {
    name: 'activate-without-a-listener',
    covers: 'Without an onActivate config activation still runs and returns its outcome.',
    plugins: [INTERACTION, { factory: 'link' }],
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [...boot, lk('activate', [uri('https://example.com')]), lk('activate', [named('FirstPage')])],
  },
  {
    name: 'activate-reveals-through-the-stage',
    covers:
      'With a stage a goto to a page of the document reveals its destination smoothly (xyz, fit, fitH, fitR on an offset crop) and reports revealed; a goto to a page the document lacks hands back the destination.',
    plugins: WITH_STAGE,
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      viewport,
      lk('activate', [goto(xyz(2, 60, 300, 2))]),
      st('camera'),
      st('currentPage'),
      lk('activate', [goto(xyz(1, null, null, 0))]),
      st('camera'),
      lk('activate', [goto({ kind: 'fit', pageObjectNumber: 1 })]),
      st('camera'),
      lk('activate', [goto({ kind: 'fitH', pageObjectNumber: 2, top: 400 })]),
      st('camera'),
      lk('activate', [goto({ kind: 'fitR', pageObjectNumber: 2, left: 50, bottom: 100, right: 250, top: 300 })]),
      st('camera'),
      lk('activate', [goto(xyz(77, 0, 0, 1))]),
      lk('activate', [uri('https://example.com')]),
    ],
  },
  {
    name: 'activate-destination-that-cannot-invert',
    covers:
      'A goto to a page whose userUnit is 0 throws the geometry error out of activate before onActivate runs.',
    plugins: WITH_STAGE,
    engine: { documents: { a: doc([], { pages: [layout(0, 1), { ...layout(1, 2, CROP2), userUnit: 0 }] }) } },
    snapshot: 'none',
    steps: [...boot, viewport, lk('activate', [goto(xyz(2, 60, 300, 2))]), lk('activate', [goto({ kind: 'fit', pageObjectNumber: 1 })])],
  },
  {
    name: 'a-throwing-listener-throws-out-of-activate',
    covers: 'onActivate runs after the built-in handling, so its throw leaves the reveal done and escapes activate.',
    plugins: [INTERACTION, STAGE, { factory: 'link', config: { onActivate: { $fn: 'onActivate', throws: { name: 'Error', message: 'listener broke' } } } }],
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [...boot, viewport, lk('activate', [goto(xyz(2, 60, 300, 2))]), st('currentPage')],
  },
  {
    name: 'activate-dispatches-through-actions',
    covers:
      'With the actions plugin and an activate tree the action engine takes the activation as a user dispatch from a link source (ref and pon when given) and the outcome carries its promise, resolved with the dispatch result; the goto executor reveals. A context without a truthy tree, or none at all, takes the classic path even with actions present.',
    plugins: FULL,
    engine: {
      documents: {
        a: annotationDoc(
          {
            1: [
              link(1, {
                target: goto(xyz(2, 60, 300, 2)),
                actions: { activate: tree(gotoNode(xyz(2, 60, 300, 2))) },
              }),
            ],
          },
          [FORMS_EMPTY, pageList(1, []), pageList(2, [])],
        ),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      viewport,
      lk('linksOn', [1]),
      lk('activate', [goto(xyz(2, 60, 300, 2)), { activate: tree(gotoNode(xyz(2, 60, 300, 2))), ref: objRef(1), pon: 1 }]),
      st('currentPage'),
      lk('activate', [uri('https://example.com'), { activate: tree(uriNode('https://example.com')) }]),
      lk('activate', [goto({ kind: 'fit', pageObjectNumber: 1 }), { activate: null, ref: objRef(1), pon: 1 }]),
      st('currentPage'),
      lk('activate', [named('NextPage'), {}]),
      lk('activate', [named('NextPage')]),
    ],
  },
];

// ── functions ─────────────────────────────────────────────────────────────────

const functions = {
  initialLinkState,
  linkReducer: (state: LinkState, action: LinkAction) => {
    const next = linkReducer(state, action);
    return { changed: next !== state, state: next };
  },
};

const item = (n: number, over: Record<string, unknown> = {}) => ({
  id: `obj:${n}`,
  rect: { x: n, y: 10, width: 20, height: 5 },
  target: uri(`https://example.com/${n}`),
  ref: objRef(n),
  attached: false,
  ...over,
});
const loaded: LinkState = { pages: { 1: [item(1)] as never, 3: [] } };

const r = (name: string, state: LinkState, action: unknown, covers?: string): FunctionCase => ({
  name,
  fn: 'linkReducer',
  args: [state, action],
  ...(covers === undefined ? {} : { covers }),
});

const functionCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialLinkState', args: [] },
  r('set-a-first-page', initialLinkState(), { type: 'SET_PAGE', pon: 1, items: [item(1)] }),
  r('set-another-page', loaded, { type: 'SET_PAGE', pon: 2, items: [item(2, { activate: tree(uriNode('u')), hoverEvents: { enter: true, exit: false } })] }),
  r('set-replaces-a-page', loaded, { type: 'SET_PAGE', pon: 1, items: [item(4), item(5, { attached: true })] }, 'SET_PAGE replaces the page and keeps the others.'),
  r('set-an-empty-page', loaded, { type: 'SET_PAGE', pon: 0, items: [] }),
  r('drop-a-page', loaded, { type: 'DROP_PAGE', pon: 1 }),
  r('drop-an-empty-page', loaded, { type: 'DROP_PAGE', pon: 3 }, 'A page cached with no links is still present, so dropping it makes a new state.'),
  r('drop-an-absent-page', loaded, { type: 'DROP_PAGE', pon: 2 }, 'Dropping a page that is not cached returns the same state.'),
  r('drop-from-empty', initialLinkState(), { type: 'DROP_PAGE', pon: 1 }),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/link.json',
    describes:
      'packages/plugin/link: the engine-backed and annotation-backed link sources, refetch on annotation events, the link-nav behavior and engaged(), and activate through the stage, the action engine, or back to the embedder.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/link-functions.json',
    describes: 'packages/plugin/link: the page-cache reducer and its initial state.',
    functions,
    cases: functionCases,
  },
];
