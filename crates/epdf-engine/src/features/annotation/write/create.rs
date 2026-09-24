//! Creating an annotation.
//!
//! G11. Ports the `create` half of
//! `services/src/features/annotations/AnnotationMutator.ts` and the draft
//! side of `internal/write/*`. Operation: `annotations.create`.
//!
//! # A new annotation is born durable or not at all
//!
//! Creation goes through the fork's `EPDFPage_CreateAnnot`, which makes an
//! INDIRECT object, so the result has a PDF object number and therefore an
//! address that survives a reordering. The object number is checked
//! afterwards rather than assumed: an annotation created weak would be one
//! nobody can refer to, and the engine refuses rather than returning it.
//!
//! # The order of writes is not arbitrary
//!
//! The subtype's own fields go first, then identity and modification
//! metadata, then the appearance. Identity after the subtype writer so a
//! buggy writer cannot clobber `/T` or `/M`; the appearance last so it is
//! baked from a dictionary that is already complete.
//!
//! # The apply boundary
//!
//! Everything that can refuse the request happens BEFORE the first write:
//! the draft is validated and the cancellation token is checked, and after
//! that the only failures are PDFium's own. A create that failed halfway
//! would leave an annotation in the file that no caller asked for, and
//! nothing here can roll one back.

use pdfium::{PageView, QuadF, RectF};
use serde::{Deserialize, Serialize};

use super::draft::{
    AnnotationActor, AnnotationDraft, AnnotationDraftBase, AnnotationDraftKind, ShapeDraftFields,
    TextMarkupDraftFields,
};
use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_date};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::dto::AnnotationDto;
use crate::features::annotation::read::page::read_page_annotations;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::read::text_markup::{
    DEFAULT_HIGHLIGHT_COLOR, DEFAULT_TEXT_MARKUP_COLOR,
};
use crate::features::annotation::vocabulary::AnnotationColorType;
use crate::features::annotation::write::draft::AnnotationFlagsPatch;
use crate::geom::PdfQuad;
use crate::identity::{AnnotationStableId, PageObjectNumber};
use crate::revision::PageState;

/// What a create produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationCreateResult {
    /// The new annotation, read back through the ordinary reader. Always
    /// `durable`, because creation goes through the indirect helper.
    pub created: AnnotationDto,
    pub meta: AnnotationListMutationMeta,
}

/// What an absent `opacity` writes.
///
/// PDFium's `/CA` defaults to 1 when the key is absent, and this writes it
/// anyway so a read always round-trips the same number rather than depending
/// on whether the key happens to be there.
const DEFAULT_OPACITY: f64 = 1.0;

