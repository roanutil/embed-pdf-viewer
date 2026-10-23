/**
 * `packages/plugin/shell`. Replayed by `crates/plugins/shell/tests/conformance.rs`.
 *
 * Named state transitions as function cases,
 * and the plugin as scenarios. The plugin is document-scoped: every
 * document keeps its own surfaces, and there is no shell before a document
 * opens or after it closes.
 */
import { shellPlugin, ShellToken } from '../../../../packages/plugin/shell/src/index';
import * as model from '../../../../packages/plugin/shell/src/model';
import type { ShellState } from '../../../../packages/plugin/shell/src/model';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

registerPlugin('shell', { factory: () => shellPlugin(), tokens: [ShellToken] });

// Current shipping transitions receive positional arguments, not a retired action envelope.
const initial = model.initialShellState;
const s = (over: Partial<ShellState> = {}): ShellState => ({
  surfaces: {},
  openMenus: [],
  ...over,
});
const two: ShellState = s({
  surfaces: {
    left: { open: true, exclusive: 'side', focusRequest: 0 },
    right: { open: true, exclusive: 'side', props: { tab: 2 }, focusRequest: 0 },
    modal: { open: false, exclusive: 'overlay', focusRequest: 0 },
    '7': { open: true, focusRequest: 0 },
    free: { open: true, exclusive: '', focusRequest: 0 },
  },
  openMenus: ['file', 'edit'],
});
// Generic typing checks the actual shipping function's complete argument tuple here.
const fn = <K extends keyof typeof model>(
  name: string,
  target: K,
  ...args: Parameters<(typeof model)[K]>
): FunctionCase => ({ name, fn: target, args });
const focused: ShellState = s({
  surfaces: {
    search: { open: true, exclusive: 'side', props: { q: 'old' }, focusRequest: 2 },
    free: { open: true, focusRequest: 4, props: { keep: 1 } },
  },
});
const reducerCases: FunctionCase[] = [
  fn('initial-state', 'initialShellState'),
  fn('open-new', 'openSurface', s(), 'search'),
  fn('open-with-props', 'openSurface', s(), 'style', {
    props: { annotation: 'a1', nested: { x: [1, 2] } },
  }),
  fn('open-exclusive-closes-same-tag', 'openSurface', two, 'thumbs', { exclusive: 'side' }),
  fn('open-exclusive-keeps-itself', 'openSurface', two, 'left', {
    exclusive: 'side',
    props: { again: true },
  }),
  fn('open-exclusive-other-tag', 'openSurface', two, 'dialog', { exclusive: 'overlay' }),
  fn('open-empty-exclusive-is-not-exclusive', 'openSurface', two, 'other-free', { exclusive: '' }),
  fn('open-existing-replaces-in-place', 'openSurface', two, 'modal'),
  fn('open-integer-like-id', 'openSurface', two, '3', { exclusive: 'side' }),
  fn('open-already-open-is-a-new-state', 'openSurface', two, '7'),
  fn('close-open', 'closeSurface', two, 'right'),
  fn('close-closed-is-ignored', 'closeSurface', two, 'modal'),
  fn('close-unknown-is-ignored', 'closeSurface', two, 'nope'),
  fn('close-proto-member-is-ignored', 'closeSurface', two, 'toString'),
  fn('toggle-open-closes', 'toggleSurface', two, 'left', { exclusive: 'side' }),
  fn('toggle-closed-opens-exclusively', 'toggleSurface', two, 'modal', {
    exclusive: 'side',
    props: { p: 1 },
  }),
  fn('toggle-unknown-opens', 'toggleSurface', two, 'new'),
  fn('toggle-proto-member-opens', 'toggleSurface', two, 'valueOf'),
  fn('open-menu', 'openMenu', two, 'view'),
  fn('open-menu-again-is-ignored', 'openMenu', two, 'file'),
  fn('close-menu', 'closeMenu', two, 'file'),
  fn('close-menu-not-open-is-ignored', 'closeMenu', two, 'view'),
  fn('close-all-menus', 'closeAllMenus', two),
  fn('close-all-menus-none-open-is-ignored', 'closeAllMenus', s()),
  fn('initial-empty-state', 'initialShellState'),
  fn('set-props-replaces-them', 'setSurfaceProps', two, 'right', { tab: 3, extra: null }),
  fn('set-props-on-a-closed-surface', 'setSurfaceProps', two, 'modal', { x: 1 }),
  fn('set-props-unknown-is-ignored', 'setSurfaceProps', two, 'nope', { x: 1 }),
  fn('close-all-surfaces', 'closeAllSurfaces', two),
  fn(
    'close-all-surfaces-none-open-is-ignored',
    'closeAllSurfaces',
    s({
      surfaces: { modal: { open: false, exclusive: 'overlay', focusRequest: 0 } },
      openMenus: ['file'],
    }),
  ),
  fn('close-all-surfaces-empty-is-ignored', 'closeAllSurfaces', s()),
  fn('apply-snapshot-replaces-everything', 'applySnapshot', two, {
    surfaces: {
      '5': { open: true },
      search: { open: false, exclusive: 'side', props: { q: 'x' } },
    },
    openMenus: ['help'],
  }),
  fn('open-focus-first', 'openSurface', s(), 'search', { focus: true }),
  fn('open-focus-repeated', 'openSurface', focused, 'search', {
    focus: true,
    exclusive: 'side',
    props: { q: 'new' },
  }),
  fn('open-without-focus-keeps-request', 'openSurface', focused, 'search'),
  fn('exclusive-open-clears-sibling-focus', 'openSurface', focused, 'other', {
    exclusive: 'side',
    focus: true,
  }),
  fn('toggle-focus-opens', 'toggleSurface', s(), 'search', { focus: true }),
  fn('toggle-focus-closes', 'toggleSurface', focused, 'search', {
    focus: true,
    props: { ignored: true },
  }),
  fn('snapshot-omits-focus', 'snapshotOf', focused),
  fn('toggle-menu-removes-open', 'toggleMenu', two, 'file'),
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [
  {
    index: 0,
    ref: { kind: 'objectNumber', pageObjectNumber: 1 },
    label: null,
    size: { width: 600, height: 800 },
    rotation: 0,
    userUnit: 1,
    boxes: { media: box, crop: box },
  },
];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const close = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'close',
  args: [id],
});
const shell = (method: string, args: unknown[] = [], doc: string | null = 'a'): Step => ({
  op: 'call',
  target: doc === null ? { token: 'shell' } : { token: 'shell', doc },
  method,
  args,
});
const reads = (doc = 'a', id = 'left'): Step[] => [
  shell('isOpen', [id], doc),
  shell('getSurface', [id], doc),
  shell('listOpenSurfaces', [], doc),
  shell('listOpenMenus', [], doc),
  shell('isMenuOpen', ['file'], doc),
];

