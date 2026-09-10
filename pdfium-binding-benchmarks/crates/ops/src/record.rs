//! The 112-byte glyph record arms C and D both write. The layout is duplicated
//! in cpp/record.h and decoded in bench/workloads.mjs; all three must agree,
//! and the fairness gate is what proves they do.

pub const RECORD_BYTES: usize = 112;
/// Set alongside the EPDF_CHARGEO bits when the char matrix has det < 0.
/// Bit 7, the first bit above EPDF_CHARGEO_SYNTHESIZED (1u << 6); the record
/// copies PDFium's flags verbatim, so this must not collide with any of them.
pub const FLAG_ASCENT_FLIP: u32 = 1 << 7;

/// Every flag pdfium-sys mirrors from epdf_text.h. The producer bit must be
/// exactly one above the highest of them and overlap none. pdfium-sys is a
/// hand-written mirror, so this only sees the flags it names;
/// bench/record-flags.test.mjs reads the real header and checks the mirror
/// is complete.
const ALL_PDFIUM_FLAGS: u32 = pdfium_sys::EPDF_CHARGEO_HAS_TIGHT_BOX
    | pdfium_sys::EPDF_CHARGEO_HAS_LOOSE_QUAD
    | pdfium_sys::EPDF_CHARGEO_HAS_TIGHT_QUAD
    | pdfium_sys::EPDF_CHARGEO_UPRIGHT
    | pdfium_sys::EPDF_CHARGEO_SPACE
    | pdfium_sys::EPDF_CHARGEO_EMPTY
    | pdfium_sys::EPDF_CHARGEO_SYNTHESIZED;
const _: () = assert!(
    FLAG_ASCENT_FLIP == pdfium_sys::EPDF_CHARGEO_SYNTHESIZED << 1,
    "FLAG_ASCENT_FLIP must be the bit directly above PDFium's highest EPDF_CHARGEO flag"
);
const _: () = assert!(
    FLAG_ASCENT_FLIP & ALL_PDFIUM_FLAGS == 0,
    "FLAG_ASCENT_FLIP collides with a PDFium EPDF_CHARGEO flag"
);

pub struct Writer {
    base: *mut u8,
    cap: usize,
}

impl Writer {
    /// # Safety
    /// `base` must point to at least `cap * RECORD_BYTES` writable bytes.
    pub unsafe fn new(base: *mut u8, cap: usize) -> Self {
        Self { base, cap }
    }

    fn slot(&self, index: usize) -> Option<*mut u8> {
        if index >= self.cap {
            return None;
        }
        Some(unsafe { self.base.add(index * RECORD_BYTES) })
    }

    pub fn write(&mut self, index: usize, rec: &Record) {
        let Some(p) = self.slot(index) else { return };
        unsafe {
            core::ptr::write_unaligned(p as *mut u32, rec.flags);
            core::ptr::write_unaligned(p.add(4) as *mut u32, rec.object_key);
            core::ptr::write_unaligned(p.add(8) as *mut f32, rec.font_size);
            core::ptr::write_unaligned(p.add(12) as *mut f32, rec.rotation);
            for (i, v) in rec.loose_box.iter().enumerate() {
                core::ptr::write_unaligned(p.add(16 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.tight_box.iter().enumerate() {
                core::ptr::write_unaligned(p.add(32 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.loose_quad.iter().enumerate() {
                core::ptr::write_unaligned(p.add(48 + i * 4) as *mut f32, *v);
            }
            for (i, v) in rec.tight_quad.iter().enumerate() {
                core::ptr::write_unaligned(p.add(80 + i * 4) as *mut f32, *v);
            }
        }
    }
}

#[derive(Default)]
pub struct Record {
    pub flags: u32,
    pub object_key: u32,
    pub font_size: f32,
    pub rotation: f32,
    pub loose_box: [f32; 4],
    pub tight_box: [f32; 4],
    pub loose_quad: [f32; 8],
    pub tight_quad: [f32; 8],
}
