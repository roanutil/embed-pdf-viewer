//! The records more than one plugin's exports use.
//!
//! `Rect` is flat (`x, y, width, height`) because that is what both
//! `epdf_kernel::geometry::Rect` and `epdf_geometry::Rect` are; the
//! `origin`/`size` shape is the TypeScript one. Each record converts from
//! and to both crates' types.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Record)]
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

/// `Quad`: four corners named in the text's own upright frame (`upper` is
/// the ascent side, `left` to `right` runs along the reading direction),
/// as `@embedpdf/geometry` names them. Page space.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Quad {
    pub upper_left: Point,
    pub upper_right: Point,
    pub lower_left: Point,
    pub lower_right: Point,
}

impl From<epdf_geometry::Quad> for Quad {
    fn from(q: epdf_geometry::Quad) -> Self {
        Quad {
            upper_left: q.upper_left.into(),
            upper_right: q.upper_right.into(),
            lower_left: q.lower_left.into(),
            lower_right: q.lower_right.into(),
        }
    }
}

impl From<Quad> for epdf_geometry::Quad {
    fn from(q: Quad) -> Self {
        epdf_geometry::Quad {
            upper_left: q.upper_left.into(),
            upper_right: q.upper_right.into(),
            lower_left: q.lower_left.into(),
            lower_right: q.lower_right.into(),
        }
    }
}
