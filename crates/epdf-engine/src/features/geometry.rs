//! G2. Page geometry. The first operation ported, and the first to flip.
//!
//! Replaces `services/src/features/geometry/PageGeometryReader.ts` and
//! `core/src/dto/PageGeometrySnapshot.ts`. Operation: `pages.geometry`.
//!
//! The reader answers in the file's coordinates, PDF user space with y-up
//! edges ([`PageGeometrySnapshot<PdfCoordinates>`]), as upstream's reader
//! does. The host boundary turns the answer into page space
//! ([`crate::page_space::text::page_geometry_of`]); the C ABI export in
//! `crates/epdf-wasm` hands the file's coordinates to the TypeScript worker
//! host, whose own boundary converts them.
//!
//! One `EPDFText_GetCharGeometry` call per glyph supplies boxes, oriented
//! cells, matrix and flags together, and [`build_runs_from_raw_glyphs`] groups
//! them into the run union. That single fork call is the whole reason this
//! operation is worth moving: the boundary crossing is per operation, and no
//! host call happens per glyph.

use serde::{Deserialize, Serialize};

use pdfium::TextObjectKey;

use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfPoint, PdfQuad, PdfRect, normalize_pdf_rect, pdf_quad_bounds};
use crate::page_space::{Coordinates, PdfCoordinates};

/// Mid-object orientation drift that forces a run split, about 0.5 degrees.
///
/// Real content keeps one char matrix per text object; this guards the exotic
/// cases so a run's `rotation` is representative of every glyph in it.
const ROTATION_SPLIT_TOLERANCE: f64 = 0.0087;

/// A record's flag: the glyph is a space. The wire carries it as `space`.
pub const GLYPH_FLAG_SPACE: u32 = 1;
/// A record's flag: the glyph has no box of its own and its geometry is
/// zeroed. The wire carries it as `empty`.
pub const GLYPH_FLAG_EMPTY: u32 = 2;

fn is_false(value: &bool) -> bool {
    !*value
}

/// One upright character's geometry (`PageGeometryGlyph<C>`).
///
/// `loose` is the loose char cell (`FPDFText_GetLooseCharBox`): the
/// font-metric box covering the full glyph cell without regard to the glyph's
/// shape. Always present, zeroed and `empty` for degenerate glyphs, and it is
/// the box selection envelopes are built from.
///
/// `tight` is the box hugging the actual outline (`FPDFText_GetCharBox`).
/// Absent for empty and whitespace glyphs, which have no outline.
///
/// `space` and `empty` are on the wire, as `true`, only when they hold.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct PageGeometryGlyph<C: Coordinates = PdfCoordinates> {
    pub loose: C::Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight: Option<C::Rect>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub space: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub empty: bool,
}

/// One non-upright character's geometry: the exact oriented cells, in the
/// page's coordinates rather than a local frame (`RotatedGeometryGlyph<C>`).
/// Each cell's corners are named in the glyph's own upright frame, as every
/// quad's are.
///
/// Degenerate glyphs carry a zeroed `loose` quad and `empty`, mirroring the
/// upright variant's zeroed-box convention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct RotatedGeometryGlyph<C: Coordinates = PdfCoordinates> {
    pub loose: C::Quad,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight: Option<C::Quad>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub space: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub empty: bool,
}

/// One text run: contiguous glyphs sharing a text object and an orientation
/// (`PageGeometryRun<C>`).
///
/// An enum rather than one struct with optional fields, so a consumer cannot
/// read axis-aligned boxes off rotated text by accident. Handling orientation
/// is a compile-time obligation.
///
/// The wire encoding is untagged, matching the TypeScript union that
/// `isRotatedGeometryRun` narrows by testing for the presence of `rotation`.
/// `Rotated` is listed first because an untagged decoder takes the first arm
/// that fits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, bound = "")]
pub enum PageGeometryRun<C: Coordinates = PdfCoordinates> {
    Rotated {
        /// An axis-aligned box over the run, for culling, like every other
        /// wire rect.
        rect: C::Rect,
        /// The run's first character; it covers `glyphs.len()` characters
        /// from here.
        start: u32,
        glyphs: Vec<RotatedGeometryGlyph<C>>,
        /// How far the text is turned from upright, in degrees clockwise as
        /// the page shows it, 0 up to 360, like every other `rotation`: text
        /// that reads straight up the page is 270. A shear-only run has
        /// `rotation == 0.0` and still uses this variant, because its cells
        /// are parallelograms an AABB would misrepresent.
        rotation: f64,
        /// True when the ascent vector maps opposite the rotated frame's +y,
        /// which is mirrored or negative-determinant content.
        #[serde(rename = "ascentFlip")]
        ascent_flip: bool,
        #[serde(rename = "fontSize", default, skip_serializing_if = "Option::is_none")]
        font_size: Option<f64>,
    },
    Upright {
        rect: C::Rect,
        /// The run's first character; it covers `glyphs.len()` characters
        /// from here.
        start: u32,
        glyphs: Vec<PageGeometryGlyph<C>>,
        #[serde(rename = "fontSize", default, skip_serializing_if = "Option::is_none")]
        font_size: Option<f64>,
    },
}

