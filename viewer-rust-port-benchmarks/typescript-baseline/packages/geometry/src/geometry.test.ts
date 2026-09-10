import { describe, expect, it } from 'vitest';
import {
  normalizeDeg,
  pointInQuad,
  quadBounds,
  quadRotateAbout,
  quadScaleAbout,
  rectQuad,
  rotatePoint,
} from './index';

const ORIGIN = { x: 0, y: 0 };
const near = (a: number, b: number) => expect(a).toBeCloseTo(b, 9);

describe('normalizeDeg', () => {
  it('folds negatives into [0, 360)', () => {
    expect(normalizeDeg(-90)).toBe(270);
    expect(normalizeDeg(450)).toBe(90);
    expect(normalizeDeg(0)).toBe(0);
  });
});

describe('rotatePoint', () => {
  it('is the identity at 0 degrees', () => {
    const p = { x: 3, y: 7 };
    expect(rotatePoint(p, ORIGIN, 0)).toBe(p);
  });

  it('rotates 90 degrees clockwise in y-down space', () => {
    const r = rotatePoint({ x: 1, y: 0 }, ORIGIN, 90);
    near(r.x, 0);
    near(r.y, 1);
  });
});

describe('rectQuad', () => {
  it('emits clockwise corners from the top-left', () => {
    const q = rectQuad({ x: 10, y: 20, width: 4, height: 6 });
    expect(q[0]).toEqual({ x: 10, y: 20 });
    expect(q[1]).toEqual({ x: 14, y: 20 });
    expect(q[2]).toEqual({ x: 14, y: 26 });
    expect(q[3]).toEqual({ x: 10, y: 26 });
  });

  it('rotates about the rect centre, preserving bounds area at 180', () => {
    const r = { x: 0, y: 0, width: 10, height: 4 };
    const b = quadBounds(rectQuad(r, 180));
    near(b.width, 10);
    near(b.height, 4);
  });
});

describe('quad transforms', () => {
  it('scale then inverse-scale round-trips', () => {
    const q = rectQuad({ x: 5, y: 5, width: 10, height: 10 });
    const back = quadScaleAbout(quadScaleAbout(q, ORIGIN, 3), ORIGIN, 1 / 3);
    q.forEach((p, i) => {
      near(back[i]!.x, p.x);
      near(back[i]!.y, p.y);
    });
  });

  it('rotate then counter-rotate round-trips', () => {
    const q = rectQuad({ x: 5, y: 5, width: 10, height: 10 });
    const back = quadRotateAbout(quadRotateAbout(q, ORIGIN, 37), ORIGIN, -37);
    q.forEach((p, i) => {
      near(back[i]!.x, p.x);
      near(back[i]!.y, p.y);
    });
  });
});

describe('pointInQuad', () => {
  const q = rectQuad({ x: 0, y: 0, width: 10, height: 10 });

  it('accepts the centre and rejects the outside', () => {
    expect(pointInQuad({ x: 5, y: 5 }, q)).toBe(true);
    expect(pointInQuad({ x: 11, y: 5 }, q)).toBe(false);
  });

  it('follows the shape through a rotation', () => {
    const spun = rectQuad({ x: 0, y: 0, width: 10, height: 2 }, 90);
    expect(pointInQuad({ x: 5, y: 5 }, spun)).toBe(true);
    expect(pointInQuad({ x: 1, y: 1 }, spun)).toBe(false);
  });
});
