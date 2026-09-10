// Arm D. The same four coarse operations as crates/ops/src/lib.rs, in C++,
// linked into the same em++ command. If this lands within noise of arm C, the
// win belongs to moving the loop and not to Rust.
#include <cmath>
#include <cstdint>

#include "epdf_text.h"
#include "fpdf_text.h"
#include "fpdfview.h"

#include "record.h"

// On wasm32 the FPDF_* handle typedefs are 32-bit; on 64-bit natives they are
// pointers. ops_napi.cc passes void*, which matches the native case.
static_assert(sizeof(FPDF_TEXTPAGE) == sizeof(void*) || sizeof(FPDF_TEXTPAGE) == 4,
              "unexpected PDFium handle width");

// The record copies EPDF_CHAR_GEOMETRY::flags verbatim and then ORs
// kFlagAscentFlip in, so the producer bit must sit directly above every
// EPDF_CHARGEO bit. `>` was not enough: it would still pass if PDFium added a
// flag at bit 7 without renaming SYNTHESIZED. Pin the exact relationship and
// the non-overlap with every flag the header names today; a flag this list
// does not know about is caught by bench/record-flags.test.mjs, which reads
// the header itself.
constexpr uint32_t kAllPdfiumChargeoFlags =
    EPDF_CHARGEO_HAS_TIGHT_BOX | EPDF_CHARGEO_HAS_LOOSE_QUAD | EPDF_CHARGEO_HAS_TIGHT_QUAD |
    EPDF_CHARGEO_UPRIGHT | EPDF_CHARGEO_SPACE | EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_SYNTHESIZED;
static_assert(kFlagAscentFlip == (EPDF_CHARGEO_SYNTHESIZED << 1),
              "kFlagAscentFlip must be the bit directly above PDFium's highest EPDF_CHARGEO flag");
static_assert((kFlagAscentFlip & kAllPdfiumChargeoFlags) == 0,
              "kFlagAscentFlip collides with a PDFium EPDF_CHARGEO flag");

extern "C" {

// Transcribes the loop in
// packages/engine/services/src/features/geometry/PageGeometryReader.ts:109.
// Writes at most `cap` records.
//
// Returns the glyph count, or NEGATIVE the count when it did not fit in `cap`,
// matching rs_read_page_geometry in crates/ops/src/lib.rs and cc_search_page
// below. geometryCoarse (bench/workloads.mjs) builds a DataView over
// n * kRecordBytes of a cap * kRecordBytes allocation, and the wasm arms cannot
// catch the overrun themselves (bench/arm.mjs's viewBuffer ignores its length).
//
// out == nullptr or cap <= 0 is the sizing probe, not an overflow: it writes
// nothing and returns the positive count.
int cc_read_page_geometry(FPDF_TEXTPAGE text_page, uint8_t* out, int cap) {
  int count = FPDFText_CountChars(text_page);
  if (count < 0) count = 0;
  if (out == nullptr || cap <= 0) return count;

  bool have_prev = false;
  FPDF_PAGEOBJECT prev_key = nullptr;
  // Declared once, outside the loop: reused every iteration rather than
  // zeroed per glyph. Both this loop and the thin path ignore the struct's
  // contents whenever EPDFText_GetCharGeometry returns false or the EMPTY
  // flag is set, so a reused struct never leaks a previous glyph's values.
  EPDF_CHAR_GEOMETRY geo;

  for (int i = 0; i < count; i++) {
    FPDF_PAGEOBJECT object_key = FPDFText_GetTextObject(text_page, i);
    FPDF_BOOL ok = EPDFText_GetCharGeometry(text_page, i, &geo);

    Record rec;
    rec.object_key = static_cast<uint32_t>(reinterpret_cast<uintptr_t>(object_key));

    // geometryThin (bench/workloads.mjs) reads the font size whenever the
    // object key changes, BEFORE it knows whether the glyph is empty
    // (PageGeometryReader.ts:116-117 assigns fontSize unconditionally on
    // key change, independent of the emptyRawGlyph() branch above it). So
    // this assignment must happen for every glyph, empty or not, and must
    // happen before the empty/!ok early-return below -- an empty glyph
    // that begins a new text object still carries a real font size.
    if (!have_prev || prev_key != object_key) {
      rec.font_size = static_cast<float>(FPDFText_GetFontSize(text_page, i));
    }
    prev_key = object_key;
    have_prev = true;

    if (!ok || (geo.flags & EPDF_CHARGEO_EMPTY)) {
      rec.flags = EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_UPRIGHT;
      WriteRecord(out, static_cast<size_t>(i), static_cast<size_t>(cap), rec);
      continue;
    }

    rec.flags = geo.flags;
    // Written exactly as PDFium reports it: the real reader's
    // normalizePdfRect is deliberately omitted from the thin transcription
    // this arm must match.
    rec.loose_box[0] = geo.loose_box.left;
    rec.loose_box[1] = geo.loose_box.top;
    rec.loose_box[2] = geo.loose_box.right;
    rec.loose_box[3] = geo.loose_box.bottom;

    if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_BOX) {
      rec.tight_box[0] = geo.tight_box.left;
      rec.tight_box[1] = geo.tight_box.top;
      rec.tight_box[2] = geo.tight_box.right;
      rec.tight_box[3] = geo.tight_box.bottom;
    }

    if (geo.flags & EPDF_CHARGEO_HAS_LOOSE_QUAD) {
      const FS_QUADPOINTSF& l = geo.loose_quad;
      float lq[8] = {l.x1, l.y1, l.x2, l.y2, l.x3, l.y3, l.x4, l.y4};
      for (int k = 0; k < 8; k++) rec.loose_quad[k] = lq[k];
      if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_QUAD) {
        const FS_QUADPOINTSF& t = geo.tight_quad;
        float tq[8] = {t.x1, t.y1, t.x2, t.y2, t.x3, t.y3, t.x4, t.y4};
        for (int k = 0; k < 8; k++) rec.tight_quad[k] = tq[k];
      }
      // Matches readGlyphRaw's gate exactly: rotation/ascentFlip only exist
      // when there is an oriented quad to read them from, not merely when
      // the glyph is non-upright.
      if (!(geo.flags & EPDF_CHARGEO_UPRIGHT)) {
        // Promoted to double before the math, matching arm C and the thin
        // arms, which read the same float matrix and compute in JS doubles
        // (`readGlyphRaw` in bench/workloads.mjs). std::atan2(float, float)
        // resolves to the float overload, and an f32 atan2 or determinant can
        // round differently at the gate's fixed 1e-6 comparison.
        const FS_MATRIX& m = geo.matrix;
        const double a = m.a, b = m.b, c = m.c, d = m.d;
        rec.rotation = static_cast<float>(std::atan2(b, a));
        if (a * d - b * c < 0) rec.flags |= kFlagAscentFlip;
      }
    }

    WriteRecord(out, static_cast<size_t>(i), static_cast<size_t>(cap), rec);
  }
  return count > cap ? -count : count;
}

