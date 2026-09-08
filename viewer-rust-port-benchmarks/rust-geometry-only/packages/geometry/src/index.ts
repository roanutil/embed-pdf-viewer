/**
 * `@poc/geometry`, now backed by Rust.
 *
 * The exported signatures match the TypeScript original one for one, on
 * purpose: nothing above this package changed its call sites. What did change
 * is invisible in the types and unmissable at runtime.
 *
 * 1. Nothing works until `initGeometry()` resolves.
 * 2. Every call allocates. The original returned the caller's own array when a
 *    transform was the identity; this one cannot, because deserializing a
 *    Float64Array into `{x, y}` objects always builds new objects.
 * 3. Every call is counted, so the demo can show what a frame really costs.
 */
import { countCrossing } from './crossings';
import { required } from './init';
import type { Quad, Rect, Point } from './types';

export type { Quad, Rect, Rotation, Point } from './types';
export { initGeometry, isReady, type GeometryHost, type GeometryWasm } from './init';
// initGeometryFromDisk is NOT re-exported here: it is `@poc/geometry/node`.
export {
  crossings,
  countCrossing,
  resetCrossings,
  subscribeCrossings,
} from './crossings';

// --- marshalling -----------------------------------------------------------

/** 4 points -> 8 floats. Allocates. */
const packQuad = (q: Quad): Float64Array =>
  new Float64Array([q[0].x, q[0].y, q[1].x, q[1].y, q[2].x, q[2].y, q[3].x, q[3].y]);

/** 8 floats -> 4 points. Allocates 4 objects plus the tuple. */
const unpackQuad = (f: Float64Array): Quad => [
  { x: f[0]!, y: f[1]! },
  { x: f[2]!, y: f[3]! },
  { x: f[4]!, y: f[5]! },
  { x: f[6]!, y: f[7]! },
];

// --- the API ---------------------------------------------------------------

export function normalizeDeg(deg: number): number {
  countCrossing();
  return required().normalizeDeg(deg);
}

export function rotatePoint(p: Point, about: Point, deg: number): Point {
  countCrossing();
  const out = required().rotatePoint(p.x, p.y, about.x, about.y, deg);
  return { x: out[0]!, y: out[1]! };
}

export function rectQuad(rect: Rect, rot = 0): Quad {
  countCrossing();
  return unpackQuad(required().rectQuad(rect.x, rect.y, rect.width, rect.height, rot));
}

export function quadScaleAbout(q: Quad, about: Point, s: number): Quad {
  countCrossing();
  return unpackQuad(required().quadScaleAbout(packQuad(q), about.x, about.y, s));
}

export function quadRotateAbout(q: Quad, about: Point, deg: number): Quad {
  countCrossing();
  return unpackQuad(required().quadRotateAbout(packQuad(q), about.x, about.y, deg));
}

export function quadTranslate(q: Quad, by: Point): Quad {
  countCrossing();
  return unpackQuad(required().quadTranslate(packQuad(q), by.x, by.y));
}

export function quadBounds(q: Quad): Rect {
  countCrossing();
  const b = required().quadBounds(packQuad(q));
  return { x: b[0]!, y: b[1]!, width: b[2]!, height: b[3]! };
}

export function pointInQuad(p: Point, q: Quad): boolean {
  countCrossing();
  return required().pointInQuad(p.x, p.y, packQuad(q));
}

// --- the alternative, kept so it can be measured --------------------------

/**
 * `rectQuad` through serde-wasm-bindgen instead of a flat Float64Array. Reads
 * better on the Rust side and costs more per call. Not used by anything; the
 * demo benchmarks it against the flat version.
 */
export function rectQuadObjects(rect: Rect, rot = 0): Quad {
  countCrossing();
  const pts = required().rectQuadObjects(rect, rot);
  return [pts[0]!, pts[1]!, pts[2]!, pts[3]!] as Quad;
}

/**
 * Host registration on its own, without `initGeometry`'s greeting.
 *
 * `initGeometry({ host })` is the normal way in and stays the normal way in.
 * This exists because the demo's bench has to measure the Rust -> TypeScript
 * crossing with a TRIVIAL logger attached: the app's real logger calls React
 * setState, and timing 20,000 of those measured the React scheduler rather than
 * the boundary. Swap, measure, swap back.
 */
export function setHostLogger(log: (message: string) => void): void {
  required().setHostLogger((m: unknown) => log(String(m)));
}

/** Measures the Rust -> TypeScript direction: n calls out of wasm into a JS fn. */
export function benchHostCalls(n: number): number {
  countCrossing();
  return required().benchHostCalls(n);
}
