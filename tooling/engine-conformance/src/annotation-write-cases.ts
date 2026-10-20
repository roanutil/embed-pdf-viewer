/**
 * The cases `generate-annotation-write-vectors.ts` runs through the two pure
 * mutation classifiers.
 *
 * Kept apart from the generator, like `scope-cases.ts`, because these are the
 * thing under review: the generator is twenty lines of plumbing and this is
 * the argument about what the classifiers should say.
 */

/** One `appearanceImpactOf` case. */
export interface ImpactCase {
  readonly name: string;
  /** Why the case is here, in one line. */
  readonly covers: string;
  /** A DTO-shaped object. Only the keys the classifier reads matter. */
  readonly current: Record<string, unknown>;
  /** A patch-shaped object. */
  readonly patch: Record<string, unknown>;
}

const RECT = { left: 10, bottom: 20, right: 110, top: 70 };
const SHIFTED = { left: 40, bottom: 5, right: 140, top: 55 };

/** The same shift the rects above describe. */
const shift = <T extends Record<string, number>>(point: T): T =>
  ({ ...point, x: point.x + 30, y: point.y - 15 }) as T;

const QUADS = [
  { p1: { x: 10, y: 70 }, p2: { x: 110, y: 70 }, p3: { x: 10, y: 20 }, p4: { x: 110, y: 20 } },
];
const SHIFTED_QUADS = QUADS.map((q) => ({
  p1: shift(q.p1),
  p2: shift(q.p2),
  p3: shift(q.p3),
  p4: shift(q.p4),
}));

