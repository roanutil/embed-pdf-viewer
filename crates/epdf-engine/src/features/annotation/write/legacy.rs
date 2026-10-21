//! The writers the pre-catch-up update and the form plugin still call.
//!
//! G11. What was left of the old `annotations.create` writer when Task 8.14
//! ported upstream's create to [`super::create`]: the stamp authoring, link
//! target, `/DA` and string helpers [`super::update`] still writes through,
//! and [`apply_widget`], which `forms.createField` places its widget with.
//! Task 8.15 ports the update and 8.21 the widget placement; each moves its
//! callers off what it owns here, and the last deletes this file.

use pdfium::{PageView, RectF};

use super::context::AnnotationWriteContext;
use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::read::style::Color;
use crate::geom::PdfQuad;

/// A `/DA` family this host can actually write.
///
/// Same reason as above: resolving a registered font can fail, and finding
/// that out after the annotation exists leaves a free text in the file with
/// no `/DA`.
pub(super) fn font_resolvable(
    family: &str,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if crate::features::annotation::vocabulary::StandardFont::from_family(family).is_none() {
        context.resolve_font(family)?;
    }
    Ok(())
}

fn set_border(
    annotation: &pdfium::Annotation<'_>,
    style: Option<crate::features::annotation::vocabulary::AnnotationBorderStyle>,
    width: Option<f64>,
    dashes: &Option<Vec<f64>>,
) -> Result<(), EngineError> {
    let style = style.unwrap_or(AnnotationBorderStyleDefault::STYLE);
    let width = width.unwrap_or(AnnotationBorderStyleDefault::WIDTH);
    if !annotation.set_border_style(style.code(), width as f32) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetBorderStyle refused the border",
        ));
    }
    if let Some(dashes) = dashes.as_ref().filter(|d| !d.is_empty()) {
        let dashes: Vec<f32> = dashes.iter().map(|d| *d as f32).collect();
        if !annotation.set_border_dash_pattern(&dashes) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetBorderDashPattern refused the pattern",
            ));
        }
    }
    Ok(())
}

/// An inert widget: placed and styled, and not yet a form control.
///
/// Every entry is written only when the draft names it, which is the
/// opposite of every other family here. The others write their defaults
/// explicitly so a read round-trips the same values; a widget's unwritten
/// entries are what the FIELD supplies when one adopts it, and stamping a
/// default now would override the field's own answer with this engine's.
pub fn apply_widget(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::WidgetDraftFields,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    set_mk_color_patch(annotation, MK_BORDER_COLOR, &fields.style.color)?;
    set_mk_color_patch(annotation, MK_BACKGROUND_COLOR, &fields.style.interior_color)?;

    // `/BS` takes both members in one call, so naming either supplies both.
    //
    // The guard is not observable: `read_border_fields` resolves a missing
    // `/BS` to the same solid, one-point border this would write, so a
    // build that wrote it unconditionally passes every vector. It stays
    // because the entry it would add is one the FIELD supplies on adoption,
    // and writing it now decides for the field. `/DA` is the same rule and
    // IS observable, because the widget reader reports its three members
    // only when the entry exists.
    if fields.style.touches_border() {
        set_border(annotation, fields.style.border_style, fields.style.stroke_width, &None)?;
    }
    // `/DA` packs all three into one string, on the same terms.
    if fields.style.touches_text_style() {
        let family = fields
            .style
            .font_family
            .unwrap_or(crate::features::annotation::vocabulary::DEFAULT_STANDARD_FONT);
        let color = fields
            .style
            .font_color
            .unwrap_or(super::super::read::free_text::DEFAULT_FREE_TEXT_COLOR);
        let size = fields.style.font_size.unwrap_or(super::super::read::redact::DEFAULT_FONT_SIZE);
        if !annotation.set_default_appearance(
            family.code(),
            size as f32,
            (color.r, color.g, color.b),
        ) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetDefaultAppearance refused the widget's text defaults",
            ));
        }
    }
    // `/Q` is attempted and its refusal ignored, which is the one place in
    // these writers that does not report a false. PDFium declines it on a
    // widget with no `/FT`, and an inert widget by definition has none: the
    // alignment belongs to the field's text, and there is no field yet. So
    // the refusal here is the expected answer rather than a failure, and
    // `widget-styled` records a left alignment for a draft that asked for a
    // centred one, on both engines.
    if let Some(align) = fields.style.text_align {
        let _ = annotation.set_text_alignment(align.code());
    }
    Ok(())
}

