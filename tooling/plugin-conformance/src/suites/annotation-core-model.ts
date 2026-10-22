/**
 * `packages/core/annotation`: the model-level modules around the reducer
 * (`kinds.ts`, `flags.ts`, `plane.ts`, `links.ts`, `group.ts`, `props.ts`,
 * `anchor.ts`, `snap.ts`, `hit.ts`, `scene.ts`, `placement.ts`, `ink.ts`).
 * Replayed by `crates/plugins/annotation-core/tests/conformance.rs`.
 *
 * Functions that take a `Model` take a model start here: fields layered over
 * `initialModel` (`modelOf`), so a case carries only what it changes. An
 * `inert` set crosses as `{"$set": [...]}`.
 */
import * as an from '../../../../packages/core/annotation/src/anchor';
import * as fl from '../../../../packages/core/annotation/src/flags';
import * as gr from '../../../../packages/core/annotation/src/group';
import * as ht from '../../../../packages/core/annotation/src/hit';
import * as ik from '../../../../packages/core/annotation/src/ink';
import * as kd from '../../../../packages/core/annotation/src/kinds';
import { blendFor } from '../../../../packages/core/annotation/src/kinds/styles';
import * as lk from '../../../../packages/core/annotation/src/links';
import * as pl from '../../../../packages/core/annotation/src/placement';
import * as pn from '../../../../packages/core/annotation/src/plane';
import * as pr from '../../../../packages/core/annotation/src/props';
import * as sc from '../../../../packages/core/annotation/src/scene';
import * as sn from '../../../../packages/core/annotation/src/snap';
import { defaultsFor, initialModel } from '../../../../packages/core/annotation/src/update';
import type {
  ChromeGeometry as ChromeGeom,
  Id,
  Model,
  Rect,
  RenderItem,
} from '../../../../packages/core/annotation/src/types';

import { removedUpstream } from '../removed';
import type { FunctionCase, Suite } from '../vectors';
import {
  DASHED,
  ENDINGS,
  GEOMS,
  PON,
  SOLID,
  annot,
  c,
  cloudy,
  ends,
  flags,
  gCaret,
  gInk,
  gLine,
  gPoly,
  gQuads,
  gRect,
  gText,
  geom,
  modelOf,
  rc,
  sq,
  start,
  style,
  tq,
  tqRot,
  v,
  type Annot,
  type Fn,
  type Geom,
  type Vec,
  pageRef,
} from './annotation-core-fixtures';

/** The old core's props patch (`types.ts`), removed in 17191ea39. */
type AnnotationPropsPatch = Record<string, unknown>;

/** Every subtype the kind table names, plus unknown and prototype keys. */
export const SUBTYPES = [
  ...Object.keys(kd.KINDS),
  'popup',
  'widget',
  'free-text-callout',
  'unknown',
  '',
  'constructor',
  'toString',
  '__proto__',
  'hasOwnProperty',
];

const FLAG_NAMES = fl.FLAG_KEYS;

// ── kinds and flags ──────────────────────────────────────────────────────────

const kindFns: Record<string, Fn> = {
  KINDS: () => kd.KINDS,
  capsFor: removedUpstream('capsFor', 'cf716b3b6'),
  propsFor: removedUpstream('propsFor', '17191ea39'),
  isMarkup: removedUpstream('isMarkup', 'cf716b3b6'),
  kindTakesLink: pr.kindTakesLink,
  sharedProps: removedUpstream('sharedProps', '17191ea39'),
};
const kinds: FunctionCase[] = [
  c('KINDS', 'table'),
  ...SUBTYPES.flatMap((s) => [
    c('capsFor', s || 'empty', s),
    c('propsFor', s || 'empty', s),
    c('isMarkup', s || 'empty', s),
    c('kindTakesLink', s || 'empty', s),
  ]),
  c('sharedProps', 'none', []),
  c('sharedProps', 'one', ['square']),
  c('sharedProps', 'duplicates', ['square', 'square', 'circle']),
  c('sharedProps', 'shape-line', ['square', 'line']),
  c('sharedProps', 'line-shape', ['line', 'square']),
  c('sharedProps', 'mixed-markup', ['highlight', 'ink', 'free-text']),
  c('sharedProps', 'with-unknown', ['square', 'unknown']),
  c('sharedProps', 'unknown-first', ['unknown', 'square']),
  c('sharedProps', 'widgets', ['widget-text', 'widget-toggle', 'widget-box']),
  c('sharedProps', 'note-attachment', ['text', 'file-attachment']),
  c('sharedProps', 'redact-free-text', ['redact', 'free-text']),
  c('sharedProps', 'everything', Object.keys(kd.KINDS)),
];

const flagFns: Record<string, Fn> = {
  NO_ANNOTATION_FLAGS: () => fl.NO_ANNOTATION_FLAGS,
  DRAWN_FLAGS: () => fl.DRAWN_FLAGS,
  FLAG_KEYS: () => fl.FLAG_KEYS,
  flagsEqual: fl.flagsEqual,
  mergeFlags: fl.mergeFlags,
  viewable: fl.viewable,
  interactive: fl.interactive,
  annotInteractive: fl.annotInteractive,
  annotTransformable: fl.annotTransformable,
  annotDeletable: fl.annotDeletable,
  annotContentsEditable: fl.annotContentsEditable,
  isConversationOnly: pn.isConversationOnly,
  isAttachedLink: pn.isAttachedLink,
  isSubstrateOnly: pn.isSubstrateOnly,
};
const AUTHORITIES = [undefined, { update: true, delete: true }, { update: false, delete: true }, { update: true, delete: false }];
const flagCases: FunctionCase[] = [
  c('NO_ANNOTATION_FLAGS', 'value'),
  c('DRAWN_FLAGS', 'value'),
  c('FLAG_KEYS', 'order'),
  c('flagsEqual', 'same', flags(), flags()),
  ...FLAG_NAMES.map((k) => c('flagsEqual', `differs-${k}`, flags(), flags({ [k]: true }))),
  c('mergeFlags', 'empty', fl.DRAWN_FLAGS, {}),
  c('mergeFlags', 'some', fl.DRAWN_FLAGS, { locked: true, print: false }),
  c('mergeFlags', 'all', flags(), Object.fromEntries(FLAG_NAMES.map((k) => [k, true]))),
];
// Every single flag and a few pairs, against every predicate.
const FLAG_SETS: [string, ReturnType<typeof flags>][] = [
  ['none', flags()],
  ...FLAG_NAMES.map((k) => [k, flags({ [k]: true })] as [string, ReturnType<typeof flags>]),
  ['noView+toggle', flags({ noView: true, toggleNoView: true })],
  ['hidden+toggle', flags({ hidden: true, toggleNoView: true, noView: true })],
  ['locked+lockedContents', flags({ locked: true, lockedContents: true })],
];
for (const [name, f] of FLAG_SETS) {
  flagCases.push(c('viewable', `${name}-idle`, f), c('viewable', `${name}-engaged`, f, true), c('interactive', name, f));
  for (const subtype of ['square', 'widget-text', 'unknown']) {
    AUTHORITIES.forEach((authority, i) => {
      const bearer = authority ? { subtype, flags: f, authority } : { subtype, flags: f };
      for (const fn of ['annotInteractive', 'annotTransformable', 'annotDeletable', 'annotContentsEditable']) {
        if (i > 0 && fn === 'annotInteractive') continue;
        flagCases.push(c(fn, `${name}-${subtype}-auth${i}`, bearer));
      }
    });
  }
}
const PLANE_CASES: [string, Partial<Annot>][] = [
  ['plain', { subtype: 'square' }],
  ['reply', { subtype: 'text', irt: 'p' }],
  ['grouped-sub', { subtype: 'strikeout', irt: 'p', group: 'p' }],
  ['group-only', { subtype: 'strikeout', group: 'p' }],
  ['state-text', { subtype: 'text', data: { subtype: 'text', state: 'accepted' } as never }],
  ['state-model', { subtype: 'text', data: { subtype: 'text', stateModel: 'review' } as never }],
  ['state-empty', { subtype: 'text', data: { subtype: 'text', state: '', stateModel: '' } as never }],
  ['state-null', { subtype: 'text', data: { subtype: 'text', state: null } as never }],
  ['state-non-string', { subtype: 'text', data: { subtype: 'text', state: 3 } as never }],
  ['state-on-square-dto', { subtype: 'text', data: { subtype: 'square', state: 'accepted' } as never }],
  ['state-on-square', { subtype: 'square', data: { subtype: 'text', state: 'accepted' } as never }],
  ['text-no-data', { subtype: 'text' }],
  ['attached-link', { subtype: 'link', group: 'p' }],
  ['standalone-link', { subtype: 'link' }],
  ['link-reply', { subtype: 'link', irt: 'p' }],
];
for (const [name, a] of PLANE_CASES) {
  flagCases.push(c('isConversationOnly', name, a), c('isAttachedLink', name, a), c('isSubstrateOnly', name, a));
}

