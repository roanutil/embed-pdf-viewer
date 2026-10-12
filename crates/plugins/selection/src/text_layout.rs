//! The engine-core text layer the plugin is built on: the geometry and text
//! snapshot DTOs (`dto/PageGeometrySnapshot.ts`, `dto/PageTextSnapshot.ts`),
//! the canonical text layout (`text/layout.ts`) and the copy primitive
//! (`text/charmap.ts` `sliceTextByChars`).
//!
//! `crates/epdf-engine` has a transcription of the same TypeScript, but that
//! crate links PDFium and the plugin layer does not, so this is a second one,
//! transcribed from the TypeScript line for line and pinned by
//! `vectors/plugins/selection-geometry.json`. Indices are `i64` where the
//! TypeScript's are numbers a caller can push out of range (a glyph pointer,
//! a range start); JavaScript's `Math.min`/`Math.max` come from
//! `epdf_geometry::js`.
//!
//! Everything here is PDF user space, y-up.

use epdf_geometry::js;
use epdf_geometry::{Point, Quad};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub type PdfPoint = Point;
/// The engine's `PdfQuad`: `p1..p4` are upper-start, upper-end, lower-start,
/// lower-end in the text's own frame.
pub type PdfQuad = Quad;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

// ── the wire DTOs ─────────────────────────────────────────────────────────────

/// One upright glyph: the loose cell (always present) and the tight box
/// (absent for whitespace). `flags`: bit 1 space, bit 2 empty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometryGlyph {
    pub loose_box: PdfRect,
    pub flags: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight_box: Option<PdfRect>,
}

/// One non-upright glyph: its oriented cells in page space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotatedGeometryGlyph {
    pub loose_quad: PdfQuad,
    pub flags: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight_quad: Option<PdfQuad>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UprightGeometryRun {
    pub rect: PdfRect,
    pub char_start: usize,
    pub glyphs: Vec<PageGeometryGlyph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotatedGeometryRun {
    pub rect: PdfRect,
    pub char_start: usize,
    pub glyphs: Vec<RotatedGeometryGlyph>,
    /// Baseline angle in radians, CCW. Advisory: the layout never reads it.
    pub rotation: f64,
    pub ascent_flip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
}

/// `PageGeometryRun`. The TypeScript narrows with `'rotation' in run`, and
/// so does deserialization.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PageGeometryRun {
    Upright(UprightGeometryRun),
    Rotated(RotatedGeometryRun),
}