int cc_read_page_text(FPDF_TEXTPAGE text_page, unsigned short* buf, int len) {
  return EPDFText_GetTextFull(text_page, buf, len);
}

int cc_render_page(FPDF_PAGE page, FPDF_BITMAP bitmap, int w, int h, int rotate) {
  FPDFBitmap_FillRect(bitmap, 0, 0, w, h, 0xffffffff);
  FPDF_RenderPageBitmap(bitmap, page, 0, 0, w, h, rotate, 0);
  return 1;
}

// Runs the whole FindNext loop in C++. Writes four little-endian f32s per rect
// (left, top, right, bottom), at most `cap` rects.
//
// Returns the hit-rect count, or NEGATIVE the count found when it did not fit
// in `cap`, matching rs_search_page in crates/ops/src/lib.rs. The count used to
// come back unclamped on overflow, which let searchCoarse
// (bench/workloads.mjs) read past the end of its allocation with no error
// anywhere.
//
// `out == nullptr` or `cap <= 0` is the sizing probe, the same contract as
// cc_read_page_geometry: it writes nothing and returns the positive count.
// Without the null test a caller following the geometry op's convention,
// cc_search_page(tp, term, nullptr, cap) with a nonzero cap, dereferenced null
// on the first hit.
//
// Returns -1 when FPDFText_FindStart itself fails (null text page or term),
// matching rs_search_page. That used to come back as 0, indistinguishable from
// a page the term does not appear on. -1 cannot collide with the overflow
// answer: overflow needs written > cap >= 1, so it is always <= -2.
int cc_search_page(FPDF_TEXTPAGE text_page, const unsigned short* term, float* out, int cap) {
  FPDF_SCHHANDLE handle = FPDFText_FindStart(text_page, term, 0, 0);
  if (handle == nullptr) return -1;
  const bool probe = out == nullptr || cap <= 0;
  int written = 0;
  while (FPDFText_FindNext(handle)) {
    int start = FPDFText_GetSchResultIndex(handle);
    int count = FPDFText_GetSchCount(handle);
    int rects = FPDFText_CountRects(text_page, start, count);
    for (int r = 0; r < rects; r++) {
      double l = 0, t = 0, ri = 0, b = 0;
      // PDFium's FPDFText_GetRect (fpdf_text.cpp) writes all four outputs
      // before returning its status, storing a default-constructed zero
      // rect on failure. searchThin (bench/workloads.mjs) never checks the
      // return value, so it records that zero rect and counts it. Match
      // that here: write and count regardless of status.
      FPDFText_GetRect(text_page, r, &l, &t, &ri, &b);
      if (!probe && written < cap) {
        out[written * 4 + 0] = static_cast<float>(l);
        out[written * 4 + 1] = static_cast<float>(t);
        out[written * 4 + 2] = static_cast<float>(ri);
        out[written * 4 + 3] = static_cast<float>(b);
      }
      written++;
    }
  }
  FPDFText_FindClose(handle);
  return !probe && written > cap ? -written : written;
}

}  // extern "C"
