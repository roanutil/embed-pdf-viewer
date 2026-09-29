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

use super::context::AnnotationWriteContext;
use super::draft::{
    AnnotationActor, AnnotationDraft, AnnotationDraftBase, AnnotationDraftKind, ShapeDraftFields,
    TextMarkupDraftFields,
};
use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_date};
use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::intent::FreeTextIntent;
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
    context: &AnnotationWriteContext<'_>,
    clock: &dyn Clock,
    cancel: &CancelToken,
) -> Result<AnnotationCreateResult, EngineError> {
    cancel.check()?;
    // The apply boundary. Everything below this line writes.
    preflight(draft, context)?;
    cancel.check()?;

    let subtype = draft.kind.subtype();
    let annotation = page.create_annotation(subtype.code()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFPage_CreateAnnot returned nothing for {}", subtype.as_str()),
        )
    })?;

    apply_base_draft(&annotation, &draft.base)?;
    apply_kind_draft(document, page, &annotation, &draft.kind, context)?;

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
fn preflight(
    draft: &AnnotationDraft,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
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
            box_rotation_satisfiable(fields.rotation, fields.unrotated_rect)?;
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
        AnnotationDraftKind::Text { fields } => {
            normalized(&fields.rect)?;
            // §12.5.6.3: a `/State` names a value WITHIN a model, so one
            // without the other is a state nothing can interpret. The rule
            // is draft-only. A patch may legitimately set one alone,
            // because the other can already be on the annotation.
            if fields.state.is_some() && fields.state_model.is_none() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "a text draft with a state needs a stateModel (ISO 32000 §12.5.6.3)",
                ));
            }
        }
        AnnotationDraftKind::Caret { fields } => {
            normalized(&fields.rect)?;
            box_rotation_satisfiable(fields.rotation, fields.unrotated_rect)?;
        }
        AnnotationDraftKind::Stamp { fields } => {
            normalized(&fields.rect)?;
            box_rotation_satisfiable(fields.rotation, fields.unrotated_rect)?;
            // The bytes are sniffed BEFORE the annotation exists. A stamp
            // with no appearance is an empty box in the file, and the only
            // thing that could tell a reader why is a log nobody has.
            let resource = context.resource(&fields.source.resource)?;
            if crate::features::binary::sniff(resource.bytes).is_none() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "a stamp's source must be PNG, JPEG or PDF bytes",
                ));
            }
            // The READER reports a custom label and this refuses to author
            // one, and the asymmetry is deliberate. A file may carry any
            // `/Name` and dropping it would lose what the document said; a
            // label this engine writes should be one a viewer can draw, and
            // the twenty-three the fork enumerates are that set.
            if let Some(name) = fields.name.as_deref()
                && !crate::features::annotation::vocabulary::is_standard_stamp_name(name)
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("'{name}' is not one of the standard stamp labels"),
                ));
            }
        }
        AnnotationDraftKind::Widget { fields } => normalized(&fields.rect)?,
        AnnotationDraftKind::Link { fields } => {
            normalized(&fields.rect)?;
            // A URI with nothing in it is a link that navigates nowhere
            // while claiming to navigate. `target: null` already says that
            // honestly, so the empty string is the caller's mistake.
            if let Some(super::draft::LinkTargetDraft::Uri { uri }) = &fields.target
                && uri.is_empty()
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "a link's uri is empty: send target null for a link with no destination yet",
                ));
            }
        }
        AnnotationDraftKind::Redact { fields } => {
            normalized(&fields.rect)?;
            if fields.touches_label_style()
                && let Some(family) = fields.font_family.as_deref()
            {
                font_resolvable(family, context)?;
            }
        }
        AnnotationDraftKind::FileAttachment { fields } => {
            normalized(&fields.rect)?;
            // The payload is checked BEFORE the annotation exists. A marker
            // in the file pointing at a filespec with no stream is worse
            // than no marker: a reader finds an attachment it cannot open.
            context.resource(&fields.file.resource)?;
            if fields.file.name.is_empty() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "an attachment's file needs a name: a filespec without one names nothing",
                ));
            }
        }
        AnnotationDraftKind::FreeText { fields } => {
            normalized(&fields.rect)?;
            // A callout with no leader is a plain box wearing the wrong
            // `/IT`, and a leader on a plain box is geometry nothing draws.
            // Both are the caller having decided one thing and said another.
            let is_callout = fields.intent == FreeTextIntent::FreeTextCallout;
            if is_callout != fields.callout_line.is_some() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    if is_callout {
                        "a free-text-callout draft needs a calloutLine"
                    } else {
                        "a calloutLine belongs to a free-text-callout, not a plain text box"
                    },
                ));
            }
            if fields.font_size <= 0.0 {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "a free-text draft's fontSize must be positive, not {}",
                        fields.font_size
                    ),
                ));
            }
            box_rotation_satisfiable(fields.rotation, fields.unrotated_rect)?;
            font_resolvable(&fields.font_family, context)?;
        }
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

