//! Rendering the `/AP` streams a page's annotations carry.
//!
//! G7. Ports `services/src/features/annotations/AnnotationAppearanceReader.ts`.
//! Operation: `annotations.renderAppearances`.
//!
//! One bitmap per annotation per requested mode, each sized to its own
//! annotation's box rather than to the page. An annotation with no `/AP` at
//! all is skipped, which is most of them: a viewer draws a highlight itself
//! and only a baked appearance needs rasterizing. That skip is also why the
//! widget appearance-generation fallback in [`render_one`] never runs, in
//! this port and in the TypeScript it came from; see the comment there.
//!
//! # Rotation is stripped for exactly one family, and the reason is narrow
//!
//! A box-family annotation written by this engine puts its rotation in the
//! appearance stream's `/Matrix` and keeps the logical box in
//! `/EMBD_Metadata /UnrotatedRect`. For those, and ONLY those, the raster is
//! drawn flat and placed by the unrotated box, so a consumer re-applies the
//! angle as a view transform and rotating never re-renders.
//!
//! Everything else renders as it is. A vertex kind has its rotation baked
//! into its geometry, so there is nothing in the matrix to strip; a foreign
//! PDF's `/Matrix` can be any affine at all and stripping it would move the
//! drawing. A free-text CALLOUT is excluded even carrying both fields,
//! because only its text box tilts, through an inline `cm` mid-stream, while
//! the leader stays in page space: the form `/Matrix` is identity and there
//! is nothing to strip there either.
//!
//! The condition is duplicated in the plugin that re-applies the angle, so
//! changing it here changes what a viewer draws somewhere else.

use pdfium::AppearanceMode;
use serde::{Deserialize, Serialize};

use super::read::identity::read_annotation_identity;
use super::read::transform::{read_rotation, read_unrotated_rect};
use super::subtype::code;
use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::intent::FreeTextIntent;
use crate::features::annotation::read::dictionary_read;
use crate::features::render::geometry::{
    RenderViewport, device_size, display_rect_to_device_matrix, pdf_rect_to_display_rect,
};
use crate::features::render::raster::PageRaster;
use crate::geom::{PdfRect, PdfRotation, normalize_pdf_rect};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::revision::{PageState, RevisionToken, WeakAnnotationState};

/// The per-appearance output-pixel ceiling.
///
/// An engine invariant, not an option. An appearance is sized by its rect
/// times the scale, and rects span orders of magnitude: a page-sized stamp at
/// a deep-zoom scale asks for gigabytes. Measured on the TypeScript engine, a
/// 600-point annotation at scale 47 asked for 3.3 GB.
///
/// It REDUCES the scale rather than refusing the request. The raster still
/// covers the same rect, so a consumer stretching it into place shows a
/// slightly soft picture instead of a missing one. A caller's own
/// `max_output_pixels` still applies afterwards, and that one refuses.
pub const APPEARANCE_PIXEL_CLAMP: f64 = 16_000_000.0;

/// The `/AP` sub-dictionaries, with the bit each occupies in
/// `EPDFAnnot_GetAvailableAppearanceModes`.
const MODES: [(AppearanceMode, i32); 3] =
    [(AppearanceMode::Normal, 1), (AppearanceMode::Rollover, 2), (AppearanceMode::Down, 4)];

/// The subtypes whose rotation lives in the appearance `/Matrix`.
///
/// Free text, square, circle, stamp and caret. This set must cover exactly
/// the kinds whose readers surface the rotation pair, because the consumer
/// that re-applies the angle keys off the same two DTO fields.
const BOX_FAMILY: [i32; 5] = [code::FREETEXT, code::SQUARE, code::CIRCLE, code::STAMP, code::CARET];

/// What to render, and how big.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceRenderOptions {
    /// Device pixels per PDF unit. A caller that cares about device pixel
    /// ratio folds it in here.
    pub scale: f64,
    pub rotation: PdfRotation,
    /// Which modes to emit. A mode an annotation does not have is skipped
    /// rather than emitted empty. Empty means normal, which is the default
    /// and the only one most documents have.
    pub modes: Vec<AppearanceModeName>,
    /// The caller's own budget, applied after [`APPEARANCE_PIXEL_CLAMP`] and
    /// with the opposite behaviour: it refuses.
    pub max_output_pixels: Option<u64>,
}

impl Default for AppearanceRenderOptions {
    fn default() -> Self {
        Self { scale: 1.0, rotation: PdfRotation::None, modes: Vec::new(), max_output_pixels: None }
    }
}

/// The wire spelling of an appearance mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceModeName {
    Normal,
    Rollover,
    Down,
}

impl AppearanceModeName {
    fn of(mode: AppearanceMode) -> Self {
        match mode {
            AppearanceMode::Normal => Self::Normal,
            AppearanceMode::Rollover => Self::Rollover,
            AppearanceMode::Down => Self::Down,
        }
    }
}

