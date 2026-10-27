import { LIVE_ANSWERS } from './annotation-live-answers';
import { ix, tool, sample } from './annotation-live-inputs';
/**
 * `packages/plugin/annotation`: the tool registry wired into the hub and the
 * five pointer handlers (`handler.ts`), driven as `PointerSample` sequences
 * through `interaction.dispatch`, plus click placement, the stamp tool, the
 * file-picker port, the footprint ghost and behaviors. Replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  PLUGINS,
  SNAP,
  advance,
  boot,
  call,
  created,
  doc,
  freeText,
  hydrate,
  objRef,
  settle,
  square,
  stamp,
  textNote,
  layout,
  ink,
  markup,
  pr,
} from './annotation-live-fixtures';
/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const SESSION_SNAPSHOT = { snapshot: SNAP };
const b64 = (bytes: number[]) => Buffer.from(bytes).toString('base64');
/** A PNG header, 100 x 50 px. */
const PNG = {
  $bytes: b64([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
    100, 0, 0, 0, 50,
  ]),
};
/** A JPEG with an APP0 segment, then SOF0: 40 x 30 px. */
const JPEG = {
  $bytes: b64([
    0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xff, 0xc0, 0, 11, 8, 0, 30, 0, 40, 0, 0, 0, 0,
  ]),
};
const PDF = { $bytes: b64([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31]) };
const JUNK = { $bytes: b64([1, 2, 3, 4, 5]) };
const pointer = (phase: string, x: number, y: number, over: Record<string, unknown> = {}) =>
  ix('dispatchPointer', [sample(phase, x, y, over)]);
const hover = (x: number, y: number, over: Record<string, unknown> = {}) =>
  pointer('move', x, y, over);
const offPage = (phase: string): Step =>
  ix('dispatchPointer', [
    {
      phase,
      viewport: { x: 900, y: 900 },
      modifiers: { shift: false, alt: false, ctrl: false, meta: false },
    },
  ]);