/// `EPDF_MK_COLORTYPE`, the same two codes the widget reader uses.
const MK_BORDER_COLOR: i32 = 0;
const MK_BACKGROUND_COLOR: i32 = 1;

/// One `/MK` colour, tri-state.
///
/// `Keep` leaves the entry alone, and on a new widget that means absent.
/// `Clear` is an explicit "no colour here", which on a background is
/// transparent and on a border is a widget that draws none.
///
/// On the create path the two are the same thing, because there is no `/MK`
/// entry to leave alone or to remove: swapping them passes every vector.
/// They separate on the update path, where a widget can already have a
/// background and the difference between keeping it and removing it is the
/// whole of what a patch means.
pub(super) fn set_mk_color_patch(
    annotation: &pdfium::Annotation<'_>,
    which: i32,
    color: &Patch<Color>,
) -> Result<(), EngineError> {
    let ok = match color {
        Patch::Keep => return Ok(()),
        Patch::Clear => annotation.clear_mk_color(which),
        Patch::Set(color) => annotation.set_mk_color(which, (color.r, color.g, color.b)),
    };
    if !ok {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "PDFium refused a widget /MK colour",
        ));
    }
    Ok(())
}

/// Author a stamp's appearance from bytes, correct under rotation.
///
/// Shared with the patch path, where replacing a stamp's picture is the same
/// problem. `logical` is the pre-rotation box when the stamp is rotated and
/// `None` when it is upright; the two transform patches are what to leave on
/// the annotation afterwards.
#[allow(clippy::too_many_arguments)]
pub(super) fn author_stamp_appearance(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    bytes: &[u8],
    fit: super::draft::StampFitDraft,
    rect: crate::geom::PdfRect,
    logical: Option<crate::geom::PdfRect>,
    rotation: &Patch<f64>,
    unrotated_rect: &Patch<crate::geom::PdfRect>,
) -> Result<(), EngineError> {
    let Some(logical) = logical else {
        set_rect(annotation, rect)?;
        set_stamp_content(document, page, annotation, bytes, fit, rect)?;
        // Still written: naming a zero rotation is saying the stamp is
        // upright, and the clear is what makes that true.
        return super::transform::write_box_transform(
            annotation,
            &super::transform::AnnotationTransform {
                rotation: rotation.clone(),
                unrotated_rect: unrotated_rect.clone(),
            },
            || Ok(false),
        );
    };

    // Author in the unrotated frame, with the transform keys cleared so the
    // internal re-fit records an image-shaped content rect rather than the
    // padded box.
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: Patch::Clear,
            unrotated_rect: Patch::Clear,
        },
        || Ok(false),
    )?;
    set_rect(annotation, logical)?;
    set_stamp_content(document, page, annotation, bytes, fit, logical)?;

    // Then apply the transform: the keys bake the `/Matrix`, `/Rect` becomes
    // the enclosing box, and one re-fit reconciles the two.
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: rotation.clone(),
            unrotated_rect: Patch::Set(logical),
        },
        || Ok(true),
    )?;
    set_rect(annotation, rect)?;
    refit(annotation, fit)
}

/// Put the content in, replacing whatever was there.
fn set_stamp_content(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    bytes: &[u8],
    fit: super::draft::StampFitDraft,
    rect: crate::geom::PdfRect,
) -> Result<(), EngineError> {
    // Replace rather than merge. Inert on create, where the annotation has
    // no objects yet, and load-bearing on the update path, where replacing
    // a stamp's bytes has to drop what was drawn rather than paint over it.
    // The two share this function so they cannot drift.
    for index in (0..annotation.object_count()).rev() {
        annotation.remove_object(index);
    }

    let format = crate::features::binary::sniff(bytes).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the stamp source is not a format this build places",
        )
    })?;
    match format {
        crate::features::binary::BinaryFormat::Pdf => {
            set_appearance_from_pdf(document, annotation, bytes)?;
        }
        crate::features::binary::BinaryFormat::Png { width, height }
        | crate::features::binary::BinaryFormat::Jpeg { width, height } => {
            append_image(document, page, annotation, bytes, format, width, height, fit, rect)?;
        }
    }

    // Normalizes the bounding box, records the content rect a later re-fit
    // reads, and applies any transform keys already on the annotation.
    //
    // The POLICY passed here is not observable on the create path: the
    // placement above already sized the content, so the re-fit finds it
    // filling the box and every policy agrees. It matters for the update
    // path, where a geometry-only change re-fits content it did not place.
    refit(annotation, fit)
}

