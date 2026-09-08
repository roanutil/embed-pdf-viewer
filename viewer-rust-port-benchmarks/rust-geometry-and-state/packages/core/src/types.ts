/**
 * The TypeScript view of the Rust types.
 *
 * These mirror `crates/core/src/model.rs`. The `Msg` union in particular has to
 * match the serde tagging exactly (`#[serde(tag = "t", rename_all = "camelCase")]`),
 * and if it drifts the failure is a runtime "bad message" string rather than a
 * type error. See `wasm-types.d.ts` for the note on generating this instead.
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

export type Quad = readonly [Point, Point, Point, Point];

export type Rotation = 0 | 90 | 180 | 270;

export interface ViewEnv {
  zoom: number;
  rotation: Rotation;
}

export type Id = string;

export type Msg =
  | { t: 'add'; at: Point; color: string }
  | { t: 'select'; id: Id | null }
  | { t: 'pointerDown'; at: Point; view: ViewEnv }
  | { t: 'pointerMove'; at: Point }
  | { t: 'pointerUp' }
  | { t: 'setAnchored'; id: Id; anchored: boolean }
  | { t: 'setRot'; id: Id; rot: number }
  | { t: 'delete'; id: Id };

export type Effect = { t: 'log'; message: string } | { t: 'persist'; ids: readonly Id[] };

/** The slowly-changing half of a shape: read on a `tableVersion` bump, not per frame. */
export interface ShapeRow {
  handle: number;
  id: Id;
  color: string;
  rot: number;
  anchored: boolean;
}

export interface DisplayItem {
  handle: number;
  id: Id;
  quad: Quad;
  color: string;
  selected: boolean;
  projected: boolean;
}

export interface DisplayList {
  items: readonly DisplayItem[];
}
