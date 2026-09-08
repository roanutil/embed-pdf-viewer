import { emptyModel, hitTest, scene, update } from '@poc/shapes';
import type { Model, Msg, ViewEnv } from '@poc/shapes';

interface Layout {
  x: number;
  y: number;
  color: string;
  anchored: boolean;
}

export const name = 'typescript-baseline';

export async function init(): Promise<void> {
  // Nothing to initialize. That is the baseline's whole advantage.
}

export function build(layout: Layout[]) {
  let model: Model = emptyModel();
  for (const s of layout) {
    model = update(model, { t: 'add', at: { x: s.x, y: s.y }, color: s.color })[0];
    if (s.anchored) {
      model = update(model, { t: 'setAnchored', id: `s${model.seq}`, anchored: true })[0];
    }
  }
  // `add` selects what it adds; clear it so all three POCs agree on the flags.
  model = update(model, { t: 'select', id: null })[0];

  const dispatch = (msg: Msg) => {
    model = update(model, msg)[0];
  };

  return {
    /** O(1) touch of both ends of the display list, so V8 cannot delete it. */
    frame(view: ViewEnv): number {
      const dl = scene(model, view);
      const items = dl.items;
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
