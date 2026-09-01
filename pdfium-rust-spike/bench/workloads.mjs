/**
 * The only file that knows what is measured.
 *
 * `geometry` is a transcription of the loop in
 * packages/engine/services/src/features/geometry/PageGeometryReader.ts:109
 * and its `readGlyphRaw` helper at :135, call for call. It is a transcription
 * and not an import because the real reader needs DocumentSession and a built
 * engine-runtime dist, which would drag the whole workspace into a spike. So
 * this measures the SHAPE of the real reader, not the reader.
 *
 * Each workload declares which arms can run it. The null probes have nothing
 * for a coarse arm to collapse, so arms c and d skip them.
 */
import { SEARCH_TERM } from './fixture.mjs';

const ALL = ['a', 'b', 'c', 'd'];
const THIN = ['a', 'b'];

/**
 * EPDF_CHAR_GEOMETRY field offsets, from epdf_text.h:35:
 * loose_box 16 + tight_box 16 + loose_quad 32 + tight_quad 32 + matrix 24 + flags 4.
 * Step 5 of this task verifies them with offsetof rather than trusting the sum.
 */
const GEO = {
  bytes: 124,
  looseBox: 0,
  tightBox: 16,
  looseQuad: 32,
  tightQuad: 64,
  matrix: 96,
  flags: 120,
};

const F = {
  HAS_TIGHT_BOX: 1 << 0,
  HAS_LOOSE_QUAD: 1 << 1,
  HAS_TIGHT_QUAD: 1 << 2,
  UPRIGHT: 1 << 3,
  SPACE: 1 << 4,
  EMPTY: 1 << 5,
};

/** The packed record arms c and d write, one per glyph. See cpp/record.h. */
export const RECORD_BYTES = 112;

const rectAt = (mem, ptr, off) => [
  mem.peekF32(ptr + off),
  mem.peekF32(ptr + off + 4),
  mem.peekF32(ptr + off + 8),
  mem.peekF32(ptr + off + 12),
];

const quadAt = (mem, ptr, off) => {
  const out = new Array(8);
  for (let i = 0; i < 8; i++) out[i] = mem.peekF32(ptr + off + i * 4);
  return out;
};

/**
 * Matches PageGeometryReader's `emptyRawGlyph()` (PageGeometryReader.ts:315),
 * which hardcodes `flags: 2` rather than 0 — a deliberate legacy sentinel
 * that downstream run-grouping tests with `flags & 2` (see :232 and :299).
 * The brief's literal transcription used `flags: 0`; that would silently
 * stop empty glyphs from being recognized as empty, so this follows the real
 * reader instead. See task-3-report.md.
 */
const emptyGlyph = () => ({
  flags: 2,
  fontSize: 0,
  rotation: 0,
  ascentFlip: false,
  upright: true,
  looseBox: [0, 0, 0, 0],
  tightBox: null,
  looseQuad: null,
  tightQuad: null,
});