// ── groups and links ─────────────────────────────────────────────────────────

const groupFns: Record<string, Fn> = {
  groupCaps: (s: Partial<Model>, ids: Id[]) => gr.groupCaps(modelOf(s), ids),
  groupKeyOf: (s: Partial<Model>, id: Id) => gr.groupKeyOf(modelOf(s), id),
  groupMembers: (s: Partial<Model>, id: Id) => gr.groupMembers(modelOf(s), id),
  expandGroups: (s: Partial<Model>, ids: Id[]) => gr.expandGroups(modelOf(s), ids),
  linkChildrenOf: (s: Partial<Model>, id: Id) => lk.linkChildrenOf(modelOf(s), id),
  linkOf: (s: Partial<Model>, id: Id) => lk.linkOf(modelOf(s), id),
  /** Per id: `[groupKeyOf, groupMembers, linkChildren ids, linkOf]`. */
  groupProbe: (s: Partial<Model>, ids: Id[]) => {
    const m = modelOf(s);
    return ids.map((id) => [gr.groupKeyOf(m, id), gr.groupMembers(m, id), lk.linkChildrenOf(m, id).map((a) => a.id), lk.linkOf(m, id)]);
  },
};
const URI = { kind: 'uri', uri: 'https://example.com' };
const GOTO = { kind: 'goto', destination: { kind: 'xyz', page: pageRef(3), left: 10, top: null } };
const linkChild = (id: string, parent: string, target: unknown, over: Partial<Annot> = {}) =>
  annot(id, 'link', gRect(rc(0, 0, 10, 10)), {
    group: parent,
    irt: parent,
    data: { subtype: 'link', target } as never,
    ...over,
  });
const GROUPED = start([
  sq('p'),
  annot('s1', 'strikeout', gQuads([tq(0, 0, 10, 5)]), { group: 'p', irt: 'p' }),
  annot('s2', 'caret', gCaret(rc(0, 0, 4, 4)), { group: 'p' }),
  linkChild('l1', 'p', URI),
  linkChild('l2', 'p', GOTO),
  sq('solo'),
  annot('orphan', 'strikeout', gQuads([tq(0, 0, 10, 5)]), { group: 'ghost' }),
  annot('ink1', 'ink', geom('ink')),
  annot('hl', 'highlight', geom('quads')),
  sq('locked', rc(0, 0, 5, 5), { flags: flags({ locked: true }) }),
  annot('note', 'text', gRect(rc(0, 0, 20, 20))),
  linkChild('lx', 'solo', undefined),
  linkChild('ly', 'ink1', URI, { data: { subtype: 'square' } as never }),
  annot('lz', 'link', gRect(rc(0, 0, 5, 5)), { group: 'hl' }),
]);
const groupCases: FunctionCase[] = [
  c('groupProbe', 'page', GROUPED, ['p', 's1', 's2', 'l1', 'solo', 'orphan', 'missing', 'ink1', 'hl', 'lx', 'ly', 'lz']),
  c('groupKeyOf', 'p', GROUPED, 'p'),
  c('groupMembers', 's1', GROUPED, 's1'),
  c('linkChildrenOf', 'p', GROUPED, 'p'),
  c('linkOf', 'p', GROUPED, 'p'),
  c('expandGroups', 'members', GROUPED, ['s1', 'solo', 'p', 'missing']),
  c('expandGroups', 'empty', GROUPED, []),
  c('groupCaps', 'empty', GROUPED, []),
  c('groupCaps', 'missing-only', GROUPED, ['missing']),
  c('groupCaps', 'shapes', GROUPED, ['p', 'solo']),
  c('groupCaps', 'with-ink', GROUPED, ['p', 'ink1']),
  c('groupCaps', 'with-markup', GROUPED, ['p', 'hl']),
  c('groupCaps', 'with-locked', GROUPED, ['p', 'locked']),
  c('groupCaps', 'with-note', GROUPED, ['p', 'note']),
  c('groupCaps', 'with-caret', GROUPED, ['s2', 'p']),
];

// ── props ────────────────────────────────────────────────────────────────────