/// PNG or JPEG: an image object painted inside `/Rect`.
///
/// The appearance's coordinate space lines up with page space, so an image
/// placed outside `/Rect` falls outside the form's clip and renders blank.
/// The fit is computed HERE from the sniffed intrinsic size, because the
/// object does not know its own dimensions until it has a matrix.
#[allow(clippy::too_many_arguments)]
fn append_image(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    bytes: &[u8],
    format: crate::features::binary::BinaryFormat,
    width: u32,
    height: u32,
    fit: super::draft::StampFitDraft,
    rect: crate::geom::PdfRect,
) -> Result<(), EngineError> {
    let object = document.new_image_object().ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFPageObj_NewImageObj returned nothing",
        )
    })?;
    let decoded = match format {
        crate::features::binary::BinaryFormat::Png { .. } => object.set_png(page, bytes),
        _ => object.set_jpeg(page, bytes),
    };
    if !decoded {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "PDFium rejected the stamp image data",
        ));
    }

    let placement = fit_into(f64::from(width), f64::from(height), rect, fit);
    // An image object's unit square IS its whole image, so the matrix is
    // how it gets a size at all: with the identity it draws one point wide.
    if !object.set_matrix(pdfium::MatrixF {
        a: placement.width as f32,
        b: 0.0,
        c: 0.0,
        d: placement.height as f32,
        e: 0.0,
        f: 0.0,
    }) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFPageObj_SetMatrix refused the image size",
        ));
    }
    object.transform(1.0, 0.0, 0.0, 1.0, placement.left, placement.bottom);
    annotation.append_object(object).map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFAnnot_AppendObject refused the image",
        )
    })
}

/// Where content of a given size sits inside a box.
struct Placement {
    left: f64,
    bottom: f64,
    width: f64,
    height: f64,
}

/// The fit policy, as arithmetic.
///
/// The aspect-preserving fits centre what is left over, which is what makes
/// `contain` letterbox evenly rather than pinning the image to a corner.
fn fit_into(
    width: f64,
    height: f64,
    rect: crate::geom::PdfRect,
    fit: super::draft::StampFitDraft,
) -> Placement {
    let box_width = (rect.right - rect.left).max(0.0);
    let box_height = (rect.top - rect.bottom).max(0.0);
    // A zero intrinsic dimension cannot reach here: the sniffer rejects one.
    // The guard is for the divide, not for the file.
    if fit == super::draft::StampFitDraft::Fill || width <= 0.0 || height <= 0.0 {
        return Placement {
            left: rect.left,
            bottom: rect.bottom,
            width: box_width,
            height: box_height,
        };
    }
    let scale = if fit == super::draft::StampFitDraft::Contain {
        (box_width / width).min(box_height / height)
    } else {
        (box_width / width).max(box_height / height)
    };
    let scaled_width = width * scale;
    let scaled_height = height * scale;
    Placement {
        left: rect.left + (box_width - scaled_width) / 2.0,
        bottom: rect.bottom + (box_height - scaled_height) / 2.0,
        width: scaled_width,
        height: scaled_height,
    }
}

/// A single-page PDF: its first page, deep-cloned in as the `/AP`.
///
/// The temporary document is opened only for the clone and closed straight
/// after, because the clone is a copy rather than a reference.
fn set_appearance_from_pdf(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    bytes: &[u8],
) -> Result<(), EngineError> {
    let source = document.open_sibling(bytes).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::MalformedPdf,
            format!("the stamp source PDF could not be opened: {e}"),
        )
    })?;
    if !annotation.set_appearance_from_page(&source, 0) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetAppearanceFromPage refused the page",
        ));
    }
    Ok(())
}

