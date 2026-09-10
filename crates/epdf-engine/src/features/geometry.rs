//! G2. Page geometry. The first operation ported, and the first to flip.
//!
//! Replaces `services/src/features/geometry/PageGeometryReader.ts` (333) and
//! `core/src/dto/PageGeometrySnapshot.ts` (127). Operation: `pages.geometry`.
//!
//! Geometry is emitted in PDF user space, y-up edges, per the coordinate
//! contract in [`crate::geom`]. No y-flip, no device transform: the viewer
//! converts through the page geometry matrix.
//!
//! One `EPDFText_GetCharGeometry` call per glyph supplies boxes, oriented
//! cells, matrix and flags together, and [`build_runs_from_raw_glyphs`] groups
//! them into the run union. That single fork call is the whole reason this
//! operation is worth moving: the boundary crossing is per operation, and no
//! host call happens per glyph.
//!
//! Upright runs are materialized exactly as the pre-orientation reader did,
//! quirks included, so upright documents never pay for orientation and their
//! snapshots stay identical.

use serde::{Deserialize, Serialize};

use pdfium::TextObjectKey;

use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfPoint, PdfQuad, PdfRect, normalize_pdf_rect, pdf_quad_bounds};

/// Mid-object orientation drift that forces a run split, about 0.5 degrees.
///
/// Real content keeps one char matrix per text object; this guards the exotic
/// cases so a run's `rotation` is representative of every glyph in it.
const ROTATION_SPLIT_TOLERANCE: f64 = 0.0087;

/// Wire flag: the glyph is a space.
pub const GLYPH_FLAG_SPACE: u32 = 1;
/// Wire flag: the glyph is degenerate and its geometry is zeroed.
pub const GLYPH_FLAG_EMPTY: u32 = 2;

/// One upright glyph's geometry.
///
/// `loose_box` is the loose char cell (`FPDFText_GetLooseCharBox`): the
/// font-metric box covering the full glyph cell without regard to the glyph's
/// shape. Always present, zeroed and flagged empty for degenerate glyphs, and
/// it is the box selection envelopes are built from.
///
/// `tight_box` is the box hugging the actual outline
/// (`FPDFText_GetCharBox`). Absent for empty and whitespace glyphs, which have
/// no outline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometryGlyph {
    pub loose_box: PdfRect,
    pub flags: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight_box: Option<PdfRect>,
}

/// One non-upright glyph's geometry: the exact oriented cells, in page
/// coordinates rather than a local frame.
///
/// The corner slots are frame-geometric in the glyph's own upright frame:
/// `p1` upper-start, `p2` upper-end, `p3` lower-start, `p4` lower-end, where
/// "upper" is the ascent side and "start" is the frame's minimum-x side. That
/// is deliberately not a bidi or reading-order statement; advance direction is
/// a glyph-sequence concern carried separately.
///
/// Degenerate glyphs carry a zeroed `loose_quad` plus the empty flag,
/// mirroring the upright variant's zeroed-box convention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotatedGeometryGlyph {
    pub loose_quad: PdfQuad,
    pub flags: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tight_quad: Option<PdfQuad>,
}

/// One text run: contiguous glyphs sharing a text object and an orientation.
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
#[serde(untagged)]
pub enum PageGeometryRun {
    Rotated {
        /// A page-space AABB, for culling, like every other wire rect.
        rect: PdfRect,
        #[serde(rename = "charStart")]
        char_start: u32,
        glyphs: Vec<RotatedGeometryGlyph>,
        /// Baseline angle in radians, counter-clockwise in PDF y-up space:
        /// `atan2(m.b, m.a)` of the run's char matrix. A shear-only run has
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
        rect: PdfRect,
        #[serde(rename = "charStart")]
        char_start: u32,
        glyphs: Vec<PageGeometryGlyph>,
        #[serde(rename = "fontSize", default, skip_serializing_if = "Option::is_none")]
        font_size: Option<f64>,
    },
}

impl PageGeometryRun {
    pub fn rect(&self) -> PdfRect {
        match self {
            Self::Upright { rect, .. } | Self::Rotated { rect, .. } => *rect,
        }
    }

