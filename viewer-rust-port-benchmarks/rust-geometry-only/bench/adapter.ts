import { initGeometryFromDisk } from '@poc/geometry/node';
import { emptyModel, hitTest, scene, update } from '@poc/shapes';
import type { Model, Msg, ViewEnv } from '@poc/shapes';

interface Layout {
  x: number;
  y: number;
  color: string;
  anchored: boolean;
}

export const name = 'rust-geometry-only';

/** The one line of difference from typescript-baseline's adapter. It is not a small one. */
export async function init(): Promise<void> {
  await initGeometryFromDisk();
}

export function build(layout: Layout[]) {
  let model: Model = emptyModel();
  for (const s of layout) {
    model = update(model, { t: 'add', at: { x: s.x, y: s.y }, color: s.color })[0];
    if (s.anchored) {
      model = update(model, { t: 'setAnchored', id: `s${model.seq}`, anchored: true })[0];
    }
  }
  model = update(model, { t: 'select', id: null })[0];

  const dispatch = (msg: Msg) => {
    model = update(model, msg)[0];
  };

  return {
    frame(view: ViewEnv): number {
      const items = scene(model, view).items;
      const last = items[items.length - 1]!;
      return items.length + items[0]!.quad[0].x + last.quad[2].y;
    },

    displayList(view: ViewEnv) {
      return scene(model, view).items;
    },

    hit(pt: { x: number; y: number }, view: ViewEnv): number {
      return hitTest(model, pt, view) === null ? 0 : 1;
    },

    gesture(down: { x: number; y: number }, view: ViewEnv, moves: { x: number; y: number }[]): number {
      dispatch({ t: 'pointerDown', at: down, view });
      for (const at of moves) dispatch({ t: 'pointerMove', at });
      dispatch({ t: 'pointerUp' });
      return model.shapes[0]!.rect.x;
    },

    dispose() {},
  };
}