/// Create one annotation on a page.
///
/// `page` is the page to create on, already acquired by the caller, and
/// `page_object_number` is its identity. The two are separate arguments
/// because this layer does not own a page pool and will not invent one: a
/// page acquired and released around a single call is a different lifetime
/// from a page held across several.
#[allow(clippy::too_many_arguments)]
pub fn create_annotation(
    document: &pdfium::Document,
    page: &PageView<'_>,
    page_object_number: PageObjectNumber,
    state_before: &PageState,
    state_after: &PageState,
    draft: &AnnotationDraft,
    actor: &AnnotationActor,
    clock: &dyn Clock,
    cancel: &CancelToken,
) -> Result<AnnotationCreateResult, EngineError> {
    cancel.check()?;
    // The apply boundary. Everything below this line writes.
    preflight(draft)?;
    cancel.check()?;

    let subtype = draft.kind.subtype();
    let annotation = page.create_annotation(subtype.code()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFPage_CreateAnnot returned nothing for {}", subtype.as_str()),
        )
    })?;

    apply_base_draft(&annotation, &draft.base)?;
    apply_kind_draft(&annotation, &draft.kind)?;

    // After the subtype writer, so a writer that set `/T` or `/M` itself
    // cannot win.
    if let Some(name) = actor.display_name.as_deref().filter(|n| !n.is_empty()) {
        write(annotation.set_string_value("T", name), "/T")?;
    }
    let modified = format_pdf_date(clock.now()).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the clock produced no PDF date")
    })?;
    write(annotation.set_string_value("M", &modified), "/M")?;

    // The appearance last, from a dictionary that is now complete.
    let blend = draft.base.blend_mode.map(|mode| mode.code());
    annotation.generate_appearance(blend);
    // Regenerating the page's CONTENT stream after creating an annotation
    // is what the TypeScript does and it changes nothing observable here:
    // an annotation lives in `/Annots`, not in the content stream, and the
    // new appearance survives a save and a reopen without this call.
    // Removing it passes every test. It is kept because the TypeScript is
    // the specification for this port, and because the day a writer touches
    // a page object rather than an annotation, its absence would be a page
    // that renders the old content.
    page.generate_content();

    let object_number = annotation.object_number();
    // Never seen to fire: the fork's helper creates an indirect object by
    // construction. It is checked anyway, because an annotation created
    // without an object number is one no caller can address, and finding
    // that out at the point of creation is far cheaper than finding it out
    // when a reference fails to resolve.
    if object_number == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFPage_CreateAnnot produced a direct object with no object number; \
             a new annotation must be addressable",
        ));
    }
    let index = page.annotation_index(&annotation);
    if index < 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the new annotation is not in the page's /Annots (index {index})"),
        ));
    }
    // Closed before the read-back, because the reader opens its own handle
    // and two live handles to one annotation is a shape nothing here needs.
    drop(annotation);
    cancel.check()?;

    // Read back through the ORDINARY reader rather than by describing what
    // was just written. What the caller receives is then what any later
    // listing will say, including anything PDFium normalized on the way in.
    let listing = read_page_annotations(
        document,
        page_index_of(document, page_object_number)?,
        page_object_number,
        &state_before.revision,
        &mut crate::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        cancel,
    )?;
    let created = listing.annotations.into_iter().nth(index as usize).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the new annotation is at index {index} and the listing is shorter"),
        )
    })?;

    let meta = compute_mutation_impact(
        AnnotationMutationKind::Create,
        state_before,
        state_after,
        vec![AnnotationStableId::ObjectNumber { value: object_number }],
    )
    .map_err(|e| EngineError::with_message(EngineErrorCode::InvalidArg, e.to_string()))?;

    Ok(AnnotationCreateResult { created, meta })
}

/// Everything that can refuse a draft, before anything is written.
fn preflight(draft: &AnnotationDraft) -> Result<(), EngineError> {
    match &draft.kind {
        AnnotationDraftKind::Highlight { fields }
        | AnnotationDraftKind::Underline { fields }
        | AnnotationDraftKind::Squiggly { fields }
        | AnnotationDraftKind::Strikeout { fields, .. } => {
            if fields.quad_points.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "a {} draft needs at least one quad: /Rect is derived from them",
                        draft.kind.subtype().as_str()
                    ),
                ));
            }
        }
        AnnotationDraftKind::Square { fields } | AnnotationDraftKind::Circle { fields } => {
            normalized(&fields.rect)?;
        }
        AnnotationDraftKind::Polygon { fields, .. }
        | AnnotationDraftKind::Polyline { fields, .. } => {
            normalized(&fields.geometry.rect)?;
            // A polygon of one point is a point, and PDFium will write it.
            // Two is the fewest that draws anything, and a caller that sent
            // fewer has a bug rather than an intent.
            if fields.vertices.len() < 2 {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "a {} draft needs at least two vertices",
                        draft.kind.subtype().as_str()
                    ),
                ));
            }
        }
        AnnotationDraftKind::Line { fields, .. } => normalized(&fields.rect)?,
        AnnotationDraftKind::Ink { fields, ink_list, .. } => {
            normalized(&fields.rect)?;
            // An ink annotation with no strokes draws nothing, and one with
            // an empty stroke has a pen path of no points.
            if ink_list.is_empty() || ink_list.iter().any(Vec::is_empty) {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "an ink draft needs at least one stroke and every stroke needs a point",
                ));
            }
        }
    }
    if let Some(opacity) = opacity_of(&draft.kind)
        && !(0.0..=1.0).contains(&opacity)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("opacity {opacity} is outside 0..=1"),
        ));
    }
    Ok(())
}

