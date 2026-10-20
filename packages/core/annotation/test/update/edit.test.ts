/**
 * An edit gesture that outlives the record it began on: the engine's answer
 * to a create arriving mid-gesture, and the record leaving mid-gesture.
 */
import { toPageRef } from '@embedpdf/engine-core/runtime';
import { describe, expect, it } from 'vitest';

import { shapeOf } from '../../src/record';
import { computeMoveSnap } from '../../src/snap';
import type { Message, Model, ModelAnnotation, Point } from '../../src/types';
import { update } from '../../src/update';
import { modelWith, recordOf, run, step, STYLE } from '../support';
import { DRAWN_FLAGS } from '../../src/flags';

const PAGE = toPageRef(1);

const draw = (phase: 'down' | 'move' | 'up', point: Point): Message => ({
  type: 'createPointer',
  phase,
  subtype: 'square',
  in: { page: PAGE, point, shift: false },
});

const edit = (phase: 'down' | 'move' | 'up', point: Point): Message => ({
  type: 'editPointer',
  phase,
  in: { page: PAGE, point, shift: false },
});

/** The model with the engine's answer to `id`'s create: the same record, confirmed. */
const confirmed = (model: Model, id: string): Model => {
  const { unconfirmed: _, ...record } = model.byId[id]!;
  return { ...model, byId: { ...model.byId, [id]: record } };
};

const square = (x: number): ModelAnnotation =>
  recordOf({
    id: 'obj:1',
    ref: { kind: 'objectNumber', page: PAGE, objectNumber: 1 },
    page: PAGE,
    subtype: 'square',
    geometry: {
      kind: 'box',
      box: { x, y: 100, width: 100, height: 60 },
      rotation: 0,
      ellipse: false,
    },
    style: STYLE,
    flags: DRAWN_FLAGS,
    source: 'baked',
  });

/** The model without record `id`, as when another session deletes it. */
const without = (model: Model, id: string): Model => {
  const byId = { ...model.byId };
  delete byId[id];
  return { ...model, byId, order: model.order.filter((other) => other !== id) };
};

describe('an edit gesture across the create ack', () => {
  it('create, pointer down, Created, move, up: moves the durable record', () => {
    const drawn = run(modelWith([]), [
      draw('down', { x: 10, y: 20 }),
      draw('move', { x: 110, y: 80 }),
      draw('up', { x: 110, y: 80 }),
    ]);
    const [id] = drawn.order;
    expect(drawn.byId[id!]!.unconfirmed).toBe(true);

    const [grabbed] = step({ ...drawn, selected: [id!] }, edit('down', { x: 50, y: 50 }));
    expect(grabbed.draft).toMatchObject({ kind: 'move', ids: [id] });

    const answered = confirmed(grabbed, id!);
    const moved = run(answered, [edit('move', { x: 80, y: 70 })]);
    // The draft still names the record the engine confirmed.
    expect(moved.draft).toMatchObject({ kind: 'move', ids: [id] });
    expect(moved.byId[id!]).toBeDefined();

    const result = update(moved, edit('up', { x: 80, y: 70 }));
    expect(result.session.draft).toBeNull();
    expect(result.change.put.map((record) => record.id)).toEqual([id]);
    expect(shapeOf(result.change.put[0]!.annotation)).toMatchObject({
      box: { x: 40, y: 40, width: 100, height: 60 },
    });
  });
});

describe('an edit gesture whose record goes', () => {
  it('a move clears the draft instead of throwing', () => {
    const [grabbed] = step(
      modelWith([square(100)], { selected: ['obj:1'] }),
      edit('down', { x: 150, y: 130 }),
    );
    expect(grabbed.draft).toMatchObject({ kind: 'move', ids: ['obj:1'] });
    const gone = without(grabbed, 'obj:1');
    const result = update(
      { ...gone, snap: { ...gone.snap, alignment: true } },
      edit('move', { x: 190, y: 160 }),
    );
    expect(result.session.draft).toBeNull();
    expect(result.effects).toEqual([]);
  });

  it('a release clears the draft instead of throwing', () => {
    const dragged = run(modelWith([square(100)], { selected: ['obj:1'] }), [
      edit('down', { x: 150, y: 130 }),
      edit('move', { x: 190, y: 160 }),
    ]);
    const result = update(without(dragged, 'obj:1'), edit('up', { x: 190, y: 160 }));
    expect(result.session.draft).toBeNull();
    expect(result.change.put).toEqual([]);
    expect(result.effects).toEqual([]);
  });

  it('a resize clears the draft on move and on release', () => {
    // The bottom-right corner handle of the square at (100, 100, 100 x 60).
    const [grabbed] = step(
      modelWith([square(100)], { selected: ['obj:1'] }),
      edit('down', { x: 200, y: 160 }),
    );
    expect(grabbed.draft).toMatchObject({ kind: 'handle', id: 'obj:1' });
    const gone = without(grabbed, 'obj:1');
    expect(update(gone, edit('move', { x: 220, y: 180 })).session.draft).toBeNull();
    const [resized] = step(grabbed, edit('move', { x: 220, y: 180 }));
    expect(
      update(without(resized, 'obj:1'), edit('up', { x: 220, y: 180 })).session.draft,
    ).toBeNull();
  });
});

describe('move snapping', () => {
  it('skips a moving id with no record instead of throwing', () => {
    const model = modelWith([square(100)]);
    const snap = computeMoveSnap(model, ['obj:1', 'obj:404'], PAGE, { x: 3, y: 0 }, 5, undefined);
    expect(snap.delta).toEqual({ x: 3, y: 0 });
  });
});
