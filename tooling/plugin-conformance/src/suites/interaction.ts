/**
 * `packages/plugin/interaction`. Replayed by
 * `crates/plugins/interaction/tests/conformance.rs`.
 *
 * The hub is document-scoped, so every scenario opens document `a` first and
 * calls the capability there. Handlers are objects of scripted callbacks
 * (`$fn` members): `returns` answers `onDown`, `enabledFor` and
 * `claimsTouch`, `seq` gives one answer per call, and every call lands in the
 * trace with the sample or tool it received.
 */
import {
  FeedbackToken,
  feedbackPlugin,
  interactionPlugin,
} from '../../../../packages/plugin/interaction/src/index';
import {
  InteractionToken,
  samplePointOn,
} from '../../../../packages/plugin/interaction/src/host-contract';
import {
  activateTool,
  builtinTools,
  initialInteractionState,
  popTool,
  pushTool,
  setCursor,
  type InteractionState,
} from '../../../../packages/plugin/interaction/src/model';
import type {
  InteractionConfig,
  PlatformFeedback,
} from '../../../../packages/plugin/interaction/src/index';
import type { PointerSample } from '../../../../packages/plugin/interaction/src/host-contract';
import type { PageRef } from '../../../../packages/core/main/src/index';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "Pins gesture routing between handlers, which the handler callbacks record in order. Its dispatched actions are compared; the state they build is not.",
} as const;

registerPlugin('interaction', {
  factory: (config) => interactionPlugin(config as InteractionConfig | undefined),
  tokens: [InteractionToken],
});

/** `{provider: true}` injects a provider that logs each call as
 *  `feedback.<verb>`; anything else is the no-op default. */
registerPlugin('feedback', {
  factory: (config, env) => {
    const wants = (config as { provider?: boolean } | undefined)?.provider === true;
    const provider: PlatformFeedback = {
      selection: () => void env.log('feedback.selection', []),
      impact: (weight) => void env.log('feedback.impact', [weight]),
      notify: (kind) => void env.log('feedback.notify', [kind]),
    };
    return feedbackPlugin(wants ? { provider } : {});
  },
  tokens: [FeedbackToken],
});

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [
  { index: 0, ref: { kind: 'objectNumber', pageObjectNumber: 1 }, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } },
];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });
const engine = { documents: { a: doc() } };

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open: Step = {
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
};
const call = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'interaction', doc: 'a' },
  method,
  args,
});
const ref = (n: number, ...args: unknown[]): Step => ({ op: 'call', target: { ref: n }, args });
const dispatch = (sample: unknown): Step => call('dispatchPointer', sample);
const pageRef = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });

const noMods = { shift: false, alt: false, ctrl: false, meta: false };

/** A pointer sample over page 1 unless `page` is overridden (null drops it). */
const s = (phase: string, over: Record<string, unknown> = {}): Record<string, unknown> => {
  const { page, ...rest } = over;
  const sample: Record<string, unknown> = {
    phase,
    viewport: { x: 10, y: 20 },
    modifiers: noMods,
    ...rest,
  };
  if (page !== null) sample.page = page ?? { ref: pageRef(1), point: { x: 5, y: 6 } };
  return sample;
};
/** The same sample over a page gap. */
const gap = (phase: string, over: Record<string, unknown> = {}) => s(phase, { ...over, page: null });

type Answer = Record<string, unknown>;
interface HandlerOpts {
  enabled?: Answer;
  down?: Answer;
  members?: readonly ('onMove' | 'onUp' | 'onCancel' | 'onHover')[];
  claims?: Answer;
}

/** A scripted handler. `members` picks which optional callbacks exist. */
const handler = (id: string, priority: number, opts: HandlerOpts = {}): Record<string, unknown> => {
  const out: Record<string, unknown> = {
    id,
    priority,
    enabledFor: { $fn: `${id}.enabledFor`, ...(opts.enabled ?? { returns: true }) },
    onDown: { $fn: `${id}.onDown`, ...(opts.down ?? { returns: true }) },
  };
  for (const member of opts.members ?? ['onMove', 'onUp', 'onCancel', 'onHover']) {
    out[member] = { $fn: `${id}.${member}` };
  }
  if (opts.claims) out.claimsTouch = { $fn: `${id}.claimsTouch`, ...opts.claims };
  return out;
};

