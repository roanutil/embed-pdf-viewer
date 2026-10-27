//! The engine-core text layer the plugin is built on: the geometry and text
//! snapshot DTOs (`dto/PageGeometrySnapshot.ts`, `dto/PageTextSnapshot.ts`),
//! the canonical text layout (`text/layout.ts`), its page-space wrapper
//! (`pageSpace/text.ts` `createTextLayout`, `pageGeometryOf`,
//! `pageTextSegmentOf`) and the copy primitive (`text/charmap.ts`
//! `sliceText`).
//!
//! `crates/epdf-engine` has a transcription of the same TypeScript
//! (`features/text/layout.rs`, `page_space/text.rs`), but that crate links
//! PDFium and the plugins also build for wasm32, where PDFium is not linked
//! (Ruling13.15). So this is a second copy, kept to the engine's semantics,
//! shared-edge tie-break included. `vectors/plugins/selection-geometry.json`
//! pins it against the TypeScript, and `tests/engine_cross_check.rs` runs
//! it beside the engine's on native hosts and asserts the two agree.
//!
//! Two departures from the engine's copy, both for the TypeScript's sake:
//! indices are `i64` where the TypeScript's are numbers a caller can push
//! out of range (`segments({ start: -1, ... })`), and runs whose `start`s
//! don't tile the glyphs (a host other than PDFium) are read without
//! indexing past the glyphs, where the TypeScript reads `undefined` and
//! throws. JavaScript's `Math.min`/`Math.max` come from `epdf_geometry::js`.
//!
//! The layout itself runs in y-up coordinates, as the engine's does. The
//! page-space [`TextLayout`] runs it on page-space geometry flipped top to
//! bottom and flips each answer back: the flip is exact.

use std::fmt::Debug;

use epdf_geometry::js;
use epdf_geometry::{
    PdfRect, Point, Quad, Rect, page_box_of, page_quad_of, pdf_point_of as geometry_pdf_point_of,
};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A point in y-up coordinates.
pub type PdfPoint = Point;
/// A box in page space: `x, y` is its top-left corner (`PageBox`).
pub type PageBox = Rect;

// ── the two spaces ────────────────────────────────────────────────────────────

/// The types a value's positions take (`Coordinates`): the file's
/// coordinates, [`PdfCoordinates`], or page space, [`PageCoordinates`].
pub trait Coordinates: Clone + Copy + Debug + PartialEq + 'static {
    /// A box: [`PdfRect`] or [`PageBox`].
    type Rect: Clone + Copy + Debug + PartialEq + Serialize + DeserializeOwned;
    /// A quad, its corners named in the text's own frame in either space.
    type Quad: Clone + Copy + Debug + PartialEq + Serialize + DeserializeOwned;
}

/// The file's coordinates: PDF user space, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfCoordinates;

impl Coordinates for PdfCoordinates {
    type Rect = PdfRect;
    type Quad = Quad;
}

/// Page space: from the top-left of the page's visible box, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageCoordinates;

impl Coordinates for PageCoordinates {
    type Rect = PageBox;
    type Quad = Quad;
}

// ── the wire DTOs ─────────────────────────────────────────────────────────────

fn is_false(value: &bool) -> bool {
    !*value
}

/// One upright character (`PageGeometryGlyph<C>`): the loose cell, always
/// present (zeroed for a character with no box, which is `empty`), and the
/// tight box, absent for whitespace. `space` and `empty` are on the wire,
/// as `true`, only when they hold. Members in the order `pageGeometryOf`
/// writes them (`{...glyph, loose, tight}`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(bound = "")]
pub struct PageGeometryGlyph<C: Coordinates = PageCoordinates> {
    #[serde(skip_serializing_if = "is_false")]
    pub space: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub empty: bool,
    pub loose: C::Rect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tight: Option<C::Rect>,
}

/// One non-upright character (`RotatedGeometryGlyph<C>`): its oriented
/// cells, corners named in the glyph's own upright frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(bound = "")]
pub struct RotatedGeometryGlyph<C: Coordinates = PageCoordinates> {
    #[serde(skip_serializing_if = "is_false")]
    pub space: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub empty: bool,
    pub loose: C::Quad,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tight: Option<C::Quad>,
}

/// One text run (`PageGeometryRun<C>`). The TypeScript narrows with
/// `'rotation' in run`, and so does deserialization.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged, bound = "")]
pub enum PageGeometryRun<C: Coordinates = PageCoordinates> {
    Upright {
        rect: C::Rect,
        /// The run's first character; it covers `glyphs.len()` from here.
        start: usize,
        #[serde(rename = "fontSize", skip_serializing_if = "Option::is_none")]
        font_size: Option<f64>,
        glyphs: Vec<PageGeometryGlyph<C>>,
    },
    Rotated {
        rect: C::Rect,
        start: usize,
        /// Degrees clockwise as the page shows it. Advisory: the layout
        /// never reads it.
        rotation: f64,
        #[serde(rename = "ascentFlip")]
        ascent_flip: bool,
        #[serde(rename = "fontSize", skip_serializing_if = "Option::is_none")]
        font_size: Option<f64>,
        glyphs: Vec<RotatedGeometryGlyph<C>>,
    },
}

impl<C: Coordinates> PageGeometryRun<C> {
    pub fn rect(&self) -> C::Rect {
        match self {
            Self::Upright { rect, .. } | Self::Rotated { rect, .. } => *rect,
        }
    }

    pub fn start(&self) -> usize {
        match self {
            Self::Upright { start, .. } | Self::Rotated { start, .. } => *start,
        }
    }

    pub fn font_size(&self) -> Option<f64> {
        match self {
            Self::Upright { font_size, .. } | Self::Rotated { font_size, .. } => *font_size,
        }
    }