/** An argument left out, as canonical JSON spells it inside an array. */
const U = { $undefined: true };
const withSelection = [
  { factory: 'interaction' },
  { factory: 'annotation-live-selection' },
  { factory: 'annotation-live' },
];
const targetBox = { x: 100, y: 800 - 700, width: 200 - 100, height: 700 - 600 };
const target = { rect: targetBox, box: targetBox };
const cases: ScenarioCase[] = [
  {
    name: 'malformed-stamp-source-inputs',
    covers:
      'Explicit out-of-contract BinarySource values preserve their exact runtime TypeErrors; a later valid PNG arm, preview and disarm recover without leaked draft or selection.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate()]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('stamps.arm', [{ source: 'not bytes' }]),
      call('stamps.arm', [{ source: null }]),
      call('stamps.arm', [{ source: { data: 5 } }]),
      call('stamps.arm', [{ source: { name: 'no-data' } }]),
      call('stamps.arm', [{}]),
      call('stamps.arm', [{ source: { data: U } }]),
      call('stamps.isArmed'),
      call('stamps.arm', [{ source: PNG }]),
      call('stamps.isArmed'),
      call('renderArmedStampPreview'),
      call('stamps.disarm'),
      call('stamps.isArmed'),
      call('draft.get'),
      call('selection.list'),
    ],
  },

  {
    name: 'init-registers-the-tools',
    covers:
      'init registers every resolved tool with the hub (drag-create tools touchDirect, text-select tools skipped without a selection plugin) and seeds their defaults; a config tool is added and a built-in configured.',
    plugins: [
      { factory: 'interaction' },
      {
        factory: 'annotation-live',
        config: {
          tools: [
            { id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'open-arrow' } } },
            { id: 'ink', ink: { groupStrokesMs: 0 } },
          ],
        },
      },
    ],
    engine: { documents: { a: doc([hydrate(), ...LIVE_ANSWERS['init-registers-the-tools']]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('listTools'),
      call('tools.list'),
      call('tools.get', ['arrow']),
      call('tools.get', ['nope']),
      call('getToolSubtype', ['arrow']),
      call('getToolSubtype', ['nope']),
      call('tools.getDefaults', ['arrow']),
    ],
  },
  {
    name: 'init-with-selection',
    covers: 'With a selection plugin the markup and text-edit tools register too.',
    plugins: withSelection,
    engine: { documents: { a: doc([hydrate(), ...LIVE_ANSWERS['init-with-selection']]) } },
    ...SESSION_SNAPSHOT,
    steps: [...boot, ix('listTools')],
  },
  {
    name: 'a-bad-config-tool-fails-the-capability',
    covers:
      'A config tool whose defaults its kind does not take makes the capability throw at construction.',
    plugins: [
      { factory: 'interaction' },
      {
        factory: 'annotation-live',
        config: { tools: [{ id: 'square', defaults: { fontSize: 3 } }] },
      },
    ],
    engine: {
      documents: { a: doc([hydrate(), ...LIVE_ANSWERS['a-bad-config-tool-fails-the-capability']]) },
    },
    ...SESSION_SNAPSHOT,
    steps: [...boot, call('tools.list')],
  },
  {
    name: 'edit-gestures',
    covers:
      'Under the pointer tool the edit handler claims over an annotation: hover sets the cursor claim and the hover state, a press selects and arms a move, moves resolve against the home page (the projected point, then the page hit), and the up commits one geometry patch. A press on empty space deselects (shift keeps it for the marquee) and declines.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            square(1, target),
            square(2, {
              rect: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
              box: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
            }),
          ]),
          ...LIVE_ANSWERS['edit-gestures'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('onGestureStarted', [{ $fn: 'gesture' }]),
      hover(150, 150),
      ix('getCursor'),
      call('selection.set', [[objRef(1)]]),
      hover(150, 150),
      ix('getCursor'),
      pointer('down', 150, 150),
      pointer('move', 160, 155, { project: { $fn: 'project', returns: { x: 170, y: 160 } } }),
      pointer('move', 175, 162),
      // A move draft is not a creation draft, and finishing one is a no-op.
      call('draft.get'),
      call('draft.finish'),
      pointer('move', 175, 162, { page: { ref: pr(2), point: { x: 1, y: 1 }, scale: 1 } }),
      pointer('up', 170, 160),
      hover(150, 150),
      ix('getCursor'),
      pointer('down', 5, 5, { modifiers: { shift: true, alt: false, ctrl: false, meta: false } }),
      pointer('up', 5, 5),
      call('selection.list'),
      pointer('down', 5, 5),
      pointer('up', 5, 5),
      call('selection.list'),
      hover(5, 5),
      ix('getCursor'),
      offPage('move'),
      ix('getCursor'),
    ],
  },
  {
    name: 'edit-gesture-cancel-and-off-page-up',
    covers:
      'An aborted gesture replays back to its down point and closes there (no net change); an up off every page still closes the gesture at the last point.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([square(1, target)]),
          ...LIVE_ANSWERS['edit-gesture-cancel-and-off-page-up'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('selection.set', [[objRef(1)]]),
      pointer('down', 150, 150),
      pointer('move', 190, 190),
      pointer('cancel', 190, 190),
      pointer('down', 150, 150),
      pointer('move', 160, 150, { page: { ref: pr(2), point: { x: 1, y: 1 }, scale: 1 } }),
      pointer('up', 160, 150, { page: { ref: pr(2), point: { x: 1, y: 1 }, scale: 1 } }),
      call('selection.list'),
      pointer('down', 150, 150),
      pointer('move', 160, 150),
      offPage('up'),
      pointer('cancel', 1, 1),
    ],
  },
  {
    name: 'touch-consent',
    covers:
      'claimsTouch: over the selection’s chrome or a selected, movable body a finger owns the gesture; over an unselected body it scrolls. A touch press grabs with the widened zones.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([square(1, target), markup('highlight', 3)]),
          ...LIVE_ANSWERS['touch-consent'],
        ]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      ix('wouldClaimTouch', [sample('down', 150, 150, { pointerType: 'touch' })]),
      call('selection.set', [[objRef(1)]]),
      ix('wouldClaimTouch', [sample('down', 150, 150, { pointerType: 'touch' })]),
      ix('wouldClaimTouch', [sample('down', 99, 99, { pointerType: 'touch' })]),
      ix('wouldClaimTouch', [sample('down', 500, 500, { pointerType: 'touch' })]),
      call('selection.set', [[objRef(3)]]),
      ix('wouldClaimTouch', [sample('down', 100, 60, { pointerType: 'touch' })]),
      ix('wouldClaimTouch', [
        {
          phase: 'down',
          viewport: { x: 0, y: 0 },
          modifiers: { shift: false, alt: false, ctrl: false, meta: false },
          pointerType: 'touch',
        },
      ]),
      call('claimsTouchAt', [pr(1), { x: 150, y: 150 }, 2, 0, 1]),
      call('getHitKind', [pr(1), { x: 100, y: 100 }, 1, U, U, true]),
      pointer('down', 100, 100, { pointerType: 'touch' }),
      pointer('up', 100, 100, { pointerType: 'touch' }),
    ],
  },
  {
    name: 'double-click-edits-free-text',
    covers:
      'A double-click over a free-text box enters text edit and captures; over another kind it falls through to a normal press. While editing, a press outside commits the edit and is consumed on empty space.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            freeText(1, target),
            square(2, {
              rect: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
              box: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
            }),
          ]),
          ...LIVE_ANSWERS['double-click-edits-free-text'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      pointer('down', 150, 150, { clickCount: 2 }),
      call('text.getEditing'),
      pointer('up', 150, 150),
      pointer('down', 5, 5),
      call('text.getEditing'),
      pointer('down', 450, 350, { clickCount: 2 }),
      pointer('up', 450, 350),
      call('selection.list'),
    ],
  },
  {
    name: 'marquee-selects',
    covers:
      'On empty space the marquee handler captures; it starts the core’s marquee only at 4px of viewport travel, grows it along the home page, and commits a selection on up. A shift marquee is additive; a cancel clears the draft; a press off every page declines.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            square(1, target),
            square(2, {
              rect: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
              box: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
            }),
          ]),
          ...LIVE_ANSWERS['marquee-selects'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('onGestureStarted', [{ $fn: 'gesture' }]),
      pointer('down', 50, 50),
      pointer('move', 52, 51),
      pointer('move', 250, 250),
      pointer('move', 250, 250, { page: { ref: pr(2), point: { x: 3, y: 3 }, scale: 1 } }),
      call('listChromeNodes', [pr(1)]),
      pointer('up', 250, 250),
      call('selection.list'),
      pointer('down', 380, 280, {
        modifiers: { shift: true, alt: false, ctrl: false, meta: false },
      }),
      pointer('move', 520, 420),
      pointer('up', 520, 420),
      call('selection.list'),
      pointer('down', 10, 10),
      pointer('up', 11, 11),
      pointer('down', 10, 10),
      pointer('move', 90, 90),
      pointer('cancel', 90, 90),
      call('listChromeNodes', [pr(1)]),
      offPage('down'),
    ],
  },
  {
    name: 'draw-shapes',
    covers:
      'Under a draw tool the draw handler captures: a drag sizes the shape and commits on up (released off-page it commits at the last point); a bare click commits the tool’s click-create default; the DOWN sample’s rotation feeds upright. Without create authority nothing drafts. A gesture’s create is announced with an unknown trigger, since nobody awaits it. Each gesture start names the capturing handler.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['draw-shapes']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('onGestureStarted', [{ $fn: 'gesture' }]),
      call('onCreated', [{ $fn: 'created' }]),
      tool('square'),
      pointer('down', 100, 100),
      pointer('move', 150, 130),
      pointer('move', 160, 140, { page: { ref: pr(2), point: { x: 1, y: 1 }, scale: 1 } }),
      offPage('up'),
      pointer('down', 300, 300),
      pointer('up', 300, 300),
      tool('free-text'),
      pointer('down', 200, 200, {
        page: { ref: pr(1), point: { x: 200, y: 200 }, scale: 1, rotation: 90 },
      }),
      pointer('up', 200, 200),
      tool('line'),
      pointer('down', 50, 50),
      pointer('up', 50, 50),
      call('text.getEditing'),
      pointer('down', 50, 50),
      pointer('up', 50, 50),
      offPage('down'),
      call('tools.register', [{ id: 'pen2', subtype: 'ink', enables: ['annotation-draw'] }]),
      tool('pen2'),
      pointer('down', 20, 20),
      pointer('up', 30, 30),
      call('createPointer', ['note', 'down', pr(1), { x: 10, y: 10 }]),
      call('createPointer', ['note', 'up', pr(1), { x: 60, y: 50 }]),
      call('list', [{ page: pr(1) }]),
    ],
  },
  {
    name: 'draw-without-authority',
    covers: 'createPointer is inert without create authority: no draft, no engine call.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['draw-without-authority']], {
          security: { allows: ['doc.annotate.read'] },
        }),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('square'),
      pointer('down', 100, 100),
      pointer('move', 150, 130),
      pointer('up', 150, 130),
    ],
  },
  {
    name: 'ink-strokes-group',
    covers:
      'An ink tool with a grouping window buffers strokes: a stroke started on the same page inside the window extends the draft, the window’s timer commits it, a stroke on another page flushes the buffer first, a tool change flushes it, and a cancel drops it.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['ink-strokes-group']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('ink'),
      pointer('down', 10, 10),
      pointer('move', 20, 15),
      pointer('up', 30, 10),
      advance(400),
      pointer('down', 40, 40),
      pointer('move', 50, 45),
      pointer('up', 60, 40),
      advance(800),
      pointer('down', 200, 200),
      pointer('move', 210, 215),
      pointer('up', 220, 210),
      ix('dispatchPointer', [sample('down', 5, 5, {}, 2)]),
      ix('dispatchPointer', [sample('move', 60, 70, {}, 2)]),
      ix('dispatchPointer', [sample('up', 9, 9, {}, 2)]),
      tool('ink-highlight'),
      pointer('down', 300, 300),
      pointer('move', 350, 302),
      pointer('up', 400, 301),
      tool('ink'),
      pointer('down', 450, 450),
      pointer('move', 460, 460),
      pointer('up', 470, 470),
      pointer('down', 480, 480),
      pointer('cancel', 480, 480),
      advance(1000),
      call('list', [{ page: pr(1) }]),
    ],
  },
  {
    name: 'grab-reach-follows-the-scale',
    covers:
      'The handle and knob reach is half the hit size over the page scale, doubled for touch, and the knob offset shrinks with the scale: probes just inside and outside both at scale 2, a zero scale and a NaN scale (both read as 1).',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate([square(1, target)]), ...LIVE_ANSWERS['grab-reach-follows-the-scale']]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('selection.set', [[objRef(1)]]),
      call('getHitKind', [pr(1), { x: 107, y: 100 }, 2, U, U, false]),
      call('getHitKind', [pr(1), { x: 107, y: 100 }, 2, U, U, true]),
      call('getHitKind', [pr(1), { x: 150, y: 77 }, 2, U, U, false]),
      call('getHitKind', [pr(1), { x: 150, y: 80 }, 2, U, U, false]),
      call('getHitKind', [pr(1), { x: 150, y: 80 }, 2, U, U, true]),
      call('getHitKind', [pr(1), { x: 150, y: 150 }, 0]),
      call('getHitKind', [pr(1), { x: 100, y: 100 }, { $number: 'NaN' }]),
    ],
  },
  {
    name: 'page-box-of-an-offset-crop',
    covers:
      'On a page whose crop starts off the origin (10, 20 to 410, 520) the page box is 400 x 500: it decides where the rotate knob flips or clamps, and where a note placed in the corner is clamped.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            square(21, {
              page: pr(2),
              rect: { x: 110, y: 520 - 515, width: 210 - 110, height: 515 - 40 },
              box: { x: 110, y: 520 - 515, width: 210 - 110, height: 515 - 40 },
            }),
            square(22, {
              page: pr(2),
              rect: { x: 110, y: 520 - 515, width: 210 - 110, height: 515 - 470 },
              box: { x: 110, y: 520 - 515, width: 210 - 110, height: 515 - 470 },
            }),
            square(23, {
              page: pr(2),
              rect: { x: 360, y: 520 - 515, width: 480 - 360, height: 515 - 40 },
              box: { x: 360, y: 520 - 515, width: 480 - 360, height: 515 - 40 },
            }),
          ]),
          ...LIVE_ANSWERS['page-box-of-an-offset-crop'],
        ]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('selection.set', [[objRef(21, 2)]]),
      call('listChromeNodes', [pr(2)]),
      call('selection.set', [[objRef(22, 2)]]),
      call('listChromeNodes', [pr(2)]),
      call('selection.set', [[objRef(23, 2)]]),
      call('listChromeNodes', [pr(2)]),
      tool('note'),
      call('placeAt', [pr(2), { x: 395, y: 495 }]),
    ],
  },
  {
    name: 'chrome-follows-the-view',
    covers:
      'chrome takes the view from a rotation alone (a noRotate note counter-rotates), recomputes when only the scale changes, and draws the rotate guides during a rotate gesture.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            square(1, target),
            textNote(9, {
              rect: { x: 300, y: 800 - 320, width: 320 - 300, height: 320 - 300 },
              ...{ noRotate: true, noZoom: true },
            }),
          ]),
          ...LIVE_ANSWERS['chrome-follows-the-view'],
        ]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('selection.set', [[objRef(9)]]),
      call('listChromeNodes', [pr(1), U, 90]),
      call('selection.set', [[objRef(1)]]),
      call('listChromeNodes', [pr(1), 1]),
      call('listChromeNodes', [pr(1), 2]),
      pointer('down', 150, 68),
      pointer('move', 200, 68),
      call('listChromeNodes', [pr(1)]),
      pointer('cancel', 200, 68),
    ],
  },
  {
    name: 'handlers-outrank-a-lower-handler',
    covers:
      'The place (95), draw (55) and marquee (50) handlers each take the press ahead of a scripted handler at priority 25 that would also claim it.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['handlers-outrank-a-lower-handler']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('registerHandler', [
        {
          id: 'low',
          priority: 25,
          enabledFor: { $fn: 'low.enabledFor', returns: true },
          onDown: { $fn: 'low.onDown', returns: true },
          onUp: { $fn: 'low.onUp' },
        },
      ]),
      tool('note'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      tool('square'),
      pointer('down', 300, 300),
      pointer('move', 350, 350),
      pointer('up', 350, 350),
      tool('pointer'),
      pointer('down', 500, 700),
      pointer('move', 520, 720),
      pointer('up', 520, 720),
    ],
  },
  {
    name: 'handlers-wake-only-for-their-tags',
    covers:
      'A tool that enables only annotation-edit wakes neither the place handler (a note subtype still places nothing) nor the marquee (a drag on empty space selects nothing).',
    plugins: PLUGINS,
    engine: {
      documents: { a: doc([hydrate(), ...LIVE_ANSWERS['handlers-wake-only-for-their-tags']]) },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('tools.register', [{ id: 'quiet-note', subtype: 'text', enables: ['annotation-edit'] }]),
      tool('quiet-note'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      call('tools.register', [
        { id: 'quiet-box', subtype: 'square', enables: ['annotation-edit'] },
      ]),
      tool('quiet-box'),
      pointer('down', 100, 100),
      pointer('move', 200, 200),
      pointer('up', 200, 200),
    ],
  },
  {
    name: 'edit-press-inside-the-touch-reach-only',
    covers:
      'A mouse press 18px diagonally off a selected square’s corner handle is outside the mouse reach (12px) and inside the touch reach (24px): it deselects rather than starting a resize.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([square(1, target)]),
          ...LIVE_ANSWERS['edit-press-inside-the-touch-reach-only'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('selection.set', [[objRef(1)]]),
      call('listChromeNodes', [pr(1)]),
      pointer('down', 82, 82),
      pointer('up', 82, 82),
      call('selection.list'),
    ],
  },
  {
    name: 'marquee-starts-at-exactly-4px',
    covers:
      'The marquee starts at 4px of viewport travel, not past it, measured as a distance from the press: once along x, once along y.',
    plugins: PLUGINS,
    engine: {
      documents: { a: doc([hydrate(), ...LIVE_ANSWERS['marquee-starts-at-exactly-4px']]) },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      pointer('down', 90, 90),
      pointer('move', 94, 90),
      call('listChromeNodes', [pr(1)]),
      pointer('up', 94, 90),
      pointer('down', 90, 90),
      pointer('move', 90, 94),
      call('listChromeNodes', [pr(1)]),
      pointer('up', 90, 94),
    ],
  },
  {
    name: 'hover-drives-only-a-multi-click-draft',
    covers:
      'Hover feeds the draft only for a poly or callout mid-creation: not for a polygon or callout tool before its first click, and not for an ink stroke waiting in its grouping window. An ink tool without a window commits on up and schedules nothing.',
    plugins: [
      { factory: 'interaction' },
      {
        factory: 'annotation-live',
        config: { tools: [{ id: 'ink-now', extends: 'ink', ink: { groupStrokesMs: 0 } }] },
      },
    ],
    engine: {
      documents: { a: doc([hydrate(), ...LIVE_ANSWERS['hover-drives-only-a-multi-click-draft']]) },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('polygon'),
      hover(40, 40),
      tool('free-text-callout'),
      hover(40, 40),
      tool('ink'),
      pointer('down', 10, 10),
      pointer('move', 20, 15),
      pointer('up', 30, 10),
      hover(60, 60),
      advance(1000),
      tool('ink-now'),
      pointer('down', 200, 200),
      pointer('move', 210, 215),
      pointer('up', 220, 210),
      advance(10),
    ],
  },
  {
    name: 'ink-cancel-inside-the-grouping-window',
    covers:
      'A cancel while an ink group is still inside its grouping window clears the window’s timer, so the buffered stroke never commits. Only a down that is not an ink down leaves the window open for the cancel to meet: re-registering the active tool id as a square does that without a tool change.',
    plugins: PLUGINS,
    engine: {
      documents: { a: doc([hydrate(), ...LIVE_ANSWERS['ink-cancel-inside-the-grouping-window']]) },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('tools.register', [{ id: 'pen', extends: 'ink' }]),
      tool('pen'),
      pointer('down', 10, 10),
      pointer('move', 20, 15),
      pointer('up', 30, 10),
      call('tools.register', [{ id: 'pen', extends: 'square' }]),
      pointer('down', 40, 40),
      pointer('cancel', 50, 50),
      advance(1000),
      call('list', [{ page: pr(1) }]),
    ],
  },
  {
    name: 'hover-follows-the-subtype-the-draft-began-with',
    covers:
      'The hover preview follows a draft only while the active tool still has the kind it began with: re-registering the active tool under the other multi-click kind mid-draft stops the preview, for a callout turned polygon and for a polygon turned callout.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['hover-follows-the-subtype-the-draft-began-with']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('tools.register', [{ id: 'shift', extends: 'free-text-callout' }]),
      tool('shift'),
      pointer('down', 10, 10),
      pointer('up', 10, 10),
      call('tools.register', [{ id: 'shift', extends: 'polygon' }]),
      hover(50, 40),
      call('tools.register', [{ id: 'turn', extends: 'polygon' }]),
      tool('turn'),
      pointer('down', 10, 10),
      pointer('up', 10, 10),
      call('tools.register', [{ id: 'turn', extends: 'free-text-callout' }]),
      hover(60, 20),
    ],
  },
  {
    name: 'poly-and-callout-clicks',
    covers:
      'Poly tools take vertices by click, ignore drags, preview on hover and finish on a double-click; a callout advances tip, knee, box by click with a hover preview. A cancel drops the draft; a tool change resets the gesture state.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['poly-and-callout-clicks']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('polygon'),
      pointer('down', 10, 10),
      pointer('move', 12, 12),
      pointer('up', 12, 12),
      hover(60, 20),
      pointer('down', 60, 20),
      pointer('up', 60, 20),
      pointer('down', 40, 60),
      pointer('up', 40, 60),
      pointer('down', 40, 60, { clickCount: 2 }),
      pointer('up', 40, 60),
      pointer('move', 1, 1),
      tool('polyline'),
      pointer('down', 10, 10),
      pointer('up', 10, 10),
      pointer('cancel', 10, 10),
      call('cancel'),
      call('draft.get'),
      hover(20, 20),
      tool('free-text-callout'),
      pointer('down', 300, 300),
      pointer('up', 300, 300),
      hover(340, 330),
      pointer('down', 340, 330),
      pointer('up', 340, 330),
      hover(410, 380),
      pointer('down', 410, 380),
      pointer('move', 510, 420),
      pointer('up', 510, 420),
      tool('polygon'),
      hover(5, 5),
      offPage('move'),
    ],
  },
  {
    name: 'note-placement-and-ghost',
    covers:
      'The note tool: hover paints the fixed 20x20 footprint ghost (vector, through pageItems), a press clears it (the ghost handler declines), and the place handler creates the note centred on the click, selected once committed. Off-page hover clears the ghost; a refused create reports to console.error.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['note-placement-and-ghost']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      ix('onGestureStarted', [{ $fn: 'gesture' }]),
      tool('note'),
      hover(100, 100),
      call('getImageGhost', [pr(1)]),
      call('getImageGhost', [pr(2)]),
      call('listPageItems', [pr(1)]),
      hover(100, 100, { page: { ref: pr(1), point: { x: 100, y: 100 }, scale: 1, rotation: 90 } }),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      hover(5, 5),
      offPage('move'),
      call('getImageGhost', [pr(1)]),
      pointer('down', 300, 300, {
        page: { ref: pr(1), point: { x: 300, y: 300 }, scale: 1, rotation: 90 },
      }),
      pointer('up', 300, 300),
      call('selection.list'),
    ],
  },
  {
    name: 'click-create-ghosts',
    covers:
      'hoverGhostAt for a click-create tool paints its default geometry (page-clamped); a tool without a footprint policy, an unknown tool, a page without a crop, and a tool without a click policy clear it. previewPlacement clamps a box to the page; clearPlacementPreview clears.',
    plugins: [
      { factory: 'interaction' },
      {
        factory: 'annotation-live',
        config: {
          tools: [
            { id: 'boxes', extends: 'square', ghost: { mode: 'footprint' } },
            { id: 'lines', extends: 'line', ghost: { mode: 'footprint' } },
            { id: 'polys', extends: 'polygon', ghost: { mode: 'footprint' } },
            {
              id: 'marks',
              subtype: 'highlight',
              clickCreate: { width: 10, height: 10 },
              ghost: { mode: 'footprint' },
            },
          ],
        },
      },
    ],
    engine: { documents: { a: doc([hydrate(), ...LIVE_ANSWERS['click-create-ghosts']]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('hoverGhostAt', ['boxes', pr(1), { x: 5, y: 5 }]),
      call('listPageItems', [pr(1)]),
      call('hoverGhostAt', ['lines', pr(1), { x: 590, y: 5 }, 90]),
      call('hoverGhostAt', ['marks', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['boxes', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['polys', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['boxes', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['square', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['nope', pr(1), { x: 50, y: 50 }]),
      call('hoverGhostAt', ['boxes', pr(7), { x: 50, y: 50 }]),
      call('previewPlacement', ['square', pr(1), { x: -10, y: 700 }, { x: 690, y: 1000 }]),
      call('getImageGhost', [pr(1)]),
      call('previewPlacement', ['square', pr(7), { x: 0, y: 0 }, { x: 1, y: 1 }]),
      call('previewPlacement', ['unregistered', pr(1), { x: 1, y: 1 }, { x: 21, y: 21 }]),
      call('listPageItems', [pr(1)]),
      call('clearPlacementPreview'),
      call('clearGhost'),
    ],
  },
  {
    name: 'stamp-prompt-through-the-file-picker',
    covers:
      'The stamp tool’s prompt source asks the installed file-picker port (the request carries the tool, its subtype, the accept filter and the click), then sniffs the picked bytes and places the stamp at their intrinsic size, fit to the page and selected. A cancel drops it; a rejection reports; a tool change while the picker is open expires the click; junk bytes report; with no provider the click is declined.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['stamp-prompt-through-the-file-picker']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('stamp'),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      call('setFilePickerProvider', [
        {
          $fn: 'picker',
          seq: [
            { resolves: { data: PNG, name: 's.png' } },
            { resolves: null },
            { rejects: { name: 'Error', message: 'dialog broke' } },
            { defer: 'slow' },
            { resolves: { data: JUNK } },
            { resolves: { data: JPEG } },
          ],
        },
      ]),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      pointer('down', 300, 500),
      pointer('up', 300, 500),
      pointer('down', 350, 500),
      pointer('up', 350, 500),
      pointer('down', 400, 500),
      pointer('up', 400, 500),
      tool('pointer'),
      settle('slow'),
      tool('stamp'),
      pointer('down', 450, 500),
      pointer('up', 450, 500),
      pointer('down', 500, 700, {
        page: { ref: pr(1), point: { x: 500, y: 700 }, scale: 1, rotation: 270 },
      }),
      pointer('up', 500, 700),
      call('setFilePickerProvider', [null]),
      call('requestStampAt', [pr(1), { x: 1, y: 1 }]),
    ],
  },
  {
    name: 'file-picker-remover-clears-only-its-own',
    covers:
      'setFilePickerProvider returns a remover that clears the slot only while its own provider is installed: the first remover after a second install leaves the second provider in place, the second remover clears it and the next click is declined.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['file-picker-remover-clears-only-its-own']]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      tool('attachment'),
      call('setFilePickerProvider', [{ $fn: 'first', resolves: null }]),
      call('setFilePickerProvider', [{ $fn: 'second', resolves: null }]),
      { op: 'call', target: { ref: 0 } },
      pointer('down', 50, 50),
      pointer('up', 50, 50),
      { op: 'call', target: { ref: 1 } },
      pointer('down', 90, 90),
      pointer('up', 90, 90),
    ],
  },
  {
    name: 'attachment-placement',
    covers:
      'The attachment tool picks the spot first and the file second: the picked file embeds at the click as a file-attachment with the tool’s icon and flags; a refused create reports.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['attachment-placement']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      tool('attachment'),
      call('setFilePickerProvider', [
        {
          $fn: 'picker',
          resolves: {
            data: { $bytes: 'AQID' },
            name: 'a.bin',
            mimeType: 'application/octet-stream',
          },
        },
      ]),
      hover(50, 50),
      pointer('down', 50, 50),
      pointer('up', 50, 50),
      pointer('down', 90, 90),
      pointer('up', 90, 90),
      tool('pointer'),
      call('placeAt', [pr(1), { x: 5, y: 5 }]),
    ],
  },
  {
    name: 'armed-stamps',
    covers:
      'stamps.arm validates the bytes, keeps the preview (the source for rasters, an explicit one for PDF, none otherwise), sizes from the intrinsic pixels or an override or a target width, bumps the arm epoch and activates the stamp tool. Hover draws the image ghost; each click places a stamp; activating any tool but the stamp tool disarms. Junk bytes reject.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['armed-stamps']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('stamps.isArmed'),
      call('renderArmedStampPreview'),
      call('stamps.arm', [{ source: PNG }]),
      call('stamps.isArmed'),
      tool('stamp'),
      call('stamps.arm', [{ source: PNG }]),
      call('stamps.isArmed'),
      call('renderArmedStampPreview'),
      call('getArmedStamp'),
      ix('getActiveToolId'),
      hover(100, 100),
      call('getImageGhost', [pr(1)]),
      pointer('down', 100, 100),
      pointer('up', 100, 100),
      call('stamps.arm', [
        {
          source: PDF,
          preview: { data: JPEG, mimeType: 'image/jpeg' },
          intrinsicSize: { width: 300, height: 100 },
        },
      ]),
      call('renderArmedStampPreview'),
      hover(100, 100, { page: { ref: pr(1), point: { x: 100, y: 100 }, scale: 1, rotation: 90 } }),
      pointer('down', 300, 300, {
        page: { ref: pr(1), point: { x: 300, y: 300 }, scale: 1, rotation: 90 },
      }),
      pointer('up', 300, 300),
      call('stamps.arm', [{ source: PDF, targetWidth: 80 }]),
      call('renderArmedStampPreview'),
      call('stamps.arm', [{ source: JPEG, targetWidth: 80 }]),
      call('placeArmedStamp', [pr(1), { x: 50, y: 50 }]),
      call('placeArmedStamp', [pr(9), { x: 50, y: 50 }]),
      call('stamps.arm', [{ source: JUNK }]),

      call('stamps.arm', [{ source: PNG, targetWidth: 40 }]),
      call('tools.register', [{ id: 'flat-stamp', extends: 'stamp', upright: false }]),
      tool('flat-stamp'),
      call('placeArmedStamp', [pr(1), { x: 50, y: 50 }, 90]),
      call('hoverGhostAt', ['flat-stamp', pr(1), { x: 50, y: 50 }, 90]),
      tool('pointer'),
      call('stamps.isArmed'),
      call('stamps.disarm'),
      call('getArmedStamp'),
    ],
  },
  {
    name: 'armed-stamps-need-the-stamp-tag',
    covers:
      'The tool-change callback keeps an armed payload on the stamp tool stamps.arm activates, and on any tool carrying the annotation-stamp tag (here the stamp tool is reconfigured with it, and a flat-stamp preset extending it inherits it); the pointer tool disarms. Armed: hover draws the image ghost, a click places it, a page without a crop declines, the preview is readable.',
    plugins: [
      { factory: 'interaction' },
      {
        factory: 'annotation-live',
        config: { tools: [{ id: 'stamp', enables: ['annotation-stamp', 'annotation-edit'] }] },
      },
    ],
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['armed-stamps-need-the-stamp-tag']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('stamps.arm', [{ source: PNG }]),
      call('stamps.isArmed'),
      call('renderArmedStampPreview'),
      hover(300, 300),
      call('getImageGhost', [pr(1)]),
      hover(300, 300, { page: { ref: pr(1), point: { x: 300, y: 300 }, scale: 1, rotation: 90 } }),
      pointer('down', 300, 300, {
        page: { ref: pr(1), point: { x: 300, y: 300 }, scale: 1, rotation: 90 },
      }),
      pointer('up', 300, 300),
      call('placeArmedStamp', [pr(9), { x: 50, y: 50 }]),
      call('placeAt', [pr(1), { x: 400, y: 600 }]),
      tool('pointer'),
      call('stamps.isArmed'),
      call('getArmedStamp'),
      call('stamps.arm', [{ source: PDF, preview: { data: JPEG, mimeType: 'image/jpeg' } }]),
      call('renderArmedStampPreview'),

      // A falsy preview falls back to the raster's own bytes; a PDF without
      // one has none. Sizes: an override with a zero side is ignored, a valid
      // one wins, a target width keeps the aspect. A NaN rotation counts as
      // none, for the placement and the ghost; a tool with upright off ignores
      // a real one.
      call('stamps.arm', [{ source: PNG, preview: false }]),
      call('renderArmedStampPreview'),
      call('stamps.arm', [{ source: PDF }]),
      call('renderArmedStampPreview'),
      call('stamps.arm', [{ source: PNG, intrinsicSize: { width: 0, height: 10 } }]),
      call('placeArmedStamp', [pr(1), { x: 300, y: 300 }]),
      call('stamps.arm', [{ source: PNG, intrinsicSize: { width: 10, height: 0 } }]),
      call('placeArmedStamp', [pr(1), { x: 300, y: 300 }]),
      call('stamps.arm', [{ source: PNG, intrinsicSize: { width: 300, height: 100 } }]),
      call('placeArmedStamp', [pr(1), { x: 300, y: 300 }]),
      call('stamps.arm', [{ source: PNG, targetWidth: 80 }]),
      call('placeArmedStamp', [pr(1), { x: 300, y: 300 }, { $number: 'NaN' }]),
      call('hoverGhostAt', ['stamp', pr(1), { x: 300, y: 300 }, { $number: 'NaN' }]),
      call('getImageGhost', [pr(1)]),
      call('tools.register', [{ id: 'flat-stamp', extends: 'stamp', upright: false }]),
      tool('flat-stamp'),
      call('stamps.isArmed'),
      call('placeArmedStamp', [pr(1), { x: 300, y: 300 }, 90]),
    ],
  },
  {
    name: 'placement-rotation-follows-upright',
    covers:
      'Fixed-byte stamp host placement normalizes NaN display rotation; typed note placement and upright:false presets preserve their current counter-rotation behavior.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['placement-rotation-follows-upright']]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('tools.register', [
        { id: 'approved', extends: 'stamp', source: { kind: 'bytes', source: PNG } },
      ]),
      tool('approved'),
      call('requestStampAt', [pr(1), { x: 100, y: 100 }, { $number: 'NaN' }]),
      call('tools.register', [
        {
          id: 'flat-approved',
          extends: 'stamp',
          source: { kind: 'bytes', source: PNG },
          upright: false,
        },
      ]),
      tool('flat-approved'),
      call('requestStampAt', [pr(1), { x: 200, y: 200 }, 90]),
      tool('note'),
      call('placeAt', [pr(1), { x: 300, y: 300 }, 0]),
      call('tools.register', [{ id: 'flat-note', extends: 'note', upright: false }]),
      tool('flat-note'),
      call('placeAt', [pr(1), { x: 400, y: 400 }, 90]),
    ],
  },
  {
    name: 'fixed-bytes-stamp-tool',
    covers:
      'tools.register at runtime: a stamp preset with fixed bytes places on click with no picker; an upright tool counter-rotates on a rotated page. The unregister removes it (the hub’s tool too); a same-id registration replaces; an invalid one throws.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['fixed-bytes-stamp-tool']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('tools.register', [
        { id: 'approved', extends: 'stamp', source: { kind: 'bytes', source: PNG } },
      ]),
      tool('approved'),
      pointer('down', 100, 100, {
        page: { ref: pr(1), point: { x: 100, y: 100 }, scale: 1, rotation: 90 },
      }),
      pointer('up', 100, 100),
      call('tools.register', [
        {
          id: 'approved',
          extends: 'stamp',
          source: { kind: 'bytes', source: JUNK },
          upright: false,
        },
      ]),
      call('requestStampAt', [pr(1), { x: 50, y: 50 }, 90]),
      call('tools.register', [
        { id: 'vector-stamp', extends: 'stamp', source: { kind: 'bytes', source: PDF } },
      ]),
      tool('vector-stamp'),
      call('requestStampAt', [pr(1), { x: 300, y: 300 }]),
      call('tools.register', [
        { id: 'attach2', subtype: 'file-attachment', enables: ['annotation-place'] },
      ]),
      call('setFilePickerProvider', [{ $fn: 'picker', resolves: null }]),
      tool('attach2'),
      call('placeAt', [pr(1), { x: 300, y: 300 }]),
      call('tools.register', [{ id: 'pens', extends: 'ink', defaults: { strokeWidth: 2 } }]),
      call('tools.getDefaults', ['pens']),
      { op: 'call', target: { ref: 0 } },
      ix('listTools'),
      call('tools.get', ['approved']),
      call('tools.register', [{ id: 'bad', subtype: 'square', defaults: { fontSize: 3 } }]),
      tool('pointer'),
      call('requestStampAt', [pr(1), { x: 50, y: 50 }]),
      call('placeAt', [pr(1), { x: 50, y: 50 }]),
    ],
  },
  {
    name: 'behaviors-make-annotations-inert',
    covers:
      'A registered behavior that matches and is engaged makes its annotations hit-test inert (hit, cursor, gestures) and prunes them from the selection on a tool change; getBehaviorFor returns it. Unregistering restores them.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([
            square(1, target),
            square(2, {
              rect: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
              box: { x: 400, y: 800 - 500, width: 500 - 400, height: 500 - 400 },
            }),
            square(3, { page: pr(2) }),
          ]),
          ...LIVE_ANSWERS['behaviors-make-annotations-inert'],
        ]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      call('selection.set', [[objRef(1)]]),
      call('selection.add', [[objRef(2)]]),
      call('registerBehavior', [
        {
          id: 'forms',
          matches: { $fn: 'matches', seq: [{ returns: true }, { returns: false }] },
          engaged: { $fn: 'engaged', returns: true },
        },
      ]),
      call('getHitKind', [pr(1), { x: 150, y: 150 }]),
      call('getCursorAt', [pr(1), { x: 150, y: 150 }]),
      call('getBehaviorFor', [square(1, target)]),
      tool('pan'),
      call('selection.list'),
      tool('pointer'),
      call('getCursorAt', [pr(2), { x: 1, y: 1 }]),
      call('getBehaviorFor', [square(2)]),
      { op: 'call', target: { ref: 0 } },
      call('getHitKind', [pr(1), { x: 150, y: 150 }]),
      call('getBehaviorFor', [square(1, target)]),
    ],
  },
  {
    name: 'placement-declines',
    covers:
      'placeAt with nothing armed declines for a tool that is neither a stamp nor an icon kind and for an unregistered active tool; placeArmedStamp with nothing armed declines.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate(), ...LIVE_ANSWERS['placement-declines']], { pages: [layout(0, 1)] }),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('placeAt', [pr(1), { x: 5, y: 5 }]),
      call('placeArmedStamp', [pr(1), { x: 5, y: 5 }]),
      ix('registerTool', [{ id: 'mystery', cursor: 'x', enables: { $set: ['annotation-place'] } }]),
      tool('mystery'),
      call('placeAt', [pr(1), { x: 5, y: 5 }]),
      call('requestStampAt', [pr(1), { x: 5, y: 5 }]),
      tool('note'),
      call('placeAt', [pr(7), { x: 5, y: 5 }]),
      offPage('down'),
      tool('square'),
      call('placeAt', [pr(1), { x: 5, y: 5 }]),
    ],
  },
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-tools.json',
    describes:
      'packages/plugin/annotation: tools in the hub, the five pointer handlers as sample sequences, click placement, stamps, the file-picker port, the ghost and behaviors.',
    cases: cases.map((input) =>
      input.name === 'a-bad-config-tool-fails-the-capability'
        ? input
        : {
            ...input,
            steps: [
              ...input.steps,
              call('list'),
              call('selection.list'),
              call('draft.get'),
              call('text.getEditing'),
            ],
          },
    ),
  },
];
