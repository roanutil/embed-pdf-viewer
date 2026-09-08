/** The value types are unchanged from the TypeScript original. Only the
 *  implementation moved, so consumers keep the same vocabulary. */

export interface Vec {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type Quad = readonly [Vec, Vec, Vec, Vec];

export type Rotation = 0 | 90 | 180 | 270;
