/** Hand-written contract for the generated wasm-bindgen glue. */
export default function init(
  opts?: { module_or_path?: string | URL | Response | BufferSource | WebAssembly.Module } | string | URL,
): Promise<unknown>;

export function setHostLogger(f: (message: unknown) => void): void;
export function benchHostCalls(n: number): number;
export function greetHost(): void;

export function normalizeDeg(deg: number): number;
export function rotatePoint(px: number, py: number, ax: number, ay: number, deg: number): Float64Array;
export function rectQuad(x: number, y: number, width: number, height: number, rot: number): Float64Array;
export function quadScaleAbout(q: Float64Array, ax: number, ay: number, s: number): Float64Array;
export function quadRotateAbout(q: Float64Array, ax: number, ay: number, deg: number): Float64Array;
export function quadTranslate(q: Float64Array, dx: number, dy: number): Float64Array;
export function quadBounds(q: Float64Array): Float64Array;
export function pointInQuad(px: number, py: number, q: Float64Array): boolean;
export function rectQuadObjects(
  rect: { x: number; y: number; width: number; height: number },
  rot: number,
): { x: number; y: number }[];
