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
/// Returns the total glyph count; writes at most `cap` records.
#[no_mangle]
pub extern "C" fn rs_read_page_geometry(text_page: u32, out: *mut u8, cap: i32) -> i32 {
    let count = unsafe { FPDFText_CountChars(text_page as _) }.max(0);
    if out.is_null() || cap <= 0 {
        return count;
    }
    let mut w = unsafe { Writer::new(out, cap as usize) };
    let mut prev_key: Option<u32> = None;
    let mut geo = EPDF_CHAR_GEOMETRY::default();

    for i in 0..count {
        // `object_key` is a grouping key compared for equality within one
        // page (see record.rs), not a pointer dereferenced later, so
        // truncating it to u32 on a 64-bit target is safe.
        let object_key = unsafe { FPDFText_GetTextObject(text_page as _, i) } as u32;
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
        if prev_key != Some(object_key) {
            rec.font_size = unsafe { FPDFText_GetFontSize(text_page as _, i) } as f32;
        }
        prev_key = Some(object_key);

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
                let m = geo.matrix;
                rec.rotation = m.b.atan2(m.a);
                if m.a * m.d - m.b * m.c < 0.0 {
                    rec.flags |= FLAG_ASCENT_FLIP;
                }
            }
        }

        w.write(i as usize, &rec);
    }
    count
}

fn quad(q: &FS_QUADPOINTSF) -> [f32; 8] {
    [q.x1, q.y1, q.x2, q.y2, q.x3, q.y3, q.x4, q.y4]
}

#[no_mangle]
pub extern "C" fn rs_read_page_text(text_page: u32, buf: *mut u16, len: i32) -> i32 {
    unsafe { EPDFText_GetTextFull(text_page as _, buf, len) }
}

#[no_mangle]
pub extern "C" fn rs_render_page(page: u32, bitmap: u32, w: i32, h: i32, rotate: i32) -> i32 {
    unsafe {
        FPDFBitmap_FillRect(bitmap as _, 0, 0, w, h, 0xffff_ffff);
        FPDF_RenderPageBitmap(bitmap as _, page as _, 0, 0, w, h, rotate, 0);
    }
    1
}

/// Runs the whole FindNext loop in Rust. Returns the hit-rect count; writes
/// four little-endian f32s per rect (left, top, right, bottom).
#[no_mangle]
pub extern "C" fn rs_search_page(text_page: u32, term: *const u16, out: *mut f32, cap: i32) -> i32 {
    let handle = unsafe { FPDFText_FindStart(text_page as _, term, 0, 0) };
    if handle == 0 {
        return 0;
    }
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
                if written < cap {
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
    written
}