fn opacity_of(kind: &AnnotationDraftKind) -> Option<f64> {
    match kind {
        AnnotationDraftKind::Highlight { fields }
        | AnnotationDraftKind::Underline { fields }
        | AnnotationDraftKind::Squiggly { fields }
        | AnnotationDraftKind::Strikeout { fields, .. } => fields.opacity,
        AnnotationDraftKind::Square { fields } | AnnotationDraftKind::Circle { fields } => {
            fields.opacity
        }
        AnnotationDraftKind::Polygon { fields, .. }
        | AnnotationDraftKind::Polyline { fields, .. } => fields.geometry.opacity,
        AnnotationDraftKind::Line { fields, .. } => fields.opacity,
        AnnotationDraftKind::Ink { fields, .. } => fields.opacity,
    }
}

/// A rect a caller can draw in.
fn normalized(rect: &crate::geom::PdfRect) -> Result<(), EngineError> {
    if rect.right < rect.left || rect.top < rect.bottom {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a draft's rect must be normalized: left <= right and bottom <= top",
        ));
    }
    Ok(())
}

fn apply_base_draft(
    annotation: &pdfium::Annotation<'_>,
    base: &AnnotationDraftBase,
) -> Result<(), EngineError> {
    write_patch_string(annotation, "Contents", &base.contents)?;
    write_patch_string(annotation, "Subj", &base.subject)?;
    // An empty `/NM` is not an identity, so it is not written: two
    // annotations could both carry it and neither would be addressable.
    if let Some(nm) = base.nm.as_deref().filter(|nm| !nm.is_empty()) {
        write(annotation.set_string_value("NM", nm), "/NM")?;
    }
    if let Some(flags) = base.flags {
        apply_flags(annotation, flags)?;
    }
    Ok(())
}

fn apply_flags(
    annotation: &pdfium::Annotation<'_>,
    patch: AnnotationFlagsPatch,
) -> Result<(), EngineError> {
    // Folded onto the live word rather than built from nothing, so a
    // reserved bit the file carried survives.
    let next = patch.fold(annotation.flags());
    if !annotation.set_flags(next) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFAnnot_SetFlags refused the /F word",
        ));
    }
    Ok(())
}

fn apply_kind_draft(
    annotation: &pdfium::Annotation<'_>,
    kind: &AnnotationDraftKind,
) -> Result<(), EngineError> {
    match kind {
        AnnotationDraftKind::Highlight { fields } => {
            apply_text_markup(annotation, fields, DEFAULT_HIGHLIGHT_COLOR)
        }
        AnnotationDraftKind::Underline { fields } | AnnotationDraftKind::Squiggly { fields } => {
            apply_text_markup(annotation, fields, DEFAULT_TEXT_MARKUP_COLOR)
        }
        AnnotationDraftKind::Strikeout { fields, intent } => {
            apply_text_markup(annotation, fields, DEFAULT_TEXT_MARKUP_COLOR)?;
            if let Some(intent) = intent {
                write(annotation.set_intent(intent.name()), "/IT")?;
            }
            Ok(())
        }
        AnnotationDraftKind::Square { fields } | AnnotationDraftKind::Circle { fields } => {
            apply_shape(annotation, fields)
        }
        AnnotationDraftKind::Polygon { fields, cloudy_intensity } => {
            apply_vertex(annotation, fields)?;
            // Only a polygon takes this: an open path has no interior for a
            // cloudy border to enclose.
            //
            // The zero filter is not observable through a reader, and it is
            // kept anyway. `read_cloudy_intensity` normalizes a `/BE` of
            // intensity zero back to absent, so a writer without the filter
            // round-trips identically; what differs is the file, which then
            // carries a `/BE` dictionary declaring a cloudy border with no
            // waviness. Another consumer is not obliged to normalize it.
            if let Some(intensity) = cloudy_intensity.as_set().copied().filter(|i| *i > 0.0)
                && !annotation.set_border_effect(intensity as f32)
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "EPDFAnnot_SetBorderEffect refused the intensity",
                ));
            }
            Ok(())
        }
        AnnotationDraftKind::Polyline { fields, line_endings } => {
            apply_vertex(annotation, fields)?;
            set_line_endings(annotation, *line_endings)
        }
        AnnotationDraftKind::Line { fields, line_points, line_endings } => {
            apply_geometry_style(annotation, fields)?;
            set_rect(annotation, fields.rect)?;
            if !annotation
                .set_line(point_to_native(line_points.start), point_to_native(line_points.end))
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "EPDFAnnot_SetLine refused the endpoints",
                ));
            }
            set_line_endings(annotation, *line_endings)?;
            write_advisory_rotation(annotation, fields.rotation)
        }
        AnnotationDraftKind::Ink { fields, ink_list, intent } => {
            apply_stroke_style(annotation, fields)?;
            set_rect(annotation, fields.rect)?;
            if let Some(intent) = intent {
                write(annotation.set_intent(intent.name()), "/IT")?;
            }
            for (index, stroke) in ink_list.iter().enumerate() {
                let points: Vec<pdfium::PointF> =
                    stroke.iter().copied().map(point_to_native).collect();
                if annotation.add_ink_stroke(&points) < 0 {
                    return Err(EngineError::with_message(
                        EngineErrorCode::Unknown,
                        format!("FPDFAnnot_AddInkStroke refused stroke {index}"),
                    ));
                }
            }
            write_advisory_rotation(annotation, fields.rotation)
        }
    }
}

