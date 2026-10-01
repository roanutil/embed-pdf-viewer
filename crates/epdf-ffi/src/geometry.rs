//! The records more than one plugin's exports use.
//!
//! `Rect` is flat (`x, y, width, height`) because that is what both
//! `epdf_kernel::geometry::Rect` and `epdf_geometry::Rect` are; the
//! `origin`/`size` shape is the TypeScript one. Each record converts from
//! and to both crates' types.

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PageRef {
    pub page_object_number: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl From<epdf_kernel::geometry::PageRef> for PageRef {
    fn from(r: epdf_kernel::geometry::PageRef) -> Self {
        PageRef { page_object_number: r.page_object_number }
    }
}

impl From<PageRef> for epdf_kernel::geometry::PageRef {
    fn from(r: PageRef) -> Self {
        epdf_kernel::geometry::PageRef::new(r.page_object_number)
    }
}

impl From<epdf_kernel::geometry::Point> for Point {
    fn from(p: epdf_kernel::geometry::Point) -> Self {
        Point { x: p.x, y: p.y }
    }
}

impl From<Point> for epdf_kernel::geometry::Point {
    fn from(p: Point) -> Self {
        epdf_kernel::geometry::Point { x: p.x, y: p.y }
    }
}

impl From<epdf_kernel::geometry::Rect> for Rect {
    fn from(r: epdf_kernel::geometry::Rect) -> Self {
        Rect { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

impl From<Rect> for epdf_kernel::geometry::Rect {
    fn from(r: Rect) -> Self {
        epdf_kernel::geometry::Rect { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

impl From<epdf_geometry::Point> for Point {
    fn from(p: epdf_geometry::Point) -> Self {
        Point { x: p.x, y: p.y }
    }
}

impl From<Point> for epdf_geometry::Point {
    fn from(p: Point) -> Self {
        epdf_geometry::Point { x: p.x, y: p.y }
    }
}

impl From<epdf_geometry::Size> for Size {
    fn from(s: epdf_geometry::Size) -> Self {
        Size { width: s.width, height: s.height }
    }
}

impl From<Size> for epdf_geometry::Size {
    fn from(s: Size) -> Self {
        epdf_geometry::Size { width: s.width, height: s.height }
    }
}

impl From<epdf_geometry::Rect> for Rect {
    fn from(r: epdf_geometry::Rect) -> Self {
        Rect { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

impl From<Rect> for epdf_geometry::Rect {
    fn from(r: Rect) -> Self {
        epdf_geometry::Rect { x: r.x, y: r.y, width: r.width, height: r.height }
    }
}

/// A quad whose orientation is known, corners named in the text's own
/// upright frame: `upper` is the ascent side, `start` to `end` runs along
/// the reading direction. Page space.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct TextQuad {
    pub upper_start: Point,
    pub upper_end: Point,
    pub lower_start: Point,
    pub lower_end: Point,
}

impl From<epdf_geometry::TextQuad> for TextQuad {
    fn from(q: epdf_geometry::TextQuad) -> Self {
        TextQuad {
            upper_start: q.upper_start.into(),
            upper_end: q.upper_end.into(),
            lower_start: q.lower_start.into(),
            lower_end: q.lower_end.into(),
        }
    }
}

impl From<TextQuad> for epdf_geometry::TextQuad {
    fn from(q: TextQuad) -> Self {
        epdf_geometry::TextQuad {
            upper_start: q.upper_start.into(),
            upper_end: q.upper_end.into(),
            lower_start: q.lower_start.into(),
            lower_end: q.lower_end.into(),
        }
    }
}