const tool = (id: string, cursor: string, enables: string[], over: Record<string, unknown> = {}) => ({
  id,
  cursor,
  enables: { $set: enables },
  ...over,
});

const plugins = [{ factory: 'interaction' }];
const opened = [start, open];
const only = ['interaction::a'];

const cases: ScenarioCase[] = [
  {
    name: 'defaults',
    covers:
      'A fresh hub: the pointer tool, the neutral cursor, the two built-in tools in order, and the capability object in literal member order.',
    plugins,
    engine,
    steps: [
      ...opened,
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'interaction' }, 'a'] },
      call('getActiveTool'),
      call('getActiveToolId'),
      call('getCursor'),
      call('listTools'),
    ],
  },
  {
    name: 'default-tool-config',
    covers: 'config.defaultTool picks the initial tool; the cursor starts neutral whatever the tool.',
    plugins: [{ factory: 'interaction', config: { defaultTool: 'pan' } }],
    engine,
    snapshot: only,
    steps: [...opened, call('getActiveToolId'), call('getActiveTool'), call('getCursor')],
  },
  {
    name: 'unknown-default-tool',
    covers:
      'A defaultTool nobody registered still reads back: activeTool() synthesizes a neutral tool with no tags, so no tagged handler is eligible.',
    plugins: [{ factory: 'interaction', config: { defaultTool: 'ghost' } }],
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('getActiveTool'),
      call('registerHandler', handler('h', 1, { enabled: { returns: false } })),
      dispatch(s('down')),
      dispatch(s('move')),
    ],
  },
  {
    name: 'activate-tool',
    covers:
      'activateTool refuses an unknown id; otherwise SET_TOOL, a cursor resync (the pan gap cursor off-page), then every tool-change callback in subscription order. Re-activating the same tool still dispatches and notifies; the reducer keeps the state. Unsubscribing answers whether the callback was still subscribed.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('activateTool', 'nope'),
      call('onToolChanged', { $fn: 'first' }),
      call('onToolChanged', { $fn: 'second' }),
      call('activateTool', 'pan'),
      call('getActiveToolId'),
      call('getCursor'),
      call('activateTool', 'pan'),
      ref(0),
      ref(0),
      call('activateTool', 'pointer'),
    ],
  },
  {
    name: 'priority-routing',
    covers:
      'A down goes to eligible handlers in descending priority; the first to return true owns the gesture, and move and up go only to it. After the up, a move with no owner is hover for every eligible handler, highest first.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('low', 10)),
      call('registerHandler', handler('high', 100)),
      dispatch(s('down')),
      dispatch(s('move')),
      dispatch(s('up')),
      dispatch(s('move')),
    ],
  },
  {
    name: 'fall-through',
    covers:
      'A handler that declines the down (falsy answers included) lets the next one try; truthy non-booleans capture. When nobody captures, move is hover and up reaches nobody.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('a', 30, { down: { seq: [{ returns: false }, { returns: 0 }, { returns: false }] } })),
      call('registerHandler', handler('b', 20, { down: { seq: [{ returns: 'yes' }, { returns: null }] } })),
      call('registerHandler', handler('c', 10, { down: { returns: false }, members: ['onUp', 'onHover'] })),
      dispatch(s('down')),
      dispatch(s('move')),
      dispatch(s('up')),
      dispatch(s('down')),
      dispatch(s('move')),
      dispatch(s('up')),
    ],
  },
  {
    name: 'equal-priority-is-registration-order',
    covers: 'Ties keep registration order (the sort is stable), for downs and for hover.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('first', 5, { down: { returns: false } })),
      call('registerHandler', handler('second', 5, { down: { returns: false } })),
      call('registerHandler', handler('top', 6, { down: { returns: false } })),
      call('registerHandler', handler('third', 5, { down: { returns: false } })),
      dispatch(s('down')),
      dispatch(s('move')),
    ],
  },
  {
    name: 'tool-gating',
    covers:
      'enabledFor sees the active tool (tags as a set) on every down and hover pass; a handler that answers false is skipped. The built-in text handler shape: pointer enables it, pan does not.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('text', 60, { enabled: { seq: [{ returns: true }, { returns: false }, { returns: false }] } })),
      call('registerHandler', handler('scroll', 5, { enabled: { seq: [{ returns: false }, { returns: true }] } })),
      dispatch(s('down')),
      dispatch(s('up')),
      call('activateTool', 'pan'),
      dispatch(s('down')),
      dispatch(s('move')),
    ],
  },
  {
    name: 'missing-members',
    covers:
      'Optional members are optional: an owner without onMove or onUp ignores those phases, and hover skips handlers without onHover.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('bare', 9, { members: [] })),
      call('registerHandler', handler('hover', 1, { down: { returns: false }, members: ['onHover'] })),
      dispatch(s('down')),
      dispatch(s('move')),
      dispatch(s('up')),
      dispatch(s('move')),
    ],
  },
  {
    name: 'cancel',
    covers:
      'cancel aborts: the owner gets onCancel, or onUp when it has none, or nothing when it has neither; the gesture is released either way, and a cancel with no owner reaches nobody.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('full', 30, { down: { seq: [{ returns: true }, { returns: false }] } })),
      call('registerHandler', handler('upOnly', 20, { down: { seq: [{ returns: true }, { returns: false }] }, members: ['onMove', 'onUp'] })),
      call('registerHandler', handler('none', 10, { members: ['onMove'] })),
      dispatch(s('down')),
      dispatch(s('cancel')),
      dispatch(s('move')),
      dispatch(s('down')),
      dispatch(s('cancel')),
      dispatch(s('down')),
      dispatch(s('cancel')),
      dispatch(s('move')),
      dispatch(s('cancel')),
    ],
  },
  {
    name: 'a-new-down-replaces-the-owner',
    covers: 'Every down starts over: the previous owner is dropped before the new pass, even when nobody captures it.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('h', 1, { down: { seq: [{ returns: true }, { returns: false }] } })),
      dispatch(s('down')),
      dispatch(s('down')),
      dispatch(s('move')),
      dispatch(s('up')),
    ],
  },
  {
    name: 'source-filtering',
    covers:
      'A handler registered with a source only sees samples stamped with that source, or unstamped ones; a handler without a source sees everything. An options object without a source is no scope.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('lensA', 30, { down: { returns: false } }), { source: 'lens-a' }),
      call('registerHandler', handler('lensB', 20, { down: { returns: false } }), { source: 'lens-b' }),
      call('registerHandler', handler('any', 10, { down: { returns: false } }), {}),
      dispatch(s('down', { source: 'lens-a' })),
      dispatch(s('down', { source: 'lens-c' })),
      dispatch(s('down')),
      dispatch(s('move', { source: 'lens-b' })),
    ],
  },
  {
    name: 'unregister-mid-gesture',
    covers:
      'Unregistering the owner releases the gesture: the next move is hover among the remaining handlers and the up reaches nobody. Unregistering again, or a handler that does not own the gesture, is harmless.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('owner', 20)),
      call('registerHandler', handler('other', 10, { down: { returns: false } })),
      dispatch(s('down')),
      ref(0),
      dispatch(s('move')),
      dispatch(s('up')),
      ref(0),
      call('registerHandler', handler('late', 30)),
      dispatch(s('down')),
      ref(1),
      dispatch(s('move')),
      dispatch(s('up')),
    ],
  },
  {
    name: 'unregister-scoped-handler',
    covers:
      'Unregistering a source-scoped handler forgets its scope along with it, and only its scope: another scoped handler keeps filtering.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('scoped', 2, { down: { returns: false } }), { source: 'lens-a' }),
      call('registerHandler', handler('other', 1, { down: { returns: false } }), { source: 'lens-b' }),
      dispatch(s('down', { source: 'lens-b' })),
      dispatch(s('down', { source: 'lens-a' })),
      ref(0),
      dispatch(s('down', { source: 'lens-a' })),
      dispatch(s('down', { source: 'lens-b' })),
    ],
  },
  {
    name: 'activate-tool-mid-gesture',
    covers: 'Switching tools drops the gesture owner and every hover claim.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('registerHandler', handler('h', 1)),
      dispatch(s('down')),
      call('claimCursor', 'hover', 'text', 10),
      call('activateTool', 'pan'),
      dispatch(s('move')),
      dispatch(s('up')),
      call('getCursor'),
    ],
  },
  {
    name: 'cursor-claims',
    covers:
      'The claim stack: the highest priority wins, ties go to the earliest claim, re-claiming a token keeps its place, null releases it, and priority defaults to 0. SET_CURSOR only when the resolved cursor changes.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('claimCursor', 'a', 'text', 10),
      call('claimCursor', 'b', 'move', 10),
      call('claimCursor', 'b', 'move', 20),
      call('claimCursor', 'b', 'grabbing', 10),
      call('claimCursor', 'c', 'crosshair'),
      call('claimCursor', 'a', null),
      call('claimCursor', 'b', null),
      call('claimCursor', 'c', 'crosshair', -1),
      call('claimCursor', 'c', null),
      call('claimCursor', 'never', null),
      call('getCursor'),
    ],
  },
  {
    name: 'page-edges',
    covers:
      'The base cursor applies only over a page: crossing a page edge resyncs, staying on the same side does not, and the gap cursor is the tool gapCursor or the neutral arrow.',
    plugins: [{ factory: 'interaction', config: { defaultTool: 'pan' } }],
    engine,
    snapshot: only,
    steps: [
      ...opened,
      dispatch(s('move')),
      dispatch(s('move')),
      dispatch(gap('move')),
      call('registerTool', tool('ink', 'crosshair', ['ink'])),
      call('activateTool', 'ink'),
      dispatch(gap('move')),
      dispatch(s('down')),
      dispatch(s('up')),
      dispatch(s('move', { page: { ref: pageRef(1), point: { x: 1, y: 2 }, scale: 2, rotation: 90, zoom: 1.5 } })),
      dispatch(gap('move')),
    ],
  },
  {
    name: 'tool-cursor-skins',
    covers:
      'A skin restyles the winning keyword (claim or base) for its tool only; unmapped keywords, gaps and other tools render as-is; a mapped null falls back; null removes the skin; each change resyncs.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('setToolCursor', 'pointer', { default: 'url(p.svg) 0 0, default', text: 'url(t.svg) 4 4, text', move: null }),
      dispatch(s('move')),
      call('claimCursor', 'hover', 'text', 1),
      call('claimCursor', 'hover', 'move', 1),
      call('claimCursor', 'hover', 'copy', 1),
      call('claimCursor', 'hover', null),
      dispatch(gap('move')),
      dispatch(s('move')),
      call('setToolCursor', 'pan', { grab: 'url(h.svg) 0 0, grab' }),
      call('setToolCursor', 'pointer', null),
      call('activateTool', 'pan'),
      dispatch(gap('move')),
      dispatch(s('move')),
      call('getCursor'),
    ],
  },
  {
    name: 'register-tool',
    covers:
      'registerTool adds in order, and replacing an id keeps its place; the unregister deletes by id (and its skin), even a later replacement, and does not touch the active tool, which then reads back as the neutral fallback until another is activated.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('registerTool', tool('ink', 'crosshair', ['ink', 'draw', 'ink'], { touchDirect: true, gapCursor: 'not-allowed' })),
      call('listTools'),
      call('registerTool', tool('pointer', 'default', ['text-select']), { replace: true }),
      call('listTools'),
      call('activateTool', 'ink'),
      call('setToolCursor', 'ink', { crosshair: 'url(i.svg) 0 0, crosshair' }),
      dispatch(s('move')),
      call('getCursor'),
      call('setToolCursor', 'pan', { grab: 'url(g.svg) 0 0, grab' }),
      ref(0),
      call('getActiveTool'),
      call('listTools'),
      call('activateTool', 'pan'),
      call('activateTool', 'ink'),
      call('registerTool', tool('ink', 'crosshair', []), { replace: true }),
      call('activateTool', 'ink'),
      call('registerTool', tool('ink', 'cell', []), { replace: true }),
      call('activateTool', 'ink'),
      call('setToolCursor', 'ink', null),
      call('getCursor'),
      ref(0),
      call('listTools'),
    ],
  },
  {
    name: 'would-claim-touch',
    covers:
      'wouldClaimTouch walks eligible handlers in priority order and stops at the first truthy claimsTouch; handlers without it never claim; source and tool gating apply; nothing is captured.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('wouldClaimTouch', s('down', { pointerType: 'touch' })),
      call('registerHandler', handler('plain', 50, { down: { returns: false } })),
      call('registerHandler', handler('declines', 40, { claims: { seq: [{ returns: false }, { returns: 0 }, { returns: false }] } })),
      call('registerHandler', handler('claims', 30, { claims: { seq: [{ returns: 1 }, { returns: true }] } }), { source: 'lens-a' }),
      call('registerHandler', handler('never-asked', 20, { claims: { returns: true } })),
      call('wouldClaimTouch', s('down', { pointerType: 'touch', clickCount: 1 })),
      call('wouldClaimTouch', s('down', { pointerType: 'touch', source: 'lens-b' })),
      call('wouldClaimTouch', s('down', { pointerType: 'touch', source: 'lens-a', gesture: 'long-press' })),
      dispatch(s('move')),
    ],
  },
  {
    name: 'sample-shape',
    covers:
      'The sample reaches handlers as given: page context, click count, pointer type, gesture marker, source and the project function.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerHandler', handler('h', 1)),
      dispatch(
        s('down', {
          page: { ref: pageRef(1), point: { x: 1.5, y: -2 }, scale: 1.25, rotation: 270, zoom: 0.5 },
          modifiers: { shift: true, alt: false, ctrl: true, meta: false },
          clickCount: 2,
          pointerType: 'pen',
          gesture: 'long-press',
          source: 'lens-a',
          project: { $fn: 'project', returns: { x: 0, y: 0 } },
        }),
      ),
    ],
  },
  {
    name: 'tool-lookups',
    covers:
      'getDefaultToolId, getTool (null for an unknown id), hasTool and activeToolEnables read the registry and the armed tool.',
    plugins: [{ factory: 'interaction', config: { defaultTool: 'pan' } }],
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('getDefaultToolId'),
      call('getTool', 'pointer'),
      call('getTool', 'ghost'),
      call('hasTool', 'pan'),
      call('hasTool', 'ghost'),
      call('activeToolEnables', 'scroll'),
      call('activeToolEnables', 'text-select'),
      call('activateTool', 'pointer'),
      call('activeToolEnables', 'text-select'),
    ],
  },
  {
    name: 'push-and-pop-tools',
    covers:
      'pushTool arms a tool over the stack and hands its payload to onToolChanged; popTool restores the previous one, and with an empty stack does nothing. pushTool refuses an unknown id. activateTool clears the stack; activateDefaultTool arms the configured default.',
    plugins,
    engine,
    snapshot: only,
    steps: [
      ...opened,
      call('onToolChanged', { $fn: 'changed' }),
      call('registerTool', tool('ink', 'crosshair', ['ink'])),
      call('pushTool', 'pan', { payload: { held: 'space' } }),
      call('pushTool', 'ink', {}),
      call('pushTool', 'ghost'),
      call('popTool'),
      call('popTool'),
      call('popTool'),
      call('pushTool', 'pan'),
      call('activateTool', 'pan'),
      call('activateTool', 'ink', { payload: 'preset-1' }),
      call('activateDefaultTool'),
      call('activateDefaultTool'),
    ],
  },
  {
    name: 'register-tool-conflict',
    covers:
      'A duplicate id is refused with conflict unless replace is set; a replaced registration\'s remover is inert, and the replacement\'s remover deletes it.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('registerTool', tool('pan', 'move', [])),
      call('registerTool', tool('pan', 'move', []), {}),
      call('registerTool', tool('ink', 'crosshair', ['ink'])),
      call('registerTool', tool('ink', 'cell', ['ink']), { replace: true }),
      ref(0),
      call('getTool', 'ink'),
      ref(1),
      call('hasTool', 'ink'),
      ref(1),
    ],
  },
  {
    name: 'gesture-events',
    covers:
      'onGestureStarted fires for the capturing handler with the page ref and pointer type (mouse when absent); onGestureEnded on up, onGestureCancelled on cancel; a down nobody captures, and an up or cancel with no owner, fire nothing. Over a gap the page is null. onCursorChanged fires on every resolved change.',
    plugins,
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      ...opened,
      call('onGestureStarted', { $fn: 'started' }),
      call('onGestureEnded', { $fn: 'ended' }),
      call('onGestureCancelled', { $fn: 'cancelled' }),
      call('onCursorChanged', { $fn: 'cursor' }),
      call('registerHandler', handler('h', 1, { down: { seq: [{ returns: true }, { returns: true }, { returns: false }] } })),
      dispatch(s('down', { pointerType: 'pen' })),
      dispatch(s('up', { pointerType: 'pen' })),
      dispatch(gap('down')),
      dispatch(gap('cancel')),
      dispatch(s('down')),
      dispatch(s('up')),
      dispatch(s('cancel')),
      call('claimCursor', 'hover', 'text'),
      call('claimCursor', 'hover', null),
    ],
  },
  {
    name: 'feedback-noop',
    covers: 'Without a provider the feedback capability is a no-op in literal member order.',
    plugins: [{ factory: 'feedback' }],
    engine,
    ...UNSNAPSHOTTED,
    steps: [
      start,
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'feedback' }] },
      { op: 'call', target: { token: 'feedback' }, method: 'selection' },
      { op: 'call', target: { token: 'feedback' }, method: 'impact', args: ['heavy'] },
      { op: 'call', target: { token: 'feedback' }, method: 'notify', args: ['error'] },
    ],
  },
  {
    name: 'feedback-provider',
    covers: 'An injected provider is the capability; the plugin is workspace-scoped and stateless.',
    plugins: [{ factory: 'feedback', config: { provider: true } }],
    engine,
    steps: [
      start,
      { op: 'call', target: { token: 'feedback' }, method: 'selection' },
      { op: 'call', target: { token: 'feedback' }, method: 'impact' },
      { op: 'call', target: { token: 'feedback' }, method: 'impact', args: ['light'] },
      { op: 'call', target: { token: 'feedback' }, method: 'notify', args: ['success'] },
    ],
  },
];