impl<C: Coordinates> PageGeometryRun<C> {
    pub fn rect(&self) -> C::Rect {
        match self {
            Self::Upright { rect, .. } | Self::Rotated { rect, .. } => *rect,
        }
    }

    pub fn start(&self) -> u32 {
        match self {
            Self::Upright { start, .. } | Self::Rotated { start, .. } => *start,
        }
    }

    pub fn glyph_count(&self) -> usize {
        match self {
            Self::Upright { glyphs, .. } => glyphs.len(),
            Self::Rotated { glyphs, .. } => glyphs.len(),
        }
    }

    pub fn font_size(&self) -> Option<f64> {
        match self {
            Self::Upright { font_size, .. } | Self::Rotated { font_size, .. } => *font_size,
        }
    }

    pub fn is_rotated(&self) -> bool {
        matches!(self, Self::Rotated { .. })
    }

    /// Whether the run has a character with a box of its own (`hasBox`).
    pub fn has_box(&self) -> bool {
        match self {
            Self::Upright { glyphs, .. } => glyphs.iter().any(|g| !g.empty),
            Self::Rotated { glyphs, .. } => glyphs.iter().any(|g| !g.empty),
        }
    }
}

impl PageGeometryRun<PdfCoordinates> {
    /// Uniform oriented-cell view over either variant, in the file's
    /// coordinates (`glyphLooseQuad`): the glyph's loose cell as a quad,
    /// synthesized from the box corners when the run is upright.
    pub fn glyph_loose_quad(&self, index: usize) -> Option<PdfQuad> {
        match self {
            Self::Rotated { glyphs, .. } => glyphs.get(index).map(|g| g.loose.into()),
            Self::Upright { glyphs, .. } => glyphs.get(index).map(|g| {
                let b = g.loose;
                PdfQuad {
                    p1: PdfPoint { x: b.left, y: b.top },
                    p2: PdfPoint { x: b.right, y: b.top },
                    p3: PdfPoint { x: b.left, y: b.bottom },
                    p4: PdfPoint { x: b.right, y: b.bottom },
                }
            }),
        }
    }

    /// Uniform AABB view, in the file's coordinates (`glyphLooseBounds`):
    /// the glyph's loose box, or the enclosing bounds of its oriented cell
    /// when the run is rotated.
    ///
    /// The conservative envelope for consumers that genuinely want a box, such
    /// as culling and scroll targets. Never a substitute for handling
    /// orientation in geometry that gets drawn.
    pub fn glyph_loose_bounds(&self, index: usize) -> Option<PdfRect> {
        match self {
            Self::Rotated { glyphs, .. } => {
                glyphs.get(index).map(|g| pdf_quad_bounds(g.loose.into()))
            }
            Self::Upright { glyphs, .. } => glyphs.get(index).map(|g| g.loose),
        }
    }
}

/// Geometry-only text layout for one page (`PageGeometrySnapshot<C>`).
///
/// Pure content, addressed and cached by content version. Carries no
/// annotation liveness envelope; liveness lives on annotation reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct PageGeometrySnapshot<C: Coordinates = PdfCoordinates> {
    pub runs: Vec<PageGeometryRun<C>>,
}

/// One glyph as read from `EPDFText_GetCharGeometry`, plus the grouping
/// context sampled alongside it.
///
/// Pure data with no native handles, so [`build_runs_from_raw_glyphs`] is a
/// pure function the tests drive directly. That split is what makes the
/// grouping rules testable without a PDF.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawGeometryGlyphRecord {
    /// Grouping key: the glyph's text object, by identity.
    pub object_key: TextObjectKey,
    /// Sampled at each text object's first glyph. Run splits inherit it.
    pub font_size: Option<f64>,
    /// [`GLYPH_FLAG_SPACE`] and [`GLYPH_FLAG_EMPTY`]. Synthesized-but-visible
    /// glyphs (`/ActualText` pieces) stay `0`, exactly like the legacy
    /// reader: they are real, selectable text.
    pub flags: u32,
    /// The cell's AABB in the file's coordinates, zeroed when empty.
    pub loose_box: PdfRect,
    pub tight_box: Option<PdfRect>,
    /// Oriented cells. Real glyphs with a usable (non-singular) matrix only.
    pub loose_quad: Option<PdfQuad>,
    pub tight_quad: Option<PdfQuad>,
    /// Native uprightness of the char matrix. Empty glyphs report `true`,
    /// which is inert because they never classify a run.
    pub upright: bool,
    /// Baseline angle, radians counter-clockwise in PDF y-up. Zero when
    /// upright.
    pub baseline_angle: f64,
    /// True when the ascent vector maps opposite the rotated +y, `det < 0`.
    pub ascent_flip: bool,
}