    /// Whether the run has a character with a box of its own (`hasBox`).
    pub fn has_box(&self) -> bool {
        match self {
            Self::Upright { glyphs, .. } => glyphs.iter().any(|g| !g.empty),
            Self::Rotated { glyphs, .. } => glyphs.iter().any(|g| !g.empty),
        }
    }
}

/// Either kind of run in one pass: `rotation` may come after `glyphs`, so a
/// glyph's cell is read as whichever shape it has and checked once the run's
/// kind is known.
#[derive(Deserialize)]
#[serde(bound = "")]
struct WireRun<C: Coordinates> {
    rect: C::Rect,
    start: usize,
    glyphs: Vec<WireGlyph<C>>,
    /// Present, even as `null`, makes the run rotated: `'rotation' in run`.
    #[serde(default, deserialize_with = "present")]
    rotation: Option<Value>,
    #[serde(rename = "ascentFlip", default)]
    ascent_flip: Option<bool>,
    #[serde(rename = "fontSize", default)]
    font_size: Option<f64>,
}

#[derive(Deserialize)]
#[serde(bound = "")]
struct WireGlyph<C: Coordinates> {
    loose: WireCell<C>,
    #[serde(default)]
    tight: Option<WireCell<C>>,
    #[serde(default)]
    space: bool,
    #[serde(default)]
    empty: bool,
}

#[derive(Deserialize)]
#[serde(untagged, bound = "")]
enum WireCell<C: Coordinates> {
    Quad(C::Quad),
    Rect(C::Rect),
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

impl<'de, C: Coordinates> Deserialize<'de> for PageGeometryRun<C> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let run = WireRun::<C>::deserialize(deserializer)?;
        let wrong =
            |what: &str| D::Error::custom(format!("a glyph's `{what}` has the wrong shape"));
        let Some(rotation) = run.rotation else {
            let rect = |cell: WireCell<C>, what: &str| match cell {
                WireCell::Rect(r) => Ok(r),
                WireCell::Quad(_) => Err(wrong(what)),
            };
            let glyphs = run
                .glyphs
                .into_iter()
                .map(|g| {
                    Ok(PageGeometryGlyph {
                        space: g.space,
                        empty: g.empty,
                        loose: rect(g.loose, "loose")?,
                        tight: g.tight.map(|t| rect(t, "tight")).transpose()?,
                    })
                })
                .collect::<Result<_, D::Error>>()?;
            return Ok(PageGeometryRun::Upright {
                rect: run.rect,
                start: run.start,
                font_size: run.font_size,
                glyphs,
            });
        };
        let rotation = rotation.as_f64().ok_or_else(|| {
            D::Error::custom(format!("invalid type: {rotation}, expected f64 for `rotation`"))
        })?;
        let quad = |cell: WireCell<C>, what: &str| match cell {
            WireCell::Quad(q) => Ok(q),
            WireCell::Rect(_) => Err(wrong(what)),
        };
        let glyphs = run
            .glyphs
            .into_iter()
            .map(|g| {
                Ok(RotatedGeometryGlyph {
                    space: g.space,
                    empty: g.empty,
                    loose: quad(g.loose, "loose")?,
                    tight: g.tight.map(|t| quad(t, "tight")).transpose()?,
                })
            })
            .collect::<Result<_, D::Error>>()?;
        Ok(PageGeometryRun::Rotated {
            rect: run.rect,
            start: run.start,
            rotation,
            ascent_flip: run
                .ascent_flip
                .ok_or_else(|| D::Error::custom("missing field `ascentFlip`"))?,
            font_size: run.font_size,
            glyphs,
        })
    }
}

/// A page's text geometry (`PageGeometrySnapshot<C>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct PageGeometrySnapshot<C: Coordinates = PageCoordinates> {
    pub runs: Vec<PageGeometryRun<C>>,
}

impl<C: Coordinates> Default for PageGeometrySnapshot<C> {
    fn default() -> Self {
        PageGeometrySnapshot { runs: Vec::new() }
    }
}

/// One merged visual line of a text range (`PdfTextSegment<C>`): `quad`
/// is the geometric authority, its corners named in the line's own frame;
/// `rect` its bounds from the same constructor; `advance` the reading
/// direction along the baseline (1 toward the frame's +x, -1 back), taken
/// from the glyph sequence.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct PdfTextSegment<C: Coordinates = PageCoordinates> {
    pub quad: C::Quad,
    pub rect: C::Rect,
    pub advance: i32,
}

/// A page-space segment, what the plugin's state holds.
pub type PageTextSegment = PdfTextSegment<PageCoordinates>;

/// Characters on a page, in its character space (`TextRange`): `count`
/// characters from `start`. `i64`, because a caller can name a range that
/// starts before the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: i64,
    pub count: i64,
}

/// A page's extracted text (`PageTextSnapshot`): `charCount` is character
/// space, `text` is UTF-16 text space, and `charMap` anchors `[char, text]`
/// join them (absent or empty is the identity).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextSnapshot {
    pub text: String,
    pub char_count: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_map: Option<Vec<(f64, f64)>>,
}

// ── page space and the flip (`geometry/pageSpace.ts`, `pageSpace/text.ts`) ────

/// A character with no box, and a run of nothing but such characters,
/// carries a zeroed box or quad in either space; it is not a place, so it
/// never converts.
const NO_BOX: PageBox = Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
const NO_RECT: PdfRect = PdfRect { left: 0.0, right: 0.0, bottom: 0.0, top: 0.0 };
const NO_POINT: Point = Point { x: 0.0, y: 0.0 };
const NO_QUAD: Quad = Quad {
    upper_left: NO_POINT,
    upper_right: NO_POINT,
    lower_left: NO_POINT,
    lower_right: NO_POINT,
};

