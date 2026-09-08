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
  ['rs_FPDFBitmap_FillRect', 'number', ['number', 'number', 'number', 'number', 'number', 'number']],
  ['rs_FPDF_RenderPageBitmap', null, [
    'number', 'number', 'number', 'number', 'number', 'number', 'number', 'number',
  ]],
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
    // A view onto the LIVE heap, not a copy -- see the coarse workloads in
    // bench/workloads.mjs, which rebuild the DataView on every call because
    // ALLOW_MEMORY_GROWTH=1 means `m.HEAPU8.buffer` can be replaced whenever
    // the heap grows.
    viewBuffer: (ptr, _len) => ({ buffer: m.HEAPU8.buffer, byteOffset: ptr }),
    writeU16: (str) => {
      const bytes = (str.length + 1) * 2;
      const ptr = m._malloc(bytes);
      // An unchecked 0 here silently writes the string over the heap
      // prologue and every later read of it returns garbage.
      if (!ptr) throw new Error(`writeU16: _malloc(${bytes}) returned 0`);
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

/**
 * Fix 1: confirm `view_buffer` (crates/napi-arms/src/lib.rs) actually hands
 * back a zero-copy view before trusting any coarse-arm number.
 *
 * `env.create_arraybuffer_with_borrowed_data` maps to
 * `napi_create_external_arraybuffer`. If the runtime returns
 * `napi_no_external_buffers_allowed` (Electron, or
 * `NODE_API_DISABLE_EXTERNAL_BUFFERS`), napi-rs silently falls back to
 * `napi_create_arraybuffer` plus a `ptr::copy_nonoverlapping` -- a full
 * memcpy per call, no error, no log -- which would restore the exact copy
 * asymmetry the coarse arms were built to remove, invisibly. Confirmed to
 * NOT happen on Node 24.20.0; this exists for whatever runs this next.
 *
 * A previous version of this file "proved" zero-copy by writing through
 * `mem.writeBytes` and then reading the pointer back -- that only shows the
 * pointer is readable, and would pass even over a copy, since the write
 * happened before the copy was ever made. The only direction that actually
 * distinguishes a view from a copy is to mutate THROUGH THE VIEW and read
 * the change back through the original pointer.
 */
function probeExternalBuffers(mem) {
  const ptr = mem.alloc(8);
  try {
    mem.writeBytes(ptr, new Uint8Array([0xab, 0xab, 0xab, 0xab]));
    const view = mem.viewBuffer(ptr, 4);
    const bytes = new Uint8Array(view.buffer, view.byteOffset, 4);
    bytes[0] = 0xcd; // mutate through the view, not through mem.writeBytes
    const readBack = mem.peek32(ptr) & 0xff;
    if (readBack !== 0xcd) {
      throw new Error(
        'native arm: view_buffer returned a COPY, not a zero-copy view (wrote 0xcd through ' +
          `the view, read back 0x${readBack.toString(16)} through the pointer). ` +
          'napi_create_external_arraybuffer must have fallen back to napi_create_arraybuffer ' +
          '+ memcpy (napi_no_external_buffers_allowed), which napi-rs does with no error and ' +
          'no log. External buffers are unavailable in this runtime, so arms c and d would be ' +
          'measured against an extra memcpy per call that the design was meant to remove -- ' +
          'refusing to run rather than publish unfair numbers.',
      );
    }
  } finally {
    mem.free(ptr);
  }
}

/**
 * Native arms. Arm a is the addon that ships, from
 * packages/engine/runtime/npm/<target>/lib/pdf-runtime.node, which uses BigInt
 * pointers; arms b and c are one napi-rs addon using f64 pointers. The
 * adapters below hide that difference so makeWorkloads sees one shape.
 */
export async function loadNativeArm(name, target = 'darwin-arm64') {
  const { createRequire } = await import('node:module');
  const require = createRequire(import.meta.url);

  if (name === 'a') {
    const addon = require(
      resolve(here, `../../packages/engine/runtime/npm/${target}/lib/pdf-runtime.node`),
    );
    // The shipping addon takes BigInt pointers. Which ARGUMENTS are pointers
    // is per-function, so it is spelled out rather than guessed: an earlier
    // draft keyed the conversion off the function name and silently passed
    // Numbers where the addon expects BigInt.
    const P = (v) => (typeof v === 'bigint' ? v : BigInt(Math.trunc(v)));
    const raw = addon;
    const fn = {
      FPDF_InitLibrary: () => raw.FPDF_InitLibrary(),
      FPDF_DestroyLibrary: () => raw.FPDF_DestroyLibrary(),
      FPDF_LoadMemDocument: (buf, len, pw) => raw.FPDF_LoadMemDocument(P(buf), len, P(pw)),
      FPDF_CloseDocument: (doc) => raw.FPDF_CloseDocument(P(doc)),
      FPDF_GetPageCount: (doc) => raw.FPDF_GetPageCount(P(doc)),
      FPDF_LoadPage: (doc, i) => raw.FPDF_LoadPage(P(doc), i),
      FPDF_ClosePage: (pg) => raw.FPDF_ClosePage(P(pg)),
      FPDF_GetPageWidthF: (pg) => raw.FPDF_GetPageWidthF(P(pg)),
      FPDF_GetPageHeightF: (pg) => raw.FPDF_GetPageHeightF(P(pg)),
      EPDF_GetPageSizeByIndexNormalized: (doc, i, out) =>
        raw.EPDF_GetPageSizeByIndexNormalized(P(doc), i, P(out)),
      FPDFText_LoadPage: (pg) => raw.FPDFText_LoadPage(P(pg)),
      FPDFText_ClosePage: (tp) => raw.FPDFText_ClosePage(P(tp)),
      FPDFText_CountChars: (tp) => raw.FPDFText_CountChars(P(tp)),
      FPDFText_GetTextObject: (tp, i) => raw.FPDFText_GetTextObject(P(tp), i),
      FPDFText_GetFontSize: (tp, i) => raw.FPDFText_GetFontSize(P(tp), i),
      EPDFText_GetCharGeometry: (tp, i, out) =>
        raw.EPDFText_GetCharGeometry(P(tp), i, P(out)),
      EPDFText_GetTextFull: (tp, buf, len) => raw.EPDFText_GetTextFull(P(tp), P(buf), len),
      FPDFText_FindStart: (tp, term, flags, start) =>
        raw.FPDFText_FindStart(P(tp), P(term), flags, start),
      FPDFText_FindNext: (h) => raw.FPDFText_FindNext(P(h)),
      FPDFText_GetSchResultIndex: (h) => raw.FPDFText_GetSchResultIndex(P(h)),
      FPDFText_GetSchCount: (h) => raw.FPDFText_GetSchCount(P(h)),
      FPDFText_FindClose: (h) => raw.FPDFText_FindClose(P(h)),
      FPDFText_CountRects: (tp, s, c) => raw.FPDFText_CountRects(P(tp), s, c),
      FPDFText_GetRect: (tp, i, l, t, r, b) =>
        raw.FPDFText_GetRect(P(tp), i, P(l), P(t), P(r), P(b)),
      FPDFBitmap_CreateEx: (w, h, fmt, buf, stride) =>
        raw.FPDFBitmap_CreateEx(w, h, fmt, P(buf), stride),
      FPDFBitmap_FillRect: (b, l, t, w, h, color) =>
        raw.FPDFBitmap_FillRect(P(b), l, t, w, h, color),
      FPDFBitmap_Destroy: (b) => raw.FPDFBitmap_Destroy(P(b)),
      FPDF_RenderPageBitmap: (b, pg, x, y, w, h, rot, flags) =>
        raw.FPDF_RenderPageBitmap(P(b), P(pg), x, y, w, h, rot, flags),
    };
    const mem = {
      // Every other arm's `mem` hands back plain JS Numbers that
      // bench/workloads.mjs offsets with ordinary `+` (geoPtr + GEO.flags,
      // and friends). The shipping addon's alloc/writeU16String return
      // BigInt, and `BigInt + Number` throws ("Cannot mix BigInt and other
      // types"), so those are narrowed to Number here; a real user-space
      // pointer is 47 bits, well inside the 53 a JS Number can hold exactly.
      // `P()` converts back to BigInt at every other call site above.
      alloc: (b) => Number(raw.alloc(b)),
      free: (p) => raw.free(P(p)),
      peek32: (p) => Number(raw.peek(P(p), 'i32')),
      peekF32: (p) => Number(raw.peek(P(p), 'f32')),
      peekF64: (p) => Number(raw.peek(P(p), 'f64')),
      readBytes: (p, len) => new Uint8Array(raw.readBytes(P(p), len)),
      writeBytes: (p, d) =>
        raw.writeBytes(P(p), d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength)),
      writeU16: (s) => Number(raw.writeU16String(s)),
      readU16: (p, units) => {
        const bytes = new Uint8Array(raw.readBytes(P(p), units * 2));
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
    return { name, m: addon, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
  }

  const addon = require(resolve(here, '../build/out/native/napi-arms.node'));

  // Arm d additionally loads the cmake-js addon and wires its `cc_*` coarse
  // entries in; arm c's `rs_*` coarse entries are already in the napi-arms
  // addon required above. Memory helpers (mem, below) always come from the
  // napi-arms addon, even for arm d -- ops_napi.cc exports only the four
  // coarse `cc_*` functions, nothing else.
  const fnExtra = {};
  if (name === 'd') {
    const cc = require(resolve(here, '../build/out/native/cc-ops.node'));
    Object.assign(fnExtra, {
      cc_read_page_geometry: cc.ccReadPageGeometry,
      cc_read_page_text: cc.ccReadPageText,
      cc_render_page: cc.ccRenderPage,
      cc_search_page: cc.ccSearchPage,
    });
  }

  const fn = {
    FPDF_InitLibrary: addon.initLibrary,
    FPDF_DestroyLibrary: addon.destroyLibrary,
    FPDF_LoadMemDocument: (_ptr, _len, _pw) => {
      throw new Error('native arms load through loadMemDocument(Buffer)');
    },
    FPDF_CloseDocument: addon.closeDocument,
    FPDF_GetPageCount: addon.getPageCount,
    FPDF_LoadPage: addon.loadPage,
    FPDF_ClosePage: addon.closePage,
    FPDF_GetPageWidthF: addon.getPageWidth,
    FPDF_GetPageHeightF: addon.getPageHeight,
    EPDF_GetPageSizeByIndexNormalized: addon.pageSizeNormalized,
    rs_EPDF_GetPageSizeByIndexNormalized: addon.pageSizeNormalized,
    FPDFText_LoadPage: addon.textLoadPage,
    FPDFText_ClosePage: addon.textClosePage,
    FPDFText_CountChars: addon.textCountChars,
    rs_FPDFText_CountChars: addon.textCountChars,
    FPDFText_GetTextObject: addon.textGetTextObject,
    rs_FPDFText_GetTextObject: addon.textGetTextObject,
    FPDFText_GetFontSize: addon.textGetFontSize,
    rs_FPDFText_GetFontSize: addon.textGetFontSize,
    EPDFText_GetCharGeometry: addon.charGeometry,
    rs_EPDFText_GetCharGeometry: addon.charGeometry,
    EPDFText_GetTextFull: addon.textFull,
    rs_EPDFText_GetTextFull: addon.textFull,
    FPDFText_FindStart: addon.textFindStart,
    rs_FPDFText_FindStart: addon.textFindStart,
    FPDFText_FindNext: addon.textFindNext,
    rs_FPDFText_FindNext: addon.textFindNext,
    FPDFText_GetSchResultIndex: addon.textGetSchResultIndex,
    rs_FPDFText_GetSchResultIndex: addon.textGetSchResultIndex,
    FPDFText_GetSchCount: addon.textGetSchCount,
    rs_FPDFText_GetSchCount: addon.textGetSchCount,
    FPDFText_FindClose: addon.textFindClose,
    rs_FPDFText_FindClose: addon.textFindClose,
    FPDFText_CountRects: addon.textCountRects,
    rs_FPDFText_CountRects: addon.textCountRects,
    FPDFText_GetRect: addon.textGetRect,
    rs_FPDFText_GetRect: addon.textGetRect,
    FPDFBitmap_CreateEx: (w, h, _fmt, buf, stride) => addon.bitmapCreate(w, h, buf, stride),
    FPDFBitmap_FillRect: addon.bitmapFillRect,
    rs_FPDFBitmap_FillRect: addon.bitmapFillRect,
    FPDFBitmap_Destroy: addon.bitmapDestroy,
    FPDF_RenderPageBitmap: (b, page, _x, _y, w, h, rot, flags) =>
      addon.renderPageBitmap(b, page, w, h, rot, flags),
    rs_FPDF_RenderPageBitmap: (b, page, _x, _y, w, h, rot, flags) =>
      addon.renderPageBitmap(b, page, w, h, rot, flags),
    rs_read_page_geometry: addon.rsReadPageGeometry,
    rs_read_page_text: addon.rsReadPageText,
    rs_render_page: addon.rsRenderPage,
    rs_search_page: addon.rsSearchPage,
    ...fnExtra,
  };
  const mem = {
    alloc: addon.alloc,
    free: addon.freePtr,
    peek32: addon.peek32,
    peekF32: addon.peekF32,
    peekF64: addon.peekF64,
    // readBytes already returns an owned Buffer (a Uint8Array). Wrapping it
    // in new Uint8Array would add a second copy inside the timed workload.
    readBytes: (p, len) => addon.readBytes(p, len),
    writeBytes: (p, d) => addon.writeBytes(p, Buffer.from(d)),
    // A zero-copy view onto native memory: `addon.viewBuffer` returns a real
    // external ArrayBuffer that ALIASES `[ptr, ptr+len)` rather than copying
    // it (crates/napi-arms/src/lib.rs's `view_buffer`), so the native coarse
    // arms pay the same "one crossing, then a typed read" cost the wasm arms
    // do and not an extra memcpy the wasm side never pays. `byteOffset: 0`
    // because the returned buffer already starts exactly at `ptr` -- unlike
    // the wasm case, there is no larger backing buffer to offset into.
    viewBuffer: (ptr, len) => ({ buffer: addon.viewBuffer(ptr, len), byteOffset: 0 }),
    writeU16: addon.writeU16,
    readU16: (p, units) => {
      const bytes = addon.readBytes(p, units * 2);
      const u16 = new Uint16Array(bytes.buffer, bytes.byteOffset, units);
      let out = '';
      const CHUNK = 0x2000;
      for (let i = 0; i < u16.length; i += CHUNK) {
        out += String.fromCharCode(...u16.subarray(i, Math.min(i + CHUNK, u16.length)));
      }
      return out;
    },
  };
  probeExternalBuffers(mem);
  fn.FPDF_InitLibrary();
  return { name, m: addon, fn, mem, close: () => fn.FPDF_DestroyLibrary() };
}