/// Either kind of run in one pass. `rotation` may come after `glyphs`, so the
/// glyphs carry both kinds' members until it is known which they are.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRun {
    rect: PdfRect,
    char_start: usize,
    glyphs: Vec<WireGlyph>,
    /// Present, even as `null`, makes the run rotated: `'rotation' in run`.
    #[serde(default, deserialize_with = "present")]
    rotation: Option<Value>,
    #[serde(default)]
    ascent_flip: Option<bool>,
    #[serde(default)]
    font_size: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireGlyph {
    #[serde(default)]
    loose_box: Option<PdfRect>,
    #[serde(default)]
    tight_box: Option<PdfRect>,
    #[serde(default)]
    loose_quad: Option<PdfQuad>,
    #[serde(default)]
    tight_quad: Option<PdfQuad>,
    flags: u32,
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

impl<'de> Deserialize<'de> for PageGeometryRun {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let run = WireRun::deserialize(deserializer)?;
        let missing = |field: &str| D::Error::custom(format!("missing field `{field}`"));
        let Some(rotation) = run.rotation else {
            let glyphs = run
                .glyphs
                .into_iter()
                .map(|g| {
                    Ok(PageGeometryGlyph {
                        loose_box: g.loose_box.ok_or_else(|| missing("looseBox"))?,
                        flags: g.flags,
                        tight_box: g.tight_box,
                    })
                })
                .collect::<Result<_, D::Error>>()?;
            return Ok(PageGeometryRun::Upright(UprightGeometryRun {
                rect: run.rect,
                char_start: run.char_start,
                glyphs,
                font_size: run.font_size,
            }));
        };
        let rotation = rotation.as_f64().ok_or_else(|| {
            D::Error::custom(format!("invalid type: {rotation}, expected f64 for `rotation`"))
        })?;
        let glyphs = run
            .glyphs
            .into_iter()
            .map(|g| {
                Ok(RotatedGeometryGlyph {
                    loose_quad: g.loose_quad.ok_or_else(|| missing("looseQuad"))?,
                    flags: g.flags,
                    tight_quad: g.tight_quad,
                })
            })
            .collect::<Result<_, D::Error>>()?;
        Ok(PageGeometryRun::Rotated(RotatedGeometryRun {
            rect: run.rect,
            char_start: run.char_start,
            glyphs,
            rotation,
            ascent_flip: run.ascent_flip.ok_or_else(|| missing("ascentFlip"))?,
            font_size: run.font_size,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageGeometrySnapshot {
    pub runs: Vec<PageGeometryRun>,
}

/// A page's extracted text: `charCount` is character space, `text` is UTF-16
/// text space, and `charMap` anchors `[char, text]` join them (absent or
/// empty is the identity).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextSnapshot {
    pub text: String,
    pub char_count: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_map: Option<Vec<(f64, f64)>>,
}

// ── the layout ────────────────────────────────────────────────────────────────

const FLAG_SPACE: u32 = 1;
const FLAG_EMPTY: u32 = 2;

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

const ZERO_RECT: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };

/// One merged visual line of a text range, in PDF user space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PdfTextSegment {
    pub quad: PdfQuad,
    pub rect: PdfRect,
    /// Reading direction along the baseline: 1 toward the frame's +x, -1 back.
    pub advance: i32,
}

/// One glyph in its frame's space. `loose` builds segments; `tight` hit-tests.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TextLayoutGlyph {
    pub loose: PdfRect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tight: Option<PdfRect>,
    pub flags: u32,
}

/// A contiguous run of glyphs in one frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayoutRun {
    pub char_start: usize,
    pub count: usize,
    pub rect: PdfRect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    /// Index into [`PageTextLayout::frames`]; 0 is page space.
    pub frame: usize,
}

/// An orientation cluster's orthonormal basis in page space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TextLayoutFrame {
    pub baseline: PdfPoint,
    pub ascent: PdfPoint,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
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

fn frame_box_of_quad(f: &TextLayoutFrame, q: &PdfQuad) -> PdfRect {
    let pts = [q.p1, q.p2, q.p3, q.p4].map(|p| to_frame(f, p));
    let xs = pts.map(|p| p.x);
    let ys = pts.map(|p| p.y);
    PdfRect {
        left: js::min_of(&xs),
        bottom: js::min_of(&ys),
        right: js::max_of(&xs),
        top: js::max_of(&ys),
    }
}

fn frame_for_run(frames: &mut Vec<TextLayoutFrame>, run: &RotatedGeometryRun) -> usize {
    let Some(seed) = run.glyphs.iter().find(|g| !is_empty(g.flags)) else { return 0 };
    let q = &seed.loose_quad;
    let bx = q.p2.x - q.p1.x;
    let by = q.p2.y - q.p1.y;
    let len = js::hypot(bx, by);
    if len <= EDGE_EPSILON {
        return 0;
    }
    let baseline = Point { x: bx / len, y: by / len };
    let ax = q.p1.x - q.p3.x;
    let ay = q.p1.y - q.p3.y;
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

/// `buildPageTextLayout`.
pub fn build_page_text_layout(snapshot: &PageGeometrySnapshot) -> PageTextLayout {
    let mut glyphs = Vec::new();
    let mut runs = Vec::new();
    let mut frames = vec![IDENTITY_FRAME];

    for run in &snapshot.runs {
        let run = match run {
            PageGeometryRun::Upright(run) => {
                for g in &run.glyphs {
                    glyphs.push(TextLayoutGlyph {
                        loose: g.loose_box,
                        tight: g.tight_box,
                        flags: g.flags,
                    });
                }
                runs.push(TextLayoutRun {
                    char_start: run.char_start,
                    count: run.glyphs.len(),
                    rect: run.rect,
                    font_size: run.font_size,
                    frame: 0,
                });
                continue;
            }
            PageGeometryRun::Rotated(run) => run,
        };
        let frame = frame_for_run(&mut frames, run);
        let f = frames[frame];
        let mut run_box: Option<PdfRect> = None;
        for g in &run.glyphs {
            if is_empty(g.flags) {
                glyphs.push(TextLayoutGlyph { loose: ZERO_RECT, tight: None, flags: g.flags });
                continue;
            }
            let loose = frame_box_of_quad(&f, &g.loose_quad);
            let tight = g.tight_quad.as_ref().map(|q| frame_box_of_quad(&f, q));
            glyphs.push(TextLayoutGlyph { loose, tight, flags: g.flags });
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
            char_start: run.char_start,
            count: run.glyphs.len(),
            rect: run_box.unwrap_or(ZERO_RECT),
            font_size: run.font_size,
            frame,
        });
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

fn point_per_frame(layout: &PageTextLayout, p: PdfPoint) -> Vec<PdfPoint> {
    layout.frames.iter().enumerate().map(|(i, f)| if i == 0 { p } else { to_frame(f, p) }).collect()
}

/// The TypeScript's default `toleranceFactor`.
pub const DEFAULT_TOLERANCE_FACTOR: f64 = 1.5;

/// `textGlyphAt`: exact tight-box containment first, then the closest glyph
/// by Manhattan distance within `tolerance_factor` average glyph heights.
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
            if dx + dy < best_dist {
                best_dist = dx + dy;
                best = Some(index);
            }
        }
    }
    best
}