/// `pdfPointOf`: a page-space point in the file's coordinates.
pub fn pdf_point_of(point: Point, visible: PdfRect) -> PdfPoint {
    geometry_pdf_point_of(point, visible)
}

/// A page-space point flipped into y-up coordinates (`mirroredPoint`).
pub fn mirrored_point(point: Point) -> PdfPoint {
    Point { x: point.x, y: -point.y }
}

/// A y-up point flipped back into page space (`unmirroredPoint`).
pub fn unmirrored_point(point: PdfPoint) -> Point {
    Point { x: point.x, y: -point.y }
}

/// A page-space box flipped into a y-up rect (`mirroredRect`).
pub fn mirrored_rect(b: PageBox) -> PdfRect {
    PdfRect { left: b.x, right: b.x + b.width, bottom: -(b.y + b.height), top: -b.y }
}

/// A y-up rect flipped back into a page-space box (`unmirroredBox`).
pub fn unmirrored_box(r: PdfRect) -> PageBox {
    Rect { x: r.left, y: -r.top, width: r.right - r.left, height: r.top - r.bottom }
}

fn map_quad(q: Quad, f: fn(Point) -> Point) -> Quad {
    Quad {
        upper_left: f(q.upper_left),
        upper_right: f(q.upper_right),
        lower_left: f(q.lower_left),
        lower_right: f(q.lower_right),
    }
}

/// `mirroredQuad`.
pub fn mirrored_quad(q: Quad) -> Quad {
    map_quad(q, mirrored_point)
}

/// `unmirroredQuad`.
pub fn unmirrored_quad(q: Quad) -> Quad {
    map_quad(q, unmirrored_point)
}

/// A page's text geometry in page space (`pageGeometryOf`). `visible` is
/// the page's visible box.
pub fn page_geometry_of(
    snapshot: &PageGeometrySnapshot<PdfCoordinates>,
    visible: PdfRect,
) -> PageGeometrySnapshot<PageCoordinates> {
    let runs = snapshot
        .runs
        .iter()
        .map(|run| {
            let rect = if run.has_box() { page_box_of(run.rect(), visible) } else { NO_BOX };
            match run {
                PageGeometryRun::Upright { start, font_size, glyphs, .. } => {
                    PageGeometryRun::Upright {
                        rect,
                        start: *start,
                        font_size: *font_size,
                        glyphs: glyphs
                            .iter()
                            .map(|g| PageGeometryGlyph {
                                space: g.space,
                                empty: g.empty,
                                loose: if g.empty { NO_BOX } else { page_box_of(g.loose, visible) },
                                tight: g.tight.map(|t| page_box_of(t, visible)),
                            })
                            .collect(),
                    }
                }
                PageGeometryRun::Rotated {
                    start,
                    rotation,
                    ascent_flip,
                    font_size,
                    glyphs,
                    ..
                } => PageGeometryRun::Rotated {
                    rect,
                    start: *start,
                    rotation: *rotation,
                    ascent_flip: *ascent_flip,
                    font_size: *font_size,
                    glyphs: glyphs
                        .iter()
                        .map(|g| RotatedGeometryGlyph {
                            space: g.space,
                            empty: g.empty,
                            loose: if g.empty { NO_QUAD } else { page_quad_of(g.loose, visible) },
                            tight: g.tight.map(|t| page_quad_of(t, visible)),
                        })
                        .collect(),
                },
            }
        })
        .collect();
    PageGeometrySnapshot { runs }
}

/// A page-space run flipped top to bottom, for the y-up layout (`mirroredRun`).
pub fn mirrored_run(run: &PageGeometryRun<PageCoordinates>) -> PageGeometryRun<PdfCoordinates> {
    let rect = if run.has_box() { mirrored_rect(run.rect()) } else { NO_RECT };
    match run {
        PageGeometryRun::Upright { start, font_size, glyphs, .. } => PageGeometryRun::Upright {
            rect,
            start: *start,
            font_size: *font_size,
            glyphs: glyphs
                .iter()
                .map(|g| PageGeometryGlyph {
                    space: g.space,
                    empty: g.empty,
                    loose: if g.empty { NO_RECT } else { mirrored_rect(g.loose) },
                    tight: g.tight.map(mirrored_rect),
                })
                .collect(),
        },
        PageGeometryRun::Rotated { start, rotation, ascent_flip, font_size, glyphs, .. } => {
            PageGeometryRun::Rotated {
                rect,
                start: *start,
                rotation: *rotation,
                ascent_flip: *ascent_flip,
                font_size: *font_size,
                glyphs: glyphs
                    .iter()
                    .map(|g| RotatedGeometryGlyph {
                        space: g.space,
                        empty: g.empty,
                        loose: if g.empty { NO_QUAD } else { mirrored_quad(g.loose) },
                        tight: g.tight.map(mirrored_quad),
                    })
                    .collect(),
            }
        }
    }
}

/// A segment in the file's coordinates, in page space (`pageTextSegmentOf`).
pub fn page_text_segment_of(
    segment: &PdfTextSegment<PdfCoordinates>,
    visible: PdfRect,
) -> PageTextSegment {
    PdfTextSegment {
        quad: page_quad_of(segment.quad, visible),
        rect: page_box_of(segment.rect, visible),
        advance: segment.advance,
    }
}

// ── the page-space layout (`createTextLayout`) ────────────────────────────────

/// Where a word or line is asked for: at a point, or around a character.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAt {
    Point(Point),
    Char(i64),
}