impl RawGeometryGlyphRecord {
    /// The degenerate-glyph convention, kept byte for byte from the legacy
    /// reader so upright snapshots do not shift: empty flag, zeroed box,
    /// upright, no quads.
    fn empty(object_key: TextObjectKey) -> Self {
        Self {
            object_key,
            font_size: None,
            flags: GLYPH_FLAG_EMPTY,
            loose_box: PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 },
            tight_box: None,
            loose_quad: None,
            tight_quad: None,
            upright: true,
            baseline_angle: 0.0,
            ascent_flip: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.flags & GLYPH_FLAG_EMPTY != 0
    }
}

/// The orientation class a run commits to at its first classifiable glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
enum RunClass {
    Upright,
    Rotated { baseline_angle: f64, ascent_flip: bool },
}

/// Reads the geometry of every glyph on a page.
///
/// Works against either ownership mode: Stage A lends a page pointer, Stage B
/// and native own one. See [`crate::access`].
pub fn read_page_geometry(
    page: &dyn PageAccess,
    cancel: &CancelToken,
) -> Result<PageGeometrySnapshot, EngineError> {
    cancel.check()?;
    let text_page = page.view().text().map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            "FPDFText_LoadPage returned null",
        )
    })?;
    cancel.check()?;

    let glyph_count = text_page.char_count();
    let mut records = Vec::new();
    records
        .try_reserve(glyph_count as usize)
        .map_err(|_| EngineError::new(EngineErrorCode::RuntimeUnavailable))?;

    let mut previous_object_key: Option<TextObjectKey> = None;
    for index in 0..glyph_count {
        cancel.check()?;
        let object_key = text_page.text_object(index);
        let mut record = match text_page.char_geometry(index) {
            Some(geometry) => raw_record(object_key, geometry),
            // PDFium refused the index and left the struct unmodified, so
            // there is nothing to read. The legacy reader treats that exactly
            // as a degenerate glyph.
            None => RawGeometryGlyphRecord::empty(object_key),
        };
        if previous_object_key != Some(object_key) {
            record.font_size = Some(text_page.font_size(index));
            previous_object_key = Some(object_key);
        }
        records.push(record);
    }

    Ok(PageGeometrySnapshot { runs: build_runs_from_raw_glyphs(&records) })
}

/// Decodes one `EPDF_CHAR_GEOMETRY` into the reader's record.
fn raw_record(object_key: TextObjectKey, geometry: pdfium::CharGeometry) -> RawGeometryGlyphRecord {
    // Degenerate geometry keeps the legacy zeroed-glyph convention, so upright
    // snapshots stay identical to the pre-orientation reader's.
    if geometry.flags.empty() {
        return RawGeometryGlyphRecord::empty(object_key);
    }

    let upright = geometry.flags.upright();
    let mut record = RawGeometryGlyphRecord {
        object_key,
        font_size: None,
        flags: if geometry.flags.space() { GLYPH_FLAG_SPACE } else { 0 },
        loose_box: normalize_pdf_rect(rect(geometry.loose_box)),
        tight_box: None,
        loose_quad: None,
        tight_quad: None,
        upright,
        baseline_angle: 0.0,
        ascent_flip: false,
    };
    if geometry.flags.has_tight_box() {
        record.tight_box = Some(normalize_pdf_rect(rect(geometry.tight_box)));
    }
    // Quad validity is never inferred from uprightness or from non-zero data,
    // only from the flag.
    if geometry.flags.has_loose_quad() {
        record.loose_quad = Some(quad(geometry.loose_quad));
        if geometry.flags.has_tight_quad() {
            record.tight_quad = Some(quad(geometry.tight_quad));
        }
        if !upright {
            let m = geometry.matrix;
            record.baseline_angle = f64::from(m.b).atan2(f64::from(m.a));
            record.ascent_flip =
                f64::from(m.a) * f64::from(m.d) - f64::from(m.b) * f64::from(m.c) < 0.0;
        }
    }
    record
}

