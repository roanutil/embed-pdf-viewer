import { normalizeDeg, quadRotateAbout, quadScaleAbout, quadBounds, rectQuad } from '@poc/geometry';
import type { Quad, Point } from '@poc/geometry';
import type { Shape, ViewEnv } from './types';

/**
 * The whole reason this POC exists.
 *
 * An anchored shape is exempt from the view's zoom factor: after the view
 * applies its own `zoom x rotation` transform, the shape should read at its
 * 100%-zoom size and its authored tilt, hanging from a fixed page point.
 * So the projection is a similarity about the anchor: scale by 1/s, rotate
 * by -r.
 *
 * `s` clamps to max(zoom, 1). Below 100% an anchored shape scales WITH the
 * page, because a screen-constant body at 25% zoom would dwarf the thing it
 * annotates.
 *
 * Returns null when the projection is the identity, and callers then skip the
 * work entirely. That null is load-bearing: it is the identity short-circuit
 * that a naive WASM port destroys, because deserialization always allocates.
 */
export function anchorFactors(
  anchored: boolean,
  view: ViewEnv,
): { s: number; r: number } | null {
  if (!anchored) return null;
  const s = Math.max(view.zoom || 1, 1);
  const r = normalizeDeg(view.rotation);
  return s !== 1 || r !== 0 ? { s, r } : null;
}

/** The fixed page point: the shape's unprojected bounds top-left. */
export const anchorOf = (q: Quad): Point => {
  const b = quadBounds(q);
  return { x: b.x, y: b.y };
};

/**
 * THE effective content-space quad of a shape at `view`. Returns the input
 * quad unchanged when there is nothing to do, so callers apply it
 * unconditionally.
 */
export function projectQuad(q: Quad, view: ViewEnv, anchored: boolean): Quad {
  const f = anchorFactors(anchored, view);
  if (!f) return q;
  const a = anchorOf(q);
  let out = q;
  if (f.s !== 1) out = quadScaleAbout(out, a, 1 / f.s);
  if (f.r !== 0) out = quadRotateAbout(out, a, normalizeDeg(-f.r));
  return out;
}

/** The quad a shape presents at `view`, projection included. */
export const shapeQuad = (shape: Shape, view: ViewEnv): Quad =>
  projectQuad(rectQuad(shape.rect, shape.rot), view, shape.anchored);

/** Whether `view` actually projects `shape`, for the demo's instrumentation. */
export const isProjected = (shape: Shape, view: ViewEnv): boolean =>
  anchorFactors(shape.anchored, view) !== null;
