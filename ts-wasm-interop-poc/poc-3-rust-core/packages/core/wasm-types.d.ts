/**
 * Hand-written contract for the generated wasm-bindgen glue.
 *
 * Every type here duplicates a Rust definition, and nothing checks the pair.
 * A real port generates this file with `ts-rs` or `tsify` and makes the Rust
 * side canonical; writing it twice is exactly the drift the port plan warns
 * about, and it is left visible here on purpose.
 */
export default function init(
  opts?: { module_or_path?: string | URL | Response | BufferSource | WebAssembly.Module } | string | URL,
): Promise<unknown>;

export function itemStride(): number;
export function flagSelected(): number;
export function flagProjected(): number;

export class Core {
  constructor();
  free(): void;
  readonly version: number;
  readonly tableVersion: number;
  readonly shapeCount: number;
  readonly seq: number;
  readonly selectedHandle: number | undefined;
  readonly isDragging: boolean;
  setHostLogger(f: (message: unknown) => void): void;
  clearHostLogger(): void;
  dispatch(msg: unknown): { changed: boolean; effects: unknown[] };
  scene(zoom: number, rotation: number): Float64Array;
  shapeTable(): unknown;
  hitTest(x: number, y: number, zoom: number, rotation: number): number;
  seedShapes(count: number, anchored: boolean): void;
}
