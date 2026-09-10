import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { pointInQuad, rectQuad } from '@poc/geometry';
import { anchorFactors, projectQuad } from './anchor';
import type { ViewEnv } from './types';

/**
 * The frozen contract. The Rust ports in rust-geometry-only and rust-geometry-and-state read this exact file,
 * so a disagreement between implementations is a test failure and not a
 * discovery made later in a browser.
 */
const file = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../vectors/anchor-vectors.json');

interface ProjectionVector {
  name: string;
  rect: { x: number; y: number; width: number; height: number };
  rot: number;
  view: ViewEnv;
  anchored: boolean;
  factors: { s: number; r: number } | null;
  quad: [number, number][];
}

interface HitVector {
  name: string;
  rect: { x: number; y: number; width: number; height: number };
  rot: number;
  point: { x: number; y: number };
  inside: boolean;
}

const vectors = JSON.parse(readFileSync(file, 'utf8')) as {
  projection: ProjectionVector[];
  hit: HitVector[];
};

describe('frozen projection vectors', () => {
  it('has vectors to check', () => {
    expect(vectors.projection.length).toBeGreaterThan(20);
  });

  for (const v of vectors.projection) {
    it(v.name, () => {
      expect(anchorFactors(v.anchored, v.view)).toEqual(v.factors);
      const out = projectQuad(rectQuad(v.rect, v.rot), v.view, v.anchored);
      out.forEach((p, i) => {
        expect(p.x).toBeCloseTo(v.quad[i]![0], 6);
        expect(p.y).toBeCloseTo(v.quad[i]![1], 6);
      });
    });
  }
});

describe('frozen hit vectors', () => {
  for (const v of vectors.hit) {
    it(v.name, () => {
      expect(pointInQuad(v.point, rectQuad(v.rect, v.rot))).toBe(v.inside);
    });
  }
});