/// The styling a filled geometric kind carries, plus its advisory rotation.
fn apply_geometry_style(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::GeometryDraftFields,
) -> Result<(), EngineError> {
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::style::DEFAULT_GEOMETRY_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    set_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;
    // The clear is inert on the create path and load-bearing off it. A
    // freshly created annotation has no `/IC` to remove, so dropping this
    // arm passes every create vector; these fields are the same ones an
    // update folds onto an annotation that may already have a fill, and
    // there the difference between clearing and doing nothing is the whole
    // point of `Patch::Clear`.
    match fields.interior_color.as_set() {
        Some(color) => set_color(annotation, AnnotationColorType::InteriorColor, *color)?,
        None => {
            annotation.clear_color(AnnotationColorType::InteriorColor as i32);
        }
    }
    Ok(())
}

/// The same without a fill.
fn apply_stroke_style(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::StrokeDraftFields,
) -> Result<(), EngineError> {
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::style::DEFAULT_GEOMETRY_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    set_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)
}

fn apply_vertex(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::VertexDraftFields,
) -> Result<(), EngineError> {
    apply_geometry_style(annotation, &fields.geometry)?;
    set_rect(annotation, fields.geometry.rect)?;
    let points: Vec<pdfium::PointF> =
        fields.vertices.iter().copied().map(point_to_native).collect();
    if !annotation.set_vertices(&points) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetVertices refused the point list",
        ));
    }
    write_advisory_rotation(annotation, fields.geometry.rotation)
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

/// `/LE`, defaulting to the pair that draws nothing.
fn set_line_endings(
    annotation: &pdfium::Annotation<'_>,
    endings: Option<crate::features::annotation::vocabulary::LineEndings>,
) -> Result<(), EngineError> {
    use crate::features::annotation::vocabulary::{LineEnding, LineEndings};
    let endings = endings.unwrap_or(LineEndings { start: LineEnding::None, end: LineEnding::None });
    if !annotation.set_line_endings(endings.start.code(), endings.end.code()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetLineEndings refused the pair",
        ));
    }
    Ok(())
}

/// The ADVISORY rotation these families carry.
///
/// Advisory because their points are already rotated: the scalar records the
/// angle that was applied so a UI can draw an oriented selection box, and
/// PDFium ignores it when baking. An unrotated annotation carries no key,
/// because zero degrees is not a rotation and stamping one would put a key
/// on a file that had none.
fn write_advisory_rotation(
    annotation: &pdfium::Annotation<'_>,
    rotation: Option<f64>,
) -> Result<(), EngineError> {
    let Some(rotation) = rotation else {
        return Ok(());
    };
    let result = if rotation == 0.0 {
        annotation.clear_embed_metadata_key("Rotation")
    } else {
        annotation.set_embed_metadata_number("Rotation", rotation as f32)
    };
    result.map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("writing the advisory rotation failed: {e}"),
        )
    })?;
    // A vertex kind never carries the logical box: its points ARE the
    // visual, so there is no pre-rotation rectangle to remember. Inert on
    // create for the same reason the interior-colour clear above is: there
    // is nothing on a new annotation to remove. It runs so that a kind
    // whose rect stopped being derived from a rotation does not keep a
    // stale box that a later reader would trust.
    annotation.clear_embed_metadata_key("UnrotatedRect").map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("clearing the unrotated rect failed: {e}"),
        )
    })?;
    Ok(())
}