export function makeWorkloads(arm, fx) {
  const { fn, mem } = arm;
  const coarse = arm.name === 'c' ? 'rs' : arm.name === 'd' ? 'cc' : null;
  const p = arm.name === 'b' ? 'rs_' : '';

  // Scratch buffers, allocated once so allocation is not in any hot loop.
  const geoPtr = mem.alloc(GEO.bytes);
  const scratch8 = mem.alloc(8);
  const recordCap = Math.max(fx.glyphCount, 1);
  const recordPtr = mem.alloc(recordCap * RECORD_BYTES);
  const textUnits = fx.glyphCount * 2 + 1;
  const textPtr = mem.alloc(textUnits * 2);
  const rectPtrs = [mem.alloc(8), mem.alloc(8), mem.alloc(8), mem.alloc(8)];
  const termPtr = mem.writeU16(SEARCH_TERM);
  const searchCap = 4096;
  const searchPtr = mem.alloc(searchCap * 16);

  const W = Math.round(fx.width);
  const H = Math.round(fx.height);
  const bmp = (scale) => {
    const w = Math.max(1, Math.round(W * scale));
    const h = Math.max(1, Math.round(H * scale));
    const stride = w * 4;
    const buf = mem.alloc(stride * h);
    const handle = fn.FPDFBitmap_CreateEx(w, h, 4 /* BGRA */, buf, stride);
    return { w, h, stride, buf, handle };
  };
  const bmp1 = bmp(1);
  const bmp4 = bmp(4);

  /** Transcribed from PageGeometryReader.readGlyphRaw at :135. */
  const readGlyphRaw = () => {
    const native = mem.peek32(geoPtr + GEO.flags) >>> 0;
    if (native & F.EMPTY) return emptyGlyph();
    const upright = Boolean(native & F.UPRIGHT);
    const g = {
      flags: native & F.SPACE ? 1 : 0,
      fontSize: 0,
      rotation: 0,
      ascentFlip: false,
      upright,
      looseBox: rectAt(mem, geoPtr, GEO.looseBox),
      tightBox: null,
      looseQuad: null,
      tightQuad: null,
    };
    if (native & F.HAS_TIGHT_BOX) g.tightBox = rectAt(mem, geoPtr, GEO.tightBox);
    if (native & F.HAS_LOOSE_QUAD) {
      g.looseQuad = quadAt(mem, geoPtr, GEO.looseQuad);
      if (native & F.HAS_TIGHT_QUAD) g.tightQuad = quadAt(mem, geoPtr, GEO.tightQuad);
      if (!upright) {
        const a = mem.peekF32(geoPtr + GEO.matrix);
        const b = mem.peekF32(geoPtr + GEO.matrix + 4);
        const c = mem.peekF32(geoPtr + GEO.matrix + 8);
        const d = mem.peekF32(geoPtr + GEO.matrix + 12);
        g.rotation = Math.atan2(b, a);
        g.ascentFlip = a * d - b * c < 0;
      }
    }
    return g;
  };

  /** Transcribed from PageGeometryReader.read at :109. Arms a and b. */
  const geometryThin = () => {
    const out = new Array(fx.glyphCount);
    let prevKey = null;
    for (let i = 0; i < fx.glyphCount; i++) {
      const objectKey = fn[`${p}FPDFText_GetTextObject`](fx.textPage, i);
      const ok = fn[`${p}EPDFText_GetCharGeometry`](fx.textPage, i, geoPtr);
      const g = ok ? readGlyphRaw() : emptyGlyph();
      g.objectKey = objectKey;
      if (objectKey !== prevKey) {
        g.fontSize = fn[`${p}FPDFText_GetFontSize`](fx.textPage, i);
        prevKey = objectKey;
      }
      out[i] = g;
    }
    return out;
  };

  /** Arms c and d. One crossing, then one typed-array walk. */
  const geometryCoarse = () => {
    const n = fn[`${coarse}_read_page_geometry`](fx.textPage, recordPtr, recordCap);
    const bytes = mem.readBytes(recordPtr, n * RECORD_BYTES);
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const out = new Array(n);
    for (let i = 0; i < n; i++) {
      const o = i * RECORD_BYTES;
      const native = dv.getUint32(o, true);
      if (native & F.EMPTY) {
        const g = emptyGlyph();
        g.objectKey = dv.getUint32(o + 4, true);
        out[i] = g;
        continue;
      }
      const upright = Boolean(native & F.UPRIGHT);
      const g = {
        flags: native & F.SPACE ? 1 : 0,
        objectKey: dv.getUint32(o + 4, true),
        fontSize: dv.getFloat32(o + 8, true),
        rotation: upright ? 0 : dv.getFloat32(o + 12, true),
        ascentFlip: !upright && (native & (1 << 6)) !== 0,
        upright,
        looseBox: [16, 20, 24, 28].map((k) => dv.getFloat32(o + k, true)),
        tightBox: null,
        looseQuad: null,
        tightQuad: null,
      };
      if (native & F.HAS_TIGHT_BOX) {
        g.tightBox = [32, 36, 40, 44].map((k) => dv.getFloat32(o + k, true));
      }
      if (native & F.HAS_LOOSE_QUAD) {
        g.looseQuad = Array.from({ length: 8 }, (_, k) => dv.getFloat32(o + 48 + k * 4, true));
        if (native & F.HAS_TIGHT_QUAD) {
          g.tightQuad = Array.from({ length: 8 }, (_, k) => dv.getFloat32(o + 80 + k * 4, true));
        }
      }
      out[i] = g;
    }
    return out;
  };

  const textThin = () => {
    const written = fn[`${p}EPDFText_GetTextFull`](fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  const textCoarse = () => {
    const written = fn[`${coarse}_read_page_text`](fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  const renderThin = (b) => {
    fn.FPDFBitmap_FillRect(b.handle, 0, 0, b.w, b.h, 0xffffffff);
    fn[`${p}FPDF_RenderPageBitmap`](b.handle, fx.page, 0, 0, b.w, b.h, 0, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const renderCoarse = (b) => {
    fn[`${coarse}_render_page`](fx.page, b.handle, b.w, b.h, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const searchThin = () => {
    const handle = fn[`${p}FPDFText_FindStart`](fx.textPage, termPtr, 0, 0);
    const hits = [];
    while (fn[`${p}FPDFText_FindNext`](handle)) {
      const start = fn[`${p}FPDFText_GetSchResultIndex`](handle);
      const count = fn[`${p}FPDFText_GetSchCount`](handle);
      const rects = fn[`${p}FPDFText_CountRects`](fx.textPage, start, count);
      for (let r = 0; r < rects; r++) {
        fn[`${p}FPDFText_GetRect`](fx.textPage, r, ...rectPtrs);
        hits.push(rectPtrs.map((q) => mem.peekF64(q)));
      }
    }
    fn[`${p}FPDFText_FindClose`](handle);
    return hits;
  };

  const searchCoarse = () => {
    const n = fn[`${coarse}_search_page`](fx.textPage, termPtr, searchPtr, searchCap);
    const bytes = mem.readBytes(searchPtr, n * 16);
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const hits = new Array(n);
    for (let i = 0; i < n; i++) {
      hits[i] = [0, 4, 8, 12].map((k) => dv.getFloat32(i * 16 + k, true));
    }
    return hits;
  };

  const geometry = () => (coarse ? geometryCoarse() : geometryThin());
  const text = () => (coarse ? textCoarse() : textThin());
  const render = (b) => (coarse ? renderCoarse(b) : renderThin(b));
  const search = () => (coarse ? searchCoarse() : searchThin());

  const hash = (bytes) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < bytes.length; i++) {
      h ^= bytes[i];
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16).padStart(8, '0');
  };

  const round = (v) => (typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
  const roundDeep = (v) =>
    Array.isArray(v) ? v.map(roundDeep) : typeof v === 'number' ? round(v) : v;

  return {
    'null-call': {
      id: 'null-call',
      arms: THIN,
      run: () => fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8),
      canonical: () => {
        fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8);
        return [round(mem.peekF32(scratch8)), round(mem.peekF32(scratch8 + 4))];
      },
    },
    'null-peek': {
      id: 'null-peek',
      arms: THIN,
      run: () => mem.peek32(geoPtr + GEO.flags),
      canonical: () => 'n/a',
    },
    'null-readbytes': {
      id: 'null-readbytes',
      arms: THIN,
      run: () => mem.readBytes(geoPtr, 64)[0],
      canonical: () => 'n/a',
    },
    geometry: {
      id: 'geometry',
      arms: ALL,
      run: () => geometry().length,
      canonical: () => roundDeep(geometry().map((g) => [
        g.flags, g.fontSize, g.rotation, g.ascentFlip ? 1 : 0, g.upright ? 1 : 0,
        g.looseBox, g.tightBox, g.looseQuad, g.tightQuad,
      ])),
    },
    text: {
      id: 'text',
      arms: ALL,
      run: () => text().length,
      canonical: () => text(),
    },
    'render-1x': {
      id: 'render-1x',
      arms: ALL,
      run: () => render(bmp1)[0],
      canonical: () => hash(render(bmp1)),
    },
    'render-4x': {
      id: 'render-4x',
      arms: ALL,
      run: () => render(bmp4)[0],
      canonical: () => hash(render(bmp4)),
    },
    search: {
      id: 'search',
      arms: ALL,
      run: () => search().length,
      canonical: () => roundDeep(search()),
    },
    dispose() {
      fn.FPDFBitmap_Destroy(bmp1.handle);
      fn.FPDFBitmap_Destroy(bmp4.handle);
      for (const q of [
        geoPtr, scratch8, recordPtr, textPtr, termPtr, searchPtr,
        bmp1.buf, bmp4.buf, ...rectPtrs,
      ]) mem.free(q);
    },
  };
}