/// A run's glyphs with their indices. The runs of a PDFium snapshot tile
/// the glyphs; another host's may not, and the glyphs a run claims past
/// the end are skipped here where the TypeScript reads `undefined` and
/// throws.
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
/// overlaps the anchor run's.
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

fn quad_of_frame_rect(f: &TextLayoutFrame, frame_index: usize, r: &PdfRect) -> PdfQuad {
    let corners = [
        Point { x: r.left, y: r.top },
        Point { x: r.right, y: r.top },
        Point { x: r.left, y: r.bottom },
        Point { x: r.right, y: r.bottom },
    ];
    let [p1, p2, p3, p4] =
        if frame_index == 0 { corners } else { corners.map(|p| from_frame(f, p)) };
    Quad { p1, p2, p3, p4 }
}

/// `textGlyphQuad`: one glyph's oriented cell in page space, or `None` for
/// a degenerate glyph.
pub fn text_glyph_quad(layout: &PageTextLayout, glyph_index: i64) -> Option<PdfQuad> {
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
        // `first_x` and `last_x` survive in the TypeScript's closure, but
        // the next glyph writes both before anything reads them.
        *self = Accumulator::new();
    }
}

/// `textSegmentsForRange`: merged visual-line segments for the half-open
/// character range `[char_start, char_start + char_count)`.
pub fn text_segments_for_range(
    layout: &PageTextLayout,
    char_start: i64,
    char_count: i64,
) -> Vec<PdfTextSegment> {
    if char_count <= 0 || layout.glyphs.is_empty() {
        return Vec::new();
    }
    let lo = char_start.max(0);
    let hi = (layout.glyphs.len() as i64 - 1).min(char_start + char_count - 1);
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

fn materialize_segment(layout: &PageTextLayout, m: &MergedSubRun) -> PdfTextSegment {
    let frame = &layout.frames[m.frame];
    let quad = quad_of_frame_rect(frame, m.frame, &m.rect);
    let rect = if m.frame == 0 {
        m.rect
    } else {
        let xs = [quad.p1.x, quad.p2.x, quad.p3.x, quad.p4.x];
        let ys = [quad.p1.y, quad.p2.y, quad.p3.y, quad.p4.y];
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

// ── the copy primitive ────────────────────────────────────────────────────────

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

/// `sliceTextByChars`: the text of the half-open character range
/// `[char_start, char_end)`.
pub fn slice_text_by_chars(snapshot: &PageTextSnapshot, char_start: f64, char_end: f64) -> String {
    if char_end <= char_start {
        return String::new();
    }
    slice_utf16(
        &snapshot.text,
        boundary_text_offset(snapshot, char_start),
        boundary_text_offset(snapshot, char_end),
    )
}
