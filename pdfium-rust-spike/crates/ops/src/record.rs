//! The 112-byte glyph record arms C and D both write. The layout is duplicated
//! in cpp/record.h and decoded in bench/workloads.mjs; all three must agree,
//! and the fairness gate is what proves they do.

pub const RECORD_BYTES: usize = 112;
/// Set alongside the EPDF_CHARGEO bits when the char matrix has det < 0.
pub const FLAG_ASCENT_FLIP: u32 = 1 << 6;

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
