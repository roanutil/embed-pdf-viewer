import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';

let api: any;
export async function initGeometry() {
  if (api) return;
  const wasi = new WASI({ version: 'preview1' });
  const bytes = await readFile(new URL('./wasm/swift_geometry.wasm', import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, wasi.getImportObject());
  api = instance.exports;
}

const withBuffer = (count: number, body: (ptr: number) => void): number[] => {
  const ptr = api.swift_alloc_doubles(count);
  try {
    body(ptr);
    return Array.from(new Float64Array(api.memory.buffer, ptr, count));
  } finally { api.swift_free_doubles(ptr, count); }
};
const flat = (quad: any[]) => quad.flatMap((point) => [point.x, point.y]);
const points = (values: number[]) => [0, 1, 2, 3].map((i) => ({ x: values[i * 2], y: values[i * 2 + 1] }));
const withQuad = (quad: any[], outputCount: number, body: (input: number, output: number) => void) => {
  const input = api.swift_alloc_doubles(8);
  try {
    new Float64Array(api.memory.buffer, input, 8).set(flat(quad));
    return withBuffer(outputCount, (output) => body(input, output));
  } finally { api.swift_free_doubles(input, 8); }
};

export const normalizeDeg = (degrees: number) => api.swift_normalize_deg(degrees);
export const rectQuad = (rect: any, rotation = 0) => points(withBuffer(8, (out) => api.swift_rect_quad(rect.x, rect.y, rect.width, rect.height, rotation, out)));
export const quadScaleAbout = (quad: any[], about: any, scale: number) => points(withQuad(quad, 8, (input, out) => api.swift_quad_scale_about(input, about.x, about.y, scale, out)));
export const quadRotateAbout = (quad: any[], about: any, degrees: number) => points(withQuad(quad, 8, (input, out) => api.swift_quad_rotate_about(input, about.x, about.y, degrees, out)));
export const quadBounds = (quad: any[]) => {
  const v = withQuad(quad, 4, (input, out) => api.swift_quad_bounds(input, out));
  return { x: v[0], y: v[1], width: v[2], height: v[3] };
};
export const pointInQuad = (point: any, quad: any[]) => {
  const input = api.swift_alloc_doubles(8);
  try {
    new Float64Array(api.memory.buffer, input, 8).set(flat(quad));
    return Boolean(api.swift_point_in_quad(point.x, point.y, input));
  } finally { api.swift_free_doubles(input, 8); }
};