impl From<Point> for TextAt {
    fn from(point: Point) -> Self {
        TextAt::Point(point)
    }
}

impl From<i64> for TextAt {
    fn from(index: i64) -> Self {
        TextAt::Char(index)
    }
}

/// Where a page's text is, in page space (`TextLayout`, what
/// `page.text.layout()` answers): points from the top-left of the page, y
/// down. Read it once per page and keep it; every method answers at once.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    runs: Vec<PageGeometryRun<PageCoordinates>>,
    layout: PageTextLayout,
}

/// `createTextLayout(snapshot)`: the y-up layout over the geometry flipped
/// top to bottom; every answer flips back.
pub fn create_text_layout(snapshot: PageGeometrySnapshot<PageCoordinates>) -> TextLayout {
    let mirrored = PageGeometrySnapshot { runs: snapshot.runs.iter().map(mirrored_run).collect() };
    TextLayout { layout: build_page_text_layout(&mirrored), runs: snapshot.runs }
}

impl TextLayout {
    /// How many characters the page has: numbered 0 to `char_count - 1`.
    pub fn char_count(&self) -> usize {
        self.layout.glyphs.len()
    }

    /// The raw geometry: characters grouped by line and font, with their boxes.
    pub fn runs(&self) -> &[PageGeometryRun<PageCoordinates>] {
        &self.runs
    }

    /// The y-up layout this one flips, frames and all.
    pub fn pdf_layout(&self) -> &PageTextLayout {
        &self.layout
    }

    /// The character at a point, or `None` with no text near it: the tight
    /// box first, then a near miss within 1.5 average glyph heights.
    pub fn char_at(&self, point: Point) -> Option<usize> {
        text_glyph_at(&self.layout, mirrored_point(point), DEFAULT_TOLERANCE_FACTOR)
    }

    fn index_of(&self, at: TextAt) -> Option<i64> {
        match at {
            TextAt::Point(point) => self.char_at(point).map(|i| i as i64),
            TextAt::Char(index) => {
                (index >= 0 && index < self.char_count() as i64).then_some(index)
            }
        }
    }

    /// The word at a point or around a character, or `None` when there is none.
    pub fn word_at(&self, at: impl Into<TextAt>) -> Option<TextRange> {
        let index = self.index_of(at.into())?;
        Some(range_of(expand_text_range_to_word(&self.layout, index)))
    }

    /// The visual line at a point or around a character, or `None`.
    pub fn line_at(&self, at: impl Into<TextAt>) -> Option<TextRange> {
        let index = self.index_of(at.into())?;
        Some(range_of(expand_text_range_to_line(&self.layout, index)))
    }

    /// One segment per visual line the range covers.
    pub fn segments(&self, range: TextRange) -> Vec<PageTextSegment> {
        text_segments_for_range(&self.layout, range.start, range.count)
            .into_iter()
            .map(|s| PdfTextSegment {
                quad: unmirrored_quad(s.quad),
                rect: unmirrored_box(s.rect),
                advance: s.advance,
            })
            .collect()
    }

    /// One character's loose cell, or `None` for a character with no box.
    pub fn char_quad(&self, index: i64) -> Option<Quad> {
        text_glyph_quad(&self.layout, index).map(unmirrored_quad)
    }
}

/// An inclusive `[from, to]` pair as a [`TextRange`] (`rangeOf`).
fn range_of((from, to): (i64, i64)) -> TextRange {
    TextRange { start: from, count: to.saturating_sub(from).saturating_add(1) }
}

// ── the y-up layout (`text/layout.ts`) ────────────────────────────────────────

const FLAG_SPACE: u32 = 1;
const FLAG_EMPTY: u32 = 2;

fn flags_of(space: bool, empty: bool) -> u32 {
    (if space { FLAG_SPACE } else { 0 }) | (if empty { FLAG_EMPTY } else { 0 })
}

fn is_boundary(flags: u32) -> bool {
    flags & (FLAG_SPACE | FLAG_EMPTY) != 0
}

fn is_empty(flags: u32) -> bool {
    flags & FLAG_EMPTY != 0
}

const CHAR_DISTANCE_FACTOR: f64 = 2.5;
const FONT_SIZE_RATIO_THRESHOLD: f64 = 1.5;
const VERTICAL_OVERLAP_THRESHOLD: f64 = 0.8;
const LINE_OVERLAP_THRESHOLD: f64 = 0.5;
const EDGE_EPSILON: f64 = 1e-6;

/// Baseline directions within about half a degree share a frame.
fn frame_dot_tolerance() -> f64 {
    0.0087f64.cos()
}

const ZERO_RECT: PdfRect = PdfRect { left: 0.0, right: 0.0, bottom: 0.0, top: 0.0 };

/// One glyph in its frame's space. `loose` builds segments; `tight` hit-tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutGlyph {
    pub loose: PdfRect,
    pub tight: Option<PdfRect>,
    pub flags: u32,
}

/// A contiguous run of glyphs in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutRun {
    pub char_start: usize,
    pub count: usize,
    /// Frame-local loose box, which is the layout's own space in frame 0.
    pub rect: PdfRect,
    pub font_size: Option<f64>,
    /// Index into [`PageTextLayout::frames`]; 0 is the layout's own space.
    pub frame: usize,
}

/// An orientation cluster's orthonormal basis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayoutFrame {
    /// Along the reading baseline, the frame's +x.
    pub baseline: PdfPoint,
    /// Toward the ascent side, the frame's +y.
    pub ascent: PdfPoint,
}

/// A page's text laid out per frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PageTextLayout {
    pub glyphs: Vec<TextLayoutGlyph>,
    pub runs: Vec<TextLayoutRun>,
    pub frames: Vec<TextLayoutFrame>,
}

