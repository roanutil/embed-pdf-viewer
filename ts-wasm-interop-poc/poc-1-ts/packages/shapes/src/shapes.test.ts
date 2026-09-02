import { describe, expect, it } from 'vitest';
import { anchorFactors, hitTest, projectQuad, scene, shapeQuad, update } from './index';
import { emptyModel } from './types';
import type { Model, ViewEnv } from './types';
import { rectQuad } from '@poc/geometry';

const V = (zoom: number, rotation: 0 | 90 | 180 | 270 = 0): ViewEnv => ({ zoom, rotation });

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
  const q = rectQuad({ x: 100, y: 60, width: 80, height: 40 });

  it('returns the SAME OBJECT when the projection is the identity', () => {
    // This identity short-circuit is what a naive WASM port destroys.
    expect(projectQuad(q, V(1, 0), true)).toBe(q);
    expect(projectQuad(q, V(4, 90), false)).toBe(q);
  });

  it('holds the bounds top-left fixed while shrinking by 1/zoom', () => {
    const out = projectQuad(q, V(2), true);
    expect(out[0]).toEqual({ x: 100, y: 60 });
    expect(out[1]!.x).toBeCloseTo(140, 9);
    expect(out[2]!.y).toBeCloseTo(80, 9);
  });

  it('counter-rotates about the anchor so the shape reads upright', () => {
    const out = projectQuad(q, V(1, 90), true);
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
});
