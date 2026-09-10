/**
 * The shared workload definitions.
 *
 * Every adapter applies THIS data through its own dispatch path. That is the
 * fairness requirement: rust-geometry-and-state has a `seedShapes` shortcut in Rust and typescript-baseline and
 * rust-geometry-only do not, so letting each build its model however it likes would compare
 * different shape sets and no number downstream would reveal it.
 *
 * The layout matches rust-geometry-and-state's `seed_shapes` grid (10 per row, 52 x 46 spacing,
 * six-colour palette) so the demo and the benchmark agree.
 */

const PALETTE = ['#c0392b', '#2980b9', '#27ae60', '#8e44ad', '#d35400', '#16a085'];

/** @returns {{x:number,y:number,color:string,anchored:boolean}[]} */
export function seedLayout(n, anchored) {
  const out = [];
  for (let i = 0; i < n; i++) {
    out.push({
      x: 60 + (i % 10) * 52,
      y: 60 + Math.floor(i / 10) * 46,
      color: PALETTE[i % PALETTE.length],
      anchored,
    });
  }
  return out;
}

/** The projection is the identity here: `max(zoom,1) === 1` and rotation 0. */
export const VIEW_FLAT = { zoom: 1, rotation: 0 };

/** Both factors active: scale by 1/3 and counter-rotate by -90 about the anchor. */
export const VIEW_PROJECTING = { zoom: 3, rotation: 90 };

export const FRAME_SIZES = [1, 10, 100, 1000, 10000];
export const HIT_SIZES = [10, 100, 1000];
export const GESTURE_SIZES = [10, 100, 1000];

/** Moves per synthetic gesture, so the result is a per-move cost. */
export const GESTURE_MOVES = 100;

/** Frames in the allocation loop. */
export const ALLOC_FRAMES = 512;
export const ALLOC_SIZE = 100;

/**
 * The fairness probe. Small, anchored, and at a projecting view, so it exercises
 * the scale AND rotate paths plus the flags word.
 */
export const PROBE = { n: 10, anchored: true, view: VIEW_PROJECTING };

/** A point over the first shape's projected body, and one over empty space. */
export const HIT_POINT = { x: 62, y: 62 };
export const MISS_POINT = { x: 5000, y: 5000 };