const IDENTITY_FRAME: TextLayoutFrame =
    TextLayoutFrame { baseline: Point { x: 1.0, y: 0.0 }, ascent: Point { x: 0.0, y: 1.0 } };

fn to_frame(f: &TextLayoutFrame, p: PdfPoint) -> PdfPoint {
    Point { x: p.x * f.baseline.x + p.y * f.baseline.y, y: p.x * f.ascent.x + p.y * f.ascent.y }
}

fn from_frame(f: &TextLayoutFrame, p: PdfPoint) -> PdfPoint {
    Point { x: f.baseline.x * p.x + f.ascent.x * p.y, y: f.baseline.y * p.x + f.ascent.y * p.y }
}

/// Frame-local bounds of a cell: exact under the frame's rotation; shear
/// shows up as an in-frame envelope.
fn frame_box_of_quad(f: &TextLayoutFrame, q: &Quad) -> PdfRect {
    let pts = [q.upper_left, q.upper_right, q.lower_left, q.lower_right].map(|p| to_frame(f, p));
    let xs = pts.map(|p| p.x);
    let ys = pts.map(|p| p.y);
    PdfRect {
        left: js::min_of(&xs),
        bottom: js::min_of(&ys),
        right: js::max_of(&xs),
        top: js::max_of(&ys),
    }
}

/// Find or create the frame for a rotated run, keyed by the baseline
/// direction and ascent handedness of its first glyph with a box.
fn frame_for_run(
    frames: &mut Vec<TextLayoutFrame>,
    glyphs: &[RotatedGeometryGlyph<PdfCoordinates>],
) -> usize {
    let Some(seed) = glyphs.iter().find(|g| !g.empty) else { return 0 };
    let q = &seed.loose;
    let bx = q.upper_right.x - q.upper_left.x;
    let by = q.upper_right.y - q.upper_left.y;
    let len = js::hypot(bx, by);
    if len <= EDGE_EPSILON {
        return 0;
    }
    let baseline = Point { x: bx / len, y: by / len };
    // Handedness from the actual ascent edge (lower-left to upper-left), so
    // mirrored text gets its own frame with "up" on its true ascent side.
    let ax = q.upper_left.x - q.lower_left.x;
    let ay = q.upper_left.y - q.lower_left.y;
    let sign = if baseline.x * ay - baseline.y * ax >= 0.0 { 1.0 } else { -1.0 };
    let ascent = Point { x: -baseline.y * sign, y: baseline.x * sign };
    for (i, f) in frames.iter().enumerate() {
        let dot = f.baseline.x * baseline.x + f.baseline.y * baseline.y;
        let same_handedness = f.ascent.x * ascent.x + f.ascent.y * ascent.y > 0.0;
        if dot >= frame_dot_tolerance() && same_handedness {
            return i;
        }
    }
    frames.push(TextLayoutFrame { baseline, ascent });
    frames.len() - 1
}