export const propFns: Record<string, Fn> = {
  initialTextStyle: () => pr.initialTextStyle,
  styleFromProps: removedUpstream('styleFromProps', '17191ea39'),
  textStyleFromProps: removedUpstream('textStyleFromProps', '17191ea39'),
  readProp: removedUpstream('readProp', '17191ea39'),
  /** Every key through `readProp`, as `[key, value]` pairs. */
  readProps: removedUpstream('readProp', '17191ea39'),
  applyProps: removedUpstream('applyProps', '17191ea39'),
  defaultsFor: (s: Partial<Model>, subtype: string) => defaultsFor(modelOf(s), subtype),
};
const PROP_KEYS = ['color', 'interiorColor', 'strokeWidth', 'opacity', 'blendMode', 'border', 'lineEndings', 'fontFamily', 'fontSize', 'fontColor', 'textAlign', 'icon', 'link'] as const;
const TEXT = { fontFamily: 'courier', fontSize: 11, fontColor: '#123456', textAlign: 'center' as const };
const PROP_ANNOTS: Annot[] = [
  sq('square'),
  annot('circle', 'circle', geom('ellipse'), { style: style({ interiorColor: '#00ff00', border: cloudy(2) }) }),
  annot('line', 'line', geom('line-arrows')),
  annot('line-noends', 'line', geom('line')),
  annot('polyline', 'polyline', geom('poly-open')),
  annot('polyline-noends', 'polyline', geom('poly-open-noends')),
  annot('polygon', 'polygon', geom('poly-closed')),
  annot('ink', 'ink', geom('ink'), { style: style({ blendMode: 'multiply' }) }),
  annot('highlight', 'highlight', geom('quads')),
  annot('caret', 'caret', geom('caret')),
  annot('free-text', 'free-text', geom('text'), { text: TEXT }),
  annot('free-text-bare', 'free-text', geom('text')),
  annot('note', 'text', gRect(rc(0, 0, 20, 20)), { icon: 'key' }),
  annot('attachment', 'file-attachment', gRect(rc(0, 0, 20, 20))),
  annot('stamp', 'stamp', gRect(rc(0, 0, 200, 100))),
  annot('link', 'link', gRect(rc(0, 0, 50, 20)), { link: URI as never }),
  annot('link-null', 'link', gRect(rc(0, 0, 50, 20)), { link: null }),
  annot('link-bare', 'link', gRect(rc(0, 0, 50, 20))),
  annot('redact', 'redact', geom('quads'), { text: TEXT, label: { text: 'X', repeat: false } }),
  annot('widget-text', 'widget-text', gRect(rc(0, 0, 100, 20)), { text: TEXT }),
  annot('widget-toggle', 'widget-toggle', gRect(rc(0, 0, 12, 12))),
  annot('unknown', 'unknown', gRect(rc(0, 0, 10, 10))),
  sq('locked', rc(0, 0, 10, 10), { flags: flags({ locked: true }) }),
  sq('read-only', rc(0, 0, 10, 10), { flags: flags({ readOnly: true }) }),
  annot('widget-read-only', 'widget-text', gRect(rc(0, 0, 100, 20)), { flags: flags({ readOnly: true }), text: TEXT }),
  sq('no-authority', rc(0, 0, 10, 10), { authority: { update: false, delete: true } }),
];
const PATCHES: [string, AnnotationPropsPatch][] = [
  ['empty', {}],
  ['color', { color: '#0000ff' }],
  ['fill', { interiorColor: '#ff00ff' }],
  ['clear-fill', { interiorColor: null }],
  ['stroke', { strokeWidth: 7 }],
  ['zero-stroke', { strokeWidth: 0 }],
  ['opacity', { opacity: 0.25 }],
  ['blend', { blendMode: 'screen' }],
  ['border-dashed', { border: DASHED }],
  ['border-cloudy', { border: cloudy(1.5) }],
  ['end-only', { lineEndings: { end: 'slash' } }],
  ['both-ends', { lineEndings: { start: 'diamond', end: 'butt' } }],
  ['empty-ends', { lineEndings: {} }],
  ['font', { fontFamily: 'times-roman', fontSize: 30 }],
  ['font-color-align', { fontColor: '#ff0000', textAlign: 'right' }],
  ['icon', { icon: 'help' }],
  ['link', { link: GOTO as never }],
  ['link-null', { link: null }],
  ['everything', { color: '#111111', interiorColor: '#222222', strokeWidth: 3, opacity: 0.5, blendMode: 'darken', border: SOLID, lineEndings: { start: 'circle' }, fontFamily: 'symbol', fontSize: 9, fontColor: '#333333', textAlign: 'left', icon: 'note', link: URI as never }],
];
const propCases: FunctionCase[] = [
  c('initialTextStyle', 'value'),
  c('styleFromProps', 'defaults', defaultsFor(initialModel, 'square')),
  c('textStyleFromProps', 'defaults', defaultsFor(initialModel, 'free-text')),
  ...PROP_ANNOTS.map((a) => c('readProps', a.id, a, PROP_KEYS)),
  c('readProp', 'square-color', PROP_ANNOTS[0], 'color'),
  ...PROP_ANNOTS.flatMap((a) => PATCHES.map(([pn, p]) => c('applyProps', `${a.id}-${pn}`, a, p))),
  c('defaultsFor', 'initial', {}, 'square'),
  c('defaultsFor', 'override', { defaults: { square: { color: '#abcdef', lineEndings: { end: 'circle' }, fontSize: 20 } } }, 'square'),
  c('defaultsFor', 'other-subtype', { defaults: { square: { color: '#abcdef' } } }, 'circle'),
  c('defaultsFor', 'null-fill-and-link', { defaults: { link: { interiorColor: null, link: null } } }, 'link'),
  c('defaultsFor', 'custom-style', { style: style({ color: '#000000', strokeWidth: 9 }) }, 'ink'),
  c('defaultsFor', 'filled-style-kept', { style: style({ interiorColor: '#123456' }), defaults: { square: { color: '#000000' } } }, 'square'),
  c('defaultsFor', 'filled-style-cleared', { style: style({ interiorColor: '#123456' }), defaults: { square: { interiorColor: null } } }, 'square'),
  c('defaultsFor', 'prototype-key', {}, 'constructor'),
  c('defaultsFor', 'extra-key', { defaults: { square: { bogus: 1 } as never } }, 'square'),
];

// ── anchors ──────────────────────────────────────────────────────────────────

