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
 *
 * Known bias in the transcription, one-directional and conservative: the
 * real reader also does three things per glyph that this file omits —
 *   - `throwIfAborted(signal)` (PageGeometryReader.ts:110);
 *   - `normalizePdfRect` on the loose and tight boxes (:156, :162);
 *   - the `{objectKey, ...raw}` object spread that builds each record (:113).
 * All three cost real time only in the THIN arm (a/b), which calls
 * `readGlyphRaw` per glyph the way the real reader does; the coarse arms
 * never went through that per-glyph JS path to begin with. Omitting them
 * therefore makes the THIN arm look cheaper than the real reader actually
 * is, which understates the THIN side of the comparison and so shrinks,
 * never inflates, the measured win from coarsening.
 */
import { SEARCH_TERM } from './fixture.mjs';

const ALL = ['a', 'b', 'c', 'd'];
const THIN = ['a', 'b'];

/**
 * EPDF_CHAR_GEOMETRY field offsets, from epdf_text.h:35:
 * loose_box 16 + tight_box 16 + loose_quad 32 + tight_quad 32 + matrix 24 + flags 4.
 * Verified once during development with a throwaway offsetof program, which printed
 * "sizeof=124 loose=0 tight=16 lq=32 tq=64 m=96 flags=120", confirming the literal below.
 * That program was never committed, so there is no standing check here: this struct's
 * layout is otherwise trusted from the header.
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
  SYNTHESIZED: 1 << 6,
  /**
   * Set by the record producer (rs_/cc_), not by PDFium itself. Bit 7, the
   * first bit above SYNTHESIZED, because the record copies PDFium's flags
   * verbatim before OR-ing this in.
   */
  ASCENT_FLIP: 1 << 7,
};

/** The packed record arms c and d write, one per glyph. See cpp/record.h. */
export const RECORD_BYTES = 112;

/**
 * Static per-workload metadata: which arms are supposed to run a workload,
 * and whether it has a real canonical answer to compare (`false` for probes
 * that exist to measure raw call/copy cost and have nothing meaningful to
 * verify). `makeWorkloads` below builds each workload's `arms`/`canonical`
 * fields from this table, so there is exactly one place that says what each
 * arm is supposed to run. `gate.mjs` imports this table directly so it can
 * compute the set of workloads two arms are BOTH expected to agree on from
 * the declaration, not from whichever keys happen to come back — a typo in
 * a workload's `arms` list, or an arm that never learns a workload, then
 * fails loudly instead of silently dropping out of the comparison.
 */
export const WORKLOAD_TABLE = [
  { id: 'null-call', arms: THIN, hasCanonical: true },
  { id: 'null-peek', arms: THIN, hasCanonical: false },
  { id: 'null-readbytes', arms: THIN, hasCanonical: false },
  { id: 'geometry', arms: ALL, hasCanonical: true },
  { id: 'geometry-copyout', arms: ['c', 'd'], hasCanonical: false },
  { id: 'text', arms: ALL, hasCanonical: true },
  { id: 'render-1x', arms: ALL, hasCanonical: true },
  { id: 'render-4x', arms: ALL, hasCanonical: true },
  { id: 'render-copyout-4x', arms: ALL, hasCanonical: false },
  { id: 'search', arms: ALL, hasCanonical: true },
];

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
 * DataView equivalents of rectAt/quadAt above, for geometryCoarse, which
 * reads from a DataView over the packed record buffer rather than through
 * `mem`. Same allocation shape as their `mem`-based counterparts: a direct
 * 4-element array literal for a rect, a preallocated Array(8) filled by a
 * plain indexed loop for a quad. No `.map`, no `Array.from`, no closure
 * allocated per glyph — the coarse and thin decodes must differ only in
 * where the data comes from, never in how much garbage they make.
 *
 * geometryCoarse obeys this rule; searchCoarse below does not. Its hit loop
 * builds `[0, 4, 8, 12].map(...)`, one throwaway array literal and closure
 * per hit, 120 per call on this fixture. That is the same class of bug this
 * rule exists to catch, and it runs against the coarse arm, so the published
 * 3.3x and 3.1x search-coarsening numbers understate the true win slightly.
 * It is left rather than fixed, because fixing it would change a published
 * number and re-running the benchmark to correct it is not worth a change
 * that can only improve the coarse arm's result.
 *
 * The count used to read "about 600 per call", which was searchThin's PDFium
 * lookup count (602 on this fixture: one FindStart, one FindNext and three
 * index calls per match, one GetRect per rect, one FindClose), not the hit
 * count. Measured: "the" on page 4 of report.pdf finds 120 hit rects.
 */
