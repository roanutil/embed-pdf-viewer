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
import type {
  Annotation,
  PageDestination,
  PdfActionNode,
  PdfLinkTarget,
} from '@embedpdf/engine-core/runtime';
import { resolveLinkTarget } from '../../../../packages/plugin/link/src/contract';
import { baseRef } from './annotation-current-fixtures';
import { tree, FORMS_EMPTY, catalog } from './actions-current-fixtures';

import { linkPlugin } from '../../../../packages/plugin/link/src/link.plugin';
import {
  LinkToken,
  type LinkActivatedEvent,
  type LinkHostCapability,
} from '../../../../packages/plugin/link/src/host-contract';

import type { ResponseSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';
import { scriptedCallback, type AnswerSpec } from './acrojs';
import { stageStep } from './stage-plugin';

import {
  boot,
  call as anno,
  doc,
  emit,
  hydrate,
  layout,
  link,
  objRef,
  pr,
  settle,
  square,
  createdFact,
  updatedFact,
  deletedFact,
  REMOTE,
  write,
  updated,
  deleted,
} from './annotation-live-fixtures';
const CROP2 = { left: 10, bottom: 20, right: 410, top: 520 };
const ix = (method: string, args: unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'interaction' },
  method,
  args,
});

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason:
    'Pins activation outcomes, reveals and hit tests, through call outcomes, onActivate callbacks and engine calls. Its dispatched actions are compared; the state they build is not.',
} as const;

// ── the factory ───────────────────────────────────────────────────────────────

type Scripted = AnswerSpec & { readonly $fn: string };
interface HarnessConfig {
  readonly onActivate?: Scripted;
}

const printable = (event: LinkActivatedEvent) => ({
  target: event.target,
  activation:
    event.activation.outcome === 'dispatched'
      ? { outcome: 'dispatched', dispatch: '[promise]' }
      : event.activation,
});

registerPlugin('link', {
  factory: (config, env) => {
    const { onActivate } = (config ?? {}) as HarnessConfig;
    const plugin = linkPlugin();
    if (!onActivate) return plugin;
    const callback = scriptedCallback(onActivate.$fn, onActivate, env);
    return {
      ...plugin,
      create: (ctx) => {
        const built = plugin.create(ctx);
        ctx.cleanup(built.api.onActivated((event) => void callback(printable(event))));
        return built;
      },
    };
  },
  tokens: [LinkToken],
});

// ── steps ─────────────────────────────────────────────────────────────────────

// Scripted hooks are the serialized host boundary; every other argument is
// checked against the shipping capability before the case is recorded.
function lk<
  M extends Exclude<keyof LinkHostCapability, 'onLoaded' | 'onActivated' | 'registerUriOpener'>,
>(method: M, args?: Parameters<LinkHostCapability[M]>, docId?: string): Step;
function lk(method: 'onLoaded' | 'onActivated', args: [Scripted], docId?: string): Step;
function lk(method: string, args: unknown[] = [], docId = 'a'): Step {
  return { op: 'call', target: { token: 'link', doc: docId }, method, args };
}

const st = (method: string, args: unknown[] = []): Step => stageStep('stage', 'a', method, args);
const ref = (n: number, args: unknown[] = []): Step => ({ op: 'call', target: { ref: n }, args });
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
const viewport: Step = st('setViewport', [{ width: 1000, height: 700 }]);

// ── plugins ───────────────────────────────────────────────────────────────────

const LOGGED = { factory: 'link', config: { onActivate: { $fn: 'onActivate' } } };
const INTERACTION = { factory: 'interaction' };
const STAGE = { factory: 'stage', config: { viewUnitsPerPoint: 1, scrollBehavior: 'instant' } };
const ACTIONS = { factory: 'actions-current', config: { openSequence: 'off' } };
const ANNOTATION = { factory: 'annotation-live' };