const anchorFns: Record<string, Fn> = {
  anchorModeOf: an.anchorModeOf,
  anchorOf: an.anchorOf,
  anchoredGeom: an.anchoredGeom,
  unanchoredGeom: an.unanchoredGeom,
  anchoredStrokeWidth: an.anchoredStrokeWidth,
  anchoredBox: an.anchoredBox,
};
const MODES = [
  ['none', null],
  ['zoom', { zoom: true, upright: false }],
  ['upright', { zoom: false, upright: true }],
  ['both', { zoom: true, upright: true }],
] as const;
const VIEWS = [
  ['none', undefined],
  ['z1-r0', { zoom: 1, rotation: 0 }],
  ['z2-r0', { zoom: 2, rotation: 0 }],
  ['z0.5-r90', { zoom: 0.5, rotation: 90 }],
  ['z3-r270', { zoom: 3, rotation: 270 }],
  ['z0-r180', { zoom: 0, rotation: 180 }],
  ['z1.5-r-90', { zoom: 1.5, rotation: -90 }],
  ['zNaN-r90', { zoom: NaN, rotation: 90 }],
] as const;
const anchorCases: FunctionCase[] = [];
for (const subtype of ['square', 'text', 'file-attachment', 'unknown']) {
  for (const [fname, f] of [['none', flags()], ['noZoom', flags({ noZoom: true })], ['noRotate', flags({ noRotate: true })], ['both', flags({ noZoom: true, noRotate: true })]] as const) {
    anchorCases.push(c('anchorModeOf', `${subtype}-${fname}`, { subtype, flags: f }));
  }
}
for (const [name, gm] of GEOMS) anchorCases.push(c('anchorOf', name, gm));
const ANCHOR_GEOMS = ['rect', 'rect-rot30', 'line-arrows', 'poly-closed', 'ink-two', 'text', 'callout-knee', 'quads', 'caret'];
for (const gname of ANCHOR_GEOMS) {
  for (const [mname, mode] of MODES) {
    for (const [vname, view] of VIEWS) {
      if (mname === 'none' && vname !== 'z2-r0') continue;
      anchorCases.push(c('anchoredGeom', `${gname}-${mname}-${vname}`, geom(gname), mode, view));
      const projected = an.anchoredGeom(geom(gname), mode, view as an.ViewEnv | undefined);
      anchorCases.push(c('unanchoredGeom', `${gname}-${mname}-${vname}`, projected, mode, view));
    }
  }
}
for (const [mname, mode] of MODES) {
  for (const [vname, view] of VIEWS) {
    anchorCases.push(c('anchoredStrokeWidth', `${mname}-${vname}`, 2, mode, view));
    anchorCases.push(c('anchoredBox', `${mname}-${vname}`, rc(100, 100, 40, 20), v(90, 95), mode, view));
  }
}

// ── snapping ─────────────────────────────────────────────────────────────────

const snapFns: Record<string, Fn> = {
  computeMoveSnap: (s: Partial<Model>, ids: Id[], pon: number, raw: Vec, threshold: number, pageBox?: Rect) =>
    sn.computeMoveSnap(modelOf(s), ids, pageRef(pon), raw, threshold, pageBox),
};
const SNAP = start([
  sq('a', rc(100, 100, 80, 60)),
  sq('b', rc(300, 100, 50, 50)),
  sq('c', rc(100, 300, 40, 40)),
  sq('rot', rc(400, 400, 60, 30), { geom: gRect(rc(400, 400, 60, 30), false, 45) }),
  sq('other-page', rc(100, 100, 80, 60), { page: pageRef(2) }),
  sq('hidden', rc(0, 0, 10, 10), { flags: flags({ hidden: true }) }),
  annot('note', 'text', gRect(rc(500, 500, 20, 20))),
  sq('nozoom', rc(250, 250, 20, 20), { flags: flags({ noZoom: true }) }),
]);
const PAGE = rc(0, 0, 600, 800);
const snapCases: FunctionCase[] = [
  c('computeMoveSnap', 'no-targets-in-range', SNAP, ['a'], PON, v(50, 50), 5),
  c('computeMoveSnap', 'x-edge', SNAP, ['a'], PON, v(118, 37), 5),
  c('computeMoveSnap', 'y-edge', SNAP, ['a'], PON, v(37, 2), 5),
  c('computeMoveSnap', 'both-axes', SNAP, ['a'], PON, v(122, 3), 5),
  c('computeMoveSnap', 'centre', SNAP, ['c'], PON, v(203, -1), 5),
  c('computeMoveSnap', 'page-edge', SNAP, ['a'], PON, v(-98, 0), 5, PAGE),
  c('computeMoveSnap', 'page-centre', SNAP, ['a'], PON, v(158, 297), 5, PAGE),
  c('computeMoveSnap', 'threshold-exclusive', SNAP, ['a'], PON, v(125, 37), 5),
  c('computeMoveSnap', 'threshold-inside', SNAP, ['a'], PON, v(124.99, 37), 5),
  c('computeMoveSnap', 'rotated-target', SNAP, ['c'], PON, v(274, 60), 5),
  c('computeMoveSnap', 'multi-moving', SNAP, ['a', 'b'], PON, v(1, 198), 5),
  c('computeMoveSnap', 'anchored-mover', SNAP, ['a', 'note'], PON, v(118, 37), 5),
  c('computeMoveSnap', 'nozoom-mover', SNAP, ['nozoom'], PON, v(50, 150), 5),
  c('computeMoveSnap', 'other-page-pon', SNAP, ['other-page'], 2, v(118, 37), 5),
  c('computeMoveSnap', 'zero-threshold', SNAP, ['a'], PON, v(200, 0), 0),
  c('computeMoveSnap', 'closest-wins', SNAP, ['c'], PON, v(-2, -199), 5),
  c('computeMoveSnap', 'equal-distance-keeps-first', SNAP, ['c'], PON, v(-2.5, 0), 5),
];

// ── hit testing ──────────────────────────────────────────────────────────────

