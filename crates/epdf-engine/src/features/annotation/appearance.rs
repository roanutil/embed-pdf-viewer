//! Rendering a page's annotation appearances, and reading one as a drawing.
//!
//! G7. Ports `services/src/features/annotations/AnnotationAppearanceReader.ts`
//! and `AnnotationFlattener.readAppearance`. Operations:
//! `annotations.renderAppearances` (and its encoded twin) and
//! `annotations.readAppearance`.
//!
//! One bitmap per annotation of one family (the page's annotations except
//! widgets, or its widgets), per mode it stores and per state of that mode,
//! each sized to its own box rather than to the page and labelled with its
//! mode and state (17350fb4c). Every state comes back, whatever `/AS` says:
//! the annotation's `appearanceState` says which one shows.
//!
//! An annotation with no normal appearance in the file is drawn as PDFium
//! draws it in memory, into the box that drawing takes
//! (`EPDFAnnot_GetDrawingRect`), which can reach past `/Rect`; nothing is
//! written (25dd21f38). One PDFium can draw only by generating an
//! appearance into the file has no raster.
//!
//! # A box kind drawn turned renders turned back upright
//!
//! A square, circle, free text, stamp or caret drawn turned
//! ([`read_annotation_turn`]), whose drawing stays inside the turned box
//! ([`pdf_appearance_turn_of`]), renders upright into its `box`, and `rect`
//! is that box: a consumer re-applies the record's `rotation` as a view
//! transform, so a new turn needs no new raster. The turn is the caller's to
//! pass (runtime 5e9241945): the fork's `EPDF_RenderAnnotBitmapUnrotated`
//! takes it rather than finding it.
//!
//! Everything else renders as the page shows it, placed by `/Rect`: a kind
//! drawn from turned points, a free-text callout (its line isn't turned), a
//! turned drawing that reaches past its box, an appearance with any other
//! matrix.
//!
//! # Skips
//!
//! An appearance that can't be drawn is left out, with nothing in its place:
//! a degenerate box, or a render PDFium refuses. That is upstream's rule
//! (`rasterize` answers `null` and the loop moves on); its result has no
//! per-annotation skip record either, and failing the page would hide every
//! other appearance on it.

use pdfium::{AppearanceMode, AppearanceTurn, PdfiumError, RectF};
use serde::{Deserialize, Serialize};

use super::read::record::annotation_ref_of;
use super::read::turn::{AnnotationTurn, read_annotation_turn};
use super::subtype::{AnnotationSubtype, code};
use crate::access::{DocumentAccess, PageAccess};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::intent::FreeTextIntent;
use crate::features::annotation::read::dictionary_read;
use crate::features::annotation::write::resolve::resolve_annotation;
use crate::features::render::geometry::{
    RenderViewport, device_size, page_rect_of_display_rect, pdf_rect_to_display_rect, render_matrix,
};
use crate::features::render::raster::{PageRaster, read_page_box};
use crate::geom::{
    PdfRect, PdfRotation, is_same_pdf_rect, normalize_pdf_rect, pdf_rect_turned_bounds,
};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::session::document::DocumentSession;

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

/// The `/AP` sub-dictionaries in the one order the engine renders and names
/// them, with the bit each occupies in `EPDFAnnot_GetAvailableAppearanceModes`.
const MODES: [(AppearanceModeName, AppearanceMode, i32); 3] = [
    (AppearanceModeName::Normal, AppearanceMode::Normal, 1),
    (AppearanceModeName::Rollover, AppearanceMode::Rollover, 2),
    (AppearanceModeName::Down, AppearanceMode::Down, 4),
];

/// The normal appearance's bit: the one PDFium draws in memory when the file
/// has none.
const NORMAL_BIT: i32 = 1;

/// At most this many states of one mode are rendered; past it, only the one
/// the annotation shows. Check boxes and radio buttons have two.
const MAX_STATES: usize = 8;

/// Which of a page's annotations a render covers (`AnnotationFamily`): its
/// annotations except widgets (`page.annotations`), or its widgets
/// (`page.forms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationFamily {
    Annotations,
    Widgets,
}