const VIEWER = [INTERACTION, LOGGED];
const WITH_STAGE = [INTERACTION, STAGE, LOGGED];
const WITH_ANNOTATION = [INTERACTION, ANNOTATION, LOGGED];
const FULL = [INTERACTION, ACTIONS, STAGE, ANNOTATION, LOGGED];
const SNAP = ['link::a'];

// ── fixtures ──────────────────────────────────────────────────────────────────

const goto = (destination: PageDestination): PdfLinkTarget => ({ kind: 'goto', destination });
const xyz = (
  pon: number,
  x: number | null,
  y: number | null,
  zoom: number | null,
): PageDestination => ({ kind: 'xyz', page: pr(pon), x, y, zoom });
const uri = (u: string): PdfLinkTarget => ({ kind: 'uri', uri: u });
const named = (name: string): PdfLinkTarget => ({ kind: 'named', name });
const gotoNode = (destination: PageDestination): PdfActionNode => ({
  type: 'goto',
  subtype: 'GoTo',
  destination,
  next: [],
});
const uriNode = (u: string): PdfActionNode => ({
  type: 'uri',
  subtype: 'URI',
  uri: u,
  isMap: false,
  next: [],
});
const list = (annotations: readonly Annotation[]) => ({ annotations });
const pageList = (
  pon: number,
  annotations: readonly Annotation[],
  times?: number,
): ResponseSpec => ({
  path: 'page.annotations.list',
  pon,
  value: list(annotations),
  ...(times === undefined ? {} : { times }),
});

/** Page 1 of the viewer-only suite: every shape `loadLinksPage` sorts. */
const PAGE1 = [
  link(1, { rect: { x: 100, y: 80, width: 100, height: 20 } }),
  link(2, {
    ref: baseRef(2),
    target: goto(xyz(2, 10, 20, 0)),
    rect: { x: 0, y: 790, width: 50, height: 10 },
  }),
  link(3, { ref: baseRef(3), target: named('NextPage') }),
  square(4),
  link(5, { target: null }),
  link(6, { target: null }),
  link(7, { hidden: true }),
  link(8, { noView: true }),
  link(9, { invisible: true, target: { kind: 'javascript' } }),
  link(10, { actions: { activate: tree(uriNode('https://a.example')) } }),
  link(11, { actions: { cursorEnter: tree(uriNode('https://enter.example')) } }),
  link(12, { actions: { cursorExit: tree(uriNode('https://exit.example')) } }),
  link(13, { actions: { cursorEnter: tree(null), cursorExit: tree(null) } }),
  link(14, { actions: {} }),
  link(15, { reply: { type: 'group', to: objRef(4) } }),
  link(16, { reply: null }),
  link(17, { reply: { type: 'reply', to: objRef(4) } }),
];
const PAGE2 = [
  link(20, { page: pr(2), ref: objRef(20, 2), rect: { x: 20, y: 430, width: 100, height: 50 } }),
];
const annotationDoc = (byPage: Record<number, readonly Annotation[]>, extra: ResponseSpec[] = []) =>
  doc([hydrate(Object.values(byPage).flat()), ...extra, FORMS_EMPTY, catalog()]);

// ── scenarios ─────────────────────────────────────────────────────────────────