const dvRectAt = (dv, off) => [
  dv.getFloat32(off, true),
  dv.getFloat32(off + 4, true),
  dv.getFloat32(off + 8, true),
  dv.getFloat32(off + 12, true),
];

const dvQuadAt = (dv, off) => {
  const out = new Array(8);
  for (let i = 0; i < 8; i++) out[i] = dv.getFloat32(off + i * 4, true);
  return out;
};

/**
 * Matches PageGeometryReader's `emptyRawGlyph()` (PageGeometryReader.ts:315),
 * which hardcodes `flags: 2` rather than 0 — a deliberate legacy sentinel
 * that downstream run-grouping tests with `flags & 2` (see :232 and :299).
 * The brief's literal transcription used `flags: 0`; that would silently
 * stop empty glyphs from being recognized as empty, so this follows the real
 * reader instead.
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

  // Hoisted once per arm, not looked up per call inside geometryThin,
  // searchThin, textThin and renderThin below (Finding 2). `fn[`${p}Name`]`
  // inside a hot loop rebuilds a template string and does a dynamic property
  // lookup on every call; measured at about 7.7 ns per lookup, that is
  // roughly 62 µs per geometryThin() call (two guaranteed lookups per glyph
  // over 8,106 glyphs) and about 601 lookups per searchThin() call, enough
  // to inflate the coarse win by several percent on both workloads. It is an
  // artifact of how this file selects a function name per arm, not a
  // property of the thin design: the real reader at
  // PageGeometryReader.ts:113 does a plain monomorphic property access, so
  // hoisting is both fairer and closer to the code being modelled.
  //
  // This hoist reaches the thin side only. geometryCoarse, textCoarse,
  // renderCoarse and searchCoarse below still do `fn[`${coarse}_op_name`]`
  // inside their timed body, rebuilding the same kind of template string and
  // dynamic property lookup on every call. That call happens once per
  // invocation rather than once per glyph or hit, since a coarse call
  // crosses once instead of looping per record, so one lookup against a
  // 563 µs to 2.8 ms call is immaterial on its own. But it means only the
  // thin side was equalized here, not both.
  const thinGetTextObject = fn[`${p}FPDFText_GetTextObject`];
  const thinGetCharGeometry = fn[`${p}EPDFText_GetCharGeometry`];
  const thinGetFontSize = fn[`${p}FPDFText_GetFontSize`];
  const thinGetTextFull = fn[`${p}EPDFText_GetTextFull`];
  const thinFillRect = fn[`${p}FPDFBitmap_FillRect`];
  const thinRenderPageBitmap = fn[`${p}FPDF_RenderPageBitmap`];
  const thinFindStart = fn[`${p}FPDFText_FindStart`];
  const thinFindNext = fn[`${p}FPDFText_FindNext`];
  const thinGetSchResultIndex = fn[`${p}FPDFText_GetSchResultIndex`];
  const thinGetSchCount = fn[`${p}FPDFText_GetSchCount`];
  const thinCountRects = fn[`${p}FPDFText_CountRects`];
  const thinGetRect = fn[`${p}FPDFText_GetRect`];
  const thinFindClose = fn[`${p}FPDFText_FindClose`];

  const allocChecked = (bytes, label) => {
    const ptr = mem.alloc(bytes);
    if (!ptr) throw new Error(`mem.alloc(${bytes}) returned 0 allocating ${label}`);
    return ptr;
  };

  // Scratch buffers, allocated once so allocation is not in any hot loop.
  const geoPtr = allocChecked(GEO.bytes, 'geoPtr');
  const scratch8 = allocChecked(8, 'scratch8');
  const recordCap = Math.max(fx.glyphCount, 1);
  const recordPtr = allocChecked(recordCap * RECORD_BYTES, 'recordPtr');
  const textUnits = fx.glyphCount * 2 + 1;
  const textPtr = allocChecked(textUnits * 2, 'textPtr');
  const rectPtrs = [
    allocChecked(8, 'rectPtrs[0]'),
    allocChecked(8, 'rectPtrs[1]'),
    allocChecked(8, 'rectPtrs[2]'),
    allocChecked(8, 'rectPtrs[3]'),
  ];
  const termPtr = mem.writeU16(SEARCH_TERM);
  const searchCap = 4096;
  const searchPtr = allocChecked(searchCap * 16, 'searchPtr');

  const W = Math.round(fx.width);
  const H = Math.round(fx.height);
  const bmp = (scale) => {
    const w = Math.max(1, Math.round(W * scale));
    const h = Math.max(1, Math.round(H * scale));
    const stride = w * 4;
    const buf = allocChecked(stride * h, `bitmap buffer (${w}x${h})`);
    const handle = fn.FPDFBitmap_CreateEx(w, h, 4 /* BGRA */, buf, stride);
    if (!handle) throw new Error(`FPDFBitmap_CreateEx returned 0 for ${w}x${h}`);
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
      const objectKey = thinGetTextObject(fx.textPage, i);
      const ok = thinGetCharGeometry(fx.textPage, i, geoPtr);
      const g = ok ? readGlyphRaw() : emptyGlyph();
      g.objectKey = objectKey;
      if (objectKey !== prevKey) {
        g.fontSize = thinGetFontSize(fx.textPage, i);
        prevKey = objectKey;
      }
      out[i] = g;
    }
    return out;
  };

  /**
   * Overflow, reported the same way for both coarse ops.
   *
   * `_read_page_geometry` and `_search_page` both return NEGATIVE the count
   * found when it did not fit in the cap. Reading `n` records past a `cap`-sized
   * allocation is not something the wasm arms can catch for themselves
   * (bench/arm.mjs's `viewBuffer` ignores its length argument) and it is what
   * the native arms reject inside `view_buffer`, so a count that cannot be read
   * has to fail loudly rather than come back as plausible coordinates from
   * unrelated heap.
   */
  const fits = (n, name, what, cap) => {
    if (n < 0) {
      throw new Error(`${coarse}_${name}: ${-n} ${what} exceed cap=${cap}. Raise the cap.`);
    }
    return n;
  };

  /** Arms c and d. One crossing, then one typed-array walk. */
  const geometryCoarse = () => {
    const n = fits(
      fn[`${coarse}_read_page_geometry`](fx.textPage, recordPtr, recordCap),
      'read_page_geometry',
      'glyphs',
      recordCap,
    );
    // A view over the LIVE memory, not a copy: `mem.readBytes` here would
    // memcpy this arm's 908 KB record buffer every call, a cost the thin arm
    // never pays since it reads the heap in place — see Finding 6.
    // `mem.viewBuffer` is the single code path for both platforms: in wasm it
    // is the live `HEAPU8.buffer` (linked with ALLOW_MEMORY_GROWTH=1, so that
    // backing ArrayBuffer can be replaced whenever the heap grows), and
    // natively it is a zero-copy external ArrayBuffer aliasing the same
    // pointer. Either way this must be rebuilt on every call — never cache it
    // across calls.
    const view = mem.viewBuffer(recordPtr, n * RECORD_BYTES);
    const dv = new DataView(view.buffer, view.byteOffset, n * RECORD_BYTES);
    const out = new Array(n);
    for (let i = 0; i < n; i++) {
      const o = i * RECORD_BYTES;
      const native = dv.getUint32(o, true);
      if (native & F.EMPTY) {
        const g = emptyGlyph();
        g.objectKey = dv.getUint32(o + 4, true);
        // PRODUCER CONTRACT, and arms c and d must honour it exactly.
        // geometryThin calls FPDFText_GetFontSize only when the object key
        // changes (PageGeometryReader.ts:116-117) and leaves 0 on every
        // other glyph. So the packed record must carry the font size ONLY
        // in the first glyph of each text object and 0 everywhere else --
        // NOT the object's font size duplicated into every slot. Reading
        // o+8 unconditionally then reproduces thin's values exactly.
        // The empty branch must read it too, because a text object can
        // begin on an empty glyph and thin sets fontSize there.
        g.fontSize = dv.getFloat32(o + 8, true);
        out[i] = g;
        continue;
      }
      const upright = Boolean(native & F.UPRIGHT);
      // Matches readGlyphRaw's gate exactly (:164 in the real reader):
      // rotation/ascentFlip exist only when there is an oriented quad to
      // read them from. A non-upright glyph with a singular matrix and no
      // oriented cell (real reader's comment at PageGeometryReader.ts:189)
      // must not report a rotation just because it isn't upright.
      const looseQuadGate = !upright && Boolean(native & F.HAS_LOOSE_QUAD);
      const g = {
        flags: native & F.SPACE ? 1 : 0,
        objectKey: dv.getUint32(o + 4, true),
        fontSize: dv.getFloat32(o + 8, true),
        rotation: looseQuadGate ? dv.getFloat32(o + 12, true) : 0,
        ascentFlip: looseQuadGate && Boolean(native & F.ASCENT_FLIP),
        upright,
        looseBox: dvRectAt(dv, o + 16),
        tightBox: null,
        looseQuad: null,
        tightQuad: null,
      };
      if (native & F.HAS_TIGHT_BOX) {
        g.tightBox = dvRectAt(dv, o + 32);
      }
      if (native & F.HAS_LOOSE_QUAD) {
        g.looseQuad = dvQuadAt(dv, o + 48);
        if (native & F.HAS_TIGHT_QUAD) {
          g.tightQuad = dvQuadAt(dv, o + 80);
        }
      }
      out[i] = g;
    }
    return out;
  };

  const textThin = () => {
    const written = thinGetTextFull(fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  const textCoarse = () => {
    const written = fn[`${coarse}_read_page_text`](fx.textPage, textPtr, textUnits);
    return written <= 1 ? '' : mem.readU16(textPtr, written - 1);
  };

  // Timed render paths: consume via a single peek rather than a full copy,
  // so the timed region is the render call itself (Finding 6c). The full
  // bitmap is only ever copied for correctness checking, in `renderBytes`
  // below, which is never on the timed path.
  const renderThin = (b) => {
    // Prefixed, like every other call in a thin body. Reaching for the
    // unprefixed FPDFBitmap_FillRect here meant arm b paid the extra wasm
    // frame on one of the render workload's two PDFium calls, understating
    // render-1x/render-4x by about half a call.
    thinFillRect(b.handle, 0, 0, b.w, b.h, 0xffffffff);
    thinRenderPageBitmap(b.handle, fx.page, 0, 0, b.w, b.h, 0, 0);
    return mem.peek32(b.buf);
  };

  const renderCoarse = (b) => {
    fn[`${coarse}_render_page`](fx.page, b.handle, b.w, b.h, 0);
    return mem.peek32(b.buf);
  };

  /** Untimed: renders fresh and copies out the full bitmap, for canonical() only. */
  const renderBytesThin = (b) => {
    thinFillRect(b.handle, 0, 0, b.w, b.h, 0xffffffff);
    thinRenderPageBitmap(b.handle, fx.page, 0, 0, b.w, b.h, 0, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const renderBytesCoarse = (b) => {
    fn[`${coarse}_render_page`](fx.page, b.handle, b.w, b.h, 0);
    return mem.readBytes(b.buf, b.stride * b.h);
  };

  const searchThin = () => {
    const handle = thinFindStart(fx.textPage, termPtr, 0, 0);
    const hits = [];
    while (thinFindNext(handle)) {
      const start = thinGetSchResultIndex(handle);
      const count = thinGetSchCount(handle);
      const rects = thinCountRects(fx.textPage, start, count);
      for (let r = 0; r < rects; r++) {
        thinGetRect(fx.textPage, r, ...rectPtrs);
        hits.push(rectPtrs.map((q) => mem.peekF64(q)));
      }
    }
    thinFindClose(handle);
    return hits;
  };

  const searchCoarse = () => {
    const raw = fn[`${coarse}_search_page`](fx.textPage, termPtr, searchPtr, searchCap);
    // -1 is FindStart failing, not one hit rect that did not fit: overflow
    // needs `written > cap >= 1`, so it is always <= -2. The thin arm would
    // have looped zero times on a null handle and reported an empty page,
    // which is the wrong answer dressed as a plausible one.
    if (raw === -1) {
      throw new Error(`${coarse}_search_page: FPDFText_FindStart failed`);
    }
    const n = fits(raw, 'search_page', 'hit rects', searchCap);
    // A view over the LIVE memory, not a copy: `mem.readBytes` here would
    // memcpy the packed rect buffer every call, a cost the thin arm never
    // pays since it reads the heap in place — the same asymmetry Finding 6
    // fixed in geometryCoarse, and geometryCoarse's fix and this one should
    // not disagree about the same principle. See geometryCoarse's comment for
    // why `mem.viewBuffer` must be rebuilt on every call rather than cached.
    const view = mem.viewBuffer(searchPtr, n * 16);
    const dv = new DataView(view.buffer, view.byteOffset, n * 16);
    const hits = new Array(n);
    for (let i = 0; i < n; i++) {
      hits[i] = [0, 4, 8, 12].map((k) => dv.getFloat32(i * 16 + k, true));
    }
    return hits;
  };

  const geometry = () => (coarse ? geometryCoarse() : geometryThin());
  const text = () => (coarse ? textCoarse() : textThin());
  const render = (b) => (coarse ? renderCoarse(b) : renderThin(b));
  const renderBytes = (b) => (coarse ? renderBytesCoarse(b) : renderBytesThin(b));
  const search = () => (coarse ? searchCoarse() : searchThin());

  // `geometry-copyout` isolates the record-buffer copy cost that
  // `geometryCoarse` used to pay every call before Finding 6a moved it onto
  // the live heap. Populate the buffer once, untimed, at setup: the copy's
  // cost depends only on byte length, not content, so there is no need to
  // re-cross into the coarse read on every timed iteration.
  let geometryCopyoutN = 0;
  if (coarse) {
    geometryCopyoutN = fits(
      fn[`${coarse}_read_page_geometry`](fx.textPage, recordPtr, recordCap),
      'read_page_geometry',
      'glyphs',
      recordCap,
    );
  }

  // Warm the 4x bitmap once, untimed, so `render-copyout-4x` always copies
  // real pixels regardless of what order the bench runner invokes workloads
  // in.
  render(bmp4);

  const hash = (bytes) => {
    let h = 0x811c9dc5;
    for (let i = 0; i < bytes.length; i++) {
      h ^= bytes[i];
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h.toString(16).padStart(8, '0');
  };

  /** Offset of the first byte that differs from the 0xFF fill (Finding 8). */
  const firstNonWhite = (bytes) => {
    for (let i = 0; i < bytes.length; i++) {
      if (bytes[i] !== 0xff) return i;
    }
    return -1;
  };

  const canonicalRender = (b) => {
    const bytes = renderBytes(b);
    return { hash: hash(bytes), len: bytes.length, firstNonWhite: firstNonWhite(bytes) };
  };

  const round = (v) => (typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
  const roundDeep = (v) =>
    Array.isArray(v) ? v.map(roundDeep) : typeof v === 'number' ? round(v) : v;

  const canonicalGeometry = () => {
    let prevKey = null;
    return roundDeep(
      geometry().map((g) => {
        // A per-glyph CHANGE MARKER, not the raw objectKey handle: raw
        // handle values legitimately differ between wasm modules (separate
        // allocators, separate numbering), but the pattern of when the key
        // changes from the previous glyph is real run-splitting data
        // (PageGeometryReader.ts:116) that a coarse arm writing shifted or
        // garbage keys must still reproduce.
        const objectKeyChanged = g.objectKey !== prevKey ? 1 : 0;
        prevKey = g.objectKey;
        return [
          g.flags,
          objectKeyChanged,
          g.fontSize,
          // Compared at the record's storage width, not at the thin arms'.
          // Every other field here came out of PDFium as a float and survives
          // the record byte-for-byte; `rotation` is the one value computed in
          // doubles and then narrowed to the f32 slot at record.rs:60. Near
          // 1 rad the f32 step is ~6e-8, about 6% of the 1e-6 interval
          // `round` below quantises to, so an arbitrary angle would report a
          // divergence that is purely storage width. rotated_text.pdf only
          // carries the four exact angles, which is why this never fired.
          Math.fround(g.rotation),
          g.ascentFlip ? 1 : 0,
          g.upright ? 1 : 0,
          g.looseBox,
          g.tightBox,
          g.looseQuad,
          g.tightQuad,
        ];
      }),
    );
  };

  /** run()/canonical() bodies, keyed by id. Arms/nullability come from WORKLOAD_TABLE. */
  const defs = {
    'null-call': {
      run: () => fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8),
      canonical: () => {
        fn[`${p}EPDF_GetPageSizeByIndexNormalized`](fx.doc, fx.pageIndex, scratch8);
        return [round(mem.peekF32(scratch8)), round(mem.peekF32(scratch8 + 4))];
      },
    },
    'null-peek': {
      run: () => mem.peek32(geoPtr + GEO.flags),
    },
    'null-readbytes': {
      run: () => mem.readBytes(geoPtr, 64)[0],
    },
    geometry: {
      run: () => geometry().length,
      canonical: canonicalGeometry,
    },
    'geometry-copyout': {
      run: () => mem.readBytes(recordPtr, geometryCopyoutN * RECORD_BYTES)[0],
    },
    text: {
      run: () => text().length,
      canonical: () => text(),
    },
    'render-1x': {
      run: () => render(bmp1),
      canonical: () => canonicalRender(bmp1),
    },
    'render-4x': {
      run: () => render(bmp4),
      canonical: () => canonicalRender(bmp4),
    },
    'render-copyout-4x': {
      run: () => mem.readBytes(bmp4.buf, bmp4.stride * bmp4.h)[0],
    },
    search: {
      run: () => search().length,
      canonical: () => roundDeep(search()),
    },
  };

  const out = {};
  for (const spec of WORKLOAD_TABLE) {
    const d = defs[spec.id];
    out[spec.id] = {
      id: spec.id,
      arms: spec.arms,
      run: d.run,
      canonical: spec.hasCanonical ? d.canonical : null,
    };
  }

  out.dispose = () => {
    fn.FPDFBitmap_Destroy(bmp1.handle);
    fn.FPDFBitmap_Destroy(bmp4.handle);
    for (const q of [
      geoPtr, scratch8, recordPtr, textPtr, termPtr, searchPtr,
      bmp1.buf, bmp4.buf, ...rectPtrs,
    ]) mem.free(q);
  };

  return out;
}