export const hitFns: Record<string, Fn> = {
  paintOrder: (s: Partial<Model>, pon: number) => ht.paintOrder(modelOf(s), pageRef(pon)),
  isSelectable: (s: Partial<Model>, id: Id) => ht.isSelectable(modelOf(s), id),
  canMove: (s: Partial<Model>, id: Id) => ht.canMove(modelOf(s), id),
  /** Per id: `[isSelectable, canMove]`. */
  selectability: (s: Partial<Model>, ids: Id[]) => {
    const m = modelOf(s);
    return ids.map((id) => [ht.isSelectable(m, id), ht.canMove(m, id)]);
  },
  groupUnionBounds: (s: Partial<Model>, pon: number, view?: an.ViewEnv) => ht.groupUnionBounds(modelOf(s), pageRef(pon), view),
  hitTest: (s: Partial<Model>, pon: number, p: Vec, gm: ChromeGeom, margin: number, pageBox?: Rect, inert?: Set<Id>, view?: an.ViewEnv) =>
    ht.hitTest(modelOf(s), pageRef(pon), p, gm, margin, pageBox, inert, view),
  cursorAt: (s: Partial<Model>, pon: number, p: Vec, gm: ChromeGeom, margin: number, pageBox?: Rect, inert?: Set<Id>, view?: an.ViewEnv) =>
    ht.cursorAt(modelOf(s), pageRef(pon), p, gm, margin, pageBox, inert, view),
  /** One page, many calls: each call sets the selection and passes every
   *  hitTest argument; the answer is `[hitTest, cursorAt]`. */
  hitCalls: (
    s: Partial<Model>,
    calls: { selected: Id[]; pon: number; p: Vec; gm: ChromeGeom; margin: number; pageBox?: Rect; inert?: Set<Id>; view?: an.ViewEnv }[],
  ) =>
    calls.map(({ selected, pon, p, gm, margin, pageBox, inert, view }) => {
      const m = { ...modelOf(s), selected };
      return [ht.hitTest(m, pageRef(pon), p, gm, margin, pageBox, inert, view), ht.cursorAt(m, pageRef(pon), p, gm, margin, pageBox, inert, view)];
    }),
  /** One page, many selections and points: per selection, `[hitTest,
   *  cursorAt]` per point. */
  hitProbe: (s: Partial<Model>, pon: number, selections: Id[][], points: Vec[], gm: ChromeGeom, margin: number) =>
    selections.map((selected) => {
      const m = { ...modelOf(s), selected };
      return points.map((p) => [ht.hitTest(m, pageRef(pon), p, gm, margin), ht.cursorAt(m, pageRef(pon), p, gm, margin)]);
    }),
};
const CG: ChromeGeom = { handleTol: 6, knobTol: 6, knobOffset: 24 };
const HIT_ANNOTS: Annot[] = [
  annot('hl', 'highlight', gQuads([tq(10, 400, 200, 14)])),
  sq('sq', rc(100, 100, 80, 60)),
  sq('filled', rc(300, 100, 80, 60), { style: style({ interiorColor: '#ffffff' }) }),
  annot('ci', 'circle', gRect(rc(100, 250, 80, 60), true)),
  annot('ln', 'line', gLine(v(300, 250), v(400, 300), ends('none', 'closed-arrow'))),
  annot('pg', 'polygon', gPoly([v(100, 500), v(200, 500), v(150, 580)], true)),
  annot('pl', 'polyline', gPoly([v(300, 500), v(350, 560), v(400, 500)], false)),
  annot('ink', 'ink', gInk([[v(450, 100), v(500, 150), v(550, 100)]])),
  annot('ft', 'free-text', gText(rc(450, 250, 100, 40)), { text: pr.initialTextStyle }),
  annot('stamp', 'stamp', gRect(rc(450, 400, 80, 40), false, 30)),
  annot('note', 'text', gRect(rc(20, 20, 20, 20))),
  annot('caret', 'caret', gCaret(rc(250, 400, 6, 6))),
  annot('link', 'link', gRect(rc(20, 700, 100, 20))),
  annot('redact-area', 'redact', gRect(rc(300, 650, 100, 50))),
  annot('redact-text', 'redact', gQuads([tq(450, 650, 100, 14)])),
  sq('locked', rc(100, 650, 60, 40), { flags: flags({ locked: true }) }),
  sq('hidden', rc(200, 650, 60, 40), { flags: flags({ hidden: true }) }),
  sq('noview', rc(200, 720, 60, 40), { flags: flags({ noView: true, toggleNoView: true }) }),
  sq('readonly', rc(20, 600, 30, 30), { flags: flags({ readOnly: true }) }),
  annot('widget', 'widget-text', gRect(rc(20, 550, 80, 20)), { flags: flags({ readOnly: true }) }),
  annot('reply', 'text', gRect(rc(100, 100, 20, 20)), { irt: 'sq' }),
  annot('review-state', 'text', gRect(rc(120, 120, 20, 20)), { data: { subtype: 'text', state: 'accepted' } as never }),
  annot('reply2', 'text', gRect(rc(200, 200, 20, 20)), { irt: 'sq' }),
  annot('child-link', 'link', gRect(rc(100, 100, 80, 60)), { group: 'sq', irt: 'sq' }),
  sq('other-page', rc(100, 100, 80, 60), { page: pageRef(2) }),
  annot('unknown', 'mystery', gRect(rc(560, 20, 20, 20))),
  sq('nozoom', rc(560, 700, 30, 30), { flags: flags({ noZoom: true, noRotate: true }) }),
];
const HIT = (selected: Id[] = [], over: Partial<Model> = {}) => start(HIT_ANNOTS, { selected, ...over });
/** Points worth probing: each annotation's centre and top-left, handle and
 *  knob spots, and empty space. */
