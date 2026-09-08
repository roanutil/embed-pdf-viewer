/**
 * Pure 2D primitives. No DOM, no async, no dependencies.
 *
 * Content space is y-down. A `Quad` is four POSITIONAL corners in
 * clockwise order starting top-left, which is what lets a rotated shape
 * survive a similarity transform without gaining a second rotation field.
 */

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Four positional corners, clockwise from the unrotated top-left. */
export type Quad = readonly [Point, Point, Point, Point];

export type Rotation = 0 | 90 | 180 | 270;

export const normalizeDeg = (deg: number): number => ((deg % 360) + 360) % 360;

export function rotatePoint(p: Point, about: Point, deg: number): Point {
  if (deg === 0) return p;
  const rad = (deg * Math.PI) / 180;
  const cos = Math.cos(rad);
  const sin = Math.sin(rad);
  const dx = p.x - about.x;
  const dy = p.y - about.y;
  return {
    x: about.x + dx * cos - dy * sin,
    y: about.y + dx * sin + dy * cos,
  };
}

export function scalePoint(p: Point, about: Point, s: number): Point {
  if (s === 1) return p;
  return { x: about.x + (p.x - about.x) * s, y: about.y + (p.y - about.y) * s };
}

/** The corners of `rect`, rotated by `rot` about the rect's own centre. */
export function rectQuad(rect: Rect, rot = 0): Quad {
  const { x, y, width: w, height: h } = rect;
  const corners: [Point, Point, Point, Point] = [
    { x, y },
    { x: x + w, y },
    { x: x + w, y: y + h },
    { x, y: y + h },
  ];
  if (normalizeDeg(rot) === 0) return corners;
  const c = { x: x + w / 2, y: y + h / 2 };
  return corners.map((p) => rotatePoint(p, c, rot)) as unknown as Quad;
}

export function quadScaleAbout(q: Quad, about: Point, s: number): Quad {
  return q.map((p) => scalePoint(p, about, s)) as unknown as Quad;
}

export function quadRotateAbout(q: Quad, about: Point, deg: number): Quad {
  return q.map((p) => rotatePoint(p, about, deg)) as unknown as Quad;
}

export function quadBounds(q: Quad): Rect {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of q) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

/** Winding test. Works for any convex quad, which is all a similarity can produce. */
export function pointInQuad(p: Point, q: Quad): boolean {
  let sign = 0;
  for (let i = 0; i < 4; i++) {
    const a = q[i]!;
    const b = q[(i + 1) % 4]!;
    const cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    if (cross === 0) continue;
    const s = cross > 0 ? 1 : -1;
    if (sign === 0) sign = s;
    else if (sign !== s) return false;
  }
  return true;
}

export function quadTranslate(q: Quad, by: Point): Quad {
  return q.map((p) => ({ x: p.x + by.x, y: p.y + by.y })) as unknown as Quad;
}
