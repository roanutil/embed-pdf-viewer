import { beforeAll, describe, expect, it } from 'vitest';
import { initGeometryFromDisk } from './init-node';
import {
  crossings,
  normalizeDeg,
  pointInQuad,
  quadBounds,
  quadRotateAbout,
  quadScaleAbout,
  rectQuad,
  rectQuadObjects,
  resetCrossings,
  rotatePoint,
} from './index';

const ORIGIN = { x: 0, y: 0 };
const near = (a: number, b: number) => expect(a).toBeCloseTo(b, 9);

const hostLines: string[] = [];

// The init the boundary forces. Nothing in this file works without it.
beforeAll(async () => {
  await initGeometryFromDisk({ log: (m) => hostLines.push(m) });
});

describe('the boundary itself', () => {
  it('let Rust call back into TypeScript during init', () => {
    // greetHost() runs inside wasm and reaches the logger we handed it.
    expect(hostLines.some((l) => l.startsWith('poc-geometry '))).toBe(true);
  });

  it('counts one crossing per call', () => {
    resetCrossings();
    normalizeDeg(-90);
    normalizeDeg(450);
    expect(crossings()).toBe(2);
  });

  it('CANNOT return the caller object on an identity transform', () => {
    // The whole point. In typescript-baseline this assertion was toBe(q).
    const q = rectQuad({ x: 0, y: 0, width: 10, height: 10 });
    const same = quadScaleAbout(q, ORIGIN, 1);
    expect(same).not.toBe(q);
    expect(same).toEqual(q);
  });
});

describe('same answers as the TypeScript original', () => {
  it('normalizeDeg folds negatives into [0, 360)', () => {
    expect(normalizeDeg(-90)).toBe(270);
    expect(normalizeDeg(450)).toBe(90);
    expect(normalizeDeg(0)).toBe(0);
  });

  it('rotatePoint turns 90 degrees clockwise in y-down space', () => {
    const r = rotatePoint({ x: 1, y: 0 }, ORIGIN, 90);
    near(r.x, 0);
    near(r.y, 1);
  });

  it('rectQuad emits clockwise corners from the top-left', () => {
    const q = rectQuad({ x: 10, y: 20, width: 4, height: 6 });
    expect(q[0]).toEqual({ x: 10, y: 20 });
    expect(q[1]).toEqual({ x: 14, y: 20 });
    expect(q[2]).toEqual({ x: 14, y: 26 });
    expect(q[3]).toEqual({ x: 10, y: 26 });
  });

  it('rectQuad rotates about the rect centre', () => {
    const b = quadBounds(rectQuad({ x: 0, y: 0, width: 10, height: 4 }, 180));
    near(b.width, 10);
    near(b.height, 4);
  });

  it('round-trips a scale and its inverse', () => {
    const q = rectQuad({ x: 5, y: 5, width: 10, height: 10 });
    const back = quadScaleAbout(quadScaleAbout(q, ORIGIN, 3), ORIGIN, 1 / 3);
    q.forEach((p, i) => {
      near(back[i]!.x, p.x);
      near(back[i]!.y, p.y);
    });
  });

  it('round-trips a rotation and its inverse', () => {
    const q = rectQuad({ x: 5, y: 5, width: 10, height: 10 });
    const back = quadRotateAbout(quadRotateAbout(q, ORIGIN, 37), ORIGIN, -37);
    q.forEach((p, i) => {
      near(back[i]!.x, p.x);
      near(back[i]!.y, p.y);
    });
  });

  it('hit-tests the centre and rejects the outside', () => {
    const q = rectQuad({ x: 0, y: 0, width: 10, height: 10 });
    expect(pointInQuad({ x: 5, y: 5 }, q)).toBe(true);
    expect(pointInQuad({ x: 11, y: 5 }, q)).toBe(false);
  });

  it('hit-tests through a rotation', () => {
    const spun = rectQuad({ x: 0, y: 0, width: 10, height: 2 }, 90);
    expect(pointInQuad({ x: 5, y: 5 }, spun)).toBe(true);
    expect(pointInQuad({ x: 1, y: 1 }, spun)).toBe(false);
  });
});

describe('the object surface', () => {
  it('agrees with the flat surface', () => {
    const rect = { x: 3, y: 4, width: 12, height: 8 };
    expect(rectQuadObjects(rect, 25)).toEqual(rectQuad(rect, 25));
  });
});