/// One rendered appearance, with what a consumer needs to place it.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationAppearanceRaster {
    pub annotation_ref: AnnotationRef,
    pub mode: AppearanceModeName,
    /// The placement box in PDF user space. The LOGICAL box for a
    /// rotation-stripped render, and `/Rect` otherwise.
    pub rect: PdfRect,
    pub raster: PageRaster,
}

/// Every appearance on one page.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationAppearancesResult {
    pub page_state: PageState,
    pub appearances: Vec<AnnotationAppearanceRaster>,
}

/// Render a page's appearance streams.
pub fn render_page_appearances(
    page: &dyn PageAccess,
    page_object_number: PageObjectNumber,
    revision: &RevisionToken,
    options: &AppearanceRenderOptions,
    cancel: &CancelToken,
) -> Result<AnnotationAppearancesResult, EngineError> {
    cancel.check()?;
    let view = page.view();
    let page_height = f64::from(view.height());
    let scale = normalize_scale(options.scale);
    let wanted = resolve_modes(&options.modes);

    let mut appearances = Vec::new();
    let mut has_any_weak = false;

    for index in 0..view.annotation_count() {
        cancel.check()?;
        let Some(annotation) = view.annotation(index) else {
            continue;
        };

        let available = annotation.available_appearance_modes();
        let identity = read_annotation_identity(&annotation, page_object_number, index, revision)?;
        if identity.quality == crate::identity::AnnotationIdentityQuality::Weak {
            has_any_weak = true;
        }
        // After the identity read, not before: the weak flag is a property of
        // the page, so an annotation with no `/AP` still contributes to it.
        if available == 0 {
            continue;
        }

        let subtype_code = annotation.subtype_code();
        let unrotated = if strips_rotation(&annotation, subtype_code)? {
            read_unrotated_rect(&annotation)?
        } else {
            None
        };
        // Normalized once, here: both the wire `rect` and the render matrix
        // rest on the invariant, and `/Rect` itself is not normalized.
        let raw = annotation.rect();
        let rect = normalize_pdf_rect(unrotated.unwrap_or(PdfRect {
            left: f64::from(raw.left),
            bottom: f64::from(raw.bottom),
            right: f64::from(raw.right),
            top: f64::from(raw.top),
        }));

        for (mode, bit) in MODES {
            if available & bit == 0 || !wanted.contains(&AppearanceModeName::of(mode)) {
                continue;
            }
            cancel.check()?;
            let Some(raster) = render_one(
                view,
                &annotation,
                mode,
                rect,
                page_height,
                options,
                scale,
                unrotated.is_some(),
                subtype_code,
            )?
            else {
                continue;
            };
            appearances.push(AnnotationAppearanceRaster {
                annotation_ref: identity.annotation_ref.clone(),
                mode: AppearanceModeName::of(mode),
                rect,
                raster,
            });
        }
    }

    Ok(AnnotationAppearancesResult {
        page_state: PageState {
            page_object_number,
            revision: revision.clone(),
            weak_annotation_state: WeakAnnotationState::Known {
                has_any_weak_annotations: has_any_weak,
            },
        },
        appearances,
    })
}

/// Whether this annotation's rotation lives in its appearance `/Matrix`.
fn strips_rotation(
    annotation: &pdfium::Annotation<'_>,
    subtype_code: i32,
) -> Result<bool, EngineError> {
    if !BOX_FAMILY.contains(&subtype_code) {
        return Ok(false);
    }
    if subtype_code == code::FREETEXT {
        let intent =
            FreeTextIntent::from_name(dictionary_read("IT", annotation.intent())?.as_deref());
        if intent == FreeTextIntent::FreeTextCallout {
            return Ok(false);
        }
    }
    Ok(read_rotation(annotation)?.is_some())
}

