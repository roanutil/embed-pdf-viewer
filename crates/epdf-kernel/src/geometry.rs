//! Page addresses and page space: `PageRef` from `engine-core` and
//! `pageSpace` from `core-geometry`, the two the kernel hands controllers
//! (`ctx.geometry`, `ctx.assertPageRef`, `ctx.getPage`, the documents page
//! registry).
//!
//! `crates/plugins/geometry` is the port of `core-geometry`, but it
//! depends on this crate for its conformance harness, so this crate cannot
//! depend back on it. The page-space conversions are six affine maps from the
//! crop box, small enough to carry here; `pdfToContentMatrix` is the one
//! rule both copies encode.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `PageRef`: a page's durable address. `objectNumber` is the only kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageRef {
    pub page_object_number: u32,
}

impl PageRef {
    /// `toPageRef(pon)`.
    pub fn new(page_object_number: u32) -> Self {
        PageRef { page_object_number }
    }

    /// Read a `{kind, pageObjectNumber}` value. Another kind names no page.
    pub fn from_json(value: &Value) -> Option<PageRef> {
        if value.get("kind").and_then(Value::as_str) != Some("objectNumber") {
            return None;
        }
        let pon = value.get("pageObjectNumber")?.as_f64()?;
        (pon >= 0.0 && pon.fract() == 0.0 && pon <= u32::MAX as f64)
            .then(|| PageRef::new(pon as u32))
    }

    pub fn to_json(self) -> Value {
        serde_json::json!({ "kind": "objectNumber", "pageObjectNumber": self.page_object_number })
    }

    /// `pageRefsEqual(page.ref, self)` for a `PageLayout` value.
    pub fn names(self, page: &Value) -> bool {
        page.get("ref").and_then(PageRef::from_json) == Some(self)
    }
}

/// `PdfEdges`: a rectangle in PDF user space by its edges.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfEdges {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// `Rect` in page space: origin and size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quad {
    pub p1: Point,
    pub p2: Point,
    pub p3: Point,
    pub p4: Point,
}

type Mat = [f64; 6];

fn apply(m: &Mat, p: Point) -> Point {
    let [a, b, c, d, e, f] = *m;
    Point { x: a * p.x + c * p.y + e, y: b * p.x + d * p.y + f }
}

fn invert(m: &Mat) -> Mat {
    let [a, b, c, d, e, f] = *m;
    let det = a * d - b * c;
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    [ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)]
}

/// `PageSpace`: page ↔ PDF conversion for one page, built from its crop box.
///
///   pageX = pdfX − crop.left
///   pageY = crop.top − pdfY
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSpace {
    pub crop: PdfEdges,
    pub width: f64,
    pub height: f64,
    to_page: Mat,
    to_pdf: Mat,
}

impl PageSpace {
    /// `pageSpace(crop)`.
    pub fn new(crop: PdfEdges) -> Self {
        let to_page = [1.0, 0.0, 0.0, -1.0, -crop.left, crop.top];
        PageSpace {
            crop,
            width: crop.right - crop.left,
            height: crop.top - crop.bottom,
            to_page,
            to_pdf: invert(&to_page),
        }
    }

    pub fn pdf_to_page(&self, point: Point) -> Point {
        apply(&self.to_page, point)
    }

    pub fn page_to_pdf(&self, point: Point) -> Point {
        apply(&self.to_pdf, point)
    }

    pub fn pdf_rect_to_page(&self, rect: PdfEdges) -> Rect {
        Rect {
            x: rect.left - self.crop.left,
            y: self.crop.top - rect.top,
            width: rect.right - rect.left,
            height: rect.top - rect.bottom,
        }
    }

    pub fn page_rect_to_pdf(&self, rect: Rect) -> PdfEdges {
        PdfEdges {
            left: rect.x + self.crop.left,
            right: rect.x + rect.width + self.crop.left,
            top: self.crop.top - rect.y,
            bottom: self.crop.top - (rect.y + rect.height),
        }
    }

    pub fn pdf_quad_to_page(&self, quad: Quad) -> Quad {
        let m = &self.to_page;
        Quad {
            p1: apply(m, quad.p1),
            p2: apply(m, quad.p2),
            p3: apply(m, quad.p3),
            p4: apply(m, quad.p4),
        }
    }

    pub fn page_quad_to_pdf(&self, quad: Quad) -> Quad {
        let m = &self.to_pdf;
        Quad {
            p1: apply(m, quad.p1),
            p2: apply(m, quad.p2),
            p3: apply(m, quad.p3),
            p4: apply(m, quad.p4),
        }
    }
}
