//! The writer `forms.createField` still places its widget with.
//!
//! G11. What was left of the old `annotations.create` writer when Task 8.14
//! ported upstream's create to [`super::create`] and 8.15 its update to
//! [`super::update`]: [`apply_widget`], and the destination builder the new
//! link writer shares. Task 8.21 ports the widget placement and deletes this
//! file, moving [`build_destination`] beside the link writer.

use pdfium::RectF;

use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::read::style::Color;

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

/// What a shape with no `/BS` resolves to, which is what the reader reports
/// for one.
struct AnnotationBorderStyleDefault;
impl AnnotationBorderStyleDefault {
    const STYLE: crate::features::annotation::vocabulary::AnnotationBorderStyle =
        crate::features::annotation::vocabulary::AnnotationBorderStyle::Solid;
    const WIDTH: f64 = 1.0;
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