/// A box kind's rotation needs the box it turns.
///
/// Checked in PREFLIGHT rather than at the write, and that placement is the
/// whole point. The transform writer refuses the same pair, but by the time
/// it runs the annotation exists, and nothing in this layer can take one back
/// out of the document. A draft is always the create case, so there is never
/// an existing `/UnrotatedRect` for a lone rotation to lean on.
fn box_rotation_satisfiable(
    rotation: Option<f64>,
    unrotated_rect: Option<crate::geom::PdfRect>,
) -> Result<(), EngineError> {
    if rotation.is_some_and(|degrees| degrees != 0.0) && unrotated_rect.is_none() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "rotation requires unrotatedRect: a box kind's appearance is baked from the pair",
        ));
    }
    Ok(())
}

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
        AnnotationDraftKind::Text { fields } => fields.opacity,
        AnnotationDraftKind::Caret { fields } => fields.opacity,
        AnnotationDraftKind::FreeText { fields } => fields.opacity,
        AnnotationDraftKind::Redact { fields } => fields.opacity,
        AnnotationDraftKind::FileAttachment { fields } => fields.opacity,
        // A link has no `/CA`: it draws nothing, so there is no opacity for
        // one to have. A stamp's content carries its own transparency.
        // A widget has no `/CA` either: its transparency is `/MK /BG` being
        // absent rather than a number on the annotation.
        AnnotationDraftKind::Link { .. }
        | AnnotationDraftKind::Stamp { .. }
        | AnnotationDraftKind::Widget { .. } => None,
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
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    kind: &AnnotationDraftKind,
    context: &AnnotationWriteContext<'_>,
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
            super::transform::write_vertex_transform(annotation, option_patch(fields.rotation))
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
            super::transform::write_vertex_transform(annotation, option_patch(fields.rotation))
        }
        AnnotationDraftKind::Text { fields } => apply_text(annotation, fields),
        AnnotationDraftKind::Caret { fields } => apply_caret(annotation, fields),
        AnnotationDraftKind::FreeText { fields } => apply_free_text(annotation, fields, context),
        AnnotationDraftKind::Redact { fields } => apply_redact(annotation, fields, context),
        AnnotationDraftKind::FileAttachment { fields } => {
            apply_file_attachment(document, annotation, fields, context)
        }
        AnnotationDraftKind::Link { fields } => apply_link(document, annotation, fields),
        AnnotationDraftKind::Stamp { fields } => {
            apply_stamp(document, page, annotation, fields, context)
        }
        AnnotationDraftKind::Widget { fields } => apply_widget(annotation, fields),
    }
}

/// A sticky note.
///
/// Nothing here describes a shape. The generator bakes the icon from `/C`
/// and `/Name`, and `/State` with `/StateModel` are dictionary-only
/// (§12.5.6.3): they never reach the generator, so a note that has been
/// accepted looks exactly like one that has not.
fn apply_text(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::TextDraftFields,
) -> Result<(), EngineError> {
    use crate::features::annotation::state::{state_model_to_pdf, state_to_pdf};

    set_rect(annotation, fields.rect)?;
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::text::DEFAULT_NOTE_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    let icon = fields.icon.unwrap_or(crate::features::annotation::vocabulary::DEFAULT_NOTE_ICON);
    if !annotation.set_name(icon.pdf_name()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetName refused the icon",
        ));
    }
    // The model lands first. Either order produces the same dictionary, and
    // this one means a reader that saw a partial write would find a model
    // with no state rather than a state belonging to no model.
    if let Some(model) = fields.state_model.as_deref() {
        write(
            annotation.set_string_value("StateModel", &state_model_to_pdf(model)),
            "/StateModel",
        )?;
    }
    if let Some(state) = fields.state.as_deref() {
        write(annotation.set_string_value("State", &state_to_pdf(state)), "/State")?;
    }
    Ok(())
}