// ── pure functions ──────────────────────────────────────────────────────────

/** `samplePointOn` with the sample's `project` built from a table: `projects`
 *  maps a pon to what `project(pon)` answers (a point, null, or absent for
 *  undefined); without `projects` the sample has no `project` member. */
const withProject = (sample: Record<string, unknown>): PointerSample => {
  const { projects, ...rest } = sample as { projects?: Record<string, unknown> };
  const out = rest as unknown as PointerSample;
  if (projects) out.project = (page: PageRef) => projects[String(page.objectNumber)] as never;
  return out;
};

const functions = {
  samplePointOn: (sample: Record<string, unknown>, pon: number) =>
    samplePointOn(withProject(sample), { kind: 'objectNumber', objectNumber: pon }),
  builtinTools,
  // Upstream's state no longer takes the config: the controller activates
  // `defaultTool` after the document opens.
  initialInteractionState: (_config: unknown) => initialInteractionState(),
  // Upstream replaced the reducer with one pure transition per action.
  reduceInteraction: (state: InteractionState, action: { type: string; toolId?: string; cursor?: string }) => {
    switch (action.type) {
      case 'SET_TOOL':
        return activateTool(state, action.toolId!);
      case 'PUSH_TOOL':
        return pushTool(state, action.toolId!);
      case 'POP_TOOL':
        return popTool(state);
      case 'SET_CURSOR':
        return setCursor(state, action.cursor as InteractionState['cursor']);
      default:
        return state;
    }
  },
};

