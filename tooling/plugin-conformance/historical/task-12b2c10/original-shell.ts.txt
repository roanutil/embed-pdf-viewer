/**
 * `packages/plugin/shell`. Replayed by `crates/plugins/shell/tests/conformance.rs`.
 *
 * The reducer as function cases (with its reference identity made visible),
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

// ── function cases: the reducer ───────────────────────────────────────────────

/** The old `ShellAction`: its `type` and fields. */
type ShellAction = { type: string; id?: string; exclusive?: string; props?: Record<string, unknown>; snapshot?: unknown };

/**
 * 4fc1a32af replaced `shellReducer` with one transition per action; each
 * action reaches its transition, with the same fields.
 */
function shellReducer(state: ShellState, action: ShellAction): ShellState {
  const id = action.id as string;
  const options = { exclusive: action.exclusive, props: action.props };
  switch (action.type) {
    case 'SHELL/OPEN_SURFACE':
      return model.openSurface(state, id, options);
    case 'SHELL/CLOSE_SURFACE':
      return model.closeSurface(state, id);
    case 'SHELL/TOGGLE_SURFACE':
      return model.toggleSurface(state, id, options);
    case 'SHELL/SET_SURFACE_PROPS':
      return model.setSurfaceProps(state, id, action.props as never);
    case 'SHELL/CLOSE_ALL_SURFACES':
      return model.closeAllSurfaces(state);
    case 'SHELL/OPEN_MENU':
      return model.openMenu(state, id);
    case 'SHELL/CLOSE_MENU':
      return model.closeMenu(state, id);
    case 'SHELL/CLOSE_ALL_MENUS':
      return model.closeAllMenus(state);
    case 'SHELL/APPLY_SNAPSHOT':
      return model.applySnapshot(state, action.snapshot as never);
    default:
      return state;
  }
}

const reduce = (state: ShellState, action: ShellAction) => {
  const next = shellReducer(state, action);
  return { same: next === state, next };
};
const initial = () => model.initialShellState();

const s = (over: Partial<ShellState> = {}): ShellState => ({ surfaces: {}, openMenus: [], ...over });
const two: ShellState = s({
  surfaces: {
    left: { open: true, exclusive: 'side' },
    right: { open: true, exclusive: 'side', props: { tab: 2 } },
    modal: { open: false, exclusive: 'overlay' },
    '7': { open: true },
    free: { open: true, exclusive: '' },
  },
  openMenus: ['file', 'edit'],
});

const fn = (name: string, action: unknown, state: ShellState = two): FunctionCase => ({ name, fn: 'shellReducer', args: [state, action] });

const reducerCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialShellState', args: [] },
  fn('open-new', { type: 'SHELL/OPEN_SURFACE', id: 'search' }, s()),
  fn('open-with-props', { type: 'SHELL/OPEN_SURFACE', id: 'style', props: { annotation: 'a1', nested: { x: [1, 2] } } }, s()),
  fn('open-exclusive-closes-same-tag', { type: 'SHELL/OPEN_SURFACE', id: 'thumbs', exclusive: 'side' }),
  fn('open-exclusive-keeps-itself', { type: 'SHELL/OPEN_SURFACE', id: 'left', exclusive: 'side', props: { again: true } }),
  fn('open-exclusive-other-tag', { type: 'SHELL/OPEN_SURFACE', id: 'dialog', exclusive: 'overlay' }),
  fn('open-empty-exclusive-is-not-exclusive', { type: 'SHELL/OPEN_SURFACE', id: 'other-free', exclusive: '' }),
  fn('open-existing-replaces-in-place', { type: 'SHELL/OPEN_SURFACE', id: 'modal' }),
  fn('open-integer-like-id', { type: 'SHELL/OPEN_SURFACE', id: '3', exclusive: 'side' }),
  fn('open-already-open-is-a-new-state', { type: 'SHELL/OPEN_SURFACE', id: '7' }),
  fn('close-open', { type: 'SHELL/CLOSE_SURFACE', id: 'right' }),
  fn('close-closed-is-ignored', { type: 'SHELL/CLOSE_SURFACE', id: 'modal' }),
  fn('close-unknown-is-ignored', { type: 'SHELL/CLOSE_SURFACE', id: 'nope' }),
  fn('close-proto-member-is-ignored', { type: 'SHELL/CLOSE_SURFACE', id: 'toString' }),
  fn('toggle-open-closes', { type: 'SHELL/TOGGLE_SURFACE', id: 'left', exclusive: 'side' }),
  fn('toggle-closed-opens-exclusively', { type: 'SHELL/TOGGLE_SURFACE', id: 'modal', exclusive: 'side', props: { p: 1 } }),
  fn('toggle-unknown-opens', { type: 'SHELL/TOGGLE_SURFACE', id: 'new' }),
  fn('toggle-proto-member-opens', { type: 'SHELL/TOGGLE_SURFACE', id: 'valueOf' }),
  fn('open-menu', { type: 'SHELL/OPEN_MENU', id: 'view' }),
  fn('open-menu-again-is-ignored', { type: 'SHELL/OPEN_MENU', id: 'file' }),
  fn('close-menu', { type: 'SHELL/CLOSE_MENU', id: 'file' }),
  fn('close-menu-not-open-is-ignored', { type: 'SHELL/CLOSE_MENU', id: 'view' }),
  fn('close-all-menus', { type: 'SHELL/CLOSE_ALL_MENUS' }),
  fn('close-all-menus-none-open-is-ignored', { type: 'SHELL/CLOSE_ALL_MENUS' }, s()),
  fn('unknown-action-is-ignored', { type: 'SHELL/NOPE' }),
  fn('set-props-replaces-them', { type: 'SHELL/SET_SURFACE_PROPS', id: 'right', props: { tab: 3, extra: null } }),
  fn('set-props-on-a-closed-surface', { type: 'SHELL/SET_SURFACE_PROPS', id: 'modal', props: { x: 1 } }),
  fn('set-props-unknown-is-ignored', { type: 'SHELL/SET_SURFACE_PROPS', id: 'nope', props: { x: 1 } }),
  fn('close-all-surfaces', { type: 'SHELL/CLOSE_ALL_SURFACES' }),
  fn(
    'close-all-surfaces-none-open-is-ignored',
    { type: 'SHELL/CLOSE_ALL_SURFACES' },
    s({ surfaces: { modal: { open: false, exclusive: 'overlay' } }, openMenus: ['file'] }),
  ),
  fn('close-all-surfaces-empty-is-ignored', { type: 'SHELL/CLOSE_ALL_SURFACES' }, s()),
  fn('apply-snapshot-replaces-everything', {
    type: 'SHELL/APPLY_SNAPSHOT',
    snapshot: { surfaces: { '5': { open: true }, search: { open: false, exclusive: 'side', props: { q: 'x' } } }, openMenus: ['help'] },
  }),
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [{ index: 0, ref: { kind: 'objectNumber', pageObjectNumber: 1 }, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } }];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const close = (id: string): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
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
    covers: 'Document-scoped: before start, before any document opens, and for an unknown document, the capability does not resolve.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [shell('isOpen', ['left'], null), start, shell('isOpen', ['left'], null), shell('isOpen', ['left'], 'zzz'), open('a'), shell('isOpen', ['left'], null)],
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
    covers: 'openMenu appends once, closeMenu removes, toggleMenu picks by membership, closeAllMenus empties (and is a no-op when empty); openMenus keeps opening order.',
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
    covers: 'Each document has its own shell; the unaddressed capability follows the active document; closing a document drops its slice, and reopening it starts fresh.',
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
      'updateSurfaceProps replaces props without reopening (and ignores an unknown id); closeAll closes every surface and keeps them; getSnapshot is the state; applySnapshot replaces surfaces and menus wholesale, dropping ones it does not name.',
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
      shell('applySnapshot', [{ surfaces: { modal: { open: true }, '4': { open: true, props: { k: 'v' } } }, openMenus: ['view', 'edit'] }]),
      ...reads('a', 'modal'),
      shell('getSurface', ['left']),
      shell('getSnapshot'),
    ],
  },
  {
    name: 'events',
    covers:
      'Every write is diffed: onSurfaceOpened and onSurfaceClosed fire per surface whose open flag changed, with its props after the change (an exclusive sibling closing included, and a surface a snapshot drops closing with no props); onMenuOpened and onMenuClosed fire per menu, closes first. A write that changes nothing fires nothing, and after unsubscribing nothing fires.',
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
    describes: 'packages/plugin/shell: shellReducer and initialShellState (`same` is reference identity).',
    functions: { shellReducer: reduce, initialShellState: initial },
    cases: reducerCases,
  },
  { kind: 'scenario', file: 'plugins/shell/plugin.json', describes: 'packages/plugin/shell: the document-scoped capability.', cases },
];