impl AnnotationFamily {
    /// The family of an annotation with subtype code `subtype_code`
    /// (`familyOfCode`).
    pub fn of_code(subtype_code: i32) -> Self {
        if subtype_code == code::WIDGET { Self::Widgets } else { Self::Annotations }
    }
}

/// What to render, and how big (`AnnotationAppearanceRenderOptions`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppearanceRenderOptions {
    /// The size, as for a page render: the appearances come out at the scale
    /// the page has at this viewport. `Width` is the width of the whole
    /// turned page. `None` is scale 1.
    pub viewport: Option<RenderViewport>,
    pub rotation: PdfRotation,
    /// Which modes to render, of those each annotation stores. `None` is
    /// every mode; an empty list asks for nothing and is refused.
    pub modes: Option<Vec<AppearanceModeName>>,
    /// The caller's own budget, applied after [`APPEARANCE_PIXEL_CLAMP`] and
    /// with the opposite behaviour: it refuses.
    pub max_output_pixels: Option<u64>,
}

impl Default for AppearanceRenderOptions {
    fn default() -> Self {
        Self { viewport: None, rotation: PdfRotation::None, modes: None, max_output_pixels: None }
    }
}

/// The wire spelling of an appearance mode (`AnnotationAppearanceMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceModeName {
    Normal,
    Rollover,
    Down,
}

/// One rendered appearance, with what a consumer needs to place it
/// (`AnnotationAppearanceRaster<PdfCoordinates>`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationAppearanceRaster {
    pub annotation_ref: AnnotationRef,
    pub mode: AppearanceModeName,
    /// The state this image draws, when the mode stores several; `None`
    /// when the mode is a single appearance or drawn in memory.
    pub state: Option<String>,
    /// The placement box in the file's coordinates: the turned box for a
    /// box kind drawn turned, the drawing's own box for one drawn in memory,
    /// and `/Rect` otherwise.
    pub rect: PdfRect,
    pub raster: PageRaster,
}

/// Every appearance of one family on one page
/// (`AnnotationAppearancesResult<PdfCoordinates>`).
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationAppearancesResult {
    pub page: PageObjectNumber,
    pub appearances: Vec<AnnotationAppearanceRaster>,
}

/// One image to render for a mode: the state to draw (`None`: the one `/AS`
/// selects) and the state the image is labelled with.
struct StateToRender {
    draw: Option<String>,
    label: Option<String>,
}

/// A mode that is a single appearance, or drawn in memory: one image, no
/// state.
fn shown_only() -> Vec<StateToRender> {
    vec![StateToRender { draw: None, label: None }]
}