    pub fn char_start(&self) -> u32 {
        match self {
            Self::Upright { char_start, .. } | Self::Rotated { char_start, .. } => *char_start,
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

    /// Uniform oriented-cell view over either variant: the glyph's loose cell
    /// as a quad, synthesized from the box corners when the run is upright, in
    /// the same frame-geometric slot order.
    pub fn glyph_loose_quad(&self, index: usize) -> Option<PdfQuad> {
        match self {
            Self::Rotated { glyphs, .. } => glyphs.get(index).map(|g| g.loose_quad),
            Self::Upright { glyphs, .. } => glyphs.get(index).map(|g| {
                let b = g.loose_box;
                PdfQuad {
                    p1: PdfPoint { x: b.left, y: b.top },
                    p2: PdfPoint { x: b.right, y: b.top },
                    p3: PdfPoint { x: b.left, y: b.bottom },
                    p4: PdfPoint { x: b.right, y: b.bottom },
                }
            }),
        }
    }

    /// Uniform AABB view: the glyph's loose box, or the enclosing bounds of
    /// its oriented cell when the run is rotated.
    ///
    /// The conservative envelope for consumers that genuinely want a box, such
    /// as culling and scroll targets. Never a substitute for handling
    /// orientation in geometry that gets drawn.
    pub fn glyph_loose_bounds(&self, index: usize) -> Option<PdfRect> {
        match self {
            Self::Rotated { glyphs, .. } => {
                glyphs.get(index).map(|g| pdf_quad_bounds(g.loose_quad))
            }
            Self::Upright { glyphs, .. } => glyphs.get(index).map(|g| g.loose_box),
        }
    }
}

/// Geometry-only text layout for one page.
///
/// Pure content, addressed and cached by content version. Carries no
/// annotation liveness envelope; liveness lives on annotation reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageGeometrySnapshot {
    pub runs: Vec<PageGeometryRun>,
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
    /// Wire flags. Synthesized-but-visible glyphs (`/ActualText` pieces) stay
    /// `0`, exactly like the legacy reader: they are real, selectable text.
    pub flags: u32,
    /// Page-space AABB, zeroed when empty.
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
    pub rotation: f64,
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
            rotation: 0.0,
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
    Rotated { rotation: f64, ascent_flip: bool },
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
fn raw_record(
    object_key: TextObjectKey,
    geometry: pdfium::CharGeometry,
) -> RawGeometryGlyphRecord {
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
        rotation: 0.0,
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
            record.rotation = f64::from(m.b).atan2(f64::from(m.a));
            record.ascent_flip = f64::from(m.a) * f64::from(m.d)
                - f64::from(m.b) * f64::from(m.c)
                < 0.0;
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
    let mut char_start: u32 = 0;
    let mut font_size: Option<f64> = None;
    let mut object_key: Option<TextObjectKey> = None;
    let mut class: Option<RunClass> = None;

    for (i, record) in records.iter().enumerate() {
        let i = i as u32;
        if object_key != Some(record.object_key) {
            flush(&mut runs, &mut buffer, &mut char_start, font_size, &mut class, i);
            object_key = Some(record.object_key);
            // Sampled at the object's first glyph. A later orientation split
            // inside the same object inherits it rather than resampling.
            font_size = record.font_size;
        }
        if let Some(record_class) = class_of(record) {
            if class.is_some_and(|current| !same_class(current, record_class)) {
                flush(&mut runs, &mut buffer, &mut char_start, font_size, &mut class, i);
            }
            if class.is_none() {
                class = Some(record_class);
            }
        }
        buffer.push(*record);
    }
    flush(
        &mut runs,
        &mut buffer,
        &mut char_start,
        font_size,
        &mut class,
        records.len() as u32,
    );
    runs
}

fn flush(
    runs: &mut Vec<PageGeometryRun>,
    buffer: &mut Vec<RawGeometryGlyphRecord>,
    char_start: &mut u32,
    font_size: Option<f64>,
    class: &mut Option<RunClass>,
    next_start: u32,
) {
    if !buffer.is_empty() {
        runs.push(materialize_run(buffer, *char_start, font_size, *class));
        buffer.clear();
        *class = None;
    }
    *char_start = next_start;
}

fn class_of(record: &RawGeometryGlyphRecord) -> Option<RunClass> {
    if record.is_empty() {
        return None; // empty glyphs never classify
    }
    match record.loose_quad {
        Some(_) if !record.upright => {
            Some(RunClass::Rotated { rotation: record.rotation, ascent_flip: record.ascent_flip })
        }
        _ => Some(RunClass::Upright),
    }
}

fn same_class(a: RunClass, b: RunClass) -> bool {
    match (a, b) {
        (RunClass::Upright, RunClass::Upright) => true,
        (RunClass::Upright, _) | (_, RunClass::Upright) => false,
        (
            RunClass::Rotated { rotation: ra, ascent_flip: fa },
            RunClass::Rotated { rotation: rb, ascent_flip: fb },
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
    char_start: u32,
    font_size: Option<f64>,
    class: Option<RunClass>,
) -> PageGeometryRun {
    if let Some(RunClass::Rotated { rotation, ascent_flip }) = class {
        let glyphs = buffer
            .iter()
            .map(|g| match g.loose_quad {
                Some(loose_quad) => {
                    RotatedGeometryGlyph { loose_quad, flags: g.flags, tight_quad: g.tight_quad }
                }
                // Empty glyphs inside a rotated run: zeroed quad plus the
                // empty flag, mirroring the upright zeroed-box convention.
                None => RotatedGeometryGlyph {
                    loose_quad: ZERO_QUAD,
                    flags: g.flags,
                    tight_quad: None,
                },
            })
            .collect();
        return PageGeometryRun::Rotated {
            rect: run_bounds(buffer, |g| {
                g.loose_quad.map_or(ZERO_RECT, pdf_quad_bounds)
            }),
            char_start,
            glyphs,
            rotation,
            ascent_flip,
            font_size,
        };
    }

    // Upright and degenerate-only runs: the legacy materialization verbatim.
    // Quads the native layer offered are dropped, the rect seeds from the
    // FIRST glyph's box (zeroed for an empty glyph, quirk included) and
    // expands over non-empty glyphs only.
    let glyphs = buffer
        .iter()
        .map(|g| PageGeometryGlyph {
            loose_box: g.loose_box,
            flags: g.flags,
            tight_box: g.tight_box,
        })
        .collect();
    PageGeometryRun::Upright {
        rect: run_bounds(buffer, |g| g.loose_box),
        char_start,
        glyphs,
        font_size,
    }
}

const ZERO_RECT: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };
const ZERO_POINT: PdfPoint = PdfPoint { x: 0.0, y: 0.0 };
const ZERO_QUAD: PdfQuad =
    PdfQuad { p1: ZERO_POINT, p2: ZERO_POINT, p3: ZERO_POINT, p4: ZERO_POINT };

/// The legacy run-rect algorithm: seed from the first glyph whatever it is,
/// then expand over non-empty glyphs only.
fn run_bounds(
    buffer: &[RawGeometryGlyphRecord],
    bounds_of: impl Fn(&RawGeometryGlyphRecord) -> PdfRect,
) -> PdfRect {
    let mut rect = bounds_of(&buffer[0]);
    for g in buffer {
        if g.is_empty() {
            continue;
        }
        let b = bounds_of(g);
        rect.left = rect.left.min(b.left);
        rect.bottom = rect.bottom.min(b.bottom);
        rect.right = rect.right.max(b.right);
        rect.top = rect.top.max(b.top);
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: usize) -> TextObjectKey {
        TextObjectKey(n)
    }

    fn box_rect(left: f64, bottom: f64, right: f64, top: f64) -> PdfRect {
        PdfRect { left, bottom, right, top }
    }

    /// An upright, non-empty glyph in one text object.
    fn upright_glyph(object: usize, left: f64) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord {
            object_key: key(object),
            font_size: None,
            flags: 0,
            loose_box: box_rect(left, 0.0, left + 10.0, 12.0),
            tight_box: Some(box_rect(left + 1.0, 1.0, left + 9.0, 11.0)),
            loose_quad: None,
            tight_quad: None,
            upright: true,
            rotation: 0.0,
            ascent_flip: false,
        }
    }

    /// A glyph whose char matrix is not upright, so it classifies a run as
    /// rotated.
    fn rotated_glyph(object: usize, rotation: f64) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord {
            object_key: key(object),
            font_size: None,
            flags: 0,
            loose_box: box_rect(0.0, 0.0, 10.0, 12.0),
            tight_box: None,
            loose_quad: Some(PdfQuad {
                p1: PdfPoint { x: 0.0, y: 12.0 },
                p2: PdfPoint { x: 10.0, y: 12.0 },
                p3: PdfPoint { x: 0.0, y: 0.0 },
                p4: PdfPoint { x: 10.0, y: 0.0 },
            }),
            tight_quad: None,
            upright: false,
            rotation,
            ascent_flip: false,
        }
    }

    fn empty_glyph(object: usize) -> RawGeometryGlyphRecord {
        RawGeometryGlyphRecord::empty(key(object))
    }

    #[test]
    fn splits_runs_on_a_text_object_change_and_tiles_char_start() {
        let records = [
            upright_glyph(1, 0.0),
            upright_glyph(1, 10.0),
            upright_glyph(2, 20.0),
            upright_glyph(3, 30.0),
            upright_glyph(3, 40.0),
            upright_glyph(3, 50.0),
        ];
        let runs = build_runs_from_raw_glyphs(&records);

        assert_eq!(runs.len(), 3);
        // charStart tiles the character range with no gap and no overlap,
        // which the orientation conformance suite asserts as a property.
        let mut next = 0;
        for run in &runs {
            assert_eq!(run.char_start(), next);
            next += run.glyph_count() as u32;
        }
        assert_eq!(next, records.len() as u32);
    }

    #[test]
    fn an_empty_record_list_produces_no_runs() {
        assert!(build_runs_from_raw_glyphs(&[]).is_empty());
    }

    #[test]
    fn a_runs_rect_seeds_from_its_first_glyph_and_expands_over_non_empty_ones() {
        // The legacy quirk, kept deliberately: an empty first glyph seeds the
        // rect at the origin even though it contributes nothing afterwards.
        let records = [empty_glyph(1), upright_glyph(1, 100.0)];
        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].rect(), box_rect(0.0, 0.0, 110.0, 12.0));