const fc = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });
const at = (pon: number, x: number, y: number) => ({ ref: pageRef(pon), point: { x, y } });

const functionCases: FunctionCase[] = [
  fc('samplePointOn', 'project-wins', { ...s('move'), projects: { 1: { x: -40, y: 900 } } }, 1),
  fc('samplePointOn', 'project-null-falls-back-to-the-same-page', { ...s('move', { page: at(2, 3, 4) }), projects: { 2: null } }, 2),
  fc('samplePointOn', 'project-undefined-falls-back', { ...s('move', { page: at(2, 3, 4) }), projects: {} }, 2),
  fc('samplePointOn', 'no-project-same-page', s('move', { page: at(3, 7, 8) }), 3),
  fc('samplePointOn', 'no-project-foreign-page', s('move', { page: at(3, 7, 8) }), 4),
  fc('samplePointOn', 'project-null-foreign-page', { ...s('move', { page: at(3, 7, 8) }), projects: { 4: null } }, 4),
  fc('samplePointOn', 'over-a-gap', gap('move'), 1),
  fc('samplePointOn', 'over-a-gap-projected', { ...gap('move'), projects: { 1: { x: 0, y: -1 } } }, 1),
  fc('builtinTools', 'pointer-and-pan'),
  fc('initialInteractionState', 'no-config', {}),
  fc('initialInteractionState', 'default-tool', { defaultTool: 'pan' }),
  fc('reduceInteraction', 'set-tool', { activeToolId: 'pointer', defaultToolId: 'pointer', toolStack: [], cursor: 'default' }, { type: 'SET_TOOL', toolId: 'pan' }),
  fc('reduceInteraction', 'set-same-tool', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: [], cursor: 'grab' }, { type: 'SET_TOOL', toolId: 'pan' }),
  fc('reduceInteraction', 'set-cursor', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: [], cursor: 'grab' }, { type: 'SET_CURSOR', cursor: 'grabbing' }),
  fc('reduceInteraction', 'set-same-tool-with-a-stack', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: ['pointer'], cursor: 'grab' }, { type: 'SET_TOOL', toolId: 'pan' }),
  fc('reduceInteraction', 'push-tool', { activeToolId: 'pointer', defaultToolId: 'pointer', toolStack: ['ink'], cursor: 'default' }, { type: 'PUSH_TOOL', toolId: 'pan' }),
  fc('reduceInteraction', 'pop-tool', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: ['ink', 'pointer'], cursor: 'grab' }, { type: 'POP_TOOL' }),
  fc('reduceInteraction', 'pop-empty-stack', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: [], cursor: 'grab' }, { type: 'POP_TOOL' }),
  fc('reduceInteraction', 'set-same-cursor', { activeToolId: 'pan', defaultToolId: 'pointer', toolStack: [], cursor: 'grab' }, { type: 'SET_CURSOR', cursor: 'grab' }),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/interaction.json',
    describes: 'packages/plugin/interaction: tools, handler routing, cursor arbitration, touch pre-flight, feedback.',
    cases,
  },
  {
    kind: 'function',
    file: 'plugins/interaction-functions.json',
    describes: 'packages/plugin/interaction: samplePointOn, the built-in tools, the reducer.',
    functions,
    cases: functionCases,
  },
];
