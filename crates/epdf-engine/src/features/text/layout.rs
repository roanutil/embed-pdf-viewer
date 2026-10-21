//! Where glyph geometry becomes hit targets, word and line ranges, and
//! merged visual segments.
//!
//! G5. A transcription of `core/src/text/layout.ts`, which stays in
//! TypeScript because web selection plugins consume it. A deliberate
//! duplicate, pinned by `tooling/engine-conformance/vectors/text/layout.json`.
//!
//! Selection owns gestures, search owns matching and cursors, and neither
//! owns segmentation. Both consume this, so a text range has one canonical
//! segmentation however it was produced.
//!
//! Everything here is PDF user space, y-up, as the engine reads it. The
//! public layout, [`crate::page_space::text::create_text_layout`], runs this
//! one on page-space geometry flipped top to bottom and flips its answers
//! back.
//!
//! The line merge is adapted from Chromium's `pdf/pdfium/pdfium_range.cc`
//! `MergeAdjacentRects` (BSD, © 2010 The Chromium Authors); hit testing
//! mirrors PDFium's `GetIndexAtPos`, exact box first and then a tolerance
//! pass.
//!
//! # Frames
//!
//! Every run belongs to a frame. Frame 0 is page space and holds every upright
//! run, which is the dominant case and takes a copying fast path. Each rotated
//! orientation cluster gets an orthonormal frame in which its text reads
//! upright, derived from the semantic edges of the first classifiable glyph's
//! quad rather than from any advisory wire field.
//!
//! Shear is deliberately not part of the frame key. A sheared run shares the
//! frame of its unsheared neighbours, so a line mixing roman and italic merges
//! into one segment, and the shear is absorbed as a small in-frame residue. An
//! affine basis that preserved shear would either fragment mixed lines or make
//! the roman body's geometry depend on whether an italic run came first.

use super::TextRange;
use crate::features::geometry::{PageGeometryRun, PageGeometrySnapshot};
use crate::geom::{PdfPoint, PdfQuad, PdfRect};
use crate::page_space::{Coordinates, PdfCoordinates};

const FLAG_SPACE: u32 = 1;
const FLAG_EMPTY: u32 = 2;

fn is_boundary(flags: u32) -> bool {
    flags & (FLAG_SPACE | FLAG_EMPTY) != 0
}

fn is_empty_glyph(flags: u32) -> bool {
    flags & FLAG_EMPTY != 0
}

/// A geometry glyph's `space` and `empty` states as this layout's flag bits.
fn flags_of(space: bool, empty: bool) -> u32 {
    (if space { FLAG_SPACE } else { 0 }) | (if empty { FLAG_EMPTY } else { 0 })
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

/// How far from a glyph a point may be and still hit it, in average glyph
/// heights: the TypeScript's default `toleranceFactor`.
pub const DEFAULT_TOLERANCE_FACTOR: f64 = 1.5;

/// Where a word or line is asked for: at a point, or around a character.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAt<P> {
    Point(P),
    Char(u32),
}

impl<P> From<u32> for TextAt<P> {
    fn from(index: u32) -> Self {
        Self::Char(index)
    }
}

impl From<PdfPoint> for TextAt<PdfPoint> {
    fn from(point: PdfPoint) -> Self {
        Self::Point(point)
    }
}

/// Where a page's text is, in the file's coordinates
/// (`createPdfTextLayout`), for the engine's own readers. Everything else
/// uses the page-space layout,
/// [`crate::page_space::text::create_text_layout`].
#[derive(Debug, Clone, PartialEq)]
pub struct PdfTextLayout {
    runs: Vec<PageGeometryRun>,
    layout: PageTextLayout,
}

impl PdfTextLayout {
    /// The layout of a page's geometry (`createPdfTextLayout`).
    pub fn new(snapshot: PageGeometrySnapshot) -> Self {
        let layout = build_page_text_layout(&snapshot);
        Self { runs: snapshot.runs, layout }
    }

    /// How many characters the page has: they are numbered 0 to
    /// `char_count - 1`.
    pub fn char_count(&self) -> u32 {
        self.layout.glyphs.len() as u32
    }

    /// The raw geometry it was built from.
    pub fn runs(&self) -> &[PageGeometryRun] {
        &self.runs
    }

    /// The flattened layout, frames and all.
    pub fn layout(&self) -> &PageTextLayout {
        &self.layout
    }

    /// The character at a point, or `None` when there's no text near it.
    pub fn char_at(&self, point: PdfPoint) -> Option<u32> {
        text_glyph_at(&self.layout, point, DEFAULT_TOLERANCE_FACTOR)
    }

    /// The word at a point or around a character, or `None` when there is
    /// none.
    pub fn word_at(&self, at: impl Into<TextAt<PdfPoint>>) -> Option<TextRange> {
        let index = self.index_of(at.into())?;
        Some(range_of(expand_text_range_to_word(&self.layout, index)))
    }

    /// The visual line at a point or around a character, or `None` when there
    /// is none.
    pub fn line_at(&self, at: impl Into<TextAt<PdfPoint>>) -> Option<TextRange> {
        let index = self.index_of(at.into())?;
        Some(range_of(expand_text_range_to_line(&self.layout, index)))
    }

    /// One segment per visual line the range covers.
    pub fn segments(&self, range: TextRange) -> Vec<PdfTextSegment> {
        text_segments_for_range(&self.layout, range.start, range.count)
    }

    /// One character's loose cell, or `None` for a character with no box.
    pub fn char_quad(&self, index: u32) -> Option<PdfQuad> {
        text_glyph_quad(&self.layout, index)
    }

    fn index_of(&self, at: TextAt<PdfPoint>) -> Option<u32> {
        match at {
            TextAt::Point(point) => self.char_at(point),
            TextAt::Char(index) => (index < self.char_count()).then_some(index),
        }
    }
}

/// An inclusive `[from, to]` pair as a [`TextRange`].
fn range_of((from, to): (u32, u32)) -> TextRange {
    TextRange { start: from, count: to - from + 1 }
}

const ZERO_RECT: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };

/// An orientation cluster's orthonormal basis in page space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLayoutFrame {
    /// Unit vector along the reading baseline, the frame's +x.
    pub baseline: PdfPoint,
    /// Unit vector toward the ascent side, the frame's +y, perpendicular to
    /// the baseline.
    pub ascent: PdfPoint,
}

const IDENTITY_FRAME: TextLayoutFrame =
    TextLayoutFrame { baseline: PdfPoint { x: 1.0, y: 0.0 }, ascent: PdfPoint { x: 0.0, y: 1.0 } };

fn to_frame(f: &TextLayoutFrame, p: PdfPoint) -> PdfPoint {
    PdfPoint { x: p.x * f.baseline.x + p.y * f.baseline.y, y: p.x * f.ascent.x + p.y * f.ascent.y }
}

fn from_frame(f: &TextLayoutFrame, p: PdfPoint) -> PdfPoint {
    PdfPoint { x: f.baseline.x * p.x + f.ascent.x * p.y, y: f.baseline.y * p.x + f.ascent.y * p.y }
}

/// One glyph in its frame's space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLayoutGlyph {
    /// Builds segments.
    pub loose: PdfRect,
    /// Hit-tests. Absent for glyphs with no outline.
    pub tight: Option<PdfRect>,
    pub flags: u32,
}

/// A contiguous run of glyphs, one text object, in one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLayoutRun {
    /// First char index, which is the flat glyph index because runs tile the
    /// page.
    pub char_start: u32,
    pub count: u32,
    /// Frame-local loose box, which is page space itself in frame 0.
    pub rect: PdfRect,
    pub font_size: Option<f64>,
    /// Index into [`PageTextLayout::frames`]. Zero is page space.
    pub frame: usize,
}

/// A page's text laid out per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PageTextLayout {
    /// Flat, index-aligned with the text page's char indices.
    pub glyphs: Vec<TextLayoutGlyph>,
    pub runs: Vec<TextLayoutRun>,
    pub frames: Vec<TextLayoutFrame>,
}

/// One merged visual line of a text range (`PdfTextSegment<C>`), in the
/// file's coordinates or in page space.
///
/// `quad` is the geometric authority, its corners named in the line's own
/// frame: upper start, upper end, lower start, lower end. Those are visual
/// semantics and not reading order. `rect` is its axis-aligned bounds from
/// the same constructor, and `advance` is the reading direction along the
/// baseline, taken from the glyph sequence rather than inferred from
/// geometry.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(bound = "")]
pub struct PdfTextSegment<C: Coordinates = PdfCoordinates> {
    pub quad: C::Quad,
    pub rect: C::Rect,
    pub advance: i8,
}