/// A caret: a `/Rect`, the inset of the drawn symbol from it, and an `/IT`
/// saying whether it is half of a replace-text edit.
fn apply_caret(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::CaretDraftFields,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    // Before the styling, so the bake at the end of the mutation sees it.
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: option_patch(fields.rotation),
            unrotated_rect: option_patch(fields.unrotated_rect),
        },
        || Ok(false),
    )?;
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::caret::DEFAULT_CARET_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    if let Some(intent) = fields.intent {
        write(annotation.set_intent(intent.name()), "/IT")?;
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
    super::transform::write_vertex_transform(annotation, option_patch(fields.geometry.rotation))
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

/// A draft's `Option` field as the tri-state the transform writer takes.
///
/// A draft has no third state: it is a whole annotation, so an absent field
/// is one the annotation does not have. `None` therefore maps to `Keep`,
/// which on a freshly created annotation means the same as `Clear` and costs
/// one fewer call.
fn option_patch<T>(value: Option<T>) -> Patch<T> {
    value.map_or(Patch::Keep, Patch::Set)
}

/// A free text, and the callout that is the same annotation with a leader.
///
/// The colour order matters. `/DA` carries the border and the default text
/// colour together, so the `TextColor` override has to land AFTER it or the
/// `/DA` write would put the border colour back over the text.
fn apply_free_text(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::FreeTextDraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;

    // `/C` on a free text is the BOX BACKGROUND, not the mark. Absent is a
    // transparent box, and it is cleared rather than left alone so a draft
    // that says nothing about a background gets one that is not there. Inert
    // on the create path, where there is nothing to clear; it is the same
    // fields an update folds onto a box that may already have a background.
    match fields.interior_color {
        Some(color) => set_color(annotation, AnnotationColorType::Color, color)?,
        None => {
            annotation.clear_color(AnnotationColorType::Color as i32);
        }
    }
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    set_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;

    let da_color = fields.color.unwrap_or(super::super::read::free_text::DEFAULT_FREE_TEXT_COLOR);
    set_default_appearance(annotation, &fields.font_family, fields.font_size, da_color, context)?;
    // The TypeScript writes the override second so that it "wins". It does
    // not have to: `/DA` and `TextColor` are two different entries, and
    // swapping these two lines changes nothing in the file or in a read.
    // The order is kept because a future `/DA` writer that also reset the
    // override would make it matter, silently.
    if let Some(font_color) = fields.font_color {
        set_color(annotation, AnnotationColorType::TextColor, font_color)?;
    }

    if !annotation.set_text_alignment(fields.text_align.code()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetTextAlignment refused the alignment",
        ));
    }
    write(annotation.set_intent(fields.intent.name()), "/IT")?;

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

    if let Some(callout) = fields.callout_line.as_ref() {
        let points: Vec<pdfium::PointF> = match callout {
            crate::geom::CalloutLine::Straight(points) => {
                points.iter().copied().map(point_to_native).collect()
            }
            crate::geom::CalloutLine::Kneed(points) => {
                points.iter().copied().map(point_to_native).collect()
            }
        };
        if !annotation.set_callout_line(&points) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetCalloutLine refused the leader",
            ));
        }
    }
    // The `/LE` pair, with the ending on the CALLED-OUT end. The other end
    // touches the text box, where an arrowhead would be drawn into the box
    // it belongs to, so it is always the one that draws nothing.
    if let Some(ending) = fields.line_ending {
        set_line_endings(
            annotation,
            Some(crate::features::annotation::vocabulary::LineEndings {
                start: crate::features::annotation::vocabulary::LineEnding::None,
                end: ending,
            }),
        )?;
    }

    // A plain box rotates like a square. A callout's rotation turns its TEXT
    // BOX only: the leader stays in page space, because a leader that turned
    // with the box would stop pointing at the thing it calls out.
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: option_patch(fields.rotation),
            unrotated_rect: option_patch(fields.unrotated_rect),
        },
        || Ok(false),
    )
}