fn rect(r: pdfium::RectF) -> PdfRect {
    PdfRect {
        left: f64::from(r.left),
        bottom: f64::from(r.bottom),
        right: f64::from(r.right),
        top: f64::from(r.top),
    }
}

fn quad(q: pdfium::QuadF) -> PdfQuad {
    PdfQuad {
        p1: PdfPoint { x: f64::from(q.x1), y: f64::from(q.y1) },
        p2: PdfPoint { x: f64::from(q.x2), y: f64::from(q.y2) },
        p3: PdfPoint { x: f64::from(q.x3), y: f64::from(q.y3) },
        p4: PdfPoint { x: f64::from(q.x4), y: f64::from(q.y4) },
    }
}

/// Groups per-glyph records into the run union. Pure, and public for tests.
///
/// Runs split on a text-object change, which is the legacy rule, and on an
/// orientation change between classifiable glyphs. A run's variant is decided
/// by its FIRST classifiable glyph:
///
/// - empty glyphs never classify; they adopt the run's variant;
/// - real glyphs with no oriented cell (singular matrix, synthesized
///   `/ActualText` pieces) classify as upright, so box-only data degrades to
///   exactly the legacy behavior;
/// - real glyphs with a non-upright matrix classify as rotated.
///
/// A run that never sees a classifiable glyph emits as upright, which is the
/// legacy degenerate behavior.
pub fn build_runs_from_raw_glyphs(records: &[RawGeometryGlyphRecord]) -> Vec<PageGeometryRun> {
    let mut runs = Vec::new();
    let mut buffer: Vec<RawGeometryGlyphRecord> = Vec::new();
    let mut start: u32 = 0;
    let mut font_size: Option<f64> = None;
    let mut object_key: Option<TextObjectKey> = None;
    let mut class: Option<RunClass> = None;

    for (i, record) in records.iter().enumerate() {
        let i = i as u32;
        if object_key != Some(record.object_key) {
            flush(&mut runs, &mut buffer, &mut start, font_size, &mut class, i);
            object_key = Some(record.object_key);
            // Sampled at the object's first glyph. A later orientation split
            // inside the same object inherits it rather than resampling.
            font_size = record.font_size;
        }
        if let Some(record_class) = class_of(record) {
            if class.is_some_and(|current| !same_class(current, record_class)) {
                flush(&mut runs, &mut buffer, &mut start, font_size, &mut class, i);
            }
            if class.is_none() {
                class = Some(record_class);
            }
        }
        buffer.push(*record);
    }
    flush(&mut runs, &mut buffer, &mut start, font_size, &mut class, records.len() as u32);
    runs
}

fn flush(
    runs: &mut Vec<PageGeometryRun>,
    buffer: &mut Vec<RawGeometryGlyphRecord>,
    start: &mut u32,
    font_size: Option<f64>,
    class: &mut Option<RunClass>,
    next_start: u32,
) {
    if !buffer.is_empty() {
        runs.push(materialize_run(buffer, *start, font_size, *class));
        buffer.clear();
        *class = None;
    }
    *start = next_start;
}

fn class_of(record: &RawGeometryGlyphRecord) -> Option<RunClass> {
    if record.is_empty() {
        return None; // empty glyphs never classify
    }
    match record.loose_quad {
        Some(_) if !record.upright => Some(RunClass::Rotated {
            baseline_angle: record.baseline_angle,
            ascent_flip: record.ascent_flip,
        }),
        _ => Some(RunClass::Upright),
    }
}

fn same_class(a: RunClass, b: RunClass) -> bool {
    match (a, b) {
        (RunClass::Upright, RunClass::Upright) => true,
        (RunClass::Upright, _) | (_, RunClass::Upright) => false,
        (
            RunClass::Rotated { baseline_angle: ra, ascent_flip: fa },
            RunClass::Rotated { baseline_angle: rb, ascent_flip: fb },
        ) => {
            if fa != fb {
                return false;
            }
            // Compare angles the short way round the circle, so a pair either
            // side of ±π is one class rather than two.
            let delta = ra - rb;
            delta.sin().atan2(delta.cos()).abs() <= ROTATION_SPLIT_TOLERANCE
        }
    }
}