const HIT_POINTS: [string, Vec][] = [
  ['empty', v(250, 180)],
  ['sq-edge', v(101, 130)],
  ['sq-inside', v(140, 130)],
  ['sq-nw', v(100, 100)],
  ['sq-se', v(180, 160)],
  ['sq-knob', v(140, 76)],
  ['filled-inside', v(340, 130)],
  ['ci-edge', v(101, 280)],
  ['ci-inside', v(140, 280)],
  ['ln-mid', v(350, 275)],
  ['ln-v1', v(400, 300)],
  ['ln-arrow', v(396, 295)],
  ['pg-inside', v(150, 530)],
  ['pg-v0', v(100, 500)],
  ['pl-v1', v(350, 560)],
  ['ink-stroke', v(475, 125)],
  ['ft-inside', v(500, 270)],
  ['stamp-centre', v(490, 420)],
  ['note', v(30, 30)],
  ['caret', v(253, 403)],
  ['hl', v(100, 405)],
  ['link', v(50, 710)],
  ['redact-area-inside', v(350, 675)],
  ['redact-area-edge', v(300, 675)],
  ['redact-text', v(480, 655)],
  ['locked-inside', v(130, 670)],
  ['locked-edge', v(100, 670)],
  ['hidden', v(200, 670)],
  ['noview', v(200, 740)],
  ['readonly', v(20, 615)],
  ['widget', v(50, 560)],
  ['unknown', v(570, 30)],
  ['nozoom', v(560, 700)],
  ['nozoom-inside', v(575, 715)],
  ['between-sq-filled', v(240, 130)],
];
const HIT_SELECTIONS: [string, Id[]][] = [
  ['none', []],
  ['sq', ['sq']],
  ['filled', ['filled']],
  ['ln', ['ln']],
  ['pg', ['pg']],
  ['ft', ['ft']],
  ['stamp', ['stamp']],
  ['hl', ['hl']],
  ['locked', ['locked']],
  ['caret', ['caret']],
  ['sq+filled', ['sq', 'filled']],
  ['sq+ln+ink', ['sq', 'ln', 'ink']],
  ['sq+hl', ['sq', 'hl']],
  ['sq+locked', ['sq', 'locked']],
  ['sq+note', ['sq', 'note']],
  ['missing', ['ghost']],
  ['sq+other-page', ['sq', 'other-page']],
  ['nozoom', ['nozoom']],
  ['hidden', ['hidden']],
];
const hitCases: FunctionCase[] = [
  c('paintOrder', 'page1', HIT(), PON),
  c('paintOrder', 'page1-noview-engaged', HIT(['noview']), PON),
  c('paintOrder', 'page2', HIT(), 2),
  c('paintOrder', 'empty-page', HIT(), 9),
  c('paintOrder', 'dangling-order', start([sq('a')], { order: ['ghost', 'a'] }), PON),
  c('isSelectable', 'sq', HIT(), 'sq'),
  c('canMove', 'sq', HIT(), 'sq'),
  c('selectability', 'page', HIT(), [...HIT_ANNOTS.map((a) => a.id), 'ghost']),
  c('groupUnionBounds', 'none', HIT(), PON),
  c('groupUnionBounds', 'two', HIT(['sq', 'filled']), PON),
  c('groupUnionBounds', 'cross-page', HIT(['sq', 'other-page', 'ghost']), PON),
  c('groupUnionBounds', 'rotated-stamp', HIT(['stamp', 'sq']), PON),
  c('groupUnionBounds', 'nozoom-view', HIT(['nozoom', 'sq']), PON, { zoom: 2, rotation: 90 }),
];
hitCases.push(c('hitProbe', 'page', HIT(), PON, HIT_SELECTIONS.map(([, sel]) => sel), HIT_POINTS.map(([, p]) => p), CG, 6));
// Knob and handles of a selection, with and without a page box and a view.
const sqKnob = v(140, 76);
type HitCall = { label: string; selected: Id[]; pon: number; p: Vec; gm: ChromeGeom; margin: number; pageBox?: Rect; inert?: Set<Id>; view?: an.ViewEnv };
const hitCalls: HitCall[] = [];
const hc = (label: string, selected: Id[], pon: number, p: Vec, gm: ChromeGeom, margin: number, pageBox?: Rect, inert?: Set<Id>, view?: an.ViewEnv): FunctionCase[] => {
  hitCalls.push({ label, selected, pon, p, gm, margin, pageBox, inert, view });
  return [];
};
hitCases.push(
  c('hitTest', 'knob-flipped-page', HIT(['sq']), PON, v(140, 184), CG, 6, rc(0, 90, 600, 800)),
  ...hc('knob-flipped-page', ['sq'], PON, v(140, 184), CG, 6, rc(0, 90, 600, 800)),
  ...hc('knob-group', ['sq', 'filled'], PON, v(240, 76), CG, 6),
  ...hc('knob-group-page', ['sq', 'filled'], PON, v(240, 184), CG, 6, rc(0, 90, 600, 800)),
  ...hc('group-handle-se', ['sq', 'filled'], PON, v(380, 160), CG, 6),
  ...hc('group-handle-nw', ['sq', 'filled'], PON, v(100, 100), CG, 6),
  ...hc('group-union-gap', ['sq', 'filled'], PON, v(240, 130), CG, 6),
  ...hc('group-union-gap-locked', ['sq', 'locked'], PON, v(130, 400), CG, 6),
  ...hc('group-not-rotatable', ['sq', 'note'], PON, v(140, 76), CG, 6),
  ...hc('group-not-resizable', ['sq', 'note'], PON, v(180, 160), CG, 6),
  ...hc('group-other-page-members', ['sq', 'other-page', 'filled'], PON, v(380, 160), CG, 6),
  ...hc('group-on-page2', ['sq', 'other-page'], 2, v(140, 130), CG, 6),
  ...hc('knob-tight-tol', ['sq'], PON, v(146.5, 76), CG, 6),
  ...hc('knob-tol-edge', ['sq'], PON, v(146, 82), CG, 6),
  ...hc('knob-wide-tol', ['sq'], PON, v(155, 90), { handleTol: 6, knobTol: 20, knobOffset: 24 }, 6),
  ...hc('knob-offset-40', ['sq'], PON, v(140, 60), { handleTol: 6, knobTol: 6, knobOffset: 40 }, 6),
  ...hc('handle-tol-edge', ['sq'], PON, v(186, 166), CG, 6),
  ...hc('handle-tol-past', ['sq'], PON, v(186.01, 166), CG, 6),
  ...hc('margin-0', [], PON, v(99, 130), CG, 0),
  ...hc('margin-12', [], PON, v(90, 130), CG, 12),
  ...hc('inert-sq', [], PON, v(101, 130), CG, 6, undefined, new Set(['sq'])),
  ...hc('inert-empty', [], PON, v(101, 130), CG, 6, undefined, new Set()),
  ...hc('selected-on-page2', ['other-page'], 2, v(140, 76), CG, 6),
  ...hc('selected-other-page', ['other-page'], PON, sqKnob, CG, 6),
  ...hc('nozoom-view-knob', ['nozoom'], PON, v(567.5, 676), CG, 6, undefined, undefined, { zoom: 2, rotation: 0 }),
  ...hc('nozoom-view-body', ['nozoom'], PON, v(565, 705), CG, 6, undefined, undefined, { zoom: 2, rotation: 90 }),
  ...hc('nozoom-view-unselected', [], PON, v(561, 701), CG, 6, undefined, undefined, { zoom: 2, rotation: 0 }),
  ...hc('nozoom-view-handle', ['nozoom'], PON, v(575, 715), CG, 6, undefined, undefined, { zoom: 2, rotation: 0 }),
  ...hc('rotated-stamp-knob', ['stamp'], PON, g2knob(), CG, 6),
  ...hc('union-of-substrate-only', ['reply', 'reply2'], PON, v(200, 180), CG, 6),
  ...hc('locked-knob', ['locked'], PON, v(130, 626), CG, 6),
  ...hc('note-knob', ['note'], PON, v(30, -4), CG, 6),
);
function g2knob(): Vec {
  // the rotated stamp's knob: the top-edge midpoint pushed out along the tilted normal
  const r = rc(450, 400, 80, 40);
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;
  const rad = (30 * Math.PI) / 180;
  const top = v(cx + 20 * Math.sin(rad) * -1 * -1, cy - 20 * Math.cos(rad));
  return v(top.x + 24 * Math.sin(rad), top.y - 24 * Math.cos(rad));
}
hitCases.push(
  c('cursorAt', 'knob', HIT(['sq']), PON, sqKnob, CG, 6),
  ...hc('knob', ['sq'], PON, sqKnob, CG, 6),
  ...hc('handle', ['sq'], PON, v(180, 160), CG, 6),
  ...hc('group-handle', ['sq', 'filled'], PON, v(380, 160), CG, 6),
  ...hc('selected-body', ['sq'], PON, v(140, 130), CG, 6),
  ...hc('selected-anchored', ['hl'], PON, v(100, 405), CG, 6),
  ...hc('unselected-body', [], PON, v(101, 130), CG, 6),
  ...hc('empty', [], PON, v(250, 180), CG, 6),
);

hitCases.push(c('hitCalls', 'page', HIT(), hitCalls));

// ── scene ────────────────────────────────────────────────────────────────────

