import { beforeAll, describe, expect, it } from 'vitest';
import { crossings, resetCrossings } from '@poc/geometry';
import { initGeometryFromDisk } from '@poc/geometry/node';
import { anchorFactors, hitTest, projectQuad, scene, shapeQuad, update } from './index';
import { emptyModel } from './types';
import type { Model, ViewEnv } from './types';
import { rectQuad } from '@poc/geometry';

// geometry is Rust now, so the brain that calls it cannot run until wasm is up.
beforeAll(async () => {
  await initGeometryFromDisk();
});

const V = (zoom: number, rotation: 0 | 90 | 180 | 270 = 0): ViewEnv => ({ zoom, rotation });

const tenShapes = (anchored: boolean): Model => {
  let m = emptyModel();
  for (let i = 0; i < 10; i++) {
    m = update(m, { t: 'add', at: { x: 60 + i * 30, y: 80 }, color: '#0c0' })[0];
    if (anchored) m = update(m, { t: 'setAnchored', id: `s${i + 1}`, anchored: true })[0];
  }
  return m;
};

const withOneShape = (anchored = false): Model => {
  const [m] = update(emptyModel(), { t: 'add', at: { x: 100, y: 100 }, color: '#c00' });
  return anchored ? update(m, { t: 'setAnchored', id: 's1', anchored: true })[0] : m;
};

describe('anchorFactors', () => {
  it('is null for an unanchored shape at any view', () => {
    expect(anchorFactors(false, V(4, 90))).toBeNull();
  });

  it('is null for an anchored shape at the identity view', () => {
    expect(anchorFactors(true, V(1, 0))).toBeNull();
  });

  it('clamps below 100 percent, so a shape scales with the page there', () => {
    expect(anchorFactors(true, V(0.25))).toBeNull();
  });

  it('reports zoom and rotation above 100 percent', () => {
    expect(anchorFactors(true, V(3, 90))).toEqual({ s: 3, r: 90 });
  });
});

describe('projectQuad', () => {
  // Lazy, not a `const` in the describe body. A describe body runs during
  // COLLECTION, before beforeAll, so with a wasm-backed geometry a top-level
  // rectQuad() call throws before a single test starts. Another tax the async
  // boundary charges, and one that shows up as a confusing collection error
  // rather than as a failing assertion.
  const q = () => rectQuad({ x: 100, y: 60, width: 80, height: 40 });

  it('still returns the SAME OBJECT when the projection is the identity', () => {
    // Good news, and the most useful thing this POC found. The short-circuit
    // survives the port because the DECISION (anchorFactors) stayed in
    // TypeScript. Only the transforms crossed the boundary, and on the identity
    // path no transform runs at all.
    const base = q();
    expect(projectQuad(base, V(1, 0), true)).toBe(base);
    expect(projectQuad(base, V(4, 90), false)).toBe(base);
  });

  it('but an anchored identity is no longer free: deciding it costs a crossing', () => {
    // The quad is built BEFORE the counter is reset. `rectQuad` is itself a
    // crossing, so counting it here would let this test pass on its own setup
    // no matter what the decision path cost.
    const base = q();

    // Unanchored decides in TypeScript and never reaches wasm: `anchorFactors`
    // returns null on its first line, before it calls `normalizeDeg`.
    resetCrossings();
    projectQuad(base, V(4, 90), false);
    expect(crossings()).toBe(0);

    // Anchored is the case that costs. Even at the identity view, where there
    // is no transform to run, `anchorFactors` has to call `normalizeDeg` to
    // find that out, and `normalizeDeg` now lives in wasm.
    resetCrossings();
    projectQuad(base, V(1, 0), true);
    expect(crossings()).toBe(1);
  });

  it('holds the bounds top-left fixed while shrinking by 1/zoom', () => {
    const out = projectQuad(q(), V(2), true);
    expect(out[0]).toEqual({ x: 100, y: 60 });
    expect(out[1]!.x).toBeCloseTo(140, 9);
    expect(out[2]!.y).toBeCloseTo(80, 9);
  });

  it('counter-rotates about the anchor so the shape reads upright', () => {
    const out = projectQuad(q(), V(1, 90), true);
    expect(out[0]).toEqual({ x: 100, y: 60 });
    // -90 in y-down space sends +x to -y.
    expect(out[1]!.x).toBeCloseTo(100, 9);
    expect(out[1]!.y).toBeCloseTo(-20, 9);
  });
});