fn refit(
    annotation: &pdfium::Annotation<'_>,
    fit: super::draft::StampFitDraft,
) -> Result<(), EngineError> {
    if !annotation.update_appearance_to_rect(fit.native()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_UpdateAppearanceToRect refused the fit",
        ));
    }
    Ok(())
}

/// `/Name` on a stamp.
///
/// Only an enumerated label reaches here: preflight refused anything else,
/// so a missing code at this point is the preflight and the writer
/// disagreeing rather than a file being unusual.
pub(super) fn set_stamp_name(
    annotation: &pdfium::Annotation<'_>,
    name: &str,
) -> Result<(), EngineError> {
    if !annotation.set_name(name) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFAnnot_SetName refused the stamp label '{name}'"),
        ));
    }
    Ok(())
}

/// Build the target's action and attach it as `/A`, replacing any there.
///
/// Shared with the patch path, where a retarget is the same two calls. It
/// does NOT strip a direct `/Dest`: §12.5.6.5 gives `/A` precedence and the
/// reader follows it, so a stale `/Dest` beside a live `/A` is inert. A
/// patch that CLEARS the target removes both, because with no `/A` the
/// `/Dest` would become live again.
pub(super) fn write_link_target(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    target: &super::draft::LinkTargetDraft,
) -> Result<(), EngineError> {
    let action = match target {
        super::draft::LinkTargetDraft::Uri { uri } => document.create_uri_action(uri),
        super::draft::LinkTargetDraft::Goto { destination } => {
            document.create_goto_action(build_destination(document, destination)?)
        }
    }
    .map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the link's action could not be built: {e}"),
        )
    })?;
    if !annotation.set_action(action) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetAction refused the action",
        ));
    }
    Ok(())
}

/// Turn a wire destination into the array PDFium builds.
///
/// `/XYZ` has its own constructor because its three components are each
/// present-or-retain, and a parameter array has nowhere to put that
/// distinction: a missing `top` would arrive as a zero, which is a real
/// coordinate at the bottom of the page.
///
/// The other fit types go through the array, where a missing optional becomes
/// a SHORTER array rather than a zero: the parameters are positional, so
/// that is the only way for one to be absent.
///
/// PDFium then pads it back. `link-goto-fit-h-retain` sends no top and reads
/// one back as zero, which is why neither shortening the array nor padding it
/// here is observable, and why `/Fit` taking a parameter it has no use for is
/// not either. The array is built to the arity anyway: what this layer hands
/// PDFium is what the destination MEANS, and the padding is PDFium's
/// convention rather than something to write into the call.
pub(super) fn build_destination(
    document: &pdfium::Document,
    destination: &PdfDestination,
) -> Result<pdfium::Destination, EngineError> {
    use pdfium::view;

    let page = destination.page_object_number().get();
    let built = match destination {
        PdfDestination::Xyz { left, top, zoom, .. } => document.create_xyz_destination(
            page,
            left.map(|v| v as f32),
            top.map(|v| v as f32),
            zoom.map(|v| v as f32),
        ),
        PdfDestination::Fit { .. } => document.create_view_destination(page, view::FIT, &[]),
        PdfDestination::FitB { .. } => document.create_view_destination(page, view::FIT_B, &[]),
        PdfDestination::FitH { top, .. } => {
            document.create_view_destination(page, view::FIT_H, &optional(*top))
        }
        PdfDestination::FitBh { top, .. } => {
            document.create_view_destination(page, view::FIT_BH, &optional(*top))
        }
        PdfDestination::FitV { left, .. } => {
            document.create_view_destination(page, view::FIT_V, &optional(*left))
        }
        PdfDestination::FitBv { left, .. } => {
            document.create_view_destination(page, view::FIT_BV, &optional(*left))
        }
        PdfDestination::FitR { left, bottom, right, top, .. } => document.create_view_destination(
            page,
            view::FIT_R,
            &[*left as f32, *bottom as f32, *right as f32, *top as f32],
        ),
    };
    built.map_err(|e| {
        EngineError::with_message(
            match e {
                pdfium::PdfiumError::PageLoadByObjectNumber(_) => EngineErrorCode::NotFound,
                _ => EngineErrorCode::Unknown,
            },
            format!("the link destination could not be built: {e}"),
        )
    })
}