/// Render the appearances of one family of `page`'s annotations
/// (`AnnotationAppearanceReader.render`).
pub fn render_page_appearances(
    document: &pdfium::Document,
    page: &dyn PageAccess,
    page_object_number: PageObjectNumber,
    family: AnnotationFamily,
    options: &AppearanceRenderOptions,
    cancel: &CancelToken,
) -> Result<AnnotationAppearancesResult, EngineError> {
    cancel.check()?;
    let view = page.view();
    let modes = resolve_modes(options.modes.as_deref())?;
    let page_box = read_page_box(view);
    let scale = viewport_scale(
        options.viewport,
        page_box.right - page_box.left,
        page_box.top - page_box.bottom,
        options.rotation,
    )?;

    let mut appearances = Vec::new();
    for index in 0..view.annotation_count() {
        cancel.check()?;
        let Some(annotation) = view.annotation(index) else {
            continue;
        };
        let subtype_code = annotation.subtype_code();
        if AnnotationFamily::of_code(subtype_code) != family {
            continue;
        }
        let available = annotation.available_appearance_modes();
        // With no normal appearance stored, where the engine draws one in
        // memory.
        let drawn = if available & NORMAL_BIT != 0 {
            None
        } else {
            annotation
                .drawing_rect(AppearanceMode::Normal)
                .map(|rect| normalize_pdf_rect(pdf_rect(rect)))
        };
        if available == 0 && drawn.is_none() {
            continue;
        }

        let annotation_ref = annotation_ref_of(document, &annotation, page_object_number, index);
        // Normalized once, here: the wire `rect` and the render matrix both
        // rest on the invariant, and `/Rect` itself is not normalized.
        let page_rect = normalize_pdf_rect(pdf_rect(annotation.rect()));
        let stripped = match read_annotation_turn(&annotation) {
            Some(turn) => {
                let intent = if subtype_code == code::FREETEXT {
                    Some(FreeTextIntent::from_name(
                        dictionary_read("IT", annotation.intent())?.as_deref(),
                    ))
                } else {
                    None
                };
                pdf_appearance_turn_of(
                    subtype_code,
                    page_rect,
                    turn.content_box,
                    turn.rotation,
                    intent,
                )
                .map(|_| turn)
            }
            None => None,
        };
        let rect = stripped.map_or(page_rect, |turn| normalize_pdf_rect(turn.content_box));

        for (name, mode, bit) in MODES {
            if !modes.contains(&name) {
                continue;
            }
            let stored = available & bit != 0;
            let target = match (stored, name, drawn) {
                (true, _, _) => rect,
                (false, AppearanceModeName::Normal, Some(drawn)) => drawn,
                _ => continue,
            };
            let states = if stored { states_of(&annotation, mode)? } else { shown_only() };
            for state in states {
                cancel.check()?;
                let raster = render_one(
                    view,
                    &annotation,
                    mode,
                    state.draw.as_deref(),
                    target,
                    page_box,
                    options,
                    scale,
                    if stored { stripped } else { None },
                )?;
                let Some(raster) = raster else {
                    continue;
                };
                appearances.push(AnnotationAppearanceRaster {
                    annotation_ref: annotation_ref.clone(),
                    mode: name,
                    state: state.label,
                    rect: target,
                    raster,
                });
            }
        }
    }

    Ok(AnnotationAppearancesResult { page: page_object_number, appearances })
}

/// The turn an annotation's appearance raster is drawn without, degrees
/// clockwise, or `None` when it is drawn as the page shows it
/// (`pdfAppearanceTurnOf`). A box kind drawn turned whose drawing stays
/// inside the turned box (`rect` is the upright box around it) renders
/// upright over `content_box`. A callout and a drawing that reaches past its
/// box render as the page shows them.
pub fn pdf_appearance_turn_of(
    subtype_code: i32,
    rect: PdfRect,
    content_box: PdfRect,
    rotation: f64,
    intent: Option<FreeTextIntent>,
) -> Option<f64> {
    let box_kind = [
        AnnotationSubtype::Square,
        AnnotationSubtype::Circle,
        AnnotationSubtype::FreeText,
        AnnotationSubtype::Stamp,
        AnnotationSubtype::Caret,
    ]
    .iter()
    .any(|subtype| subtype.code() == subtype_code);
    if !box_kind || rotation == 0.0 || rotation.is_nan() {
        return None;
    }
    if subtype_code == code::FREETEXT && intent == Some(FreeTextIntent::FreeTextCallout) {
        return None;
    }
    is_same_pdf_rect(pdf_rect_turned_bounds(content_box, rotation), rect).then_some(rotation)
}

/// The images to render for a stored mode (`statesOf`): one per state it
/// stores, so a caller has every look of a check box whatever it shows now;
/// one, with no state, for a single appearance; only the shown state when
/// there are more than [`MAX_STATES`].
fn states_of(
    annotation: &pdfium::Annotation<'_>,
    mode: AppearanceMode,
) -> Result<Vec<StateToRender>, EngineError> {
    let names = annotation.appearance_state_names(mode).map_err(read_failed("/AP states"))?;
    if names.is_empty() {
        return Ok(shown_only());
    }
    if names.len() > MAX_STATES {
        let shown = annotation.appearance_state().map_err(read_failed("/AS"))?;
        return Ok(vec![StateToRender { draw: None, label: shown }]);
    }
    Ok(names
        .into_iter()
        .map(|name| StateToRender { draw: Some(name.clone()), label: Some(name) })
        .collect())
}