describe('update', () => {
  it('adds a shape, selects it, and asks the shell to persist', () => {
    const [model, effects] = update(emptyModel(), {
      t: 'add',
      at: { x: 10, y: 10 },
      color: '#00f',
    });
    expect(model.shapes).toHaveLength(1);
    expect(model.selected).toBe('s1');
    expect(effects).toEqual([
      { t: 'log', message: 'added s1' },
      { t: 'persist', ids: ['s1'] },
    ]);
  });

  it('is pure: the input model is untouched', () => {
    const before = emptyModel();
    const snapshot = JSON.stringify(before);
    update(before, { t: 'add', at: { x: 1, y: 1 }, color: '#000' });
    expect(JSON.stringify(before)).toBe(snapshot);
  });

  it('drags absolutely from the DOWN origin, not cumulatively', () => {
    let m = withOneShape();
    m = update(m, { t: 'pointerDown', at: { x: 100, y: 100 }, view: V(1) })[0];
    m = update(m, { t: 'pointerMove', at: { x: 150, y: 100 } })[0];
    m = update(m, { t: 'pointerMove', at: { x: 120, y: 100 } })[0];
    expect(m.shapes[0]!.rect.x).toBeCloseTo(75, 9);
  });

  it('drags an anchored shape 1:1 with the pointer, cursor still on it', () => {
    let m = withOneShape(true);
    const view = V(2);
    // Grab an interior point of the PROJECTED quad, not a corner.
    m = update(m, { t: 'pointerDown', at: { x: 70, y: 76 }, view })[0];
    expect(m.drag).not.toBeNull();
    m = update(m, { t: 'pointerMove', at: { x: 80, y: 76 } })[0];
    // The projection holds the unprojected bounds top-left fixed, so the
    // projected quad already sits at the stored rect position: 10 pointer
    // units is 10 stored units. Scaling the delta by max(zoom, 1) sent it 20
    // and left the cursor behind the shape.
    expect(m.shapes[0]!.rect.x).toBeCloseTo(55 + 10, 9);
    expect(hitTest(m, { x: 80, y: 76 }, view)).toBe('s1');
  });

  it('drops a drag when its shape is deleted mid-gesture', () => {
    let m = withOneShape();
    m = update(m, { t: 'pointerDown', at: { x: 100, y: 100 }, view: V(1) })[0];
    expect(m.drag).not.toBeNull();
    m = update(m, { t: 'delete', id: 's1' })[0];
    expect(m.drag).toBeNull();
    expect(m.selected).toBeNull();
  });
});

describe('hitTest', () => {
  it('finds the shape under the point and misses outside it', () => {
    const m = withOneShape();
    expect(hitTest(m, { x: 100, y: 100 }, V(1))).toBe('s1');
    expect(hitTest(m, { x: 500, y: 500 }, V(1))).toBeNull();
  });

  it('follows the projection, so what you click is what you see', () => {
    const m = withOneShape(true);
    const projectedCentre = (() => {
      const q = shapeQuad(m.shapes[0]!, V(4));
      return { x: (q[0]!.x + q[2]!.x) / 2, y: (q[0]!.y + q[2]!.y) / 2 };
    })();
    expect(hitTest(m, projectedCentre, V(4))).toBe('s1');
    // The unprojected centre is now outside the shrunken shape.
    expect(hitTest(m, { x: 100, y: 100 }, V(4))).toBeNull();
  });

  it('returns the topmost shape when two overlap', () => {
    let m = withOneShape();
    m = update(m, { t: 'add', at: { x: 100, y: 100 }, color: '#0c0' })[0];
    expect(hitTest(m, { x: 100, y: 100 }, V(1))).toBe('s2');
  });
});

describe('scene', () => {
  it('emits one item per shape and marks which ones the view projects', () => {
    let m = withOneShape(true);
    m = update(m, { t: 'add', at: { x: 300, y: 300 }, color: '#0c0' })[0];

    const plain = scene(m, V(1));
    expect(plain.items.map((i) => i.projected)).toEqual([false, false]);

    const zoomed = scene(m, V(3));
    expect(zoomed.items.map((i) => i.projected)).toEqual([true, false]);
    expect(zoomed.items[0]!.id).toBe('s1');
  });

  it('costs a crossing per shape even when nothing is projected', () => {
    const m = tenShapes(false);
    resetCrossings();
    scene(m, V(3));
    // One rectQuad per shape, and nothing else: anchorFactors short-circuits on
    // `!anchored` before it reaches wasm.
    expect(crossings()).toBe(10);
  });

  it('costs seven crossings per shape once the view actually projects', () => {
    const m = tenShapes(true);
    resetCrossings();
    scene(m, V(3, 90));
    // Per shape: rectQuad, anchorFactors -> normalizeDeg, quadBounds,
    // quadScaleAbout, normalizeDeg(-r), quadRotateAbout, and anchorFactors
    // again for the `projected` flag. Seven boundary hops to place one
    // rectangle, and note that two of the seven are normalizeDeg, which is
    // three arithmetic operations. This is what rust-geometry-and-state replaces with a single
    // crossing for the whole frame.
    expect(crossings()).toBe(70);
  });
});