const plugins = [{ factory: 'shell' }];

const cases: ScenarioCase[] = [
  {
    name: 'no-shell-without-a-document',
    covers:
      'Document-scoped: before start, before any document opens, and for an unknown document, the capability does not resolve.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      shell('isOpen', ['left'], null),
      start,
      shell('isOpen', ['left'], null),
      shell('isOpen', ['left'], 'zzz'),
      open('a'),
      shell('isOpen', ['left'], null),
    ],
  },
  {
    name: 'surfaces',
    covers:
      'open/close/toggle and every selector: exclusive tags close their siblings, props ride along and are replaced on reopen, closing keeps the surface (closed), toggle opens with its options and closes without them.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open('a'),
      ...reads(),
      shell('open', ['left', { exclusive: 'side' }]),
      shell('open', ['9']),
      shell('open', ['0']),
      shell('open', ['right', { exclusive: 'side', props: { tab: 'layers' } }]),
      ...reads('a', 'right'),
      shell('isOpen', ['left']),
      shell('open', ['right', { props: { tab: 'outline' } }]),
      shell('getSurface', ['right']),
      shell('open', ['modal', { exclusive: '' }]),
      shell('close', ['right']),
      shell('close', ['right']),
      shell('getSurface', ['right']),
      shell('listOpenSurfaces'),
      shell('toggle', ['left', { exclusive: 'side', props: { n: 1 } }]),
      shell('toggle', ['right', { exclusive: 'side' }]),
      shell('toggle', ['right']),
      shell('toggle', ['2', {}]),
      shell('listOpenSurfaces'),
      shell('isOpen', ['constructor']),
      shell('getSurface', ['toString']),
      shell('close', ['nope']),
    ],
  },
  {
    name: 'menus',
    covers:
      'openMenu appends once, closeMenu removes, toggleMenu picks by membership, closeAllMenus empties (and is a no-op when empty); openMenus keeps opening order.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open('a'),
      shell('openMenu', ['file']),
      shell('openMenu', ['edit']),
      shell('openMenu', ['file']),
      ...reads(),
      shell('toggleMenu', ['file']),
      shell('toggleMenu', ['view']),
      shell('listOpenMenus'),
      shell('closeMenu', ['nope']),
      shell('closeMenu', ['edit']),
      shell('isMenuOpen', ['edit']),
      shell('closeAllMenus'),
      shell('closeAllMenus'),
      shell('listOpenMenus'),
    ],
  },
  {
    name: 'per-document-state',
    covers:
      'Each document has its own shell; the unaddressed capability follows the active document; closing a document drops its slice, and reopening it starts fresh.',
    plugins,
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      shell('open', ['left'], 'a'),
      shell('openMenu', ['file'], 'b'),
      ...reads('a'),
      ...reads('b'),
      shell('listOpenSurfaces', [], null),
      { op: 'call', target: 'documents', method: 'setActive', args: ['a'] },
      shell('listOpenSurfaces', [], null),
      close('a'),
      shell('isOpen', ['left'], 'a'),
      shell('isMenuOpen', ['file'], 'b'),
      open('a'),
      shell('isOpen', ['left'], 'a'),
    ],
  },
  {
    name: 'props-close-all-and-snapshots',
    covers:
      'updateSurfaceProps replaces props without reopening (and ignores an unknown id); closeAll closes every surface and keeps them; getSnapshot omits focus requests; applySnapshot replaces surfaces and menus wholesale, dropping ones it does not name.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open('a'),
      shell('open', ['left', { exclusive: 'side', props: { tab: 1 } }]),
      shell('open', ['modal']),
      shell('openMenu', ['file']),
      shell('updateSurfaceProps', ['left', { tab: 2 }]),
      shell('getSurface', ['left']),
      shell('updateSurfaceProps', ['nope', { tab: 2 }]),
      shell('getSurface', ['nope']),
      shell('getSnapshot'),
      shell('closeAll'),
      shell('closeAll'),
      shell('listOpenSurfaces'),
      shell('getSnapshot'),
      shell('applySnapshot', [
        {
          surfaces: { modal: { open: true }, '4': { open: true, props: { k: 'v' } } },
          openMenus: ['view', 'edit'],
        },
      ]),
      ...reads('a', 'modal'),
      shell('getSurface', ['left']),
      shell('getSnapshot'),
    ],
  },
  {
    name: 'focus-requests',
    covers:
      'Repeated focused opens raise a moment counter without another opened event; every close and snapshot restore clears it; unrelated surfaces keep their records.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open('a'),
      shell('onSurfaceOpened', [{ $fn: 'opened', returns: null }]),
      shell('onSurfaceClosed', [{ $fn: 'closed', returns: null }]),
      shell('open', ['free', { focus: true, props: { keep: 1 } }]),
      shell('open', ['left', { exclusive: 'side', focus: true, props: { tab: 1 } }]),
      shell('open', ['left', { exclusive: 'side', focus: true, props: { tab: 2 } }]),
      shell('getSurface', ['left']),
      shell('open', ['left', { exclusive: 'side' }]),
      shell('getSurface', ['left']),
      shell('open', ['right', { exclusive: 'side', focus: true }]),
      shell('getSurface', ['left']),
      shell('getSurface', ['free']),
      shell('toggle', ['right', { focus: true }]),
      shell('getSurface', ['right']),
      shell('toggle', ['right', { focus: true, props: { new: true } }]),
      shell('getSurface', ['right']),
      shell('getSnapshot'),
      shell('closeAll'),
      shell('listOpenSurfaces'),
      shell('applySnapshot', [
        { surfaces: { free: { open: true, props: { restored: true } } }, openMenus: ['file'] },
      ]),
      shell('getSurface', ['free']),
      shell('getSnapshot'),
    ],
  },
  {
    name: 'events',
    covers:
      'Every write is diffed: onSurfaceOpened and onSurfaceClosed fire per surface whose open flag changed, with props when opened and id only when closed (including exclusive siblings and surfaces a snapshot drops); onMenuOpened and onMenuClosed fire per menu, closes first. A write that changes nothing fires nothing, and after unsubscribing nothing fires.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open('a'),
      shell('onSurfaceOpened', [{ $fn: 'opened', returns: null }]),
      shell('onSurfaceClosed', [{ $fn: 'closed', returns: null }]),
      shell('onMenuOpened', [{ $fn: 'menu-opened', returns: null }]),
      shell('onMenuClosed', [{ $fn: 'menu-closed', returns: null }]),
      shell('open', ['left', { exclusive: 'side', props: { tab: 1 } }]),
      shell('open', ['right', { exclusive: 'side' }]),
      shell('open', ['right', { exclusive: 'side', props: { again: true } }]),
      shell('updateSurfaceProps', ['right', { tab: 9 }]),
      shell('toggle', ['right']),
      shell('close', ['right']),
      shell('open', ['modal']),
      shell('closeAll'),
      shell('openMenu', ['file']),
      shell('toggleMenu', ['edit']),
      shell('openMenu', ['file']),
      shell('applySnapshot', [{ surfaces: { gone: { open: true } }, openMenus: ['edit', 'view'] }]),
      shell('applySnapshot', [{ surfaces: {}, openMenus: ['view'] }]),
      shell('closeAllMenus'),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 } },
      { op: 'call', target: { ref: 2 } },
      { op: 'call', target: { ref: 3 } },
      shell('open', ['left']),
      shell('openMenu', ['file']),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/shell/reducer.json',
    describes: 'packages/plugin/shell: named shell state transitions and snapshot projection.',
    functions: {
      initialShellState: initial,
      openSurface: model.openSurface,
      closeSurface: model.closeSurface,
      toggleSurface: model.toggleSurface,
      setSurfaceProps: model.setSurfaceProps,
      closeAllSurfaces: model.closeAllSurfaces,
      openMenu: model.openMenu,
      closeMenu: model.closeMenu,
      closeAllMenus: model.closeAllMenus,
      toggleMenu: model.toggleMenu,
      snapshotOf: model.snapshotOf,
      applySnapshot: model.applySnapshot,
    },
    cases: reducerCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/shell/plugin.json',
    describes: 'packages/plugin/shell: the document-scoped capability.',
    cases,
  },
];
