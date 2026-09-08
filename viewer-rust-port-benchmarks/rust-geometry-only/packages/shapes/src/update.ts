import { pointInQuad } from '@poc/geometry';
import type { Point } from '@poc/geometry';
import type { Effect, Id, Model, Msg, Shape, ViewEnv } from './types';
import { shapeQuad } from './anchor';

const DEFAULT_W = 90;
const DEFAULT_H = 64;

/** Topmost shape under `at`, or null. Reverse order: last drawn wins. */
export function hitTest(model: Model, at: Point, view: ViewEnv): Id | null {
  for (let i = model.shapes.length - 1; i >= 0; i--) {
    const s = model.shapes[i]!;
    if (pointInQuad(at, shapeQuad(s, view))) return s.id;
  }
  return null;
}

/**
 * Returns null when `id` matches nothing, so the caller can tell a no-op from
 * an edit and hand the caller's own model straight back. `shapes.map` always
 * builds a new array, so the store's `if (next !== model)` at
 * packages/store/src/index.ts:65 rebuilt the display list for a message that
 * changed nothing. rust-geometry-and-state's Rust `update` reports `changed: false` for the same
 * case (crates/core/src/model.rs:257), and this benchmark exists to compare
 * those two, not one of them against a version that skipped the check.
 */
const replace = (
  shapes: readonly Shape[],
  id: Id,
  fn: (s: Shape) => Shape,
): readonly Shape[] | null => {
  // Still `shapes.map`, with a flag, and deliberately not findIndex + slice.
  // The per-element closure IS the cost this POC is measuring against Rust
  // mutating one shape in place, so replacing it with a memcpy would be
  // hand-optimising the side under test. All the flag buys is the miss, which
  // pointerMove was paying a separate `some` scan to learn.
  let found = false;
  const next = shapes.map((s) => {
    if (s.id !== id) return s;
    found = true;
    return fn(s);
  });
  return found ? next : null;
};

/**
 * The one entry point. Pure: same model and msg give the same result, and
 * nothing outside is touched. Effects are DESCRIPTIONS; the shell performs
 * them.
 */
export function update(model: Model, msg: Msg): [Model, Effect[]] {
  switch (msg.t) {
    case 'add': {
      const id = `s${model.seq + 1}`;
      const shape: Shape = {
        id,
        rect: {
          x: msg.at.x - DEFAULT_W / 2,
          y: msg.at.y - DEFAULT_H / 2,
          width: DEFAULT_W,
          height: DEFAULT_H,
        },
        rot: 0,
        anchored: false,
        color: msg.color,
      };
      return [
        { ...model, shapes: [...model.shapes, shape], selected: id, seq: model.seq + 1 },
        [{ t: 'log', message: `added ${id}` }, { t: 'persist', ids: [id] }],
      ];
    }

    case 'select':
      if (msg.id === model.selected) return [model, []];
      return [{ ...model, selected: msg.id }, []];

    case 'pointerDown': {
      const hit = hitTest(model, msg.at, msg.view);
      if (!hit) return model.selected === null ? [model, []] : [{ ...model, selected: null }, []];
      const shape = model.shapes.find((s) => s.id === hit)!;
      return [
        {
          ...model,
          selected: hit,
          drag: { id: hit, from: msg.at, origin: shape.rect },
        },
        [],
      ];
    }

    case 'pointerMove': {
      const d = model.drag;
      if (!d) return [model, []];
      // The delta is NOT scaled by zoom, not even for an anchored shape.
      // `projectQuad` is a similarity about the shape's own unprojected bounds
      // top-left, which makes that point a fixed point of the projection: the
      // projected quad sits at the stored rect position at every zoom. Pointer
      // coordinates already live in that space, so a 1:1 delta is what makes
      // the shape follow the cursor. Scaling by max(zoom, 1) moved it that
      // many times too far and the next pointerDown missed it.
      const dx = msg.at.x - d.from.x;
      const dy = msg.at.y - d.from.y;
      // A drag whose shape was deleted mid-gesture is a no-op, not a crash.
      const shapes = replace(model.shapes, d.id, (sh) => ({
        ...sh,
        rect: { ...d.origin, x: d.origin.x + dx, y: d.origin.y + dy },
      }));
      if (!shapes) return [model, []];
      return [{ ...model, shapes }, []];
    }

    case 'pointerUp': {
      const d = model.drag;
      if (!d) return [model, []];
      return [{ ...model, drag: null }, [{ t: 'persist', ids: [d.id] }]];
    }

    case 'setAnchored': {
      const shapes = replace(model.shapes, msg.id, (s) => ({ ...s, anchored: msg.anchored }));
      if (!shapes) return [model, []];
      return [
        { ...model, shapes },
        [{ t: 'log', message: `${msg.id} anchored=${msg.anchored}` }, { t: 'persist', ids: [msg.id] }],
      ];
    }

    case 'setRot': {
      const shapes = replace(model.shapes, msg.id, (s) => ({ ...s, rot: msg.rot }));
      if (!shapes) return [model, []];
      return [{ ...model, shapes }, [{ t: 'persist', ids: [msg.id] }]];
    }

    case 'delete': {
      const shapes = model.shapes.filter((s) => s.id !== msg.id);
      if (shapes.length === model.shapes.length) return [model, []];
      return [
        {
          ...model,
          shapes,
          selected: model.selected === msg.id ? null : model.selected,
          drag: model.drag?.id === msg.id ? null : model.drag,
        },
        [{ t: 'log', message: `deleted ${msg.id}` }],
      ];
    }
  }
}