export const IMPACT_CASES: readonly ImpactCase[] = [
  {
    name: 'empty-patch',
    covers: 'A patch with nothing in it. Never touch /AP.',
    current: { subtype: 'square', rect: RECT },
    patch: { subtype: 'square' },
  },
  {
    name: 'inert-keys-only',
    covers:
      'Flags, reply relationship, group and the conversation plane. None of them is ever painted, so a change to all of them at once is still inert.',
    current: {
      subtype: 'text',
      rect: RECT,
      flags: { print: true },
      state: 'accepted',
      subject: 'old',
    },
    patch: {
      subtype: 'text',
      flags: { print: false, hidden: true },
      state: 'rejected',
      stateModel: 'review',
      subject: 'new',
      groupId: 'team-2',
      replyType: 'group',
    },
  },
  {
    name: 'no-op-full-object-patch',
    covers:
      'A client that sends the whole object back unchanged. Value-diffing is what stops that being a re-bake, and a classifier keyed on which keys are present would regenerate.',
    current: {
      subtype: 'square',
      rect: RECT,
      color: { r: 255, g: 0, b: 0 },
      strokeWidth: 2,
      interiorColor: null,
    },
    patch: {
      subtype: 'square',
      rect: { ...RECT },
      color: { r: 255, g: 0, b: 0 },
      strokeWidth: 2,
      interiorColor: null,
    },
  },
  {
    name: 'no-op-within-epsilon',
    covers:
      'A rect that came back through an f32 round trip. A thousandth of a point is below anything visible and above the drift, so this is still inert.',
    current: { subtype: 'square', rect: RECT },
    patch: { subtype: 'square', rect: { left: 10.0001, bottom: 20, right: 110, top: 70 } },
  },
  {
    name: 'clearing-an-explicitly-null-entry',
    covers:
      'A tri-state clear of something the DTO already reports as null. Nothing changed.',
    current: { subtype: 'square', rect: RECT, interiorColor: null },
    patch: { subtype: 'square', interiorColor: null },
  },
  {
    name: 'clearing-an-entry-the-dto-omits',
    covers:
      'A tri-state clear of a key the DTO does not carry AT ALL. Absent and null both mean the entry is not there, and a comparison that told them apart would re-bake for a write that changes nothing. The case above cannot catch that, because there both sides are null.',
    current: { subtype: 'square', rect: RECT },
    patch: { subtype: 'square', dashArray: null },
  },
  {
    name: 'rect-translation',
    covers: 'The ordinary move: same size, new place, nothing else to carry.',
    current: { subtype: 'caret', rect: RECT },
    patch: { subtype: 'caret', rect: SHIFTED },
  },
  {
    name: 'resize-is-not-a-translation',
    covers:
      'A rect that changed size. The pixels would be scaled rather than shifted, so /AP has to be re-baked.',
    current: { subtype: 'caret', rect: RECT },
    patch: { subtype: 'caret', rect: { left: 10, bottom: 20, right: 200, top: 70 } },
  },
  {
    name: 'markup-quads-ride-along',
    covers: 'A text markup whose quads move with the rect by the same delta.',
    current: { subtype: 'highlight', rect: RECT, quadPoints: QUADS },
    patch: { subtype: 'highlight', rect: SHIFTED, quadPoints: SHIFTED_QUADS },
  },
  {
    name: 'markup-quads-left-behind',
    covers:
      'The rect moved and the quads did not. The dictionary would desync from the pixels, so this is not a translation however much it looks like one.',
    current: { subtype: 'highlight', rect: RECT, quadPoints: QUADS },
    patch: { subtype: 'highlight', rect: SHIFTED, quadPoints: QUADS },
  },
  {
    name: 'markup-rect-only',
    covers:
      'The rect moved and the quads were not mentioned. They are still on the annotation, unshifted, so the geometry is inconsistent and this regenerates.',
    current: { subtype: 'highlight', rect: RECT, quadPoints: QUADS },
    patch: { subtype: 'highlight', rect: SHIFTED },
  },
  {
    name: 'vertices-ride-along',
    covers: 'A polygon whose vertices move with the rect.',
    current: {
      subtype: 'polygon',
      rect: RECT,
      vertices: [
        { x: 10, y: 20 },
        { x: 110, y: 20 },
        { x: 60, y: 70 },
      ],
    },
    patch: {
      subtype: 'polygon',
      rect: SHIFTED,
      vertices: [
        { x: 40, y: 5 },
        { x: 140, y: 5 },
        { x: 90, y: 55 },
      ],
    },
  },
  {
    name: 'ink-strokes-ride-along',
    covers: 'A nested array: the ink list is a list of point lists.',
    current: {
      subtype: 'ink',
      rect: RECT,
      inkList: [
        [
          { x: 10, y: 20 },
          { x: 110, y: 70 },
        ],
      ],
    },
    patch: {
      subtype: 'ink',
      rect: SHIFTED,
      inkList: [
        [
          { x: 40, y: 5 },
          { x: 140, y: 55 },
        ],
      ],
    },
  },
  {
    name: 'ink-advisory-rotation',
    covers:
      'A vertex kind whose rotation scalar changed. It never drives the generator, so it is inert even though nothing else changed.',
    current: { subtype: 'ink', rect: RECT, rotation: 30, inkList: [[{ x: 10, y: 20 }]] },
    patch: { subtype: 'ink', rotation: 45 },
  },
  {
    name: 'box-rotation-changed',
    covers:
      'A box kind whose rotation changed along with a move. The pixels rotate, so this is not a rigid translation.',
    current: { subtype: 'square', rect: RECT, rotation: 0 },
    patch: { subtype: 'square', rect: SHIFTED, rotation: 30 },
  },
  {
    name: 'box-rotation-preserved-by-omission',
    covers:
      'A box kind with a rotation the patch does not mention. Omitted preserves, so the after-state matches and the move is rigid.',
    current: { subtype: 'square', rect: RECT, rotation: 30, unrotatedRect: RECT },
    patch: { subtype: 'square', rect: SHIFTED, unrotatedRect: SHIFTED },
  },
  {
    name: 'box-rotation-respelled',
    covers:
      'A rotation written as 390 against a current 30. Those are the same angle and this still REGENERATES, because `rotation` is not in the subtype\'s translatable-geometry list: it lands in the touched set before the normalization inside the rigid-translation check can be reached. The normalization only ever runs for a rotation the patch omits or repeats verbatim.',
    current: { subtype: 'square', rect: RECT, rotation: 30, unrotatedRect: RECT },
    patch: { subtype: 'square', rect: SHIFTED, rotation: 390, unrotatedRect: SHIFTED },
  },
  {
    name: 'box-rotation-repeated-verbatim',
    covers:
      'The same move with the rotation repeated exactly. Value-diffing drops it before the touched set, so the move is rigid, which is the case that reaches the normalization at all.',
    current: { subtype: 'square', rect: RECT, rotation: 30, unrotatedRect: RECT },
    patch: { subtype: 'square', rect: SHIFTED, rotation: 30, unrotatedRect: SHIFTED },
  },
  {
    name: 'unrotated-rect-left-behind',
    covers:
      'The rect moved and the logical box did not. Preserved-yet-unshifted geometry fails the congruence check.',
    current: { subtype: 'square', rect: RECT, rotation: 30, unrotatedRect: RECT },
    patch: { subtype: 'square', rect: SHIFTED },
  },
  {
    name: 'style-change',
    covers: 'A colour. Not geometry, so it re-bakes however little else moved.',
    current: { subtype: 'square', rect: RECT, color: { r: 255, g: 0, b: 0 } },
    patch: { subtype: 'square', color: { r: 0, g: 0, b: 255 } },
  },
  {
    name: 'contents-on-a-shape',
    covers:
      "A shape's contents is popup note text and is never painted, so editing the comment must not destroy the appearance.",
    current: { subtype: 'square', rect: RECT, contents: 'old note' },
    patch: { subtype: 'square', contents: 'new note' },
  },
  {
    name: 'contents-on-free-text',
    covers: 'The same key on a kind whose /AP paints it. Here it re-bakes.',
    current: { subtype: 'free-text', rect: RECT, contents: 'old' },
    patch: { subtype: 'free-text', contents: 'new' },
  },
  {
    name: 'unknown-key',
    covers: 'A key from a newer client. Conservative by default.',
    current: { subtype: 'square', rect: RECT },
    patch: { subtype: 'square', somethingNew: 42 },
  },
  {
    name: 'unknown-subtype',
    covers: 'A subtype with no geometry table. Also conservative.',
    current: { subtype: 'unsupported', rect: RECT },
    patch: { subtype: 'unsupported', rect: SHIFTED },
  },
  {
    name: 'subtype-mismatch',
    covers:
      'A patch for a different subtype than the annotation. Nothing about the geometry can be reasoned about across that.',
    current: { subtype: 'square', rect: RECT },
    patch: { subtype: 'circle', rect: SHIFTED },
  },
  {
    name: 'link-move',
    covers:
      'A link paints nothing of its own and is still in the geometry table, so a move classifies as a translation rather than asking for an /AP that does not exist.',
    current: { subtype: 'link', rect: RECT },
    patch: { subtype: 'link', rect: SHIFTED },
  },
  {
    name: 'callout-line-rides-along',
    covers: "A free text's leader line moves with its box.",
    current: {
      subtype: 'free-text',
      rect: RECT,
      calloutLine: [
        { x: 0, y: 0 },
        { x: 10, y: 20 },
      ],
    },
    patch: {
      subtype: 'free-text',
      rect: SHIFTED,
      calloutLine: [
        { x: 30, y: -15 },
        { x: 40, y: 5 },
      ],
    },
  },
  {
    name: 'geometry-appearing',
    covers:
      'A move that also adds geometry the annotation did not have. Something appeared, so nothing was merely translated.',
    current: { subtype: 'polygon', rect: RECT },
    patch: { subtype: 'polygon', rect: SHIFTED, vertices: [{ x: 40, y: 5 }] },
  },
  {
    name: 'line-points-ride-along',
    covers: 'A line annotation whose two endpoints move with the rect.',
    current: {
      subtype: 'line',
      rect: RECT,
      linePoints: { start: { x: 10, y: 20 }, end: { x: 110, y: 70 } },
    },
    patch: {
      subtype: 'line',
      rect: SHIFTED,
      linePoints: { start: { x: 40, y: 5 }, end: { x: 140, y: 55 } },
    },
  },
];

