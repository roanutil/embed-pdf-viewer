/**
 * `packages/plugin/shell`. Replayed by `crates/plugins/shell/tests/conformance.rs`.
 *
 * The reducer as function cases (with its reference identity made visible),
 * and the plugin as scenarios. The plugin is document-scoped: every
 * document keeps its own surfaces, and there is no shell before a document
 * opens or after it closes.
 */
import { initialShellState, shellPlugin, shellReducer, ShellToken } from '../../../../packages/plugin/shell/src/index';
import type { ShellAction, ShellState } from '../../../../packages/plugin/shell/src/types';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

registerPlugin('shell', { factory: () => shellPlugin(), tokens: [ShellToken] });

// ── function cases: the reducer ───────────────────────────────────────────────

const reduce = (state: ShellState, action: ShellAction) => {
  const next = shellReducer(state, action);
  return { same: next === state, next };
};
const initial = () => initialShellState;

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
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [{ index: 0, pageObjectNumber: 1, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } }];
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
  shell('surfaceProps', [id], doc),
  shell('openSurfaces', [], doc),
  shell('openMenus', [], doc),
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
      shell('open', ['right', { exclusive: 'side', props: { tab: 'layers' } }]),
      ...reads('a', 'right'),
      shell('isOpen', ['left']),
      shell('open', ['right', { props: { tab: 'outline' } }]),
      shell('surfaceProps', ['right']),
      shell('open', ['modal', { exclusive: '' }]),
      shell('close', ['right']),
      shell('close', ['right']),
      shell('surfaceProps', ['right']),
      shell('openSurfaces'),
      shell('toggle', ['left', { exclusive: 'side', props: { n: 1 } }]),
      shell('toggle', ['right', { exclusive: 'side' }]),
      shell('toggle', ['right']),
      shell('toggle', ['2', {}]),
      shell('openSurfaces'),
      shell('isOpen', ['constructor']),
      shell('surfaceProps', ['toString']),
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
      shell('openMenus'),
      shell('closeMenu', ['nope']),
      shell('closeMenu', ['edit']),
      shell('isMenuOpen', ['edit']),
      shell('closeAllMenus'),
      shell('closeAllMenus'),
      shell('openMenus'),
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
      shell('openSurfaces', [], null),
      { op: 'call', target: 'documents', method: 'setActive', args: ['a'] },
      shell('openSurfaces', [], null),
      close('a'),
      shell('isOpen', ['left'], 'a'),
      shell('isMenuOpen', ['file'], 'b'),
      open('a'),
      shell('isOpen', ['left'], 'a'),
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