const cases: ScenarioCase[] = [
  {
    name: 'viewer-only-page-cache',
    covers:
      'Visible current page-space annotations load once; absent pages reject not-found. Object-number and base-index references keep stable ids. Hidden and noView are filtered; invisible and reply attachment survive.',
    plugins: VIEWER,
    engine: { documents: { a: doc([pageList(1, PAGE1), pageList(2, PAGE2)]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('listLinks', [pr(1)]),
      lk('ensureLoaded', [pr(1)]),
      lk('listLinks', [pr(1)]),
      lk('ensureLoaded', [pr(1)]),
      lk('ensureLoaded', [pr(2)]),
      lk('listLinks', [pr(2)]),
      lk('ensureLoaded', [pr(99)]),
      lk('listLinks', [pr(99)]),
      lk('isNavigationEngaged'),
    ],
  },
  {
    name: 'viewer-only-malformed-lists',
    covers:
      'Malformed Annotation records are outside the current typed engine boundary. Each page read refusal is observable as error status and can be retried; no malformed DTO is manufactured.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc(
          [1, 2, 3, 4, 5, 6].flatMap((pon) => [
            {
              path: 'page.annotations.list',
              pon,
              times: 1,
              error: { name: 'EngineError', message: `refused page ${pon}`, code: 'Refused' },
            },
            pageList(pon, [link(pon, { page: pr(pon), ref: objRef(pon, pon) })]),
          ]),
          { pages: [1, 2, 3, 4, 5, 6].map((pon, i) => layout(i, pon)) },
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ...[1, 2, 3, 4, 5, 6].flatMap((n) => [
        lk('ensureLoaded', [pr(n)]),
        lk('getStatus', [pr(n)]),
        lk('ensureLoaded', [pr(n)]),
        lk('getStatus', [pr(n)]),
        lk('listLinks', [pr(n)]),
      ]),
    ],
  },
  {
    name: 'viewer-only-loads-in-flight',
    covers:
      'A second ensureLoaded while a list is in flight joins it (v2 listed the page again). A refused list leaves the page unloaded, so the next ensureLoaded retries.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          { path: 'page.annotations.list', pon: 1, times: 1, defer: 'first' },
          {
            path: 'page.annotations.list',
            pon: 2,
            times: 1,
            error: { name: 'EngineError', message: 'refused', code: 'Refused' },
          },
          pageList(2, PAGE2),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('ensureLoaded', [pr(1)]),
      lk('ensureLoaded', [pr(1)]),
      settle('first', list([link(1), link(2)])),
      lk('listLinks', [pr(1)]),
      lk('ensureLoaded', [pr(2)]),
      lk('listLinks', [pr(2)]),
      lk('ensureLoaded', [pr(2)]),
      lk('listLinks', [pr(2)]),
    ],
  },
  {
    name: 'viewer-only-refetch-on-events',
    covers:
      'Current create, update, reorder and delete facts refetch a loaded page; uncached pages and page rotation do not read. Closing releases subscriptions.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          pageList(1, [link(1)], 1),
          pageList(1, [link(1), link(2)], 1),
          pageList(1, [link(2)], 1),
          pageList(1, [link(2), link(3)], 1),
          pageList(1, [link(3)], 1),
          pageList(1, [], 1),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('ensureLoaded', [pr(1)]),
      emit(createdFact(link(2))),
      emit(updatedFact(link(2))),
      emit(updatedFact(link(3))),
      emit({
        type: 'annotations.reordered',
        page: pr(1),
        order: [],
        origin: REMOTE,
        meta: {
          affectedPages: [pr(1)],
          cacheDelta: null,
          changed: [],
          opId: 'reorder',
          undoable: true,
        },
      }),
      emit(deletedFact([objRef(3)])),
      emit(createdFact(link(9, { page: pr(2), ref: objRef(9, 2) }))),
      emit(updatedFact(link(4, { page: pr(2), ref: objRef(4, 2) }))),
      emit({
        type: 'pages.rotated',
        pages: [pr(1)],
        rotation: 90,
        layout: {
          pageCount: 2,
          namedPages: [],
          pages: [layout(0, 1, undefined, 90), layout(1, 2, CROP2)],
        },
        origin: REMOTE,
        meta: { affectedPages: [pr(1)], cacheDelta: null, opId: 'rotate', undoable: true },
      }),
      lk('listLinks', [pr(1)]),
      close,
    ],
  },
  {
    name: 'annotation-backed-source',
    covers:
      'Current annotation mirror owns link items without page reads. Remote create/delete and local confirmed update/delete are immediately visible through the link capability.',
    plugins: WITH_ANNOTATION,
    engine: {
      documents: {
        a: annotationDoc(
          {
            1: [
              link(1),
              link(2, { hidden: true }),
              link(3, { actions: { activate: tree(uriNode('https://a.example')) } }),
              square(4),
            ],
            2: PAGE2,
          },
          [
            write(1, [{ item: updated(link(1, { target: uri('https://updated.example') })) }]),
            write(2, [{ item: deleted(objRef(1)), changed: [objRef(1)] }]),
          ],
        ),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('listLinks', [pr(1)]),
      lk('ensureLoaded', [pr(1)]),
      lk('listLinks', [pr(2)]),
      lk('ensureLoaded', [pr(3)]),
      emit(createdFact(link(5))),
      lk('listLinks', [pr(1)]),
      anno('update', [objRef(1), { target: uri('https://updated.example') }]),
      lk('listLinks', [pr(1)]),
      anno('delete', [objRef(1)]),
      lk('listLinks', [pr(1)]),
      emit(deletedFact([objRef(5)])),
      lk('listLinks', [pr(1)]),
    ],
  },
  {
    name: 'link-nav-behavior',
    covers:
      'With the annotation plugin init registers the link-nav behavior: it matches link annotations only and is engaged while the active tool enables link-nav, as engaged() is. The link tool (no link-nav) hands links to the annotation plane; pan takes them back.',
    plugins: WITH_ANNOTATION,
    engine: { documents: { a: annotationDoc({ 1: [link(1), square(2)] }) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('isNavigationEngaged'),
      anno('getBehaviorFor', [link(1)]),
      ref(0, [square(2)]),
      ref(0, [link(1)]),
      ref(1),
      anno('getBehaviorFor', [square(2)]),
      ix('activateTool', ['link']),
      lk('isNavigationEngaged'),
      ref(1),
      anno('getBehaviorFor', [link(1)]),
      ix('activateTool', ['pan']),
      lk('isNavigationEngaged'),
      ref(1),
    ],
  },
  {
    name: 'engaged-follows-the-active-tool',
    covers:
      'Without the annotation plugin engaged() still reads the hub: true under pointer, false under a tool without link-nav.',
    plugins: VIEWER,
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('isNavigationEngaged'),
      ix('registerTool', [{ id: 'plain', cursor: 'default', enables: { $set: ['text-select'] } }]),
      ix('activateTool', ['plain']),
      lk('isNavigationEngaged'),
      ix('activateTool', ['pan']),
      lk('isNavigationEngaged'),
    ],
  },
  {
    name: 'activate-without-a-stage',
    covers:
      'With no stage a goto hands back its explicit destination; uri and named report their member; javascript, goto-remote, launch and unsupported are reported whole. An activate tree without the actions plugin takes the same classic path. onActivate sees every activation.',
    plugins: VIEWER,
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('activate', [goto(xyz(2, 10, 20, 3))]),
      lk('activate', [uri('https://example.com/x')]),
      lk('activate', [named('LastPage')]),
      lk('activate', [{ kind: 'javascript' }]),
      lk('activate', [{ kind: 'goto-remote', file: 'other.pdf' }]),
      lk('activate', [{ kind: 'launch', path: 'calc.exe' }]),
      lk('activate', [{ kind: 'unsupported' }]),
      lk('activate', [
        goto({ kind: 'fit', page: pr(1) }),
        { activate: tree(gotoNode({ kind: 'fit', page: pr(1) })), ref: objRef(1), page: pr(1) },
      ]),
    ],
  },
  {
    name: 'activate-without-a-listener',
    covers: 'Without an onActivate config activation still runs and returns its outcome.',
    plugins: [INTERACTION, { factory: 'link' }],
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('activate', [uri('https://example.com')]),
      lk('activate', [named('FirstPage')]),
    ],
  },
  {
    name: 'activate-reveals-through-the-stage',
    covers:
      'With a stage a goto to a page of the document reveals its destination smoothly (xyz, fit, fitH, fitR on an offset crop) and reports revealed; a goto to a page the document lacks hands back the destination.',
    plugins: WITH_STAGE,
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      viewport,
      lk('activate', [goto(xyz(2, 60, 300, 2))]),
      st('camera'),
      st('currentPage'),
      lk('activate', [goto(xyz(1, null, null, 0))]),
      st('camera'),
      lk('activate', [goto({ kind: 'fit', page: pr(1) })]),
      st('camera'),
      lk('activate', [goto({ kind: 'fitH', page: pr(2), y: 120 })]),
      st('camera'),
      lk('activate', [goto({ kind: 'fitR', page: pr(2), x: 40, y: 220, width: 200, height: 200 })]),
      st('camera'),
      lk('activate', [goto(xyz(77, 0, 0, 1))]),
      lk('activate', [uri('https://example.com')]),
    ],
  },
  {
    name: 'activate-reveals-smoothly',
    clock: { frames: true },
    covers:
      'The current stage scrollBehavior smooth setting animates on host frames until the destination is reached.',
    plugins: [
      INTERACTION,
      { factory: 'stage', config: { ...STAGE.config, scrollBehavior: 'smooth' } },
      LOGGED,
    ],
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      viewport,
      lk('activate', [goto(xyz(2, 60, 300, 2))]),
      st('cameraInMotion'),
      { op: 'advance', ms: 400 },
      st('cameraInMotion'),
      st('camera'),
    ],
  },
  {
    name: 'activate-destination-that-cannot-invert',
    covers:
      'Page-space destinations no longer invert PDF geometry in link activation. Even userUnit zero follows current stage handling and emits successful activation.',
    plugins: WITH_STAGE,
    engine: {
      documents: { a: doc([], { pages: [layout(0, 1), { ...layout(1, 2, CROP2), userUnit: 0 }] }) },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      viewport,
      lk('activate', [goto(xyz(2, 60, 300, 2))]),
      lk('activate', [goto({ kind: 'fit', page: pr(1) })]),
    ],
  },
  {
    name: 'a-throwing-listener-throws-out-of-activate',
    covers:
      'The event source reports a listener exception after reveal; activation returns successfully and the current page is already updated.',
    plugins: [
      INTERACTION,
      STAGE,
      {
        factory: 'link',
        config: {
          onActivate: { $fn: 'onActivate', throws: { name: 'Error', message: 'listener broke' } },
        },
      },
    ],
    engine: { documents: { a: doc([]) } },
    ...UNSNAPSHOTTED,
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
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      viewport,
      lk('listLinks', [pr(1)]),
      lk('activate', [
        goto(xyz(2, 60, 300, 2)),
        { activate: tree(gotoNode(xyz(2, 60, 300, 2))), ref: objRef(1), page: pr(1) },
      ]),
      st('currentPage'),
      lk('activate', [
        uri('https://example.com'),
        { activate: tree(uriNode('https://example.com')) },
      ]),
      lk('activate', [goto({ kind: 'fit', page: pr(1) }), { ref: objRef(1), page: pr(1) }]),
      st('currentPage'),
      lk('activate', [named('NextPage'), {}]),
      lk('activate', [named('NextPage')]),
    ],
  },
  {
    name: 'hit-testing-and-lookup',
    covers:
      'getLinkAt answers the smallest link whose bounds hold the point (edges included, the first of equal areas), null off every link; getLink finds a link by id or answers null. activateAt activates the link under the point with the page in its context, a Link input carrying its own ref and activate tree, and answers null off every link.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          pageList(1, [
            link(1, { rect: { x: 100, y: 100, width: 200, height: 100 } }),
            link(2, { rect: { x: 150, y: 120, width: 50, height: 60 }, target: named('NextPage') }),
            link(3, {
              rect: { x: 160, y: 110, width: 50, height: 60 },
              actions: { activate: tree(uriNode('https://a.example')) },
            }),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('ensureLoaded', [pr(1)]),
      lk('getLinkAt', [pr(1), { x: 170, y: 150 }]),
      lk('getLinkAt', [pr(1), { x: 120, y: 110 }]),
      lk('getLinkAt', [pr(1), { x: 100, y: 100 }]),
      lk('getLinkAt', [pr(1), { x: 500, y: 500 }]),
      lk('getLinkAt', [pr(2), { x: 170, y: 150 }]),
      lk('getLink', [pr(1), 'obj:3']),
      lk('getLink', [pr(1), 'obj:9']),
      lk('activateAt', [pr(1), { x: 170, y: 150 }]),
      lk('activateAt', [pr(1), { x: 205, y: 185 }, { ref: objRef(9) }]),
      lk('activateAt', [pr(1), { x: 500, y: 500 }]),
    ],
  },
  {
    name: 'hit-testing-by-area',
    covers:
      "getLinkAt compares areas, not perimeters or aspect ratios: under a point inside a tall, a square and a wide link it answers the tall one (the first of the two smallest areas), and off the tall one the wide one, whose area is smaller than the square's though its sides sum higher. A point right of every link, or below every link, answers null.",
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          pageList(1, [
            link(1, { rect: { x: 100, y: 100, width: 10, height: 100 } }),
            link(2, { rect: { x: 100, y: 100, width: 40, height: 40 } }),
            link(3, { rect: { x: 100, y: 100, width: 100, height: 10 } }),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      lk('ensureLoaded', [pr(1)]),
      lk('getLinkAt', [pr(1), { x: 105, y: 105 }]),
      lk('getLinkAt', [pr(1), { x: 120, y: 105 }]),
      lk('getLinkAt', [pr(1), { x: 400, y: 105 }]),
      lk('getLinkAt', [pr(1), { x: 105, y: 300 }]),
    ],
  },
  {
    name: 'activate-at-dispatches-with-the-page',
    covers:
      "With the actions plugin activateAt dispatches the tree the link under the point carries, from a link source that names the hit page and the link ref. An empty context keeps both the page and the link's own tree.",
    plugins: FULL,
    engine: {
      documents: {
        a: annotationDoc(
          {
            1: [
              link(1, {
                target: uri('https://t.example'),
                actions: { activate: tree(uriNode('https://a.example')) },
              }),
            ],
          },
          [FORMS_EMPTY, pageList(1, [])],
        ),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      {
        op: 'call',
        target: { token: 'actions', doc: 'a' },
        method: 'setUiAdapter',
        args: [{ openUri: { $fn: 'ui.openUri' }, print: { $fn: 'ui.print' } }],
      },
      {
        op: 'call',
        target: { token: 'actions', doc: 'a' },
        method: 'onExecuted',
        args: [{ $fn: 'executed' }],
      },
      lk('activateAt', [pr(1), { x: 150, y: 150 }]),
      lk('activateAt', [pr(1), { x: 150, y: 150 }, {}]),
    ],
  },
  {
    name: 'labels-and-resolution',
    covers:
      'Labels and resolution use current Link and PdfLinkTarget shapes. Missing-page goto is reported; valid goto resolves to a PageDestination without side effects.',
    plugins: VIEWER,
    engine: { documents: { a: doc([]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('getLabel', [
        {
          id: 'obj:1',
          bounds: { x: 0, y: 0, width: 1, height: 1 },
          target: uri('https://l.example'),
          attached: false,
        },
      ]),
      lk('getLabel', [uri('https://example.com')]),
      lk('getLabel', [goto(xyz(1, 0, 0, 1))]),
      lk('getLabel', [named('FirstPage')]),
      lk('getLabel', [{ kind: 'launch', path: 'calc.exe' }]),
      lk('resolve', [goto(xyz(2, 60, 300, 2))]),
      lk('resolve', [goto({ kind: 'fit', page: pr(1) })]),
      lk('resolve', [goto(xyz(77, 0, 0, 1))]),
      lk('resolve', [uri('https://example.com')]),
      lk('resolve', [named('LastPage')]),
      lk('resolve', [{ kind: 'javascript' }]),
    ],
  },
  {
    name: 'list-all-links-and-loaded-events',
    covers:
      'Without the annotation plugin isLoaded follows the cache; listAllLinks loads every page and answers their links in page order; onLoaded fires for each page that loaded (not for a refused read) and again when an annotation event refetches a cached page.',
    plugins: VIEWER,
    engine: {
      documents: {
        a: doc([
          pageList(1, [link(1)], 1),
          {
            path: 'page.annotations.list',
            pon: 2,
            times: 1,
            error: { name: 'EngineError', message: 'refused', code: 'Refused' },
          },
          pageList(2, PAGE2),
          pageList(1, [link(1), link(2)]),
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      lk('onLoaded', [{ $fn: 'loaded' }]),
      lk('isLoaded', [pr(1)]),
      lk('listAllLinks'),
      lk('isLoaded', [pr(1)]),
      lk('isLoaded', [pr(2)]),
      lk('listAllLinks'),
      emit(createdFact(link(2))),
      lk('listLinks', [pr(1)]),
    ],
  },
  {
    name: 'list-all-links-annotation-backed',
    covers:
      'With the annotation plugin every page is loaded and listAllLinks reads its folded model without an engine read.',
    plugins: WITH_ANNOTATION,
    engine: { documents: { a: annotationDoc({ 1: [link(1)], 2: PAGE2 }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, lk('isLoaded', [pr(1)]), lk('isLoaded', [pr(9)]), lk('listAllLinks')],
  },
];

// The removed reducer's cache semantics now belong to the kernel page mirror.
// Each former occurrence executes that owner; immutable definitions are archived.
const cache = (name: string, responses: ResponseSpec[], steps: Step[]): ScenarioCase => ({
  name,
  covers:
    'Current page-mirror ownership of former cache transition; see C6 accounting for retired state identity.',
  plugins: VIEWER,
  engine: {
    documents: { a: doc(responses, { pages: [layout(0, 1), layout(1, 2, CROP2), layout(2, 3)] }) },
  },
  ...UNSNAPSHOTTED,
  steps: [...boot, ...steps],
});
const dropPage = (n: number) =>
  emit({
    type: 'pages.deleted',
    pages: [pr(n)],
    origin: REMOTE,
    layout: {
      pageCount: 2,
      namedPages: [],
      pages: [1, 2, 3]
        .filter((p) => p !== n)
        .map((p, i) => layout(i, p, p === 2 ? CROP2 : undefined)),
    },
    meta: { affectedPages: [pr(n)], cacheDelta: null, opId: 'drop-page', undoable: true },
  });
const cacheLink = (n: number, over: Partial<Extract<Annotation, { subtype: 'link' }>> = {}) =>
  link(n, {
    rect: { x: n, y: 10, width: 20, height: 5 },
    target: uri(`https://example.com/${n}`),
    actions: {},
    ...over,
  });
const loadedResponses = () => [pageList(1, [cacheLink(1)], 1), pageList(3, [], 1)];
const loadOriginal: Step[] = [lk('ensureLoaded', [pr(1)]), lk('ensureLoaded', [pr(3)])];
const cacheCases: ScenarioCase[] = [
  cache(
    'initial-state',
    [],
    [lk('isLoaded', [pr(1)]), lk('getStatus', [pr(1)]), lk('listLinks', [pr(1)])],
  ),
  cache(
    'set-a-first-page',
    [pageList(1, [cacheLink(1)], 1)],
    [lk('ensureLoaded', [pr(1)]), lk('listLinks', [pr(1)]), lk('getStatus', [pr(1)])],
  ),
  cache(
    'set-another-page',
    [
      ...loadedResponses(),
      pageList(
        2,
        [
          cacheLink(2, {
            page: pr(2),
            ref: objRef(2, 2),
            actions: { activate: tree(uriNode('u')), cursorEnter: tree(uriNode('enter')) },
          }),
        ],
        1,
      ),
    ],
    [
      ...loadOriginal,
      lk('ensureLoaded', [pr(2)]),
      lk('listLinks', [pr(1)]),
      lk('listLinks', [pr(2)]),
      lk('listLinks', [pr(3)]),
      lk('isLoaded', [pr(3)]),
      lk('getStatus', [pr(3)]),
    ],
  ),
  cache(
    'set-replaces-a-page',
    [
      ...loadedResponses(),
      pageList(1, [cacheLink(4), cacheLink(5, { reply: { type: 'group', to: objRef(4) } })], 1),
    ],
    [
      ...loadOriginal,
      emit(updatedFact(cacheLink(4))),
      lk('listLinks', [pr(1)]),
      lk('listLinks', [pr(3)]),
      lk('isLoaded', [pr(3)]),
      lk('getStatus', [pr(3)]),
    ],
  ),
  cache(
    'set-an-empty-page',
    [...loadedResponses(), pageList(2, [], 1)],
    [
      ...loadOriginal,
      lk('ensureLoaded', [pr(2)]),
      lk('listLinks', [pr(1)]),
      lk('isLoaded', [pr(1)]),
      lk('getStatus', [pr(1)]),
      lk('listLinks', [pr(3)]),
      lk('isLoaded', [pr(3)]),
      lk('getStatus', [pr(3)]),
      lk('listLinks', [pr(2)]),
      lk('isLoaded', [pr(2)]),
      lk('getStatus', [pr(2)]),
    ],
  ),
  cache('drop-a-page', loadedResponses(), [
    ...loadOriginal,
    dropPage(1),
    lk('listLinks', [pr(1)]),
    lk('getStatus', [pr(1)]),
    lk('listLinks', [pr(3)]),
    lk('isLoaded', [pr(3)]),
  ]),
  cache('drop-an-empty-page', loadedResponses(), [
    ...loadOriginal,
    dropPage(3),
    lk('listLinks', [pr(3)]),
    lk('isLoaded', [pr(3)]),
    lk('listLinks', [pr(1)]),
  ]),
  cache('drop-an-absent-page', loadedResponses(), [
    ...loadOriginal,
    dropPage(2),
    lk('listLinks', [pr(1)]),
    lk('listLinks', [pr(3)]),
    lk('getStatus', [pr(2)]),
    lk('isLoaded', [pr(1)]),
    lk('isLoaded', [pr(3)]),
  ]),
  cache('drop-from-empty', [], [dropPage(1), lk('listLinks', [pr(1)]), close]),
];
const targets: PdfLinkTarget[] = [
  goto(xyz(1, 10, 20, 2)),
  goto(xyz(77, 0, 0, 1)),
  uri('https://example.com'),
  named('NextPage'),
  { kind: 'javascript' },
  { kind: 'goto-remote', file: 'other.pdf' },
  { kind: 'launch', path: 'calc.exe' },
  { kind: 'unsupported' },
];
const functionCases: FunctionCase[] = targets.map((target, i) => ({
  name: 'resolve-target-' + i,
  fn: 'resolveLinkTarget',
  args: [target, [pr(1), pr(2)]],
}));
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/link.json',
    describes:
      'Current engine and annotation link sources, activation, page-mirror ownership and lifetime.',
    cases: [...cases, ...cacheCases],
  },
  {
    kind: 'function',
    file: 'plugins/link-functions.json',
    describes: 'Current shared pure target resolution.',
    functions: {
      resolveLinkTarget: (target: PdfLinkTarget, pages: ReturnType<typeof pr>[]) =>
        resolveLinkTarget(target, (page) =>
          pages.some((p) => p.objectNumber === page.objectNumber),
        ),
    },
    cases: functionCases,
  },
];