/// A redaction marking.
///
/// `/Rect` is written from the draft rather than derived from the quads, and
/// that is the difference from the text-markup family. A text redaction's
/// quads are the regions and the box is the caller's arithmetic over them; an
/// AREA redaction has no quads at all and `/Rect` IS the region, so there
/// would be nothing to derive it from.
fn apply_redact(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::RedactDraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    for quad in &fields.quad_points {
        if !annotation.append_quad_point(quad_to_native(*quad)) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "FPDFAnnot_AppendAttachmentPoints refused a quad",
            ));
        }
    }

    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::redact::DEFAULT_REDACT_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    // `/IC` is WRITTEN or absent, never cleared. Absent is the ISO default
    // and means the region is left transparent after the apply, which is a
    // different instruction from filling it with anything, white included.
    if let Some(fill) = fields.interior_color {
        set_color(annotation, AnnotationColorType::InteriorColor, fill)?;
    }

    // An empty label is not a label. Writing one would put an `/OverlayText`
    // on the annotation that draws nothing and obliges a `/DA` beside it.
    //
    // The filter is not observable: PDFium answers an empty `/OverlayText`
    // the same way it answers an absent one, so a build without it passes
    // `a_redaction_gets_a_default_appearance_only_when_it_has_a_label` too.
    // It stays because the `/DA` rule above reads the SAME emptiness, and
    // two places deciding what an empty label means should decide it once.
    if let Some(text) = fields.overlay_text.as_deref().filter(|text| !text.is_empty())
        && !annotation.set_overlay_text(text)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOverlayText refused the label",
        ));
    }
    // Only written when true: `/Repeat` absent already means do not tile.
    if fields.repeat && !annotation.set_overlay_text_repeat(true) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOverlayTextRepeat refused",
        ));
    }

    if fields.touches_label_style() {
        let family = fields
            .font_family
            .as_deref()
            .unwrap_or(crate::features::annotation::vocabulary::DEFAULT_STANDARD_FONT.as_str());
        set_default_appearance(
            annotation,
            family,
            fields.font_size.unwrap_or(super::super::read::redact::DEFAULT_FONT_SIZE),
            fields.font_color.unwrap_or(super::super::read::redact::DEFAULT_LABEL_COLOR),
            context,
        )?;
    }
    // `/Q` only when asked. Unlike free text, where the alignment is part of
    // the required triple, a redaction with no label has nothing to align.
    if let Some(align) = fields.text_align
        && !annotation.set_text_alignment(align.code())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetTextAlignment refused the alignment",
        ));
    }
    Ok(())
}

/// A file-attachment marker, and the file it names.
///
/// Two writes that look like one. `FPDFAnnot_AddFileAttachment` creates the
/// `/FS` filespec, which carries the NAME; the bytes go into the stream it
/// points at, and PDFium derives the size, the checksum and the creation date
/// from them. A marker with a filespec and no stream is a reader finding an
/// attachment it cannot open, which is why the payload is checked in
/// preflight rather than here.
fn apply_file_attachment(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::FileAttachmentDraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    set_color(
        annotation,
        AnnotationColorType::Color,
        fields.color.unwrap_or(super::super::read::file_attachment::DEFAULT_FILE_ATTACHMENT_COLOR),
    )?;
    set_opacity(annotation, fields.opacity.unwrap_or(DEFAULT_OPACITY))?;
    let icon = fields.icon.unwrap_or(DEFAULT_DRAFT_FILE_ATTACHMENT_ICON);
    if !annotation.set_name(icon.pdf_name()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetName refused the icon",
        ));
    }

    let resource = context.resource(&fields.file.resource)?;
    let attachment = annotation.add_file_attachment(&fields.file.name).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFAnnot_AddFileAttachment returned nothing",
        )
    })?;
    // A zero-byte attachment is a real attachment, so this call is made
    // whatever the length: skipping it would leave a filespec naming a
    // stream that does not exist.
    if !attachment.set_file(document, resource.bytes) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "FPDFAttachment_SetFile refused the bytes",
        ));
    }
    let mime = fields
        .file
        .mime_type
        .as_deref()
        .or(resource.mime_type)
        .unwrap_or(DEFAULT_ATTACHMENT_MIME_TYPE);
    match attachment.set_mime_type(mime) {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("EPDFAttachment_SetSubtype refused '{mime}'"),
            ));
        }
    }
    if let Some(description) = fields.file.description.as_deref()
        && !attachment.set_description(description)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAttachment_SetDescription refused the description",
        ));
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