        // With a real glyph first, the origin is not included.
        let records = [upright_glyph(1, 100.0), empty_glyph(1)];
        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs[0].rect(), box_rect(100.0, 0.0, 110.0, 12.0));
    }

    #[test]
    fn empty_glyphs_never_classify_and_adopt_the_runs_variant() {
        // Leading empties do not make the run upright; the first classifiable
        // glyph does, and here it is rotated.
        let records = [empty_glyph(1), empty_glyph(1), rotated_glyph(1, 0.5)];
        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].is_rotated());
        assert_eq!(runs[0].glyph_count(), 3);

        // A run of nothing but empties emits as upright, the legacy
        // degenerate behavior.
        let runs = build_runs_from_raw_glyphs(&[empty_glyph(1), empty_glyph(1)]);
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].is_rotated());
    }

    #[test]
    fn an_empty_glyph_inside_a_rotated_run_gets_a_zeroed_quad() {
        let records = [rotated_glyph(1, 0.5), empty_glyph(1)];
        let runs = build_runs_from_raw_glyphs(&records);
        let PageGeometryRun::Rotated { glyphs, .. } = &runs[0] else {
            panic!("expected a rotated run");
        };
        assert_eq!(glyphs[1].loose_quad, ZERO_QUAD);
        assert_eq!(glyphs[1].flags, GLYPH_FLAG_EMPTY);
        assert_eq!(glyphs[1].tight_quad, None);
    }

    #[test]
    fn a_real_glyph_with_no_oriented_cell_classifies_upright() {
        // Synthesized /ActualText pieces and singular matrices have no loose
        // quad, so box-only data degrades to exactly the legacy behavior even
        // when the native flags say the matrix is not upright.
        let mut synthesized = upright_glyph(1, 0.0);
        synthesized.upright = false;
        synthesized.loose_quad = None;

        let runs = build_runs_from_raw_glyphs(&[synthesized]);
        assert!(!runs[0].is_rotated());
    }

    #[test]
    fn orientation_drift_splits_a_run_only_past_the_tolerance() {
        let inside = ROTATION_SPLIT_TOLERANCE * 0.9;
        let records = [rotated_glyph(1, 0.5), rotated_glyph(1, 0.5 + inside)];
        assert_eq!(build_runs_from_raw_glyphs(&records).len(), 1);

        let outside = ROTATION_SPLIT_TOLERANCE * 2.0;
        let records = [rotated_glyph(1, 0.5), rotated_glyph(1, 0.5 + outside)];
        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs.len(), 2);
        // A mid-object split still tiles.
        assert_eq!(runs[0].char_start(), 0);
        assert_eq!(runs[1].char_start(), 1);
    }

    #[test]
    fn angles_either_side_of_pi_are_one_class() {
        // Comparing raw angles would call these 2π apart and split the run.
        let records = [
            rotated_glyph(1, std::f64::consts::PI - 0.001),
            rotated_glyph(1, -std::f64::consts::PI + 0.001),
        ];
        assert_eq!(build_runs_from_raw_glyphs(&records).len(), 1);
    }

    #[test]
    fn a_mirrored_glyph_splits_from_an_unmirrored_one_at_the_same_angle() {
        let mut mirrored = rotated_glyph(1, 0.5);
        mirrored.ascent_flip = true;
        let records = [rotated_glyph(1, 0.5), mirrored];
        assert_eq!(build_runs_from_raw_glyphs(&records).len(), 2);
    }

    #[test]
    fn an_orientation_change_splits_the_run_but_keeps_the_objects_font_size() {
        // Font size is sampled at the text object's first glyph. A split
        // inside that object inherits it rather than resampling, which would
        // read the size at a glyph that is not the object's first.
        let mut first = upright_glyph(1, 0.0);
        first.font_size = Some(11.0);
        let records = [first, rotated_glyph(1, 1.2)];

        let runs = build_runs_from_raw_glyphs(&records);
        assert_eq!(runs.len(), 2);
        assert!(!runs[0].is_rotated());
        assert!(runs[1].is_rotated());
        assert_eq!(runs[0].font_size(), Some(11.0));
        assert_eq!(runs[1].font_size(), Some(11.0));
    }

    #[test]
    fn a_new_text_object_resamples_the_font_size() {
        let mut first = upright_glyph(1, 0.0);
        first.font_size = Some(11.0);
        let mut second = upright_glyph(2, 10.0);
        second.font_size = Some(24.0);

        let runs = build_runs_from_raw_glyphs(&[first, second]);
        assert_eq!(runs[0].font_size(), Some(11.0));
        assert_eq!(runs[1].font_size(), Some(24.0));
    }

    #[test]
    fn upright_runs_drop_the_quads_the_native_layer_offered() {
        // The dominant case never pays for orientation: an upright run's wire
        // shape is the pre-orientation one.
        let mut with_quad = upright_glyph(1, 0.0);
        with_quad.loose_quad = Some(ZERO_QUAD);
        let runs = build_runs_from_raw_glyphs(&[with_quad]);

        let PageGeometryRun::Upright { glyphs, .. } = &runs[0] else {
            panic!("expected an upright run");
        };
        assert_eq!(glyphs[0].loose_box, box_rect(0.0, 0.0, 10.0, 12.0));
        assert_eq!(glyphs[0].tight_box, Some(box_rect(1.0, 1.0, 9.0, 11.0)));
    }

    #[test]
    fn the_uniform_views_agree_across_both_variants() {
        let upright = build_runs_from_raw_glyphs(&[upright_glyph(1, 0.0)]).remove(0);
        let quad = upright.glyph_loose_quad(0).unwrap();
        // Synthesized in the same frame-geometric slot order: upper-start,
        // upper-end, lower-start, lower-end.
        assert_eq!(quad.p1, PdfPoint { x: 0.0, y: 12.0 });
        assert_eq!(quad.p4, PdfPoint { x: 10.0, y: 0.0 });
        assert_eq!(upright.glyph_loose_bounds(0), Some(box_rect(0.0, 0.0, 10.0, 12.0)));

        let rotated = build_runs_from_raw_glyphs(&[rotated_glyph(1, 0.5)]).remove(0);
        assert_eq!(rotated.glyph_loose_bounds(0), Some(box_rect(0.0, 0.0, 10.0, 12.0)));
        assert_eq!(rotated.glyph_loose_quad(1), None);
    }

    #[test]
    fn the_wire_shape_distinguishes_the_variants_by_the_rotation_key() {
        // `isRotatedGeometryRun` narrows on `'rotation' in run`, so an upright
        // run must not carry the key and a rotated one must.
        let upright = build_runs_from_raw_glyphs(&[upright_glyph(1, 0.0)]).remove(0);
        let json = serde_json::to_value(&upright).unwrap();
        assert_eq!(json.get("rotation"), None);
        assert_eq!(json.get("ascentFlip"), None);
        assert_eq!(json["charStart"], 0);
        assert!(json["glyphs"][0].get("looseBox").is_some());
        // An absent font size is omitted, not null.
        assert_eq!(json.get("fontSize"), None);

        let rotated = build_runs_from_raw_glyphs(&[rotated_glyph(1, 0.5)]).remove(0);
        let json = serde_json::to_value(&rotated).unwrap();
        assert_eq!(json["rotation"], 0.5);
        assert_eq!(json["ascentFlip"], false);
        assert!(json["glyphs"][0].get("looseQuad").is_some());
    }

    #[test]
    fn runs_round_trip_through_the_untagged_wire_form() {
        let snapshot = PageGeometrySnapshot {
            runs: build_runs_from_raw_glyphs(&[
                upright_glyph(1, 0.0),
                rotated_glyph(2, 0.5),
                empty_glyph(3),
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