fn read_failed(what: &'static str) -> impl Fn(PdfiumError) -> EngineError {
    move |e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading {what} of an annotation failed: {e}"),
        )
    }
}

fn pdf_rect(rect: RectF) -> PdfRect {
    PdfRect {
        left: f64::from(rect.left),
        bottom: f64::from(rect.bottom),
        right: f64::from(rect.right),
        top: f64::from(rect.top),
    }
}

/// The counterclockwise angle the file and the fork take for a clockwise
/// one, in `(-180, 180]` (`pdfFromClockwise`).
pub(crate) fn pdf_from_clockwise(degrees: f64) -> f64 {
    let counterclockwise = (((-degrees % 360.0) + 360.0) % 360.0) + 0.0;
    if counterclockwise > 180.0 { counterclockwise - 360.0 } else { counterclockwise }
}

/// One appearance into one bitmap, sized to `rect` (`renderOne`): the
/// stored appearance, or the one PDFium draws in memory. `None` when there
/// is nothing to draw or PDFium refuses the draw.
#[allow(clippy::too_many_arguments)]
fn render_one(
    view: &pdfium::PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    mode: AppearanceMode,
    state: Option<&str>,
    rect: PdfRect,
    page_box: PdfRect,
    options: &AppearanceRenderOptions,
    scale: f64,
    turn: Option<AnnotationTurn>,
) -> Result<Option<PageRaster>, EngineError> {
    // Degenerate: no renderable output, and it would divide by zero in the
    // matrix (`rasterize`).
    if rect.right <= rect.left || rect.top <= rect.bottom {
        return Ok(None);
    }
    let area = ((rect.right - rect.left) * (rect.top - rect.bottom)).max(1.0);
    let effective_scale = scale.min((APPEARANCE_PIXEL_CLAMP / area).sqrt());

    let display_rect = pdf_rect_to_display_rect(rect, page_box);
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
                // `allocateRaster`'s text, word for word.
                format!(
                    "render output {width}x{height} exceeds the {budget}-pixel budget \u{2014} \
                     request a smaller width (or tiles, once available)"
                ),
            ));
        }
    }

    let matrix =
        render_matrix(page_rect_of_display_rect(display_rect), options.rotation, width, height);
    // A box kind drawn turned renders turned back upright into its own box;
    // the fork takes the turn the read found, in the file's counterclockwise
    // angle.
    let turn = turn.map(|turn| AppearanceTurn {
        degrees: pdf_from_clockwise(turn.rotation) as f32,
        unrotated_box: RectF {
            left: turn.content_box.left as f32,
            top: turn.content_box.top as f32,
            right: turn.content_box.right as f32,
            bottom: turn.content_box.bottom as f32,
        },
    });
    let bitmap = match view.render_annotation_rgba_with_matrix(
        annotation,
        mode,
        state,
        width,
        height,
        matrix.to_array(),
        turn,
    ) {
        Ok(bitmap) => bitmap,
        // The bitmap couldn't be made or PDFium drew nothing: no raster,
        // as `rasterize` answers `null`.
        Err(PdfiumError::Bitmap) => return Ok(None),
        Err(e) => {
            return Err(EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                e.to_string(),
            ));
        }
    };

    Ok(Some(PageRaster {
        width: bitmap.width,
        height: bitmap.height,
        stride: bitmap.stride,
        rgba: bitmap.bgra,
    }))
}

/// An annotation's `appearance` resource: what its normal appearance draws,
/// as a one-page PDF, without the turn it reads (`readAppearance`,
/// `exportUnturnedAppearance`). Only a kind that takes an appearance
/// resource has one (`ANNOTATION_RESOURCE_ROLES`: the stamp).
pub fn read_appearance(
    session: &DocumentSession,
    page_object_number: PageObjectNumber,
    annotation_ref: &AnnotationRef,
    cancel: &CancelToken,
) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    let owned = session.document()?;
    let page = owned.page(page_object_number)?;
    if let AnnotationRef::Index { revision, .. } = annotation_ref {
        session.validate_revision(revision)?;
    }
    let annotation = resolve_annotation(owned.document(), page.view(), annotation_ref)?;
    let subtype = AnnotationSubtype::from_code(annotation.subtype_code());
    if subtype != AnnotationSubtype::Stamp {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a {} annotation has no 'appearance' resource", subtype.as_str()),
        ));
    }
    unturned_appearance(&annotation)?.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the annotation has no normal appearance to read",
        )
    })
}

