//! Plain-data mirrors of the three PDFium geometry structs, so nothing
//! downstream has to hold a `bindgen` type.
//!
//! Field order and units are PDFium's, unchanged: `FS_RECTF` carries
//! `left, top, right, bottom` in that order and in whatever corner order the
//! file used, so a rect from here is NOT normalized. Normalizing is
//! `epdf_engine::geom::normalize_pdf_rect`, the single enforcement point for
//! the y-up invariant.

/// `FS_RECTF`, PDF user space, un-normalized.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RectF {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// `FS_QUADPOINTSF`. The four points are POSITIONAL (PDF 32000 §12.5.6.10);
/// this type asserts no corner semantics.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuadF {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub x3: f32,
    pub y3: f32,
    pub x4: f32,
    pub y4: f32,
}

/// `FS_MATRIX`, the 2x3 affine `[a b c d e f]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MatrixF {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl From<pdfium_sys::FS_RECTF> for RectF {
    fn from(r: pdfium_sys::FS_RECTF) -> Self {
        Self { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

impl From<pdfium_sys::FS_QUADPOINTSF> for QuadF {
    fn from(q: pdfium_sys::FS_QUADPOINTSF) -> Self {
        Self {
            x1: q.x1,
            y1: q.y1,
            x2: q.x2,
            y2: q.y2,
            x3: q.x3,
            y3: q.y3,
            x4: q.x4,
            y4: q.y4,
        }
    }
}

impl From<pdfium_sys::FS_MATRIX> for MatrixF {
    fn from(m: pdfium_sys::FS_MATRIX) -> Self {
        Self { a: m.a, b: m.b, c: m.c, d: m.d, e: m.e, f: m.f }
    }
}