/// A stamp: a picture in a box.
///
/// The only writer that builds an `/AP` itself. Everything else fills a
/// dictionary and lets the closing bake draw it; there is no generator arm
/// for a stamp, so the appearance authored here is what ships.
///
/// # Why a rotated stamp is authored twice
///
/// The appearance has to be built in the UNROTATED frame and turned by an
/// `/AP /Matrix` afterwards. Fitting the image into the rotated `/Rect`
/// instead bakes the letterbox bands INTO the appearance, and the closing
/// re-fit, which records the content rect from that padded box and fits it
/// into the logical box, then shrinks the image by the aspect ratio a second
/// time. The result is a small picture adrift in white space, and only when
/// rotated: at zero degrees the two frames coincide, so it fills and nothing
/// looks wrong.
///
/// So a rotated stamp clears the transform keys, authors into the logical
/// box, writes the transform, sets the real `/Rect`, and re-fits once.
fn apply_stamp(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::StampDraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if let Some(name) = fields.name.as_deref() {
        set_stamp_name(annotation, name)?;
    }
    let resource = context.resource(&fields.source.resource)?;
    // A rotation with no logical box was refused in preflight, so the pair
    // is either both present or the stamp is upright.
    let logical =
        fields.unrotated_rect.filter(|_| fields.rotation.is_some_and(|degrees| degrees != 0.0));
    author_stamp_appearance(
        document,
        page,
        annotation,
        resource.bytes,
        fields.fit.unwrap_or_default(),
        fields.rect,
        logical,
        &option_patch(fields.rotation),
        &option_patch(fields.unrotated_rect),
    )
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

/// A link: a hit rectangle and where it goes.
///
/// No colour, no opacity, no border. A link draws nothing of its own, and the
/// families that do all write those keys explicitly so a read round-trips;
/// writing them here would put entries on an annotation that has no
/// appearance for them to affect.
fn apply_link(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    fields: &super::draft::LinkDraftFields,
) -> Result<(), EngineError> {
    set_rect(annotation, fields.rect)?;
    let Some(target) = fields.target.as_ref() else {
        // Nothing to write. A new annotation has neither `/A` nor `/Dest`,
        // so the dead link is what it already is.
        return Ok(());
    };
    write_link_target(document, annotation, target)
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
fn build_destination(
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

/// What a draft with no icon creates.
///
/// Not the reader's default. §12.5.6.15 says an ABSENT `/Name` reads as a
/// push pin; a tool creating an attachment marker draws a paperclip. The
/// writer always writes the key, so the two never disagree about one file.
const DEFAULT_DRAFT_FILE_ATTACHMENT_ICON:
    crate::features::annotation::vocabulary::FileAttachmentIcon =
    crate::features::annotation::vocabulary::FileAttachmentIcon::Paperclip;

/// What a file whose producer declared nothing is recorded as.
const DEFAULT_ATTACHMENT_MIME_TYPE: &str = "application/octet-stream";

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
    // `/Rect` above is the ROTATED visual box. The pair below is what tells
    // the generator to bake a turned appearance, and it has to land before
    // the bake, which is why it is here rather than after the styling.
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: option_patch(fields.rotation),
            unrotated_rect: option_patch(fields.unrotated_rect),
        },
        // A draft creates the annotation, so there is never an existing box
        // for a lone rotation to lean on.
        || Ok(false),
    )?;
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