/// An annotation's normal appearance as a one-page drawing document, saved
/// (`EPDFAnnot_ExportAppearance`), without the turn the annotation reads: a
/// copy made from its data turns it again (`exportUnturnedAppearance`).
/// `None` when there is no normal appearance.
pub fn unturned_appearance(
    annotation: &pdfium::Annotation<'_>,
) -> Result<Option<Vec<u8>>, EngineError> {
    let exported = match read_annotation_turn(annotation) {
        None => annotation.export_appearance(0.0, None),
        Some(turn) => annotation.export_appearance(
            pdf_from_clockwise(turn.rotation) as f32,
            Some(RectF {
                left: turn.content_box.left as f32,
                top: turn.content_box.top as f32,
                right: turn.content_box.right as f32,
                bottom: turn.content_box.bottom as f32,
            }),
        ),
    };
    exported.map_err(|_| {
        EngineError::with_message(EngineErrorCode::DocOpenFailed, "failed to save the appearance")
    })
}

/// The scale the page has at `viewport`, as a full-page render would use it
/// (`viewportScale`): a `Scale` viewport's own, or a `Width` viewport's width
/// over the width of the page as `rotation` turns it.
fn viewport_scale(
    viewport: Option<RenderViewport>,
    page_width: f64,
    page_height: f64,
    rotation: PdfRotation,
) -> Result<f64, EngineError> {
    match viewport {
        None => Ok(1.0),
        Some(RenderViewport::Scale { scale }) => Ok(normalize_scale(scale)),
        // An appearance takes a screen viewport (`PageScreenViewport`). A
        // `dpi` one reaches `viewportScale`'s width branch with no width,
        // and is refused there.
        Some(RenderViewport::Dpi { .. }) => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "render viewport width must be positive",
        )),
        Some(RenderViewport::Width { width }) => {
            let page_width =
                if matches!(rotation, PdfRotation::Clockwise90 | PdfRotation::Clockwise270) {
                    page_height
                } else {
                    page_width
                };
            // `!(page_width > 0)`, which also refuses a NaN.
            if !width.is_finite()
                || width <= 0.0
                || page_width.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "render viewport width must be positive",
                ));
            }
            Ok(width / page_width)
        }
    }
}