fn materialize_run(
    buffer: &[RawGeometryGlyphRecord],
    start: u32,
    font_size: Option<f64>,
    class: Option<RunClass>,
) -> PageGeometryRun {
    if let Some(RunClass::Rotated { baseline_angle, ascent_flip }) = class {
        let glyphs = buffer
            .iter()
            .map(|g| RotatedGeometryGlyph {
                // Empty glyphs inside a rotated run: zeroed quad plus
                // `empty`, mirroring the upright zeroed-box convention.
                loose: g.loose_quad.unwrap_or(ZERO_QUAD).into(),
                tight: g.loose_quad.and(g.tight_quad).map(Into::into),
                space: g.flags & GLYPH_FLAG_SPACE != 0,
                empty: g.flags & GLYPH_FLAG_EMPTY != 0,
            })
            .collect();
        return PageGeometryRun::Rotated {
            rect: run_bounds(buffer, |g| g.loose_quad.map_or(ZERO_RECT, pdf_quad_bounds)),
            start,
            glyphs,
            rotation: rotation_of_baseline(baseline_angle),
            ascent_flip,
            font_size,
        };
    }

    // Upright and degenerate-only runs: the quads the native layer offered
    // are dropped, and the rect is the union of the real glyphs' boxes.
    let glyphs = buffer
        .iter()
        .map(|g| PageGeometryGlyph {
            loose: g.loose_box,
            tight: g.tight_box,
            space: g.flags & GLYPH_FLAG_SPACE != 0,
            empty: g.flags & GLYPH_FLAG_EMPTY != 0,
        })
        .collect();
    PageGeometryRun::Upright { rect: run_bounds(buffer, |g| g.loose_box), start, glyphs, font_size }
}

/// A baseline's angle (radians, counter-clockwise) as the API's turn:
/// degrees clockwise, 0 up to 360 (`rotationOfBaseline`).
fn rotation_of_baseline(radians: f64) -> f64 {
    let degrees = (-radians * 180.0) / std::f64::consts::PI;
    ((degrees % 360.0) + 360.0) % 360.0
}

const ZERO_RECT: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };
const ZERO_POINT: PdfPoint = PdfPoint { x: 0.0, y: 0.0 };
const ZERO_QUAD: PdfQuad =
    PdfQuad { p1: ZERO_POINT, p2: ZERO_POINT, p3: ZERO_POINT, p4: ZERO_POINT };

/// A run's box: the union of its glyphs' boxes (`runBounds`). A glyph with no
/// box ([`GLYPH_FLAG_EMPTY`]) has only a zeroed stand-in, so it takes no
/// part; a run of nothing but such glyphs gets the zeroed box itself.
fn run_bounds(
    buffer: &[RawGeometryGlyphRecord],
    bounds_of: impl Fn(&RawGeometryGlyphRecord) -> PdfRect,
) -> PdfRect {
    let mut rect: Option<PdfRect> = None;
    for g in buffer {
        if g.is_empty() {
            continue;
        }
        let b = bounds_of(g);
        rect = Some(match rect {
            None => b,
            Some(r) => PdfRect {
                left: r.left.min(b.left),
                bottom: r.bottom.min(b.bottom),
                right: r.right.max(b.right),
                top: r.top.max(b.top),
            },
        });
    }
    rect.unwrap_or(ZERO_RECT)
}

#[cfg(test)]
mod tests {
    //! `packages/engine/main/test/page-geometry-reader.test.ts`
    //! (`buildRunsFromRawGlyphs`), and the wire shape.

    use super::*;

    fn key(n: usize) -> TextObjectKey {
        TextObjectKey(n)
    }

    fn box_rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    /// Axis-aligned quad over a box.
    fn quad_of(left: f64, bottom: f64, right: f64, top: f64) -> PdfQuad {
        PdfQuad {
            p1: PdfPoint { x: left, y: top },
            p2: PdfPoint { x: right, y: top },
            p3: PdfPoint { x: left, y: bottom },
            p4: PdfPoint { x: right, y: bottom },
        }
    }

    /// Quad of the w×h cell at (x, y), turned 90° counter-clockwise
    /// (baseline along +y).
    fn rotated_quad(x: f64, y: f64, w: f64, h: f64) -> PdfQuad {
        PdfQuad {
            p1: PdfPoint { x: x + h, y },
            p2: PdfPoint { x: x + h, y: y + w },
            p3: PdfPoint { x, y },
            p4: PdfPoint { x, y: y + w },
        }
    }