/// Frame-local bounds of a page-space cell.
///
/// Exact under the frame's rotation; shear shows up as the documented
/// in-frame envelope rather than being represented.
fn frame_box_of_quad(f: &TextLayoutFrame, q: PdfQuad) -> PdfRect {
    let pts = [to_frame(f, q.p1), to_frame(f, q.p2), to_frame(f, q.p3), to_frame(f, q.p4)];
    let xs = pts.map(|p| p.x);
    let ys = pts.map(|p| p.y);
    PdfRect {
        left: xs.iter().copied().fold(f64::INFINITY, f64::min),
        bottom: ys.iter().copied().fold(f64::INFINITY, f64::min),
        right: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        top: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

/// Find or create the frame for a rotated run.
///
/// Keyed by baseline direction and ascent handedness, both taken from the
/// semantic edges of the first classifiable glyph's quad. Handedness comes
/// from the actual ascent edge so mirrored text gets its own frame with "up"
/// on its true ascent side.
fn frame_for_run(frames: &mut Vec<TextLayoutFrame>, run: &PageGeometryRun) -> usize {
    let PageGeometryRun::Rotated { glyphs, .. } = run else {
        return 0;
    };
    let Some(seed) = glyphs.iter().find(|g| !g.empty) else {
        return 0;
    };
    let q = PdfQuad::from(seed.loose);
    let bx = q.p2.x - q.p1.x;
    let by = q.p2.y - q.p1.y;
    let len = bx.hypot(by);
    if len <= EDGE_EPSILON {
        return 0;
    }
    let baseline = PdfPoint { x: bx / len, y: by / len };
    let ax = q.p1.x - q.p3.x;
    let ay = q.p1.y - q.p3.y;
    let sign = if baseline.x * ay - baseline.y * ax >= 0.0 { 1.0 } else { -1.0 };
    let ascent = PdfPoint { x: -baseline.y * sign, y: baseline.x * sign };

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

/// Flatten a geometry snapshot into the canonical layout.
///
/// Upright runs copy their wire boxes verbatim into frame 0, which is
/// byte-identical to the pre-orientation behaviour. Rotated runs project their
/// glyph cells into their cluster's frame.
pub fn build_page_text_layout(snapshot: &PageGeometrySnapshot) -> PageTextLayout {
    let mut glyphs = Vec::new();
    let mut runs = Vec::new();
    let mut frames = vec![IDENTITY_FRAME];

    for run in &snapshot.runs {
        match run {
            PageGeometryRun::Upright { rect, start, glyphs: run_glyphs, font_size } => {
                for g in run_glyphs {
                    glyphs.push(TextLayoutGlyph {
                        loose: g.loose,
                        tight: g.tight,
                        flags: flags_of(g.space, g.empty),
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: *start,
                    count: run_glyphs.len() as u32,
                    rect: *rect,
                    font_size: *font_size,
                    frame: 0,
                });
            }
            PageGeometryRun::Rotated { start, glyphs: run_glyphs, font_size, .. } => {
                let frame_index = frame_for_run(&mut frames, run);
                let f = frames[frame_index];
                let mut run_box: Option<PdfRect> = None;
                for g in run_glyphs {
                    let flags = flags_of(g.space, g.empty);
                    if g.empty {
                        glyphs.push(TextLayoutGlyph { loose: ZERO_RECT, tight: None, flags });
                        continue;
                    }
                    let loose = frame_box_of_quad(&f, g.loose.into());
                    let tight = g.tight.map(|q| frame_box_of_quad(&f, q.into()));
                    glyphs.push(TextLayoutGlyph { loose, tight, flags });
                    run_box = Some(match run_box {
                        Some(b) => PdfRect {
                            left: b.left.min(loose.left),
                            bottom: b.bottom.min(loose.bottom),
                            right: b.right.max(loose.right),
                            top: b.top.max(loose.top),
                        },
                        None => loose,
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: *start,
                    count: run_glyphs.len() as u32,
                    rect: run_box.unwrap_or(ZERO_RECT),
                    font_size: *font_size,
                    frame: frame_index,
                });
            }
        }
    }

    PageTextLayout { glyphs, runs, frames }
}

fn in_rect(r: PdfRect, p: PdfPoint) -> bool {
    p.x >= r.left && p.x <= r.right && p.y >= r.bottom && p.y <= r.top
}

fn avg_glyph_height(layout: &PageTextLayout) -> f64 {
    let mut total = 0.0;
    let mut count = 0usize;
    for g in &layout.glyphs {
        if is_empty_glyph(g.flags) {
            continue;
        }
        total += g.loose.top - g.loose.bottom;
        count += 1;
    }
    if count == 0 { 0.0 } else { total / count as f64 }
}

/// The point expressed in every frame, index-aligned with the layout's.
///
/// Frame 0 returns the point itself, so the dominant case takes no float
/// round trip.
fn point_per_frame(layout: &PageTextLayout, p: PdfPoint) -> Vec<PdfPoint> {
    layout.frames.iter().enumerate().map(|(i, f)| if i == 0 { p } else { to_frame(f, p) }).collect()
}

/// The glyph index at a page-space point, or `None` when nothing is near.
///
/// `None` is what lets a caller show the pointer cursor off text. Exact
/// containment of the tight box first, then a tolerance pass taking the
/// closest by Manhattan distance within `tolerance_factor` times the average
/// glyph height. Each run tests the point in its own frame, so rotated text
/// hit-tests exactly rather than through an envelope.
pub fn text_glyph_at(
    layout: &PageTextLayout,
    point: PdfPoint,
    tolerance_factor: f64,
) -> Option<u32> {
    let pts = point_per_frame(layout, point);
    for run in &layout.runs {
        let q = pts[run.frame];
        if !in_rect(run.rect, q) {
            continue;
        }
        for i in 0..run.count {
            let g = &layout.glyphs[(run.char_start + i) as usize];
            if in_rect(g.tight.unwrap_or(g.loose), q) {
                return Some(run.char_start + i);
            }
        }
    }
    if tolerance_factor <= 0.0 {
        return None;
    }

    let half = avg_glyph_height(layout) * tolerance_factor / 2.0;
    let mut best: Option<u32> = None;
    let mut best_dist = f64::INFINITY;
    for run in &layout.runs {
        let q = pts[run.frame];
        let r = run.rect;
        if q.x < r.left - half
            || q.x > r.right + half
            || q.y < r.bottom - half
            || q.y > r.top + half
        {
            continue;
        }
        for i in 0..run.count {
            let g = &layout.glyphs[(run.char_start + i) as usize];
            if is_empty_glyph(g.flags) {
                continue;
            }
            let b = g.tight.unwrap_or(g.loose);
            if q.x < b.left - half
                || q.x > b.right + half
                || q.y < b.bottom - half
                || q.y > b.top + half
            {
                continue;
            }
            let dx = (q.x - b.left).abs().min((q.x - b.right).abs());
            let dy = (q.y - b.bottom).abs().min((q.y - b.top).abs());
            // Neighbours share an edge, so a point nearest to it is as near
            // to both: the first one wins, whatever the last bit of rounding
            // says.
            if dx + dy < best_dist - EDGE_EPSILON {
                best_dist = dx + dy;
                best = Some(run.char_start + i);
            }
        }
    }
    best
}

/// The word around a glyph, walking to a boundary glyph both ways.
///
/// INCLUSIVE `[from, to]`, not half-open. That is the TypeScript's shape and
/// changing it here would make every caller convert.
pub fn expand_text_range_to_word(layout: &PageTextLayout, glyph: u32) -> (u32, u32) {
    let n = layout.glyphs.len() as u32;
    if glyph >= n {
        return (glyph, glyph);
    }
    let mut from = glyph;
    while from > 0 && !is_boundary(layout.glyphs[(from - 1) as usize].flags) {
        from -= 1;
    }
    let mut to = glyph;
    while to < n - 1 && !is_boundary(layout.glyphs[(to + 1) as usize].flags) {
        to += 1;
    }
    (from, to)
}

/// The full visual line: same-frame runs whose vertical extent overlaps the
/// anchor run's. A differently oriented run is a line boundary.
///
/// INCLUSIVE, as [`expand_text_range_to_word`] is.
pub fn expand_text_range_to_line(layout: &PageTextLayout, glyph: u32) -> (u32, u32) {
    let Some(ri) =
        layout.runs.iter().position(|r| glyph >= r.char_start && glyph < r.char_start + r.count)
    else {
        return (glyph, glyph);
    };
    let anchor = layout.runs[ri];
    let (bottom, top) = (anchor.rect.bottom, anchor.rect.top);
    let mut from = anchor.char_start;
    let mut to = anchor.char_start + anchor.count - 1;

    for run in layout.runs[..ri].iter().rev() {
        if is_zero(run.rect) {
            continue;
        }
        if run.frame != anchor.frame || !overlap_v(run.rect.bottom, run.rect.top, bottom, top) {
            break;
        }
        from = run.char_start;
    }
    for run in &layout.runs[ri + 1..] {
        if is_zero(run.rect) {
            continue;
        }
        if run.frame != anchor.frame || !overlap_v(run.rect.bottom, run.rect.top, bottom, top) {
            break;
        }
        to = run.char_start + run.count - 1;
    }
    (from, to)
}

/// A single glyph's oriented cell in page space, or `None` for a degenerate
/// one.
///
/// The anchor for caret placement at a selection boundary, which is why a
/// degenerate glyph reports nothing rather than a zero-area quad somebody
/// would draw.
pub fn text_glyph_quad(layout: &PageTextLayout, glyph: u32) -> Option<PdfQuad> {
    let g = layout.glyphs.get(glyph as usize)?;
    if is_empty_glyph(g.flags) {
        return None;
    }
    let b = g.loose;
    if b.right <= b.left || b.top <= b.bottom {
        return None;
    }
    let frame_index = layout
        .runs
        .iter()
        .find(|r| glyph >= r.char_start && glyph < r.char_start + r.count)
        .map_or(0, |r| r.frame);
    Some(quad_of_frame_rect(&layout.frames[frame_index], frame_index, b))
}

#[derive(Debug, Clone, Copy)]
struct SubRun {
    rect: PdfRect,
    char_count: u32,
    font_size: Option<f64>,
    frame: usize,
    /// Baseline x of the first and last glyph in sequence order, in frame
    /// space. The reading-direction signal.
    first_x: f64,
    last_x: f64,
}

#[derive(Debug, Clone, Copy)]
struct MergedSubRun {
    rect: PdfRect,
    frame: usize,
    first_x: f64,
    last_x: f64,
}

/// Merged visual-line segments for the half-open char range. The canonical
/// segmentation.
///
/// Chars outside the layout are ignored rather than refused, because a
/// selection dragged past the end of a page is a normal gesture.
pub fn text_segments_for_range(
    layout: &PageTextLayout,
    char_start: u32,
    char_count: u32,
) -> Vec<PdfTextSegment> {
    if char_count == 0 || layout.glyphs.is_empty() {
        return Vec::new();
    }
    let lo = char_start;
    let hi = (layout.glyphs.len() as u32 - 1).min(char_start.saturating_add(char_count - 1));
    if hi < lo {
        return Vec::new();
    }

    let mut sub_runs: Vec<SubRun> = Vec::new();
    for run in &layout.runs {
        let run_end = run.char_start + run.count - 1;
        if run_end < lo || run.char_start > hi {
            continue;
        }
        let s = lo.max(run.char_start);
        let e = hi.min(run_end);

        let mut acc = Accumulator::new();
        for ci in s..=e {
            let g = &layout.glyphs[ci as usize];
            if is_empty_glyph(g.flags) {
                continue;
            }
            let b = g.loose;
            // A gap wider than a few average advances means the two sides are
            // not one visual run, whatever the text object says.
            if acc.char_count > 0
                && acc.prev_right > f64::NEG_INFINITY
                && acc.avg_width() > 0.0
                && (b.left - acc.prev_right).abs() > CHAR_DISTANCE_FACTOR * acc.avg_width()
            {
                acc.flush(&mut sub_runs, run);
            }
            acc.push(b);
        }
        acc.flush(&mut sub_runs, run);
    }

    merge_adjacent_sub_runs(&sub_runs).into_iter().map(|m| materialize_segment(layout, m)).collect()
}

/// The running box of one sub-run being accumulated.
struct Accumulator {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
    char_count: u32,
    width_sum: f64,
    prev_right: f64,
    first_x: f64,
    last_x: f64,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            left: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::INFINITY,
            top: f64::NEG_INFINITY,
            char_count: 0,
            width_sum: 0.0,
            prev_right: f64::NEG_INFINITY,
            first_x: 0.0,
            last_x: 0.0,
        }
    }

    fn avg_width(&self) -> f64 {
        self.width_sum / f64::from(self.char_count)
    }

    fn push(&mut self, b: PdfRect) {
        let center_x = (b.left + b.right) / 2.0;
        if self.char_count == 0 {
            self.first_x = center_x;
        }
        self.last_x = center_x;
        self.left = self.left.min(b.left);
        self.right = self.right.max(b.right);
        self.bottom = self.bottom.min(b.bottom);
        self.top = self.top.max(b.top);
        self.char_count += 1;
        self.width_sum += b.right - b.left;
        self.prev_right = b.right;
    }

    fn flush(&mut self, out: &mut Vec<SubRun>, run: &TextLayoutRun) {
        if self.char_count > 0 && self.left.is_finite() {
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
        let first_x = self.first_x;
        let last_x = self.last_x;
        *self = Self::new();
        // `firstX`/`lastX` are not reset by the TypeScript's flush, and the
        // difference is observable when a flush is followed by no further
        // glyph. Carried deliberately rather than tidied.
        self.first_x = first_x;
        self.last_x = last_x;
    }
}

fn merge_adjacent_sub_runs(runs: &[SubRun]) -> Vec<MergedSubRun> {
    let mut out: Vec<MergedSubRun> = Vec::new();
    let mut prev: Option<SubRun> = None;
    let mut cur: Option<MergedSubRun> = None;

    for run in runs {
        // Frames are canonical per cluster, so index equality IS the
        // same-orientation test: every participant was projected through the
        // same frame, which is what keeps merged coordinates commensurable.
        let mergeable = match (prev, cur) {
            (Some(p), Some(_)) => p.frame == run.frame && should_merge(&p, run),
            _ => false,
        };
        if mergeable {
            let c = cur.expect("checked above");
            cur = Some(MergedSubRun {
                rect: PdfRect {
                    left: c.rect.left.min(run.rect.left),
                    bottom: c.rect.bottom.min(run.rect.bottom),
                    right: c.rect.right.max(run.rect.right),
                    top: c.rect.top.max(run.rect.top),
                },
                frame: c.frame,
                first_x: c.first_x,
                last_x: run.last_x,
            });
        } else {
            if let Some(c) = cur {
                out.push(c);
            }
            cur = Some(MergedSubRun {
                rect: run.rect,
                frame: run.frame,
                first_x: run.first_x,
                last_x: run.last_x,
            });
        }
        prev = Some(*run);
    }
    // The last one is dropped when it has no area, unlike the ones pushed
    // above. That asymmetry is the TypeScript's and is reproduced rather than
    // corrected.
    if let Some(c) = cur
        && c.rect.right > c.rect.left
        && c.rect.top > c.rect.bottom
    {
        out.push(c);
    }
    out
}

/// A segment quad for a frame-local rect.
///
/// Slot order is upper start, upper end, lower start, lower end. The frame is
/// y-up, so the upper corners sit at the frame's top. Frame 0 uses the rect's
/// own corners, with no float round trip.
fn quad_of_frame_rect(f: &TextLayoutFrame, frame_index: usize, r: PdfRect) -> PdfQuad {
    if frame_index == 0 {
        return PdfQuad {
            p1: PdfPoint { x: r.left, y: r.top },
            p2: PdfPoint { x: r.right, y: r.top },
            p3: PdfPoint { x: r.left, y: r.bottom },
            p4: PdfPoint { x: r.right, y: r.bottom },
        };
    }
    PdfQuad {
        p1: from_frame(f, PdfPoint { x: r.left, y: r.top }),
        p2: from_frame(f, PdfPoint { x: r.right, y: r.top }),
        p3: from_frame(f, PdfPoint { x: r.left, y: r.bottom }),
        p4: from_frame(f, PdfPoint { x: r.right, y: r.bottom }),
    }
}

fn materialize_segment(layout: &PageTextLayout, m: MergedSubRun) -> PdfTextSegment {
    let frame = layout.frames[m.frame];
    let quad = quad_of_frame_rect(&frame, m.frame, m.rect);
    let rect = if m.frame == 0 {
        m.rect
    } else {
        let xs = [quad.p1.x, quad.p2.x, quad.p3.x, quad.p4.x];
        let ys = [quad.p1.y, quad.p2.y, quad.p3.y, quad.p4.y];
        PdfRect {
            left: xs.iter().copied().fold(f64::INFINITY, f64::min),
            bottom: ys.iter().copied().fold(f64::INFINITY, f64::min),
            right: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            top: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    };
    PdfTextSegment { quad: quad.into(), rect, advance: if m.last_x >= m.first_x { 1 } else { -1 } }
}

fn should_merge(a: &SubRun, b: &SubRun) -> bool {
    if let (Some(af), Some(bf)) = (a.font_size, b.font_size)
        && af > 0.0
        && bf > 0.0
    {
        let ratio = af.max(bf) / af.min(bf);
        if ratio > FONT_SIZE_RATIO_THRESHOLD {
            return false;
        }
    }
    if vertical_overlap(a.rect, b.rect) < VERTICAL_OVERLAP_THRESHOLD {
        return false;
    }
    // Each side is widened by its own average advance, so two boxes that are
    // one space apart still count as adjacent.
    let aw = (a.rect.right - a.rect.left) / f64::from(a.char_count);
    let bw = (b.rect.right - b.rect.left) / f64::from(b.char_count);
    let a_l = a.rect.left - aw;
    let a_r = a.rect.right + aw;
    let b_l = b.rect.left - bw;
    let b_r = b.rect.right + bw;
    a_l < b_r && a_r > b_l
}

fn vertical_overlap(a: PdfRect, b: PdfRect) -> f64 {
    let ah = a.top - a.bottom;
    let bh = b.top - b.bottom;
    if ah <= 0.0 || bh <= 0.0 {
        return 0.0;
    }
    let u = a.top.max(b.top) - a.bottom.min(b.bottom);
    if u == ah || u == bh {
        return 1.0;
    }
    let i = 0.0f64.max(a.top.min(b.top) - a.bottom.max(b.bottom));
    i / u
}

fn is_zero(r: PdfRect) -> bool {
    r.right == r.left && r.top == r.bottom
}

fn overlap_v(bottom1: f64, top1: f64, bottom2: f64, top2: f64) -> bool {
    let u = top1.max(top2) - bottom1.min(bottom2);
    if u == 0.0 {
        return false;
    }
    let i = 0.0f64.max(top1.min(top2) - bottom1.max(bottom2));
    i / u >= LINE_OVERLAP_THRESHOLD
}

/// `test/text/layout.test.ts`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::geometry::{PageGeometryGlyph, RotatedGeometryGlyph};
    use crate::geom::NamedPdfQuad;

    // ── fixture builders ──

    #[derive(Default)]
    struct GlyphOpts {
        w: Option<f64>,
        bottom: Option<f64>,
        space: bool,
        empty: bool,
    }

    fn upright_glyph(x: f64, opts: GlyphOpts) -> PageGeometryGlyph {
        let bottom = opts.bottom.unwrap_or(100.0);
        PageGeometryGlyph {
            loose: PdfRect {
                left: x,
                right: x + opts.w.unwrap_or(10.0),
                bottom,
                top: bottom + 10.0,
            },
            tight: None,
            space: opts.space,
            empty: opts.empty,
        }
    }

    fn glyph(x: f64) -> PageGeometryGlyph {
        upright_glyph(x, GlyphOpts::default())
    }

    fn upright_run(
        start: u32,
        glyphs: Vec<PageGeometryGlyph>,
        font_size: Option<f64>,
    ) -> PageGeometryRun {
        let rect = glyphs.iter().fold(
            PdfRect {
                left: f64::INFINITY,
                right: f64::NEG_INFINITY,
                bottom: f64::INFINITY,
                top: f64::NEG_INFINITY,
            },
            |acc, g| PdfRect {
                left: acc.left.min(g.loose.left),
                right: acc.right.max(g.loose.right),
                bottom: acc.bottom.min(g.loose.bottom),
                top: acc.top.max(g.loose.top),
            },
        );
        PageGeometryRun::Upright { rect, start, glyphs, font_size }
    }

    struct Oriented {
        count: u32,
        w: f64,
        h: f64,
        rotation: f64,
        shear: f64,
    }

    impl Oriented {
        fn turned(rotation: f64) -> Self {
            Self { count: 3, w: 8.0, h: 12.0, rotation, shear: 0.0 }
        }
    }

    /// A run of glyph cells along an arbitrary baseline. `origin` is the
    /// first cell's lower-left corner, `u` the baseline unit, `n` the ascent
    /// unit.
    fn oriented_run(
        start: u32,
        origin: PdfPoint,
        u: PdfPoint,
        n: PdfPoint,
        opts: Oriented,
    ) -> PageGeometryRun {
        let Oriented { count, w, h, rotation, shear } = opts;
        let at = |t: f64, up: f64| PdfPoint {
            x: origin.x + u.x * t + n.x * up + u.x * shear * (up / h),
            y: origin.y + u.y * t + n.y * up + u.y * shear * (up / h),
        };
        let glyphs: Vec<RotatedGeometryGlyph> = (0..count)
            .map(|i| {
                let t = f64::from(i) * w;
                RotatedGeometryGlyph {
                    loose: NamedPdfQuad {
                        upper_left: at(t, h),
                        upper_right: at(t + w, h),
                        lower_left: at(t, 0.0),
                        lower_right: at(t + w, 0.0),
                    },
                    tight: None,
                    space: false,
                    empty: false,
                }
            })
            .collect();
        let rect = glyphs
            .iter()
            .flat_map(|g| {
                [g.loose.upper_left, g.loose.upper_right, g.loose.lower_left, g.loose.lower_right]
            })
            .fold(
                PdfRect {
                    left: f64::INFINITY,
                    right: f64::NEG_INFINITY,
                    bottom: f64::INFINITY,
                    top: f64::NEG_INFINITY,
                },
                |acc, p| PdfRect {
                    left: acc.left.min(p.x),
                    right: acc.right.max(p.x),
                    bottom: acc.bottom.min(p.y),
                    top: acc.top.max(p.y),
                },
            );
        PageGeometryRun::Rotated {
            rect,
            start,
            glyphs,
            rotation,
            ascent_flip: false,
            font_size: None,
        }
    }

    fn with_start(run: PageGeometryRun, start: u32) -> PageGeometryRun {
        match run {
            PageGeometryRun::Upright { rect, glyphs, font_size, .. } => {
                PageGeometryRun::Upright { rect, start, glyphs, font_size }
            }
            PageGeometryRun::Rotated { rect, glyphs, rotation, ascent_flip, font_size, .. } => {
                PageGeometryRun::Rotated { rect, start, glyphs, rotation, ascent_flip, font_size }
            }
        }
    }

    fn layout_of(runs: Vec<PageGeometryRun>) -> PdfTextLayout {
        PdfTextLayout::new(PageGeometrySnapshot { runs })
    }

    fn range(start: u32, count: u32) -> TextRange {
        TextRange { start, count }
    }

    fn point(x: f64, y: f64) -> PdfPoint {
        PdfPoint { x, y }
    }

    fn rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    fn close(actual: f64, expected: f64, digits: i32) {
        // `toBeCloseTo(expected, digits)`: within half a unit of the last
        // digit.
        let tolerance = 10f64.powi(-digits) / 2.0;
        assert!((actual - expected).abs() < tolerance, "{actual} against {expected}");
    }

    fn expect_rect_equals_bounds(segments: &[PdfTextSegment]) {
        for s in segments {
            let b = frame_box_of_quad(&IDENTITY_FRAME, s.quad.into());
            close(s.rect.left, b.left, 6);
            close(s.rect.right, b.right, 6);
            close(s.rect.bottom, b.bottom, 6);
            close(s.rect.top, b.top, 6);
        }
    }

    const R2: f64 = std::f64::consts::FRAC_1_SQRT_2;

    // ── upright behavior ──

    /// Five 10pt-wide glyphs on one line starting at `x`.
    fn line(start: u32, x: f64, bottom: f64, font_size: Option<f64>) -> PageGeometryRun {
        let glyphs = (0..5)
            .map(|i| {
                upright_glyph(
                    x + f64::from(i) * 10.0,
                    GlyphOpts { bottom: Some(bottom), ..GlyphOpts::default() },
                )
            })
            .collect();
        upright_run(start, glyphs, font_size)
    }

    #[test]
    fn adjacent_text_objects_on_one_line_merge_into_one_segment() {
        let layout = layout_of(vec![line(0, 10.0, 100.0, None), line(5, 60.0, 100.0, None)]);
        let segments = layout.segments(range(0, 10));
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].rect, rect(10.0, 100.0, 110.0, 110.0));
        // Upright quads are the rect's own corners (y up: upper is larger y).
        assert_eq!(segments[0].quad.upper_left, point(10.0, 110.0));
        assert_eq!(segments[0].quad.lower_right, point(110.0, 100.0));
        assert_eq!(segments[0].advance, 1);
        expect_rect_equals_bounds(&segments);
    }

    #[test]
    fn large_intra_run_gaps_split_and_separate_lines_never_merge() {
        let gappy =
            upright_run(0, vec![glyph(10.0), glyph(20.0), glyph(200.0), glyph(210.0)], None);
        let layout = layout_of(vec![gappy, line(4, 10.0, 60.0, None)]);
        assert_eq!(layout.segments(range(0, 9)).len(), 3);
    }

    #[test]
    fn empty_glyphs_contribute_nothing_and_the_range_is_half_open() {
        let layout = layout_of(vec![upright_run(
            0,
            vec![
                glyph(10.0),
                upright_glyph(20.0, GlyphOpts { empty: true, ..GlyphOpts::default() }),
                glyph(30.0),
            ],
            None,
        )]);
        assert!(layout.segments(range(0, 0)).is_empty());
        let first_two = layout.segments(range(0, 2));
        assert_eq!(first_two.len(), 1);
        assert_eq!(first_two[0].rect.right, 20.0, "the empty glyph added nothing");
        assert_eq!(layout.segments(range(0, 3))[0].rect, rect(10.0, 100.0, 40.0, 110.0));
    }

    #[test]
    fn font_size_ratio_and_vertical_overlap_still_gate_merging() {
        let big = upright_run(0, vec![glyph(10.0), glyph(20.0)], Some(20.0));
        let small = upright_run(2, vec![glyph(40.0), glyph(50.0)], Some(8.0));
        assert_eq!(layout_of(vec![big, small]).segments(range(0, 4)).len(), 2);
    }

    #[test]
    fn rtl_glyph_sequences_report_advance_minus_one_with_unchanged_geometry() {
        let narrow = |x| upright_glyph(x, GlyphOpts { w: Some(8.0), ..GlyphOpts::default() });
        let rtl = upright_run(0, vec![glyph(50.0), narrow(42.0), narrow(34.0)], None);
        let segments = layout_of(vec![rtl]).segments(range(0, 3));
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].advance, -1);
        assert_eq!(segments[0].rect.left, 34.0);
    }

    // ── oriented behavior ──

    /// A 90 degree CCW column: baseline +y, ascent -x. Cells x 88..100,
    /// y 20..44.
    fn column(start: u32) -> PageGeometryRun {
        oriented_run(
            start,
            point(100.0, 20.0),
            point(0.0, 1.0),
            point(-1.0, 0.0),
            Oriented::turned(270.0),
        )
    }

    #[test]
    fn a_90_degree_column_is_one_exact_oriented_segment() {
        let segments = layout_of(vec![column(0)]).segments(range(0, 3));
        assert_eq!(segments.len(), 1);
        let q: PdfQuad = segments[0].quad.into();
        close(q.p1.x, 88.0, 6);
        close(q.p1.y, 20.0, 6);
        close(q.p2.x, 88.0, 6);
        close(q.p2.y, 44.0, 6);
        close(q.p3.x, 100.0, 6);
        close(q.p3.y, 20.0, 6);
        close(segments[0].rect.left, 88.0, 6);
        close(segments[0].rect.top, 44.0, 6);
        assert_eq!(segments[0].advance, 1);
        expect_rect_equals_bounds(&segments);
    }

    #[test]
    fn text_at_45_degrees_produces_the_exact_rotated_cell_union_not_an_aabb_blob() {
        let diagonal = oriented_run(
            0,
            point(60.0, 20.0),
            point(R2, R2),
            point(-R2, R2),
            Oriented { count: 4, ..Oriented::turned(315.0) },
        );
        let PageGeometryRun::Rotated { glyphs, .. } = &diagonal else { unreachable!() };
        let (first, last) = (glyphs[0].loose, glyphs[3].loose);
        let segments = layout_of(vec![diagonal.clone()]).segments(range(0, 4));
        assert_eq!(segments.len(), 1);
        let q: PdfQuad = segments[0].quad.into();
        close(q.p1.x, first.upper_left.x, 5);
        close(q.p1.y, first.upper_left.y, 5);
        close(q.p2.x, last.upper_right.x, 5);
        close(q.p2.y, last.upper_right.y, 5);
        close(q.p3.x, first.lower_left.x, 5);
        close(q.p4.x, last.lower_right.x, 5);
        expect_rect_equals_bounds(&segments);
    }

    #[test]
    fn mirrored_text_keeps_semantic_corners_and_forward_advance() {
        // Horizontal mirror: baseline -x, ascent +y. Logical order advances
        // visually leftward; inside its frame that is still +x.
        let mirrored = oriented_run(
            0,
            point(90.0, 100.0),
            point(-1.0, 0.0),
            point(0.0, 1.0),
            Oriented::turned(180.0),
        );
        let segments = layout_of(vec![mirrored]).segments(range(0, 3));
        assert_eq!(segments.len(), 1);
        let q: PdfQuad = segments[0].quad.into();
        // Upper-left sits at the visual right: the mirror is part of the frame.
        close(q.p1.x, 90.0, 6);
        close(q.p2.x, 66.0, 6);
        close(q.p1.y, 112.0, 6);
        assert_eq!(segments[0].advance, 1);
        expect_rect_equals_bounds(&segments);
    }

    #[test]
    fn mixed_roman_and_fake_italic_on_one_line_merge_into_one_segment() {
        let roman = upright_run(0, vec![glyph(10.0), glyph(20.0)], None);
        // A shear-only run (rotation 0): baseline (1, 0) and a sheared ascent.
        // It shares frame 0 with the roman text by design; the shear is
        // in-frame residue.
        let italic = oriented_run(
            2,
            point(30.0, 100.0),
            point(1.0, 0.0),
            point(0.0, 1.0),
            Oriented { count: 2, w: 10.0, h: 10.0, rotation: 0.0, shear: 2.5 },
        );
        let segments = layout_of(vec![roman, italic]).segments(range(0, 4));
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].rect.left, 10.0);
        close(segments[0].rect.right, 52.5, 6); // shear residue included
        expect_rect_equals_bounds(&segments);
    }

    #[test]
    fn differently_oriented_runs_never_merge_and_near_angles_share_one_frame() {
        let line = upright_run(0, vec![glyph(84.0), glyph(94.0)], None);
        let segments = layout_of(vec![line, with_start(column(0), 2)]).segments(range(0, 5));
        assert_eq!(segments.len(), 2);

        let quarter = std::f64::consts::FRAC_PI_4;
        let a = || {
            oriented_run(
                0,
                point(60.0, 20.0),
                point(R2, R2),
                point(-R2, R2),
                Oriented { count: 2, ..Oriented::turned(315.0) },
            )
        };
        let delta = 0.005; // within the 0.5 degree cluster tolerance
        let u2 = point((quarter + delta).cos(), (quarter + delta).sin());
        let n2 = point(-u2.y, u2.x);
        let b = oriented_run(
            2,
            point(71.4, 31.2),
            u2,
            n2,
            Oriented { count: 2, ..Oriented::turned(315.0 - delta.to_degrees()) },
        );
        // One canonical frame: the two runs merge into one line.
        assert_eq!(layout_of(vec![a(), b]).segments(range(0, 4)).len(), 1);

        let far = oriented_run(
            2,
            point(75.0, 35.0),
            point((quarter + 0.05).cos(), (quarter + 0.05).sin()),
            point(-(quarter + 0.05).sin(), (quarter + 0.05).cos()),
            Oriented { count: 2, ..Oriented::turned(315.0 - 0.05f64.to_degrees()) },
        );
        // Different frames never merge.
        assert_eq!(layout_of(vec![a(), far]).segments(range(0, 4)).len(), 2);
    }

    // ── hit-testing and range expansion ──

    fn interaction() -> PdfTextLayout {
        layout_of(vec![
            upright_run(
                0,
                vec![
                    glyph(10.0),
                    upright_glyph(18.0, GlyphOpts { space: true, ..GlyphOpts::default() }),
                ],
                None,
            ),
            column(2),
        ])
    }

    #[test]
    fn char_at_hits_upright_and_rotated_glyphs_in_their_own_frames() {
        let layout = interaction();
        assert_eq!(layout.char_at(point(14.0, 105.0)), Some(0));
        // The middle column glyph: baseline span y 28..36, cell x 88..100.
        assert_eq!(layout.char_at(point(94.0, 32.0)), Some(3));
        assert_eq!(layout.char_at(point(300.0, 300.0)), None);
    }

    #[test]
    fn a_near_miss_on_an_edge_two_characters_share_finds_the_first() {
        // Not in upstream's suite: it pins `textGlyphAt`'s tie-break, which no
        // corpus probe reaches. The first cell's right edge is 0.1 + 0.2, a
        // rounding bit past the second's left edge at 0.3, so the point is
        // nearer the second by that bit alone.
        let cell = |left: f64, right: f64| PageGeometryGlyph {
            loose: rect(left, 100.0, right, 110.0),
            tight: None,
            space: false,
            empty: false,
        };
        let layout =
            layout_of(vec![upright_run(0, vec![cell(0.1, 0.1 + 0.2), cell(0.3, 0.5)], None)]);
        assert_eq!(layout.char_at(point(0.3, 110.0 + 1e-12)), Some(0));
    }

    #[test]
    fn word_expansion_stops_at_spaces_and_empty_characters() {
        let layout = interaction();
        assert_eq!(layout.word_at(0), Some(range(0, 1)), "the space at 1 ends the word");
        assert_eq!(layout.word_at(3), Some(range(2, 3)));
    }

    #[test]
    fn line_expansion_stays_within_the_anchor_frame() {
        assert_eq!(interaction().line_at(3), Some(range(2, 3)));
    }

    #[test]
    fn char_quad_returns_the_oriented_cell_for_endpoints() {
        let layout = interaction();
        let q = layout.char_quad(2).expect("a quad");
        close(q.p1.x, 88.0, 6);
        close(q.p1.y, 20.0, 6);
        close(q.p4.x, 100.0, 6);
        close(q.p4.y, 28.0, 6);
        assert!(layout.char_quad(1).is_some(), "spaces still have cells");
    }

    #[test]
    fn a_point_works_where_a_character_number_does() {
        let layout = interaction();
        assert_eq!(layout.word_at(point(94.0, 32.0)), layout.word_at(3));
        assert_eq!(layout.line_at(point(94.0, 32.0)), layout.line_at(3));
        assert_eq!(layout.word_at(point(300.0, 300.0)), None);
    }

    #[test]
    fn char_count_counts_every_character_and_a_number_outside_it_finds_nothing() {
        // Upstream also asks `lineAt(-1)`; a `u32` can't be negative.
        let layout = interaction();
        assert_eq!(layout.char_count(), 5);
        assert_eq!(layout.word_at(5), None);
        assert_eq!(layout.line_at(5), None);
        assert_eq!(layout.char_quad(9), None);
    }

    #[test]
    fn runs_are_the_raw_geometry_it_was_built_from() {
        let runs = vec![upright_run(0, vec![glyph(10.0)], None)];
        assert_eq!(layout_of(runs.clone()).runs(), runs.as_slice());
    }

    // ── segmentation (search and selection share it) ──

    fn rects_of(runs: Vec<PageGeometryRun>, start: u32, count: u32) -> Vec<PdfRect> {
        layout_of(runs).segments(range(start, count)).into_iter().map(|s| s.rect).collect()
    }

    #[test]
    fn a_within_run_range_yields_one_line_rect_covering_exactly_those_glyphs() {
        assert_eq!(
            rects_of(vec![line(0, 0.0, 100.0, None)], 1, 3),
            [rect(10.0, 100.0, 40.0, 110.0)]
        );
    }

    #[test]
    fn adjacent_runs_on_the_same_line_merge_into_one_rect() {
        // Two text objects, visually one line: per-glyph or per-run boxes
        // would draw a fragmented highlight, so the merge must give one rect.
        assert_eq!(
            rects_of(vec![line(0, 0.0, 100.0, None), line(5, 50.0, 100.0, None)], 2, 6),
            [rect(20.0, 100.0, 80.0, 110.0)]
        );
    }

    #[test]
    fn a_range_across_a_line_break_yields_one_rect_per_line() {
        assert_eq!(
            rects_of(vec![line(0, 0.0, 100.0, None), line(5, 0.0, 80.0, None)], 3, 4),
            [rect(30.0, 100.0, 50.0, 110.0), rect(0.0, 80.0, 20.0, 90.0)]
        );
    }

    #[test]
    fn a_big_intra_run_gap_splits() {
        // Columns can share a text object.
        let columns =
            upright_run(0, vec![glyph(0.0), glyph(10.0), glyph(300.0), glyph(310.0)], None);
        assert_eq!(
            rects_of(vec![columns], 0, 4),
            [rect(0.0, 100.0, 20.0, 110.0), rect(300.0, 100.0, 320.0, 110.0)]
        );
    }

    #[test]
    fn wildly_different_font_sizes_on_one_line_do_not_merge() {
        let runs = vec![line(0, 0.0, 100.0, Some(8.0)), line(5, 50.0, 100.0, Some(30.0))];
        assert_eq!(rects_of(runs, 0, 10).len(), 2);
    }

    #[test]
    fn empty_characters_contribute_nothing() {
        let empty = |x| upright_glyph(x, GlyphOpts { empty: true, ..GlyphOpts::default() });
        assert!(
            rects_of(vec![upright_run(0, vec![empty(0.0), empty(10.0)], None)], 0, 2).is_empty()
        );
    }

    #[test]
    fn ranges_outside_the_page_and_zero_length_ranges_are_empty() {
        assert!(rects_of(vec![line(0, 0.0, 100.0, None)], 50, 3).is_empty());
        assert!(rects_of(vec![line(0, 0.0, 100.0, None)], 0, 0).is_empty());
    }

    #[test]
    fn runs_are_clipped_to_the_requested_range() {
        // The range starts mid-run A and ends mid-run B, on another line.
        assert_eq!(
            rects_of(vec![line(0, 0.0, 100.0, None), line(5, 0.0, 80.0, None)], 4, 3),
            [rect(40.0, 100.0, 50.0, 110.0), rect(0.0, 80.0, 20.0, 90.0)]
        );
    }
}