/// One appearance into one bitmap.
#[allow(clippy::too_many_arguments)]
fn render_one(
    view: &pdfium::PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    mode: AppearanceMode,
    rect: PdfRect,
    page_height: f64,
    options: &AppearanceRenderOptions,
    scale: f64,
    strip_rotation: bool,
    subtype_code: i32,
) -> Result<Option<PageRaster>, EngineError> {
    if !annotation.has_appearance_stream(mode) {
        // THE WIDGET FALLBACK IS UNREACHABLE, HERE AND IN THE TYPESCRIPT.
        //
        // It is meant for a widget that ships with no baked `/AP` because the
        // producer expected the viewer to draw the control. It never runs,
        // and not for the reason it looks like: the caller reads
        // `available_appearance_modes` ONCE, before any generation could
        // happen, and then skips every mode whose bit is clear in that same
        // word. A widget with no `/AP` has a word of zero, so every mode is
        // skipped and this function is never entered for it. The guard higher
        // up that also skips a zero word is redundant with the per-mode check
        // and removing it changes nothing.
        //
        // Not for want of PDFium support: `EPDFAnnot_GenerateFormFieldAP`
        // does produce an appearance for a widget reachable from `/AcroForm`,
        // which `annotation_appearances.pdf` carries and
        // `generating_a_form_fields_appearance_never_reaches_the_reader`
        // demonstrates by calling it directly. The reader still emits nothing
        // for that widget, on either side.
        //
        // Ported as it stands rather than fixed. Generating an appearance is
        // a WRITE into the open document, and making a read start performing
        // one is a behaviour change that belongs to whoever owns the
        // operation, not to a port of it.
        let generated = subtype_code == code::WIDGET
            && !dictionary_read("AP", annotation.has_key("AP"))?
            && annotation.generate_form_field_appearance()
            && annotation.has_appearance_stream(mode);
        if !generated {
            return Ok(None);
        }
    }

    let area = ((rect.right - rect.left) * (rect.top - rect.bottom)).max(1.0);
    let effective_scale = scale.min((APPEARANCE_PIXEL_CLAMP / area).sqrt());

    let display_rect = pdf_rect_to_display_rect(rect, page_height);
    let (width, height) = device_size(
        display_rect,
        options.rotation,
        RenderViewport::Scale { scale: effective_scale },
    )?;

    if let Some(budget) = options.max_output_pixels {
        let pixels = u64::from(width) * u64::from(height);
        if pixels > budget {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "appearance output {width}x{height} exceeds the {budget}-pixel budget; \
                     request a smaller scale"
                ),
            ));
        }
    }

    let matrix = display_rect_to_device_matrix(display_rect, options.rotation, width, height);
    let bitmap = view
        .render_annotation_rgba_with_matrix(
            annotation,
            mode,
            width,
            height,
            matrix.to_array(),
            strip_rotation,
        )
        .map_err(|e| {
            EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
        })?;

    Ok(Some(PageRaster {
        width: bitmap.width,
        height: bitmap.height,
        stride: bitmap.stride,
        rgba: bitmap.bgra,
    }))
}

/// A scale that is not a usable number becomes one.
///
/// Zero, negative and non-finite all read as 1 rather than as an error. The
/// TypeScript does the same, and a caller that computed a scale from a
/// viewport that had not laid out yet gets a picture rather than a failure.
fn normalize_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// An empty request means normal, which is what nearly every document has.
fn resolve_modes(requested: &[AppearanceModeName]) -> Vec<AppearanceModeName> {
    if requested.is_empty() {
        return vec![AppearanceModeName::Normal];
    }
    requested.to_vec()
}

#[cfg(test)]
mod tests {
    use super::super::subtype::AnnotationSubtype;
    use super::*;

    #[test]
    fn the_box_family_is_the_five_subtypes_whose_readers_carry_the_rotation_pair() {
        // Stated against the subtype catalog rather than as five numbers, so
        // a renumbering is caught here and not by a picture drawn in the
        // wrong place.
        let expected = [
            AnnotationSubtype::FreeText,
            AnnotationSubtype::Square,
            AnnotationSubtype::Circle,
            AnnotationSubtype::Stamp,
            AnnotationSubtype::Caret,
        ];
        assert_eq!(BOX_FAMILY.len(), expected.len());
        for subtype in expected {
            assert!(BOX_FAMILY.contains(&subtype.code()), "{subtype:?}");
        }
        // The vertex kinds must NOT be here: their rotation is baked into
        // their geometry, so stripping a matrix would move the drawing.
        for subtype in [
            AnnotationSubtype::Line,
            AnnotationSubtype::Polygon,
            AnnotationSubtype::Polyline,
            AnnotationSubtype::Ink,
        ] {
            assert!(!BOX_FAMILY.contains(&subtype.code()), "{subtype:?}");
        }
    }

    #[test]
    fn an_unusable_scale_becomes_one_rather_than_an_error() {
        assert_eq!(normalize_scale(2.5), 2.5);
        assert_eq!(normalize_scale(0.0), 1.0);
        assert_eq!(normalize_scale(-3.0), 1.0);
        assert_eq!(normalize_scale(f64::NAN), 1.0);
        assert_eq!(normalize_scale(f64::INFINITY), 1.0);
    }

    #[test]
    fn no_requested_mode_means_normal() {
        assert_eq!(resolve_modes(&[]), vec![AppearanceModeName::Normal]);
        assert_eq!(resolve_modes(&[AppearanceModeName::Down]), vec![AppearanceModeName::Down],);
    }

    #[test]
    fn the_clamp_reduces_the_scale_for_a_rect_that_would_not_fit() {
        // A 600-point square at scale 47 is the measured case: 28,200 pixels
        // a side, 3.3 GB. The clamp brings it to the square root of the
        // ceiling over the area.
        let area = 600.0f64 * 600.0;
        let clamped = 47.0f64.min((APPEARANCE_PIXEL_CLAMP / area).sqrt());
        assert!(clamped < 47.0);
        assert!((clamped * 600.0).powi(2) <= APPEARANCE_PIXEL_CLAMP * 1.001);

        // And leaves an ordinary annotation alone: a 40-point icon at scale 3
        // is 120 pixels a side.
        let small = 3.0f64.min((APPEARANCE_PIXEL_CLAMP / (40.0f64 * 40.0)).sqrt());
        assert_eq!(small, 3.0);
    }
}