/// A scale that is not a usable number becomes one.
///
/// Zero, negative and non-finite all read as 1 rather than as an error. The
/// TypeScript does the same, and a caller that computed a scale from a
/// viewport that had not laid out yet gets a picture rather than a failure.
fn normalize_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// The modes a request asks for, in [`MODES`] order (`appearanceModesOf`):
/// every mode when it names none. An empty list asks for nothing and is
/// refused.
fn resolve_modes(
    requested: Option<&[AppearanceModeName]>,
) -> Result<Vec<AppearanceModeName>, EngineError> {
    let every = MODES.map(|(name, _, _)| name);
    let Some(requested) = requested else {
        return Ok(every.to_vec());
    };
    if requested.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "modes must name at least one mode",
        )
        .with_details(serde_json::json!({ "field": "modes" })));
    }
    Ok(every.into_iter().filter(|mode| requested.contains(mode)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `box` turned by `degrees` (clockwise) about its middle: the `/Rect`
    /// a drawing that stays inside the turned box has.
    fn turned(content_box: PdfRect, degrees: f64) -> PdfRect {
        pdf_rect_turned_bounds(content_box, degrees)
    }

    const BOX: PdfRect = PdfRect { left: 240.0, bottom: 600.0, right: 340.0, top: 660.0 };

    #[test]
    fn only_a_box_kind_whose_drawing_stays_inside_its_turned_box_renders_upright() {
        // `appearanceTurn.ts`: stated against the subtype catalog rather than
        // as five numbers, so a renumbering is caught here and not by a
        // picture drawn in the wrong place.
        for subtype in [
            AnnotationSubtype::FreeText,
            AnnotationSubtype::Square,
            AnnotationSubtype::Circle,
            AnnotationSubtype::Stamp,
            AnnotationSubtype::Caret,
        ] {
            assert_eq!(
                pdf_appearance_turn_of(subtype.code(), turned(BOX, 330.0), BOX, 330.0, None),
                Some(330.0),
                "{subtype:?}",
            );
        }
        // A kind drawn from points has its turn in its points: nothing to
        // take out.
        for subtype in [
            AnnotationSubtype::Line,
            AnnotationSubtype::Polygon,
            AnnotationSubtype::Polyline,
            AnnotationSubtype::Ink,
        ] {
            assert_eq!(
                pdf_appearance_turn_of(subtype.code(), turned(BOX, 330.0), BOX, 330.0, None),
                None,
                "{subtype:?}",
            );
        }
        let square = AnnotationSubtype::Square.code();
        // A drawing that reaches past its turned box (a cloudy border's
        // bumps) renders as the page shows it.
        let wider = PdfRect { left: 200.0, ..turned(BOX, 330.0) };
        assert_eq!(pdf_appearance_turn_of(square, wider, BOX, 330.0, None), None);
        // Upright is no turn.
        assert_eq!(pdf_appearance_turn_of(square, BOX, BOX, 0.0, None), None);
        // A callout's line isn't turned.
        let free_text = AnnotationSubtype::FreeText.code();
        assert_eq!(
            pdf_appearance_turn_of(
                free_text,
                turned(BOX, 330.0),
                BOX,
                330.0,
                Some(FreeTextIntent::FreeTextCallout),
            ),
            None,
        );
        assert_eq!(
            pdf_appearance_turn_of(
                free_text,
                turned(BOX, 330.0),
                BOX,
                330.0,
                Some(FreeTextIntent::FreeText),
            ),
            Some(330.0),
        );
    }

    #[test]
    fn a_clockwise_turn_goes_to_the_fork_counterclockwise() {
        // `pdfFromClockwise`: 330 clockwise is 30 counterclockwise.
        assert_eq!(pdf_from_clockwise(330.0), 30.0);
        assert_eq!(pdf_from_clockwise(90.0), -90.0);
        assert_eq!(pdf_from_clockwise(180.0), 180.0);
        assert_eq!(pdf_from_clockwise(0.0), 0.0);
    }

    #[test]
    fn a_widget_is_the_forms_and_everything_else_the_annotations() {
        assert_eq!(AnnotationFamily::of_code(code::WIDGET), AnnotationFamily::Widgets);
        assert_eq!(AnnotationFamily::of_code(code::SQUARE), AnnotationFamily::Annotations);
        assert_eq!(AnnotationFamily::of_code(code::STAMP), AnnotationFamily::Annotations);
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
    fn a_width_viewport_is_the_width_of_the_turned_page() {
        let scale = |viewport, rotation| viewport_scale(viewport, 200.0, 400.0, rotation);
        assert_eq!(scale(None, PdfRotation::None), Ok(1.0));
        assert_eq!(
            scale(Some(RenderViewport::Scale { scale: 3.0 }), PdfRotation::Clockwise90),
            Ok(3.0)
        );
        assert_eq!(scale(Some(RenderViewport::Width { width: 400.0 }), PdfRotation::None), Ok(2.0));
        assert_eq!(
            scale(Some(RenderViewport::Width { width: 400.0 }), PdfRotation::Clockwise90),
            Ok(1.0)
        );
        let refused = scale(Some(RenderViewport::Width { width: 0.0 }), PdfRotation::None)
            .expect_err("a zero width is refused");
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    }

    #[test]
    fn no_modes_means_every_mode_and_an_empty_list_is_refused() {
        use AppearanceModeName::{Down, Normal, Rollover};
        assert_eq!(resolve_modes(None), Ok(vec![Normal, Rollover, Down]));
        assert_eq!(resolve_modes(Some(&[Down, Normal])), Ok(vec![Normal, Down]));
        let refused = resolve_modes(Some(&[])).expect_err("an empty list is refused");
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
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