export const sceneFns: Record<string, Fn> = {
  scene: sc.scene,
  blendFor,
  layoutRedactLabel: sc.layoutRedactLabel,
};
const item = (subtype: string, gm: Geom, over: Partial<RenderItem> = {}): RenderItem => ({
  id: 'i',
  ref: null,
  subtype,
  geom: gm,
  box: rc(0, 0, 0, 0),
  style: style(),
  source: 'vector',
  selected: false,
  ...over,
});
const sceneCases: FunctionCase[] = [
  ...['normal', 'multiply', 'screen', 'luminosity'].map((b) => c('blendFor', b, style({ blendMode: b as never }))),
];
const SCENE_STYLES: [string, ReturnType<typeof style>][] = [
  ['default', style()],
  ['filled-dashed', style({ interiorColor: '#00ff00', border: DASHED, blendMode: 'multiply', opacity: 0.4 })],
  ['cloudy', style({ border: cloudy(1), interiorColor: '#0000ff' })],
  ['zero-stroke', style({ strokeWidth: 0, interiorColor: '' })],
];
for (const [gname, gm] of GEOMS) {
  for (const [sname, st] of SCENE_STYLES) {
    const subtype =
      gm.t === 'rect' ? 'square' : gm.t === 'line' ? 'line' : gm.t === 'poly' ? 'polygon' : gm.t === 'quads' ? 'highlight' : gm.t === 'caret' ? 'caret' : gm.t === 'ink' ? 'ink' : 'free-text';
    sceneCases.push(c('scene', `${gname}-${sname}`, item(subtype, gm, { style: st })));
  }
}
const MARKUP_QUADS = [tq(10, 10, 200, 14), tqRot(50, 100, 120, 10, 30), tq(0, 0, 0, 10), tq(0, 0, 10, 0), tq(0, 0, 3, 50), tq(0, 0, 400, 2)];
for (const subtype of ['highlight', 'underline', 'strikeout', 'squiggly', 'mystery-markup']) {
  sceneCases.push(c('scene', `markup-${subtype}`, item(subtype, gQuads(MARKUP_QUADS), { style: style({ blendMode: 'multiply', color: '#ffcc00', opacity: 0.5 }) })));
  sceneCases.push(c('scene', `markup-${subtype}-normal`, item(subtype, gQuads([tq(5, 5, 33.3, 7.7)]))));
}
sceneCases.push(c('scene', 'squiggly-long', item('squiggly', gQuads([tq(0.5, 0.25, 1000.123, 30)]))));
sceneCases.push(c('scene', 'squiggly-short', item('squiggly', gQuads([tq(0, 0, 2, 10)]))));
// Bounded output: a NaN or backwards run draws one hump and no more.
sceneCases.push(c('scene', 'squiggly-nan-width', item('squiggly', gQuads([tq(0, 0, NaN, 10)]))));
sceneCases.push(c('scene', 'squiggly-backwards', item('squiggly', gQuads([tq(100, 0, -80, 10)]))));
sceneCases.push(c('scene', 'squiggly-rotated-tall', item('squiggly', gQuads([tqRot(50, 50, 120, 20, 25)]))));
for (const e of ENDINGS) sceneCases.push(c('scene', `line-${e}`, item('line', gLine(v(0, 0), v(100, 50), ends(e, e)), { style: style({ interiorColor: '#ff0000' }) })));
sceneCases.push(c('scene', 'link', item('link', geom('rect'))));
sceneCases.push(c('scene', 'caret-rot', item('caret', geom('caret-rot45'), { style: style({ opacity: 0.3 }) })));
sceneCases.push(c('scene', 'ink-cloudy', item('ink', geom('ink'), { style: style({ border: cloudy(1), blendMode: 'multiply' }) })));
const REDACT_TEXT = { fontFamily: 'helvetica', fontSize: 10, fontColor: '#ff0000', textAlign: 'left' as const };
for (const [gname, gm] of [
  ['area', gRect(rc(10, 10, 200, 60))],
  ['text', gQuads([tq(10, 100, 150, 14), tqRot(10, 130, 150, 14, 20), tq(10, 160, 0, 14), tq(10, 190, 60, 0)])],
  ['line', gLine(v(0, 0), v(10, 10))],
] as const) {
  sceneCases.push(
    c('scene', `redact-${gname}-rest`, item('redact', gm, { style: style({ interiorColor: '#000000' }) })),
    c('scene', `redact-${gname}-rest-zero-stroke`, item('redact', gm, { style: style({ strokeWidth: 0 }) })),
    c('scene', `redact-${gname}-hover`, item('redact', gm, { hovered: true, style: style({ interiorColor: '#000000' }), label: { text: 'REDACTED', repeat: false }, text: REDACT_TEXT })),
    c('scene', `redact-${gname}-hover-repeat`, item('redact', gm, { hovered: true, style: style({ interiorColor: '#000000' }), label: { text: 'X', repeat: true }, text: REDACT_TEXT })),
    c('scene', `redact-${gname}-hover-bare`, item('redact', gm, { hovered: true })),
    c('scene', `redact-${gname}-hover-no-fill`, item('redact', gm, { hovered: true, label: { text: 'Y', repeat: false } })),
  );
}
const REGION = rc(10, 20, 200, 50);
for (const [label, lb, text] of [
  ['empty-text', { text: '', repeat: false }, REDACT_TEXT],
  ['left', { text: 'Secret', repeat: false }, REDACT_TEXT],
  ['center', { text: 'Secret', repeat: false }, { ...REDACT_TEXT, textAlign: 'center' }],
  ['right', { text: 'Secret', repeat: false }, { ...REDACT_TEXT, textAlign: 'right' }],
  ['center-overflow', { text: 'A very long redaction label here', repeat: false }, { ...REDACT_TEXT, textAlign: 'center' }],
  ['right-overflow', { text: 'A very long redaction label here', repeat: false }, { ...REDACT_TEXT, textAlign: 'right' }],
  ['auto-size', { text: 'Auto', repeat: false }, { ...REDACT_TEXT, fontSize: 0 }],
  ['no-text-style', { text: 'Auto', repeat: false }, undefined],
  ['too-tall', { text: 'Big', repeat: false }, { ...REDACT_TEXT, fontSize: 60 }],
  ['tiny-font', { text: 'Tiny', repeat: false }, { ...REDACT_TEXT, fontSize: 1 }],
  ['no-family', { text: 'NF', repeat: false }, { ...REDACT_TEXT, fontFamily: '' }],
  ['repeat', { text: 'R', repeat: true }, REDACT_TEXT],
  ['repeat-wide', { text: 'Wider than the region itself', repeat: true }, REDACT_TEXT],
  ['repeat-auto', { text: 'AB', repeat: true }, undefined],
  ['utf16', { text: '\u{1F600}é', repeat: false }, REDACT_TEXT],
] as const) {
  sceneCases.push(c('layoutRedactLabel', label, REGION, lb, text));
}
sceneCases.push(c('layoutRedactLabel', 'repeat-capped-400', rc(0, 0, 2000, 2000), { text: 'x', repeat: true }, { ...REDACT_TEXT, fontSize: 4 }));
sceneCases.push(c('layoutRedactLabel', 'repeat-tiny-region', rc(0, 0, 1, 1), { text: 'x', repeat: true }, REDACT_TEXT));
sceneCases.push(c('layoutRedactLabel', 'font-equals-height', rc(0, 0, 200, 50), { text: 'Fit', repeat: false }, { ...REDACT_TEXT, fontSize: 50 }));

// ── placement and ink ────────────────────────────────────────────────────────

