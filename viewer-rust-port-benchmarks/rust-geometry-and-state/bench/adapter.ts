import { createCoreStore } from '@poc/core';
import { initCoreFromDisk } from '@poc/core/node';
import type { CoreStore, ViewEnv } from '@poc/core';

interface Layout {
  x: number;
  y: number;
  color: string;
  anchored: boolean;
}

export const name = 'rust-geometry-and-state';

export async function init(): Promise<void> {
  await initCoreFromDisk();
}

/**
 * Note what this does NOT use: `store.seed()`, which calls Rust's `seedShapes`
 * and would build the model through a completely different path from typescript-baseline and
 * rust-geometry-only. The whole point of the shared layout is that all three models are
 * built by dispatching the same messages, so the fairness gate can prove they
 * match.
 */
export function build(layout: Layout[]) {
  const store: CoreStore = createCoreStore();
  layout.forEach((s, i) => {
    store.dispatch({ t: 'add', at: { x: s.x, y: s.y }, color: s.color });
    if (s.anchored) store.dispatch({ t: 'setAnchored', id: `s${i + 1}`, anchored: true });
  });
  store.dispatch({ t: 'select', id: null });

  return {
    frame(view: ViewEnv): number {
      const items = store.scene(view).items;
      const last = items[items.length - 1]!;
      return items.length + items[0]!.quad[0].x + last.quad[2].y;
    },

    /**
     * The same frame without the object decode, to attribute rust-geometry-and-state's frame cost
     * between "crossing the boundary" and "building JavaScript objects".
     * Only rust-geometry-and-state has this: there is no undecoded form in the other two.
     */
    frameRaw(view: ViewEnv): number {
      const buf = store.sceneRaw(view);
      return buf[0]! + buf[1]! + buf[buf.length - 1]!;
    },

    displayList(view: ViewEnv) {
      return store.scene(view).items;
    },

    hit(pt: { x: number; y: number }, view: ViewEnv): number {
      return store.hitTest(pt, view) === null ? 0 : 1;
    },

    gesture(down: { x: number; y: number }, view: ViewEnv, moves: { x: number; y: number }[]): number {
      store.dispatch({ t: 'pointerDown', at: down, view });
      for (const at of moves) store.dispatch({ t: 'pointerMove', at });
      store.dispatch({ t: 'pointerUp' });
      return store.getVersion();
    },

    dispose() {
      store.destroy();
    },
  };
}
