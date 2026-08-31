import type { Quad, Rect, Rotation, Vec } from '@poc/geometry';

export type { Quad, Rect, Rotation, Vec };

export type Id = string;

/**
 * One shape in CONTENT space. `rect` is the unrotated box, `rot` the authored
 * tilt in degrees, and `anchored` is this POC's stand-in for the PDF `noZoom`
 * flag: an exemption from the view's zoom factor, holding the shape's bounds
 * top-left fixed on the page.
 */
export interface Shape {
  id: Id;
  rect: Rect;
  rot: number;
  anchored: boolean;
  color: string;
}

/** Per-call view environment. Never stored on the model. */
export interface ViewEnv {
  zoom: number;
  rotation: Rotation;
}

export interface Drag {
  id: Id;
  /** Pointer position at DOWN, in content space. */
  from: Vec;
  /** The shape's rect at DOWN, so a drag is always absolute, never cumulative. */
  origin: Rect;
  /** The view captured at DOWN, so an anchored drag commits under the view it started in. */
  view: ViewEnv;
}

export interface Model {
  shapes: readonly Shape[];
  selected: Id | null;
  drag: Drag | null;
  seq: number;
}

export type Msg =
  | { t: 'add'; at: Vec; color: string }
  | { t: 'select'; id: Id | null }
  | { t: 'pointerDown'; at: Vec; view: ViewEnv }
  | { t: 'pointerMove'; at: Vec }
  | { t: 'pointerUp' }
  | { t: 'setAnchored'; id: Id; anchored: boolean }
  | { t: 'setRot'; id: Id; rot: number }
  | { t: 'delete'; id: Id };

/** What the brain asks the shell to do. The brain never does it itself. */
export type Effect =
  | { t: 'log'; message: string }
  | { t: 'persist'; ids: readonly Id[] };

export interface DisplayItem {
  id: Id;
  quad: Quad;
  color: string;
  selected: boolean;
  /** True when the anchor projection was not the identity for this item. */
  projected: boolean;
}

export interface DisplayList {
  items: readonly DisplayItem[];
}

export const emptyModel = (): Model => ({ shapes: [], selected: null, drag: null, seq: 0 });
