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
//! Everything here is PDF user space, y-up. The line merge is adapted from
//! Chromium's `pdf/pdfium/pdfium_range.cc` `MergeAdjacentRects` (BSD, © 2010
//! The Chromium Authors); hit testing mirrors PDFium's `GetIndexAtPos`, exact
//! box first and then a tolerance pass.
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

use crate::features::geometry::{PageGeometryRun, PageGeometrySnapshot};
use crate::geom::{PdfPoint, PdfQuad, PdfRect};

const FLAG_SPACE: u32 = 1;
const FLAG_EMPTY: u32 = 2;

fn is_boundary(flags: u32) -> bool {
    flags & (FLAG_SPACE | FLAG_EMPTY) != 0
}

fn is_empty_glyph(flags: u32) -> bool {
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

/// One merged visual line of a text range, in PDF user space.
///
/// `quad` is the geometric authority, in frame-geometric slot order: upper
/// start, upper end, lower start, lower end. Those are visual semantics and
/// not reading order. `rect` is its axis-aligned bounds from the same
/// constructor, and `advance` is the reading direction along the baseline,
/// taken from the glyph sequence rather than inferred from geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfTextSegment {
    pub quad: PdfQuad,
    pub rect: PdfRect,
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
    let Some(seed) = glyphs.iter().find(|g| !is_empty_glyph(g.flags)) else {
        return 0;
    };
    let q = seed.loose_quad;
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
            PageGeometryRun::Upright { rect, char_start, glyphs: run_glyphs, font_size } => {
                for g in run_glyphs {
                    glyphs.push(TextLayoutGlyph {
                        loose: g.loose_box,
                        tight: g.tight_box,
                        flags: g.flags,
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: *char_start,
                    count: run_glyphs.len() as u32,
                    rect: *rect,
                    font_size: *font_size,
                    frame: 0,
                });
            }
            PageGeometryRun::Rotated { char_start, glyphs: run_glyphs, font_size, .. } => {
                let frame_index = frame_for_run(&mut frames, run);
                let f = frames[frame_index];
                let mut run_box: Option<PdfRect> = None;
                for g in run_glyphs {
                    if is_empty_glyph(g.flags) {
                        glyphs.push(TextLayoutGlyph {
                            loose: ZERO_RECT,
                            tight: None,
                            flags: g.flags,
                        });
                        continue;
                    }
                    let loose = frame_box_of_quad(&f, g.loose_quad);
                    let tight = g.tight_quad.map(|q| frame_box_of_quad(&f, q));
                    glyphs.push(TextLayoutGlyph { loose, tight, flags: g.flags });
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
                    char_start: *char_start,
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
            if dx + dy < best_dist {
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
    let hi = (layout.glyphs.len() as u32 - 1).min(char_start + char_count - 1);
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
    PdfTextSegment { quad, rect, advance: if m.last_x >= m.first_x { 1 } else { -1 } }
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