/// One optional coordinate as the parameter list a fit type takes.
fn optional(value: Option<f64>) -> Vec<f32> {
    value.map(|v| vec![v as f32]).unwrap_or_default()
}

/// `/DA`: the font, the size and the colour, as one entry.
///
/// A family that is not one of the fourteen standard names is a key into this
/// thread's font registry. A key with no registry behind it is refused rather
/// than quietly downgraded to Helvetica, which would embed the wrong glyphs
/// into a document that asked for a specific face.
pub(super) fn set_default_appearance(
    annotation: &pdfium::Annotation<'_>,
    family: &str,
    size: f64,
    color: Color,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let rgb = (color.r, color.g, color.b);
    let ok = match crate::features::annotation::vocabulary::StandardFont::from_family(family) {
        Some(standard) => annotation.set_default_appearance(standard.code(), size as f32, rgb),
        None => annotation.set_default_appearance_registered_font(
            context.resolve_font(family)?,
            size as f32,
            rgb,
        ),
    };
    if !ok {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("PDFium refused a /DA naming '{family}'"),
        ));
    }
    Ok(())
}

/// What a shape with no `/BS` resolves to, which is what the reader reports
/// for one.
struct AnnotationBorderStyleDefault;
impl AnnotationBorderStyleDefault {
    const STYLE: crate::features::annotation::vocabulary::AnnotationBorderStyle =
        crate::features::annotation::vocabulary::AnnotationBorderStyle::Solid;
    const WIDTH: f64 = 1.0;
}

/// The smallest box enclosing every quad.
///
/// Shared with the update path, which recomputes it whenever a patch
/// changes the quads: the two must agree or a patched markup would keep a
/// box that no longer encloses it.
pub(super) fn enclosing_rect(quads: &[PdfQuad]) -> crate::geom::PdfRect {
    let mut left = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut top = f64::NEG_INFINITY;
    for quad in quads {
        for point in [quad.p1, quad.p2, quad.p3, quad.p4] {
            left = left.min(point.x);
            right = right.max(point.x);
            bottom = bottom.min(point.y);
            top = top.max(point.y);
        }
    }
    crate::geom::PdfRect { left, bottom, right, top }
}

fn set_rect(
    annotation: &pdfium::Annotation<'_>,
    rect: crate::geom::PdfRect,
) -> Result<(), EngineError> {
    let ok = annotation.set_rect(RectF {
        left: rect.left as f32,
        top: rect.top as f32,
        right: rect.right as f32,
        bottom: rect.bottom as f32,
    });
    if !ok {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetRect refused the rectangle",
        ));
    }
    Ok(())
}

/// Set an entry, or REMOVE it when the patch is a clear.
///
/// Setting a null value is not removal: it writes an empty string, and a
/// cleared field would then read back as present-and-empty rather than as
/// absent. The read side distinguishes the two, so the write side has to.
pub(super) fn write_patch_string(
    annotation: &pdfium::Annotation<'_>,
    key: &str,
    value: &crate::dto::Patch<String>,
) -> Result<(), EngineError> {
    match value {
        crate::dto::Patch::Keep => Ok(()),
        crate::dto::Patch::Set(value) => write(annotation.set_string_value(key, value), key),
        crate::dto::Patch::Clear => {
            // A key that was not there is not an error to remove.
            //
            // Indistinguishable from writing an empty string on a FRESH
            // annotation: PDFium leaves no key behind either way, which is
            // why `highlight-cleared-strings` cannot tell the two apart.
            // The difference is on the update path, where an existing value
            // has to become absent rather than empty, and the read side
            // reports those differently.
            annotation.remove_key(key).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("removing /{key} failed: {e}"),
                )
            })?;
            Ok(())
        }
    }
}

pub(super) fn write(
    result: Result<bool, pdfium::PdfiumError>,
    what: &str,
) -> Result<(), EngineError> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("PDFium refused the write to {what}"),
        )),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("writing {what} failed: {e}"),
        )),
    }
}
