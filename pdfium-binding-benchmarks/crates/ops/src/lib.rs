//! Arm C. Four operations that each run a whole loop inside Rust and hand back
//! one packed buffer, so a page of geometry costs one boundary crossing
//! instead of tens of thousands.
#![allow(non_snake_case)]

mod record;

use pdfium_sys::*;
use record::{Record, Writer, FLAG_ASCENT_FLIP, RECORD_BYTES};

const _: () = assert!(RECORD_BYTES == 112);

/// Transcribes the loop in
/// packages/engine/services/src/features/geometry/PageGeometryReader.ts:109.
/// Writes at most `cap` records.
///
/// Returns the glyph count, or NEGATIVE the count when it did not fit in `cap`,
/// the same overflow contract `rs_search_page` uses below and for the same
/// reason: `geometryCoarse` (bench/workloads.mjs) builds a `DataView` over
/// `n * RECORD_BYTES` of a `cap * RECORD_BYTES` allocation, and the wasm arms
/// cannot catch the overrun themselves because bench/arm.mjs's `viewBuffer`
/// ignores its length argument. No caller passes a short `cap` today
/// (`recordCap` is the fixture's glyph count), so this was latent rather than
/// live, but a count that cannot be read is not an answer.
///
/// `out == null` or `cap <= 0` is the sizing probe, not an overflow: it writes
/// nothing and returns the positive count so a caller can allocate for it.
#[no_mangle]
pub extern "C" fn rs_read_page_geometry(text_page: FPDF_TEXTPAGE, out: *mut u8, cap: i32) -> i32 {
    let count = unsafe { FPDFText_CountChars(text_page as _) }.max(0);
    if out.is_null() || cap <= 0 {
        return count;
    }
    let mut w = unsafe { Writer::new(out, cap as usize) };
    let mut prev_key: Option<u64> = None;
    let mut geo = EPDF_CHAR_GEOMETRY::default();

    for i in 0..count {
        // Two widths on purpose. The RECORD's key is 32 bits (record.rs), and
        // arm D truncates it the same way (cpp/record.h), so the packed buffers
        // agree. The COMPARISON below must stay full width: `FPDF_PAGEOBJECT`
        // is a `u32` on wasm32 but a pointer on 64-bit natives, and arm D plus
        // both thin arms compare the whole handle. Comparing the truncated one
        // made arm C skip an `FPDFText_GetFontSize` call the other three make
        // whenever two consecutive text objects agreed in their low 32 bits,
        // writing `font_size` 0 and making arm C measurably cheaper than the
        // arms it is compared against.
        let object = unsafe { FPDFText_GetTextObject(text_page as _, i) } as u64;
        let object_key = object as u32;
        let ok = unsafe { EPDFText_GetCharGeometry(text_page as _, i, &mut geo) };

        let mut rec = Record {
            object_key,
            ..Default::default()
        };

        // geometryThin (bench/workloads.mjs) reads the font size whenever the
        // object key changes, BEFORE it knows whether the glyph is empty
        // (PageGeometryReader.ts:116-117 assigns fontSize unconditionally on
        // key change, independent of the emptyRawGlyph() branch above it).
        // So this assignment must happen for every glyph, empty or not, and
        // must happen before the empty/ok early-return below -- an empty
        // glyph that begins a new text object still carries a real font size.
        if prev_key != Some(object) {
            rec.font_size = unsafe { FPDFText_GetFontSize(text_page as _, i) } as f32;
        }
        prev_key = Some(object);

        if ok == 0 || (geo.flags & EPDF_CHARGEO_EMPTY) != 0 {
            rec.flags = EPDF_CHARGEO_EMPTY | EPDF_CHARGEO_UPRIGHT;
            w.write(i as usize, &rec);
            continue;
        }

        rec.flags = geo.flags;
        // Written exactly as PDFium reports it: the real reader's
        // normalizePdfRect (PageGeometryReader.ts:156,:162) is deliberately
        // omitted from the thin transcription this arm must match.
        rec.loose_box = [
            geo.loose_box.left,
            geo.loose_box.top,
            geo.loose_box.right,
            geo.loose_box.bottom,
        ];
        if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_BOX) != 0 {
            rec.tight_box = [
                geo.tight_box.left,
                geo.tight_box.top,
                geo.tight_box.right,
                geo.tight_box.bottom,
            ];
        }
        if (geo.flags & EPDF_CHARGEO_HAS_LOOSE_QUAD) != 0 {
            rec.loose_quad = quad(&geo.loose_quad);
            if (geo.flags & EPDF_CHARGEO_HAS_TIGHT_QUAD) != 0 {
                rec.tight_quad = quad(&geo.tight_quad);
            }
            // Matches readGlyphRaw's gate exactly: rotation/ascentFlip only
            // exist when there is an oriented quad to read them from, not
            // merely when the glyph is non-upright.
            if (geo.flags & EPDF_CHARGEO_UPRIGHT) == 0 {
                // Promoted to f64 before the math, because the thin arms read
                // the same f32 matrix and compute in JS doubles
                // (`readGlyphRaw` in bench/workloads.mjs). An f32 `atan2` and
                // an f32 determinant can round to a different value at the
                // gate's fixed 1e-6 comparison, which has no tolerance, and a
                // near-singular matrix can flip `ascentFlip` outright. The
                // rotated fixture only exercises 4 exact angles, so this was
                // passing on lucky samples rather than on agreement.
                let m = geo.matrix;
                let (a, b, c, d) = (m.a as f64, m.b as f64, m.c as f64, m.d as f64);
                rec.rotation = b.atan2(a) as f32;
                if a * d - b * c < 0.0 {
                    rec.flags |= FLAG_ASCENT_FLIP;
                }
            }
        }

        w.write(i as usize, &rec);
    }
    if count > cap {
        -count
    } else {
        count
    }
}