fn point_to_native(point: crate::geom::PdfPoint) -> pdfium::PointF {
    pdfium::PointF { x: point.x as f32, y: point.y as f32 }
}

fn apply_text_markup(
    annotation: &pdfium::Annotation<'_>,
    fields: &TextMarkupDraftFields,
    fallback: Color,
) -> Result<(), EngineError> {
    for quad in &fields.quad_points {
        if !annotation.append_quad_point(quad_to_native(*quad)) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "FPDFAnnot_AppendAttachmentPoints refused a quad",
            ));
        }
    }
    // `/Rect` is the smallest box enclosing every quad, derived rather than
    // taken: §12.5.6.10 requires it, and a caller supplying both could
    // supply two things that disagree.
    set_rect(annotation, enclosing_rect(&fields.quad_points))?;

    let color = fields.color.unwrap_or(fallback);
    set_color(annotation, AnnotationColorType::Color, color)?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))
}

fn apply_shape(
    annotation: &pdfium::Annotation<'_>,
    fields: &ShapeDraftFields,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::style::DEFAULT_GEOMETRY_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;

    // `/BS` is one call taking both, so a draft that names only one still
    // has to supply the other. The defaults are the resolved ones a reader
    // would report for an annotation with no `/BS` at all.
    set_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;

    // `/IC` is CLEARED when the draft does not name one, rather than left
    // alone. On a fresh annotation the two are the same thing, so no test
    // can separate them yet; they are written this way because the
    // TypeScript is, and because a draft that says nothing about a fill
    // means a shape with no fill rather than a shape whose fill is whatever
    // was there.
    match fields.interior_color.as_set() {
        Some(color) => set_color(annotation, AnnotationColorType::InteriorColor, *color)?,
        None => {
            annotation.clear_color(AnnotationColorType::InteriorColor as i32);
        }
    }
    // `/BE` and `/RD` are the other way round: a draft only ever WRITES them.
    // There is nothing on a new annotation to clear, and an intensity of
    // zero draws no clouds, so it is skipped rather than written as a value
    // the reader would normalize away. Skipping and writing the zero produce
    // the same DTO, because the reader normalizes it either way; what they
    // differ in is whether the file carries a `/BE` that means nothing.
    if let Some(intensity) = fields.cloudy_intensity.as_set().copied().filter(|i| *i > 0.0)
        && !annotation.set_border_effect(intensity as f32)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetBorderEffect refused the intensity",
        ));
    }
    if let Some(rd) = fields.rect_differences.as_set().copied()
        && !annotation.set_rectangle_differences(
            rd.left as f32,
            rd.bottom as f32,
            rd.right as f32,
            rd.top as f32,
        )
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetRectangleDifferences refused the margins",
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

fn quad_to_native(quad: PdfQuad) -> QuadF {
    QuadF {
        x1: quad.p1.x as f32,
        y1: quad.p1.y as f32,
        x2: quad.p2.x as f32,
        y2: quad.p2.y as f32,
        x3: quad.p3.x as f32,
        y3: quad.p3.y as f32,
        x4: quad.p4.x as f32,
        y4: quad.p4.y as f32,
    }
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

fn set_color(
    annotation: &pdfium::Annotation<'_>,
    which: AnnotationColorType,
    color: Color,
) -> Result<(), EngineError> {
    if !annotation.set_color(which as i32, color.r, color.g, color.b) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetColor refused the colour",
        ));
    }
    Ok(())
}

fn set_opacity(annotation: &pdfium::Annotation<'_>, opacity: f64) -> Result<(), EngineError> {
    if !annotation.set_opacity(opacity) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOpacity refused the value",
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

/// The page index for an object number.
fn page_index_of(document: &pdfium::Document, page: PageObjectNumber) -> Result<u32, EngineError> {
    (0..document.page_count())
        .find(|index| document.page_object_number_by_index(*index) == page.get())
        .ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no page with object number {}", page.get()),
            )
        })
}