/// `buildPageTextLayout`: upright runs copy their wire boxes verbatim into
/// frame 0; rotated runs project their cells into their cluster's frame.
pub fn build_page_text_layout(snapshot: &PageGeometrySnapshot<PdfCoordinates>) -> PageTextLayout {
    let mut glyphs = Vec::new();
    let mut runs = Vec::new();
    let mut frames = vec![IDENTITY_FRAME];

    for run in &snapshot.runs {
        match run {
            PageGeometryRun::Upright { rect, start, font_size, glyphs: run_glyphs } => {
                for g in run_glyphs {
                    glyphs.push(TextLayoutGlyph {
                        loose: g.loose,
                        tight: g.tight,
                        flags: flags_of(g.space, g.empty),
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: *start,
                    count: run_glyphs.len(),
                    rect: *rect,
                    font_size: *font_size,
                    frame: 0,
                });
            }
            PageGeometryRun::Rotated { start, font_size, glyphs: run_glyphs, .. } => {
                let frame = frame_for_run(&mut frames, run_glyphs);
                let f = frames[frame];
                let mut run_box: Option<PdfRect> = None;
                for g in run_glyphs {
                    if g.empty {
                        glyphs.push(TextLayoutGlyph {
                            loose: ZERO_RECT,
                            tight: None,
                            flags: flags_of(g.space, true),
                        });
                        continue;
                    }
                    let loose = frame_box_of_quad(&f, &g.loose);
                    let tight = g.tight.as_ref().map(|q| frame_box_of_quad(&f, q));
                    glyphs.push(TextLayoutGlyph { loose, tight, flags: flags_of(g.space, false) });
                    run_box = Some(match run_box {
                        Some(b) => PdfRect {
                            left: js::min(b.left, loose.left),
                            bottom: js::min(b.bottom, loose.bottom),
                            right: js::max(b.right, loose.right),
                            top: js::max(b.top, loose.top),
                        },
                        None => loose,
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: *start,
                    count: run_glyphs.len(),
                    rect: run_box.unwrap_or(ZERO_RECT),
                    font_size: *font_size,
                    frame,
                });
            }
        }
    }
    PageTextLayout { glyphs, runs, frames }
}

fn in_rect(r: &PdfRect, p: PdfPoint) -> bool {
    p.x >= r.left && p.x <= r.right && p.y >= r.bottom && p.y <= r.top
}

fn avg_glyph_height(layout: &PageTextLayout) -> f64 {
    let mut total = 0.0;
    let mut count = 0.0;
    for g in &layout.glyphs {
        if is_empty(g.flags) {
            continue;
        }
        total += g.loose.top - g.loose.bottom;
        count += 1.0;
    }
    if count == 0.0 { 0.0 } else { total / count }
}

/// The point in every frame; frame 0 is the point itself.
fn point_per_frame(layout: &PageTextLayout, p: PdfPoint) -> Vec<PdfPoint> {
    layout.frames.iter().enumerate().map(|(i, f)| if i == 0 { p } else { to_frame(f, p) }).collect()
}

/// The TypeScript's default `toleranceFactor`, the one `charAt` uses.
pub const DEFAULT_TOLERANCE_FACTOR: f64 = 1.5;

/// `textGlyphAt`: exact tight-box containment first, then the closest glyph
/// by Manhattan distance within `tolerance_factor` average glyph heights.
/// Of two cells that share an edge, a point as near to both keeps the
/// first, whatever the last bit of rounding says (`EDGE_EPSILON`).
pub fn text_glyph_at(
    layout: &PageTextLayout,
    point: PdfPoint,
    tolerance_factor: f64,
) -> Option<usize> {
    let pts = point_per_frame(layout, point);
    for run in &layout.runs {
        let q = pts[run.frame];
        if !in_rect(&run.rect, q) {
            continue;
        }
        for (index, g) in run_glyphs(layout, run) {
            if in_rect(g.tight.as_ref().unwrap_or(&g.loose), q) {
                return Some(index);
            }
        }
    }
    if tolerance_factor <= 0.0 {
        return None;
    }

    let half = (avg_glyph_height(layout) * tolerance_factor) / 2.0;
    let mut best: Option<usize> = None;
    let mut best_dist = f64::INFINITY;
    for run in &layout.runs {
        let q = pts[run.frame];
        let r = &run.rect;
        if q.x < r.left - half
            || q.x > r.right + half
            || q.y < r.bottom - half
            || q.y > r.top + half
        {
            continue;
        }
        for (index, g) in run_glyphs(layout, run) {
            if is_empty(g.flags) {
                continue;
            }
            let b = g.tight.as_ref().unwrap_or(&g.loose);
            if q.x < b.left - half
                || q.x > b.right + half
                || q.y < b.bottom - half
                || q.y > b.top + half
            {
                continue;
            }
            let dx = js::min((q.x - b.left).abs(), (q.x - b.right).abs());
            let dy = js::min((q.y - b.bottom).abs(), (q.y - b.top).abs());
            if dx + dy < best_dist - EDGE_EPSILON {
                best_dist = dx + dy;
                best = Some(index);
            }
        }
    }
    best
}

/// A run's glyphs with their indices. The runs of a PDFium snapshot tile
/// the glyphs; another host's may not, and the glyphs a run claims past the
/// end are skipped here where the TypeScript reads `undefined` and throws.
fn run_glyphs<'a>(
    layout: &'a PageTextLayout,
    run: &TextLayoutRun,
) -> impl Iterator<Item = (usize, &'a TextLayoutGlyph)> {
    let end = run.char_start.saturating_add(run.count).min(layout.glyphs.len());
    let start = run.char_start.min(end);
    layout.glyphs[start..end].iter().enumerate().map(move |(i, g)| (start + i, g))
}

/// A run's first character, saturating where a host's value passes `i64`.
fn run_start(run: &TextLayoutRun) -> i64 {
    i64::try_from(run.char_start).unwrap_or(i64::MAX)
}

/// One past a run's last character, saturating likewise.
fn run_end(run: &TextLayoutRun) -> i64 {
    i64::try_from(run.char_start.saturating_add(run.count)).unwrap_or(i64::MAX)
}

fn glyph(layout: &PageTextLayout, index: i64) -> Option<&TextLayoutGlyph> {
    usize::try_from(index).ok().and_then(|i| layout.glyphs.get(i))
}

/// `expandTextRangeToWord`: walk to space or empty glyphs both ways.
/// Inclusive `[from, to]`.
pub fn expand_text_range_to_word(layout: &PageTextLayout, glyph_index: i64) -> (i64, i64) {
    let n = layout.glyphs.len() as i64;
    if glyph_index < 0 || glyph_index >= n {
        return (glyph_index, glyph_index);
    }
    let mut from = glyph_index;
    while from > 0 && !is_boundary(layout.glyphs[(from - 1) as usize].flags) {
        from -= 1;
    }
    let mut to = glyph_index;
    while to < n - 1 && !is_boundary(layout.glyphs[(to + 1) as usize].flags) {
        to += 1;
    }
    (from, to)
}

fn run_of(layout: &PageTextLayout, glyph_index: i64) -> Option<usize> {
    layout.runs.iter().position(|r| glyph_index >= run_start(r) && glyph_index < run_end(r))
}

fn is_zero(r: &PdfRect) -> bool {
    r.right == r.left && r.top == r.bottom
}

fn overlap_v(bottom1: f64, top1: f64, bottom2: f64, top2: f64) -> bool {
    let u = js::max(top1, top2) - js::min(bottom1, bottom2);
    if u == 0.0 {
        return false;
    }
    let i = js::max(0.0, js::min(top1, top2) - js::max(bottom1, bottom2));
    i / u >= LINE_OVERLAP_THRESHOLD
}

/// `expandTextRangeToLine`: the same-frame runs whose vertical extent
/// overlaps the anchor run's. Inclusive `[from, to]`.
pub fn expand_text_range_to_line(layout: &PageTextLayout, glyph_index: i64) -> (i64, i64) {
    let Some(ri) = run_of(layout, glyph_index) else { return (glyph_index, glyph_index) };
    let anchor = &layout.runs[ri];
    let bottom = anchor.rect.bottom;
    let top = anchor.rect.top;
    let mut from = run_start(anchor);
    let mut to = run_end(anchor) - 1;
    for run in layout.runs[..ri].iter().rev() {
        if is_zero(&run.rect) {
            continue;
        }
        if run.frame != anchor.frame || !overlap_v(run.rect.bottom, run.rect.top, bottom, top) {
            break;
        }
        from = run_start(run);
    }
    for run in &layout.runs[ri + 1..] {
        if is_zero(&run.rect) {
            continue;
        }
        if run.frame != anchor.frame || !overlap_v(run.rect.bottom, run.rect.top, bottom, top) {
            break;
        }
        to = run_end(run) - 1;
    }
    (from, to)
}

/// A segment's quad for a frame-local rect; frame 0 takes the rect's own
/// corners with no float round trip. The frame is y-up, so the upper corners
/// sit at its top.
fn quad_of_frame_rect(f: &TextLayoutFrame, frame_index: usize, r: &PdfRect) -> Quad {
    let corners = [
        Point { x: r.left, y: r.top },
        Point { x: r.right, y: r.top },
        Point { x: r.left, y: r.bottom },
        Point { x: r.right, y: r.bottom },
    ];
    let [upper_left, upper_right, lower_left, lower_right] =
        if frame_index == 0 { corners } else { corners.map(|p| from_frame(f, p)) };
    Quad { upper_left, upper_right, lower_left, lower_right }
}

/// `textGlyphQuad`: one glyph's oriented cell, or `None` for a glyph with
/// no box.
pub fn text_glyph_quad(layout: &PageTextLayout, glyph_index: i64) -> Option<Quad> {
    let g = glyph(layout, glyph_index)?;
    if is_empty(g.flags) {
        return None;
    }
    let b = g.loose;
    if b.right <= b.left || b.top <= b.bottom {
        return None;
    }
    let frame = run_of(layout, glyph_index).map_or(0, |ri| layout.runs[ri].frame);
    Some(quad_of_frame_rect(&layout.frames[frame], frame, &b))
}

struct SubRun {
    rect: PdfRect,
    char_count: f64,
    font_size: Option<f64>,
    frame: usize,
    first_x: f64,
    last_x: f64,
}

struct MergedSubRun {
    rect: PdfRect,
    frame: usize,
    first_x: f64,
    last_x: f64,
}

/// The sub-run a glyph walk is building.
struct Accumulator {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
    char_count: f64,
    width_sum: f64,
    prev_right: f64,
    first_x: f64,
    last_x: f64,
}

impl Accumulator {
    fn new() -> Self {
        Accumulator {
            left: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::INFINITY,
            top: f64::NEG_INFINITY,
            char_count: 0.0,
            width_sum: 0.0,
            prev_right: f64::NEG_INFINITY,
            first_x: 0.0,
            last_x: 0.0,
        }
    }

    fn flush(&mut self, run: &TextLayoutRun, out: &mut Vec<SubRun>) {
        if self.char_count > 0.0 && self.left != f64::INFINITY {
            out.push(SubRun {
                rect: PdfRect {
                    left: self.left,
                    bottom: self.bottom,
                    right: self.right,
                    top: self.top,
                },
                char_count: self.char_count,
                font_size: run.font_size,
                frame: run.frame,
                first_x: self.first_x,
                last_x: self.last_x,
            });
        }
        // `firstX` and `lastX` survive in the TypeScript's closure, but the
        // next glyph writes both before anything reads them.
        *self = Accumulator::new();
    }
}

/// `textSegmentsForRange`: merged visual-line segments for the half-open
/// character range `[char_start, char_start + char_count)`. Characters
/// outside the layout are ignored.
pub fn text_segments_for_range(
    layout: &PageTextLayout,
    char_start: i64,
    char_count: i64,
) -> Vec<PdfTextSegment<PdfCoordinates>> {
    if char_count <= 0 || layout.glyphs.is_empty() {
        return Vec::new();
    }
    let lo = char_start.max(0);
    let hi = (layout.glyphs.len() as i64 - 1).min(char_start.saturating_add(char_count) - 1);
    if hi < lo {
        return Vec::new();
    }
    let mut sub_runs: Vec<SubRun> = Vec::new();

    for run in &layout.runs {
        let run_end = run_end(run) - 1;
        if run_end < lo || run_start(run) > hi {
            continue;
        }
        let s = lo.max(run_start(run));
        let e = hi.min(run_end);

        let mut acc = Accumulator::new();
        for ci in s..=e {
            let g = &layout.glyphs[ci as usize];
            if is_empty(g.flags) {
                continue;
            }
            let b = g.loose;
            if acc.char_count > 0.0 && acc.prev_right > f64::NEG_INFINITY {
                let avg = acc.width_sum / acc.char_count;
                if avg > 0.0 && (b.left - acc.prev_right).abs() > CHAR_DISTANCE_FACTOR * avg {
                    acc.flush(run, &mut sub_runs);
                }
            }
            let center_x = (b.left + b.right) / 2.0;
            if acc.char_count == 0.0 {
                acc.first_x = center_x;
            }
            acc.last_x = center_x;
            acc.left = js::min(acc.left, b.left);
            acc.right = js::max(acc.right, b.right);
            acc.bottom = js::min(acc.bottom, b.bottom);
            acc.top = js::max(acc.top, b.top);
            acc.char_count += 1.0;
            acc.width_sum += b.right - b.left;
            acc.prev_right = b.right;
        }
        acc.flush(run, &mut sub_runs);
    }

    merge_adjacent_sub_runs(&sub_runs).iter().map(|m| materialize_segment(layout, m)).collect()
}

fn merge_adjacent_sub_runs(runs: &[SubRun]) -> Vec<MergedSubRun> {
    let mut out = Vec::new();
    let mut prev: Option<&SubRun> = None;
    let mut cur: Option<MergedSubRun> = None;
    for run in runs {
        match (prev, cur.take()) {
            (Some(p), Some(c)) if p.frame == run.frame && should_merge(p, run) => {
                cur = Some(MergedSubRun {
                    rect: PdfRect {
                        left: js::min(c.rect.left, run.rect.left),
                        bottom: js::min(c.rect.bottom, run.rect.bottom),
                        right: js::max(c.rect.right, run.rect.right),
                        top: js::max(c.rect.top, run.rect.top),
                    },
                    frame: c.frame,
                    first_x: c.first_x,
                    last_x: run.last_x,
                });
            }
            (_, previous) => {
                if let Some(c) = previous {
                    out.push(c);
                }
                cur = Some(MergedSubRun {
                    rect: run.rect,
                    frame: run.frame,
                    first_x: run.first_x,
                    last_x: run.last_x,
                });
            }
        }
        prev = Some(run);
    }
    if let Some(c) = cur
        && c.rect.right > c.rect.left
        && c.rect.top > c.rect.bottom
    {
        out.push(c);
    }
    out
}

fn materialize_segment(
    layout: &PageTextLayout,
    m: &MergedSubRun,
) -> PdfTextSegment<PdfCoordinates> {
    let frame = &layout.frames[m.frame];
    let quad = quad_of_frame_rect(frame, m.frame, &m.rect);
    let rect = if m.frame == 0 {
        m.rect
    } else {
        let xs = [quad.upper_left.x, quad.upper_right.x, quad.lower_left.x, quad.lower_right.x];
        let ys = [quad.upper_left.y, quad.upper_right.y, quad.lower_left.y, quad.lower_right.y];
        PdfRect {
            left: js::min_of(&xs),
            bottom: js::min_of(&ys),
            right: js::max_of(&xs),
            top: js::max_of(&ys),
        }
    };
    PdfTextSegment { quad, rect, advance: if m.last_x >= m.first_x { 1 } else { -1 } }
}

fn should_merge(a: &SubRun, b: &SubRun) -> bool {
    if let (Some(fa), Some(fb)) = (a.font_size, b.font_size)
        && fa > 0.0
        && fb > 0.0
    {
        let ratio = js::max(fa, fb) / js::min(fa, fb);
        if ratio > FONT_SIZE_RATIO_THRESHOLD {
            return false;
        }
    }
    if vertical_overlap(&a.rect, &b.rect) < VERTICAL_OVERLAP_THRESHOLD {
        return false;
    }
    let aw = (a.rect.right - a.rect.left) / a.char_count;
    let bw = (b.rect.right - b.rect.left) / b.char_count;
    let a_l = a.rect.left - aw;
    let a_r = a.rect.right + aw;
    let b_l = b.rect.left - bw;
    let b_r = b.rect.right + bw;
    a_l < b_r && a_r > b_l
}

fn vertical_overlap(a: &PdfRect, b: &PdfRect) -> f64 {
    let ah = a.top - a.bottom;
    let bh = b.top - b.bottom;
    if ah <= 0.0 || bh <= 0.0 {
        return 0.0;
    }
    let u = js::max(a.top, b.top) - js::min(a.bottom, b.bottom);
    if u == ah || u == bh {
        return 1.0;
    }
    let i = js::max(0.0, js::min(a.top, b.top) - js::max(a.bottom, b.bottom));
    i / u
}

// ── the copy primitive (`text/charmap.ts`) ────────────────────────────────────

/// `clampInt`: `Math.min(Math.max(Math.floor(value), lo), hi)`.
fn clamp_int(value: f64, lo: f64, hi: f64) -> f64 {
    js::min(js::max(value.floor(), lo), hi)
}

fn anchor_at_or_before(map: &[(f64, f64)], boundary: f64) -> (f64, f64) {
    let mut lo: i64 = 0;
    let mut hi: i64 = map.len() as i64 - 1;
    let mut best = None;
    while lo <= hi {
        let mid = (lo + hi) >> 1;
        if map[mid as usize].0 <= boundary {
            best = Some(map[mid as usize]);
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    best.unwrap_or((0.0, 0.0))
}

fn utf16_len(text: &str) -> f64 {
    text.encode_utf16().count() as f64
}

/// `boundaryTextOffset`: the UTF-16 text offset of a character boundary.
pub fn boundary_text_offset(snapshot: &PageTextSnapshot, boundary: f64) -> f64 {
    let b = clamp_int(boundary, 0.0, snapshot.char_count);
    match snapshot.char_map.as_deref() {
        None | Some([]) => js::min(b, utf16_len(&snapshot.text)),
        Some(map) => {
            let (char, text) = anchor_at_or_before(map, b);
            text + (b - char)
        }
    }
}

/// `String.prototype.slice` over UTF-16 code units.
fn slice_utf16(text: &str, start: f64, end: f64) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let len = units.len() as f64;
    let at = |v: f64| {
        let v = if v.is_nan() { 0.0 } else { v.trunc() };
        let v = if v < 0.0 { js::max(len + v, 0.0) } else { js::min(v, len) };
        v as usize
    };
    let (from, to) = (at(start), at(end));
    if from >= to { String::new() } else { String::from_utf16_lossy(&units[from..to]) }
}

/// `sliceText(snapshot, { start, count })`: the text of a character range.
/// Dropped characters inside it contribute nothing; a supplementary
/// character contributes its whole surrogate pair.
pub fn slice_text(snapshot: &PageTextSnapshot, start: f64, count: f64) -> String {
    if count <= 0.0 {
        return String::new();
    }
    slice_utf16(
        &snapshot.text,
        boundary_text_offset(snapshot, start),
        boundary_text_offset(snapshot, start + count),
    )
}
