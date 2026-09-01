/**
 * The only file that knows how an arm is loaded.
 *
 * Every arm is cwrapped here, by hand and identically, rather than going
 * through `createWasmFunctions` in
 * packages/engine/runtime/src/wasm/wasm-runtime.ts. That bridge costs the
 * same for every arm, so including it would add noise without separating
 * anything. The consequence: these numbers exclude the engine's per-call
 * argument marshalling, and are therefore a LOWER bound on real per-call cost.
 */
import { resolve } from 'node:path';

const here = import.meta.dirname;

/** [name, returnType, argTypes] for every PDFium function the workloads call. */
const PDFIUM = [
  ['FPDF_InitLibrary', null, []],
  ['FPDF_DestroyLibrary', null, []],
  ['FPDF_LoadMemDocument', 'number', ['number', 'number', 'number']],
  ['FPDF_CloseDocument', null, ['number']],
  ['FPDF_GetPageCount', 'number', ['number']],
  ['FPDF_LoadPage', 'number', ['number', 'number']],
  ['FPDF_ClosePage', null, ['number']],
  ['FPDF_GetPageWidthF', 'number', ['number']],
  ['FPDF_GetPageHeightF', 'number', ['number']],
  ['EPDF_GetPageSizeByIndexNormalized', 'number', ['number', 'number', 'number']],
  ['FPDFText_LoadPage', 'number', ['number']],
  ['FPDFText_ClosePage', null, ['number']],
  ['FPDFText_CountChars', 'number', ['number']],
  ['FPDFText_GetTextObject', 'number', ['number', 'number']],
  ['FPDFText_GetFontSize', 'number', ['number', 'number']],
  ['EPDFText_GetCharGeometry', 'number', ['number', 'number', 'number']],
  ['EPDFText_GetTextFull', 'number', ['number', 'number', 'number']],
  ['FPDFText_FindStart', 'number', ['number', 'number', 'number', 'number']],
  ['FPDFText_FindNext', 'number', ['number']],
  ['FPDFText_GetSchResultIndex', 'number', ['number']],
  ['FPDFText_GetSchCount', 'number', ['number']],
  ['FPDFText_FindClose', null, ['number']],
  ['FPDFText_CountRects', 'number', ['number', 'number', 'number']],
  ['FPDFText_GetRect', 'number', ['number', 'number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_CreateEx', 'number', ['number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_FillRect', 'number', ['number', 'number', 'number', 'number', 'number', 'number']],
  ['FPDFBitmap_Destroy', null, ['number']],
  ['FPDF_RenderPageBitmap', null, [
    'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number',
  ]],
];

/** Arm B mirrors PDFIUM one for one under an rs_ prefix. */
const SHIM = [
  ['rs_EPDF_GetPageSizeByIndexNormalized', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_CountChars', 'number', ['number']],
  ['rs_FPDFText_GetTextObject', 'number', ['number', 'number']],
  ['rs_FPDFText_GetFontSize', 'number', ['number', 'number']],
  ['rs_EPDFText_GetCharGeometry', 'number', ['number', 'number', 'number']],
  ['rs_EPDFText_GetTextFull', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_FindStart', 'number', ['number', 'number', 'number', 'number']],
  ['rs_FPDFText_FindNext', 'number', ['number']],
  ['rs_FPDFText_GetSchResultIndex', 'number', ['number']],
  ['rs_FPDFText_GetSchCount', 'number', ['number']],
  ['rs_FPDFText_FindClose', null, ['number']],
  ['rs_FPDFText_CountRects', 'number', ['number', 'number', 'number']],
  ['rs_FPDFText_GetRect', 'number', [
    'number', 'number', 'number', 'number', 'number', 'number',
  ]],
  ['rs_FPDF_RenderPageBitmap', null, [
    'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number',
  ]],
  ['rs_peek_i32', 'number', ['number']],
  ['rs_probe_page_size', 'number', ['number', 'number', 'number']],
];

const COARSE = (prefix) => [
  [`${prefix}_read_page_geometry`, 'number', ['number', 'number', 'number']],
  [`${prefix}_read_page_text`, 'number', ['number', 'number', 'number']],
  [`${prefix}_render_page`, 'number', ['number', 'number', 'number', 'number', 'number']],
  [`${prefix}_search_page`, 'number', ['number', 'number', 'number', 'number']],
];

const EXTRA = { a: [], b: SHIM, c: COARSE('rs'), d: COARSE('cc') };

export async function loadArm(name) {
  if (!EXTRA[name]) throw new Error(`unknown arm: ${name}`);
  const { default: createArm } = await import(resolve(here, `../build/out/${name}/arm.mjs`));
  const m = await createArm();

  const fn = {};
  for (const [n, ret, args] of [...PDFIUM, ...EXTRA[name]]) fn[n] = m.cwrap(n, ret, args);

  const mem = {
    alloc: (bytes) => m._malloc(bytes),
    free: (ptr) => m._free(ptr),
    peek32: (ptr) => m.getValue(ptr, 'i32'),
    peekF32: (ptr) => m.getValue(ptr, 'float'),
    peekF64: (ptr) => m.getValue(ptr, 'double'),
    readBytes: (ptr, len) => m.HEAPU8.slice(ptr, ptr + len),
    writeBytes: (ptr, data) => m.HEAPU8.set(data, ptr),
    writeU16: (str) => {
      const bytes = (str.length + 1) * 2;
      const ptr = m._malloc(bytes);
      m.stringToUTF16(str, ptr, bytes);
      return ptr;
    },
    readU16(ptr, units) {
      const bytes = m.HEAPU8.slice(ptr, ptr + units * 2);
      const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, units);
      let out = '';
      const CHUNK = 0x2000;
      for (let i = 0; i < u16.length; i += CHUNK) {
        out += String.fromCharCode(...u16.subarray(i, Math.min(i + CHUNK, u16.length)));
      }
      return out;
    },
  };

  fn.FPDF_InitLibrary();
  return { name, m, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
}

export async function wasmBytes(name) {
  const { statSync } = await import('node:fs');
  return statSync(resolve(here, `../build/out/${name}/arm.wasm`)).size;
}