fn quad(q: &FS_QUADPOINTSF) -> [f32; 8] {
    [q.x1, q.y1, q.x2, q.y2, q.x3, q.y3, q.x4, q.y4]
}

#[no_mangle]
pub extern "C" fn rs_read_page_text(text_page: FPDF_TEXTPAGE, buf: *mut u16, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(text_page as _, buf, len) }
}

#[no_mangle]
pub extern "C" fn rs_render_page(page: FPDF_PAGE, bitmap: FPDF_BITMAP, w: i32, h: i32, rotate: i32) -> i32 {
    unsafe {
        FPDFBitmap_FillRect(bitmap as _, 0, 0, w, h, 0xffff_ffff);
        FPDF_RenderPageBitmap(bitmap as _, page as _, 0, 0, w, h, rotate, 0);
    }
    1
}

/// Runs the whole FindNext loop in Rust. Writes four little-endian f32s per
/// rect (left, top, right, bottom), at most `cap` rects.
///
/// Returns the hit-rect count, or NEGATIVE the count found when it did not fit
/// in `cap`. The count used to come back unclamped on overflow, which let
/// `searchCoarse` (bench/workloads.mjs) build a `DataView` over `n * 16` bytes
/// of a `cap * 16`-byte allocation and read whatever followed it: at `cap = 16`
/// on the timing fixture this returned 920, spanned 14,720 bytes of a 256-byte
/// allocation, and reported 904 rects with real-looking coordinates read from
/// unrelated Emscripten heap. The wasm arms cannot catch that themselves
/// (bench/arm.mjs's `viewBuffer` ignores its length argument), so overflow has
/// to be a distinct answer rather than a plausible one.
///
/// `out == null` or `cap <= 0` is the sizing probe, the same contract as
/// `rs_read_page_geometry`: it writes nothing and returns the positive count.
/// Without the null test a caller following the geometry op's convention,
/// `rs_search_page(tp, term, null, cap)` with a nonzero cap, dereferenced null
/// on the first hit.
///
/// Returns -1 when `FPDFText_FindStart` itself fails (null text page or term).
/// That used to come back as 0, indistinguishable from a page the term does
/// not appear on. -1 cannot collide with the overflow answer: overflow needs
/// `written > cap >= 1`, so it is always <= -2.
#[no_mangle]
pub extern "C" fn rs_search_page(text_page: FPDF_TEXTPAGE, term: *const u16, out: *mut f32, cap: i32) -> i32 {
    let handle = unsafe { FPDFText_FindStart(text_page as _, term, 0, 0) };
    if handle == 0 {
        return -1;
    }
    let probe = out.is_null() || cap <= 0;
    let mut written = 0i32;
    unsafe {
        while FPDFText_FindNext(handle) != 0 {
            let start = FPDFText_GetSchResultIndex(handle);
            let count = FPDFText_GetSchCount(handle);
            let rects = FPDFText_CountRects(text_page as _, start, count);
            for r in 0..rects {
                let (mut l, mut t, mut ri, mut b) = (0f64, 0f64, 0f64, 0f64);
                // PDFium's FPDFText_GetRect (fpdf_text.cpp) writes all four
                // outputs before returning its status, storing a
                // default-constructed zero rect on failure. searchThin
                // (bench/workloads.mjs) never checks the return value, so it
                // records that zero rect and counts it. Match that here:
                // write and count regardless of status.
                FPDFText_GetRect(text_page as _, r, &mut l, &mut t, &mut ri, &mut b);
                if !probe && written < cap {
                    let p = out.add((written * 4) as usize);
                    core::ptr::write_unaligned(p, l as f32);
                    core::ptr::write_unaligned(p.add(1), t as f32);
                    core::ptr::write_unaligned(p.add(2), ri as f32);
                    core::ptr::write_unaligned(p.add(3), b as f32);
                }
                written += 1;
            }
        }
        FPDFText_FindClose(handle);
    }
    if !probe && written > cap {
        -written
    } else {
        written
    }
}