/**
 * One `computeMutationImpact` case. The generator no longer runs these:
 * upstream deleted `computeMutationImpact` with weak annotation refs in
 * bc0057f3a, so the question each case asks has no answer in the engine.
 * Kept for the record and for Task 5's decision on the Rust side.
 */
export interface MutationCase {
  readonly name: string;
  readonly covers: string;
  readonly mutation: 'create' | 'update' | 'delete' | 'move';
  /** The weak-annotation state BEFORE the mutation. */
  readonly weakBefore: { kind: 'known'; hasAnyWeakAnnotations: boolean } | { kind: 'unknown' };
  readonly weakAfter: { kind: 'known'; hasAnyWeakAnnotations: boolean } | { kind: 'unknown' };
  readonly generationBefore: number;
  readonly generationAfter: number;
  readonly changed: readonly ({ kind: 'objectNumber'; value: number } | { kind: 'nm'; value: string })[];
}

export const MUTATION_CASES: readonly MutationCase[] = [
  {
    name: 'create-on-a-weak-page',
    covers:
      'A create appends, so no existing index moves even on a page full of weak annotations. No bump, no refetch.',
    mutation: 'create',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: true },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: true },
    generationBefore: 4,
    generationAfter: 4,
    changed: [{ kind: 'objectNumber', value: 12 }],
  },
  {
    name: 'update-stamping-an-nm',
    covers:
      'An update that opportunistically strengthened a weak annotation. The weak state changes and the index space does not, so still no refetch.',
    mutation: 'update',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: true },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: false },
    generationBefore: 4,
    generationAfter: 4,
    changed: [{ kind: 'nm', value: 'stamped-uuid' }],
  },
  {
    name: 'delete-on-a-durable-page',
    covers:
      'A delete shifts indices, and nobody can be holding a stale one because every annotation already had a durable id.',
    mutation: 'delete',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: false },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: false },
    generationBefore: 4,
    generationAfter: 5,
    changed: [{ kind: 'objectNumber', value: 9 }],
  },
  {
    name: 'delete-on-a-weak-page',
    covers: 'The same delete where a weak annotation existed. This is the refetch.',
    mutation: 'delete',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: true },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: true },
    generationBefore: 4,
    generationAfter: 5,
    changed: [],
  },
  {
    name: 'move-on-a-weak-page',
    covers:
      'A move rewrites indices on both ends of the gap. The moved annotations are strengthened on the way out; the others are not.',
    mutation: 'move',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: true },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: true },
    generationBefore: 4,
    generationAfter: 5,
    changed: [
      { kind: 'nm', value: 'a' },
      { kind: 'nm', value: 'b' },
    ],
  },
  {
    name: 'move-on-a-durable-page',
    covers: 'The same move with nothing weak to invalidate.',
    mutation: 'move',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: false },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: false },
    generationBefore: 7,
    generationAfter: 8,
    changed: [{ kind: 'objectNumber', value: 3 }],
  },
  {
    name: 'delete-removing-the-last-weak-annotation',
    covers:
      'The page had a weak annotation and does not any more, because the delete removed it. The refetch is decided on the state BEFORE: a client holding an index is holding it from before, and the page being durable now says nothing about that.',
    mutation: 'delete',
    weakBefore: { kind: 'known', hasAnyWeakAnnotations: true },
    weakAfter: { kind: 'known', hasAnyWeakAnnotations: false },
    generationBefore: 2,
    generationAfter: 3,
    changed: [],
  },
  {
    name: 'create-with-an-unknown-weak-state',
    covers:
      'A non-shifting mutation never consults the weak state, so an unknown one is not an error for it.',
    mutation: 'create',
    weakBefore: { kind: 'unknown' },
    weakAfter: { kind: 'unknown' },
    generationBefore: 0,
    generationAfter: 0,
    changed: [],
  },
];