const placeFns: Record<string, Fn> = {
  clampRectToBox: pl.clampRectToBox,
  resolveClickPlacement: pl.resolveClickPlacement,
  clickCreateGeom: removedUpstream('clickCreateGeom', '23a0181b3'),
  straightenInkStroke: ik.straightenInkStroke,
};
const placeCases: FunctionCase[] = [
  c('clampRectToBox', 'no-box', rc(10, 10, 50, 50)),
  c('clampRectToBox', 'inside', rc(10, 10, 50, 50), PAGE),
  c('clampRectToBox', 'left-top', rc(-10, -20, 50, 50), PAGE),
  c('clampRectToBox', 'right-bottom', rc(580, 790, 50, 50), PAGE),
  c('clampRectToBox', 'bigger-than-box', rc(100, 100, 700, 900), PAGE),
  c('clampRectToBox', 'offset-box', rc(0, 0, 10, 10), rc(50, 60, 100, 100)),
];
const POLICIES = [
  ['box', { width: 100, height: 40 }],
  ['box-center', { width: 100, height: 40, anchor: 'center' }],
  ['box-top-left', { width: 180, height: 40, anchor: 'top-left' }],
  ['segment', { length: 100 }],
  ['segment-45', { length: 100, angleDeg: 45 }],
  ['segment-180', { length: 50, angleDeg: 180 }],
  ['segment-up', { length: 100, angleDeg: -90 }],
  ['segment-and-box', { length: 30, width: 5, height: 5 }],
] as const;
for (const [pname, policy] of POLICIES) {
  for (const [oname, opts] of [
    ['none', undefined],
    ['empty', {}],
    ['page', { pageBox: PAGE }],
    ['upright-0', { upright: true, displayRotation: 0 }],
    ['upright-90', { upright: true, displayRotation: 90 }],
    ['upright-180', { upright: true, displayRotation: 180, pageBox: PAGE }],
    ['upright-270', { upright: true, displayRotation: 270 }],
    ['rotation-no-upright', { displayRotation: 90 }],
  ] as const) {
    placeCases.push(c('resolveClickPlacement', `${pname}-${oname}-mid`, v(300, 400), policy, opts));
    placeCases.push(c('resolveClickPlacement', `${pname}-${oname}-corner`, v(590, 5), policy, opts));
  }
}
const DEF = defaultsFor(initialModel, 'square');
const DEF_CLOUDY = { ...DEF, border: cloudy(2), lineEndings: ends('open-arrow', 'none') };
for (const subtype of ['free-text', 'square', 'circle', 'line', 'stamp', 'ink', 'link']) {
  for (const [plname, placement] of [
    ['box', { kind: 'box', rect: rc(10, 20, 100, 40), rot: 0 }],
    ['box-rot', { kind: 'box', rect: rc(10, 20, 100, 40), rot: 90 }],
    ['segment', { kind: 'segment', a: v(0, 0), b: v(100, 0) }],
  ] as const) {
    placeCases.push(c('clickCreateGeom', `${subtype}-${plname}`, subtype, placement, DEF));
    placeCases.push(c('clickCreateGeom', `${subtype}-${plname}-cloudy`, subtype, placement, DEF_CLOUDY));
  }
}
const OPTS = { deviationThreshold: 0.1, axisSnapDegrees: 10 };
for (const [label, pts, opts] of [
  ['two-points', [v(0, 0), v(100, 0)], OPTS],
  ['empty', [], OPTS],
  ['short', [v(0, 0), v(1, 1), v(2, 1)], OPTS],
  ['horizontal', [v(0, 0), v(50, 2), v(100, 1)], OPTS],
  ['vertical', [v(0, 0), v(2, 50), v(1, 100)], OPTS],
  ['diagonal', [v(0, 0), v(50, 52), v(100, 100)], OPTS],
  ['wobbly', [v(0, 0), v(50, 30), v(100, 0)], OPTS],
  ['at-threshold', [v(0, 0), v(50, 10), v(100, 0)], OPTS],
  // Just inside and outside the 10° snap: the exact boundary is a last-bit
  // question for atan2, which V8 and the platform libm may answer apart.
  ['snap-inside', [v(0, 0), v(50, 8.8), v(100, 17.6)], OPTS],
  ['snap-outside', [v(0, 0), v(50, 8.8), v(100, 17.7)], OPTS],
  ['vertical-inside', [v(0, 0), v(8.8, 50), v(17.6, 100)], OPTS],
  ['vertical-outside', [v(0, 0), v(8.8, 50), v(17.7, 100)], OPTS],
  ['reversed', [v(100, 5), v(50, 3), v(0, 0)], OPTS],
  ['loose-threshold', [v(0, 0), v(50, 30), v(100, 0)], { deviationThreshold: 0.5, axisSnapDegrees: 45 }],
  ['closed-loop', [v(0, 0), v(50, 50), v(0, 1)], OPTS],
  ['offset-horizontal', [v(40, 30), v(90, 32), v(140, 31)], OPTS],
  ['offset-vertical', [v(40, 30), v(42, 80), v(41, 130)], OPTS],
  ['length-exactly-3', [v(10, 10), v(11.5, 10.05), v(13, 10)], OPTS],
] as const) {
  placeCases.push(c('straightenInkStroke', label, pts, opts));
}

export const suites: Suite[] = [
  { kind: 'function', file: 'core/annotation-kinds.json', describes: 'packages/core/annotation kinds.ts and props.ts sharedProps/kindTakesLink: the kind table, caps and prop specs per subtype.', functions: kindFns, cases: kinds },
  { kind: 'function', file: 'core/annotation-flags.json', describes: 'packages/core/annotation flags.ts and plane.ts: /F predicates, authority, conversation and substrate planes.', functions: flagFns, cases: flagCases },
  { kind: 'function', file: 'core/annotation-groups.json', describes: 'packages/core/annotation group.ts and links.ts: group keys, members, caps and attached link children.', functions: groupFns, cases: groupCases },
  { kind: 'function', file: 'core/annotation-props.json', describes: 'packages/core/annotation props.ts: readProp and applyProps for every kind and patch key, plus defaultsFor.', functions: propFns, cases: propCases },
  { kind: 'function', file: 'core/annotation-anchor.json', describes: 'packages/core/annotation anchor.ts: noZoom/noRotate projection and its inverse.', functions: anchorFns, cases: anchorCases },
  { kind: 'function', file: 'core/annotation-snap.json', describes: 'packages/core/annotation snap.ts: move snapping guides.', functions: snapFns, cases: snapCases },
  { kind: 'function', file: 'core/annotation-hit.json', describes: 'packages/core/annotation hit.ts: paint order, selectability, hitTest and cursorAt over a mixed page.', functions: hitFns, cases: hitCases },
  { kind: 'function', file: 'core/annotation-scene.json', describes: 'packages/core/annotation scene.ts: painted scene nodes per kind, markup, redaction and label layout.', functions: sceneFns, cases: sceneCases },
  { kind: 'function', file: 'core/annotation-placement.json', describes: 'packages/core/annotation placement.ts and ink.ts: click placement, click geometry and ink straightening.', functions: placeFns, cases: placeCases },
];