    fn upright(object: usize, loose_box: PdfRect) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord {
            object_key: key(object),
            font_size: None,
            flags: 0,
            loose_box,
            tight_box: None,
            loose_quad: None,
            tight_quad: None,
            upright: true,
            baseline_angle: 0.0,
            ascent_flip: false,
        }
    }

    fn rotated(object: usize, loose_quad: PdfQuad, baseline_angle: f64) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord {
            object_key: key(object),
            font_size: None,
            flags: 0,
            loose_box: ZERO_RECT,
            tight_box: None,
            loose_quad: Some(loose_quad),
            tight_quad: None,
            upright: false,
            baseline_angle,
            ascent_flip: false,
        }
    }

    fn empty(object: usize) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord::empty(key(object))
    }

    fn glyph(loose: PdfRect, tight: Option<PdfRect>, space: bool) -> PageGeometryGlyph {
        PageGeometryGlyph { loose, tight, space, empty: false }
    }

    #[test]
    fn groups_upright_glyphs_per_text_object_and_drops_native_offered_quads() {
        let mut first = upright(1, box_rect(10.0, 10.0, 20.0, 22.0));
        first.font_size = Some(12.0);
        first.tight_box = Some(box_rect(11.0, 12.0, 19.0, 20.0));
        // The native layer offers quads for every real glyph; upright runs
        // must not carry them onto the wire.
        first.loose_quad = Some(quad_of(10.0, 10.0, 20.0, 22.0));
        first.tight_quad = Some(quad_of(11.0, 12.0, 19.0, 20.0));
        let mut space = upright(1, box_rect(20.0, 10.0, 30.0, 22.0));
        space.flags = GLYPH_FLAG_SPACE;
        let mut other = upright(2, box_rect(40.0, 10.0, 50.0, 22.0));
        other.font_size = Some(9.0);

        let runs = build_runs_from_raw_glyphs(&[first, space, other]);
        assert_eq!(runs.len(), 2);
        let PageGeometryRun::Upright { rect, start, glyphs, font_size } = &runs[0] else {
            panic!("expected an upright run");
        };
        assert_eq!(*start, 0);
        assert_eq!(*font_size, Some(12.0));
        assert_eq!(*rect, box_rect(10.0, 10.0, 30.0, 22.0));
        // Boxes, not quads, and the space state only where it is true.
        assert_eq!(
            glyphs,
            &vec![
                glyph(
                    box_rect(10.0, 10.0, 20.0, 22.0),
                    Some(box_rect(11.0, 12.0, 19.0, 20.0)),
                    false
                ),
                glyph(box_rect(20.0, 10.0, 30.0, 22.0), None, true),
            ]
        );
        assert_eq!(runs[1].start(), 2);
        assert_eq!(runs[1].font_size(), Some(9.0));
    }

    #[test]
    fn a_runs_rect_covers_its_real_glyphs_and_a_glyph_with_no_box_takes_no_part() {
        let runs = build_runs_from_raw_glyphs(&[
            empty(1), // a generated space opening the object
            upright(1, box_rect(100.0, 200.0, 110.0, 212.0)),
            empty(1),
            upright(1, box_rect(110.0, 200.0, 120.0, 212.0)),
        ]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].rect(), box_rect(100.0, 200.0, 120.0, 212.0));
        let PageGeometryRun::Upright { glyphs, .. } = &runs[0] else {
            panic!("expected an upright run");
        };
        assert_eq!(
            glyphs[0],
            PageGeometryGlyph { loose: ZERO_RECT, tight: None, space: false, empty: true }
        );
    }

    #[test]
    fn a_run_of_nothing_but_glyphs_with_no_box_keeps_the_zeroed_rect() {
        let runs = build_runs_from_raw_glyphs(&[empty(1), empty(1)]);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].is_rotated());
        assert_eq!(runs[0].rect(), ZERO_RECT);
    }

    #[test]
    fn a_rotated_run_opening_with_a_glyph_with_no_box_covers_only_its_real_cells() {
        let q1 = rotated_quad(50.0, 100.0, 10.0, 14.0);
        let runs =
            build_runs_from_raw_glyphs(&[empty(1), rotated(1, q1, std::f64::consts::FRAC_PI_2)]);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].is_rotated());
        assert_eq!(runs[0].rect(), box_rect(50.0, 100.0, 64.0, 110.0));
    }

    #[test]
    fn a_rotated_object_emits_the_rotated_variant_and_empty_glyphs_get_zeroed_quads() {
        let q1 = rotated_quad(50.0, 100.0, 10.0, 14.0);
        let q2 = rotated_quad(50.0, 112.0, 10.0, 14.0);
        let mut first = rotated(1, q1, std::f64::consts::FRAC_PI_2);
        first.font_size = Some(10.0);
        first.tight_quad = Some(rotated_quad(51.0, 101.0, 8.0, 12.0));
        let runs = build_runs_from_raw_glyphs(&[
            first,
            empty(1),
            rotated(1, q2, std::f64::consts::FRAC_PI_2),
        ]);

        assert_eq!(runs.len(), 1);
        let PageGeometryRun::Rotated { rect, glyphs, rotation, ascent_flip, font_size, .. } =
            &runs[0]
        else {
            panic!("expected a rotated run");
        };
        assert!((rotation - 270.0).abs() < 1e-6, "rotation {rotation}");
        assert!(!ascent_flip);
        assert_eq!(*font_size, Some(10.0));
        assert_eq!(glyphs[0].loose, q1.into());
        assert_eq!(glyphs[0].tight, Some(rotated_quad(51.0, 101.0, 8.0, 12.0).into()));
        assert_eq!(
            glyphs[1],
            RotatedGeometryGlyph {
                loose: ZERO_QUAD.into(),
                tight: None,
                space: false,
                empty: true
            }
        );
        // The AABB over the real glyphs' cells.
        assert_eq!(*rect, box_rect(50.0, 100.0, 64.0, 122.0));
    }

    #[test]
    fn an_orientation_change_mid_object_splits_the_run_and_inherits_the_font_size() {
        let mut first = upright(1, box_rect(10.0, 10.0, 20.0, 22.0));
        first.font_size = Some(12.0);
        let runs = build_runs_from_raw_glyphs(&[
            first,
            upright(1, box_rect(20.0, 10.0, 30.0, 22.0)),
            rotated(1, rotated_quad(40.0, 10.0, 10.0, 12.0), std::f64::consts::FRAC_PI_2),
        ]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].start(), 0);
        assert_eq!(runs[1].start(), 2);
        assert!(!runs[0].is_rotated());
        assert!(runs[1].is_rotated());
        // The same text object, so inherited rather than resampled.
        assert_eq!(runs[1].font_size(), Some(12.0));
    }

    #[test]
    fn drift_splits_beyond_the_tolerance_and_holds_within_it() {
        let base = std::f64::consts::FRAC_PI_4;
        let within = build_runs_from_raw_glyphs(&[
            rotated(1, rotated_quad(0.0, 0.0, 10.0, 12.0), base),
            rotated(1, rotated_quad(0.0, 12.0, 10.0, 12.0), base + 0.005),
        ]);
        assert_eq!(within.len(), 1);

        let beyond = build_runs_from_raw_glyphs(&[
            rotated(1, rotated_quad(0.0, 0.0, 10.0, 12.0), base),
            rotated(1, rotated_quad(0.0, 12.0, 10.0, 12.0), base + 0.02),
        ]);
        assert_eq!(beyond.len(), 2);
        // A mid-object split still tiles.
        assert_eq!(beyond[1].start(), 1);
    }

    #[test]
    fn an_ascent_flip_mismatch_splits_the_run() {
        let mut flipped = rotated(1, rotated_quad(0.0, 0.0, 10.0, 12.0), std::f64::consts::PI);
        flipped.ascent_flip = true;
        let runs = build_runs_from_raw_glyphs(&[
            flipped,
            rotated(1, rotated_quad(0.0, 12.0, 10.0, 12.0), std::f64::consts::PI),
        ]);
        assert_eq!(runs.len(), 2);
    }

    #[test]
    fn quad_less_non_upright_glyphs_degrade_to_an_upright_run() {
        // Synthesized /ActualText pieces and singular-matrix glyphs have boxes
        // but no oriented cells, so they classify upright.
        let mut synthesized = upright(1, box_rect(10.0, 10.0, 30.0, 22.0));
        synthesized.upright = false;
        synthesized.baseline_angle = std::f64::consts::FRAC_PI_2;
        let runs = build_runs_from_raw_glyphs(&[synthesized]);
        assert_eq!(runs.len(), 1);
        let PageGeometryRun::Upright { glyphs, .. } = &runs[0] else {
            panic!("expected an upright run");
        };
        assert_eq!(glyphs[0].loose, box_rect(10.0, 10.0, 30.0, 22.0));
    }

    #[test]
    fn runs_split_on_a_text_object_change_and_their_starts_tile() {
        let records: Vec<_> = [1, 1, 2, 3, 3, 3]
            .iter()
            .enumerate()
            .map(|(i, &object)| {
                upright(object, box_rect(i as f64 * 10.0, 0.0, i as f64 * 10.0 + 10.0, 12.0))
            })
            .collect();
        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs.len(), 3);
        let mut next = 0;
        for run in &runs {
            assert_eq!(run.start(), next);
            next += run.glyph_count() as u32;
        }
        assert_eq!(next, records.len() as u32);
        assert!(build_runs_from_raw_glyphs(&[]).is_empty());
    }

    #[test]
    fn angles_either_side_of_pi_are_one_class() {
        // Comparing raw angles would call these 2π apart and split the run.
        let q = rotated_quad(0.0, 0.0, 10.0, 12.0);
        let runs = build_runs_from_raw_glyphs(&[
            rotated(1, q, std::f64::consts::PI - 0.001),
            rotated(1, q, -std::f64::consts::PI + 0.001),
        ]);
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn a_baseline_angle_reads_as_degrees_clockwise_from_0_up_to_360() {
        // 30° counter-clockwise (0.5236 rad on the old wire) is 330°
        // clockwise.
        assert!((rotation_of_baseline(std::f64::consts::FRAC_PI_6) - 330.0).abs() < 1e-9);
        assert_eq!(rotation_of_baseline(0.0), 0.0);
        assert!((rotation_of_baseline(-std::f64::consts::FRAC_PI_2) - 90.0).abs() < 1e-9);
        assert!((rotation_of_baseline(std::f64::consts::PI) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn the_uniform_views_agree_across_both_variants() {
        let upright_run =
            build_runs_from_raw_glyphs(&[upright(1, box_rect(0.0, 0.0, 10.0, 12.0))]).remove(0);
        // Synthesized in the same corner order: upper-left, upper-right,
        // lower-left, lower-right.
        assert_eq!(upright_run.glyph_loose_quad(0), Some(quad_of(0.0, 0.0, 10.0, 12.0)));
        assert_eq!(upright_run.glyph_loose_bounds(0), Some(box_rect(0.0, 0.0, 10.0, 12.0)));

        let q = rotated_quad(0.0, 0.0, 10.0, 12.0);
        let rotated_run = build_runs_from_raw_glyphs(&[rotated(1, q, 0.5)]).remove(0);
        assert_eq!(rotated_run.glyph_loose_bounds(0), Some(pdf_quad_bounds(q)));
        assert_eq!(rotated_run.glyph_loose_quad(1), None);
    }

    #[test]
    fn the_wire_shape_is_upstreams_in_the_files_coordinates() {
        // `isRotatedGeometryRun` narrows on `'rotation' in run`, so an upright
        // run must not carry the key and a rotated one must.
        let mut space = upright(1, box_rect(10.0, 0.0, 20.0, 12.0));
        space.flags = GLYPH_FLAG_SPACE;
        let upright_run = build_runs_from_raw_glyphs(&[
            upright(1, box_rect(0.0, 0.0, 10.0, 12.0)),
            space,
            empty(1),
        ])
        .remove(0);
        assert_eq!(
            serde_json::to_value(&upright_run).unwrap(),
            serde_json::json!({
                "rect": { "left": 0.0, "bottom": 0.0, "right": 20.0, "top": 12.0 },
                "start": 0,
                "glyphs": [
                    { "loose": { "left": 0.0, "bottom": 0.0, "right": 10.0, "top": 12.0 } },
                    { "loose": { "left": 10.0, "bottom": 0.0, "right": 20.0, "top": 12.0 }, "space": true },
                    { "loose": { "left": 0.0, "bottom": 0.0, "right": 0.0, "top": 0.0 }, "empty": true },
                ],
            })
        );

        let q = rotated_quad(0.0, 0.0, 10.0, 12.0);
        let rotated_run = build_runs_from_raw_glyphs(&[rotated(1, q, 0.5)]).remove(0);
        let json = serde_json::to_value(&rotated_run).unwrap();
        assert_eq!(json["start"], 0);
        assert_eq!(json["ascentFlip"], false);
        assert!(json["rotation"].as_f64().unwrap() > 300.0);
        assert_eq!(
            json["glyphs"][0]["loose"]["upperLeft"],
            serde_json::json!({ "x": 12.0, "y": 0.0 })
        );
        assert_eq!(
            json["glyphs"][0]["loose"]["lowerRight"],
            serde_json::json!({ "x": 0.0, "y": 10.0 })
        );
    }

    #[test]
    fn runs_round_trip_through_the_untagged_wire_form() {
        let snapshot = PageGeometrySnapshot {
            runs: build_runs_from_raw_glyphs(&[
                upright(1, box_rect(0.0, 0.0, 10.0, 12.0)),
                rotated(2, rotated_quad(0.0, 0.0, 10.0, 12.0), 0.5),
                empty(3),
            ]),
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: PageGeometrySnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snapshot);
        assert_eq!(back.runs.len(), 3);
        assert!(!back.runs[0].is_rotated());
        assert!(back.runs[1].is_rotated());
        assert!(!back.runs[2].is_rotated());
    }
}
