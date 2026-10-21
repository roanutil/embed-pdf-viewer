//! Changing an annotation.
//!
//! G11. Ports the `update` half of
//! `services/src/features/annotations/AnnotationMutator.ts` and the patch
//! side of `internal/write/*`. Operation: `annotations.update`.
//!
//! # An update is not structural
//!
//! It does not touch `/Annots`, so no index moves, no revision is bumped and
//! no client is asked to re-read. The one identity effect it can have is
//! STRENGTHENING: a weak annotation gets an `/NM` stamped, which changes what
//! it is called and not where it is.
//!
//! # The appearance decision is made against the annotation as it was
//!
//! The classifier value-diffs the patch against the CURRENT DTO, which is
//! read before anything is written. That is what lets a client send a whole
//! object back and still have a colour-only edit recognized as one, and it is
//! why the read happens first rather than being reconstructed from the patch.
//!
//! # Blend mode is captured before the re-bake
//!
//! It lives in the appearance stream's graphics state rather than in the
//! dictionary, so re-baking without carrying it forward would silently reset
//! it on every unrelated edit.

use pdfium::{AppearanceMode, PageView};
use serde::{Deserialize, Serialize};

use super::appearance::{AppearanceImpact, appearance_impact_of};
use super::context::AnnotationWriteContext;
use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use super::legacy::write;
use super::patch::{AnnotationPatch, AnnotationPatchBase, AnnotationPatchKind};
use super::resolve::{capture_or_stamp_stable_id, resolve_annotation};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_timestamp};
use crate::dto::Patch;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::annotation::read::dto::AnnotationDto;
use crate::features::annotation::read::page::read_page_annotations;
use crate::features::annotation::read::style::Color;
use crate::features::annotation::vocabulary::{AnnotationColorType, BlendMode};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::ids::IdSource;
use crate::revision::PageState;

/// What happened to the appearance stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceAction {
    Preserved,
    Regenerated,
    /// The subtype has no generic generator, so nothing was written. A
    /// widget is the usual case.
    GenerationUnavailable,
}

/// The engine's appearance verdict.
///
/// `changed` rather than `action` is the raster-invalidation signal, and the
/// difference matters: a regeneration that could not happen still changed the
/// dictionary, so a form-layer renderer may draw differently even though no
/// `/AP` was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceOutcome {
    pub action: AppearanceAction,
    pub changed: bool,
}

/// What an update produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationUpdateResult {
    /// The annotation after the patch, read back through the ordinary
    /// reader.
    ///
    /// `updated.ref` may be STRONGER than the reference that was passed in:
    /// updating a weak annotation stamps an `/NM`, so a caller that addressed
    /// by index gets an `nm` reference back and should update its cache key.
    pub updated: AnnotationDto,
    pub appearance: AppearanceOutcome,
    pub meta: AnnotationListMutationMeta,
}

/// Apply a patch to one annotation.
#[allow(clippy::too_many_arguments)]
pub fn update_annotation(
    document: &pdfium::Document,
    page: &PageView<'_>,
    page_index: u32,
    page_object_number: PageObjectNumber,
    annotation_ref: &AnnotationRef,
    patch: &AnnotationPatch,
    context: &AnnotationWriteContext<'_>,
    state_before: &PageState,
    state_after: &PageState,
    clock: &dyn Clock,
    ids: &dyn IdSource,
    cancel: &CancelToken,
) -> Result<AnnotationUpdateResult, EngineError> {
    cancel.check()?;
    let annotation = resolve_annotation(document, page, annotation_ref)?;

    // Everything that can refuse comes first. After the identity stamp below
    // this function has written to the document and cannot take it back.
    preflight(&annotation, patch, context)?;
    cancel.check()?;

    let stable_id = capture_or_stamp_stable_id(&annotation, ids)?;
    let previous_blend = BlendMode::from_code(annotation.blend_mode_code());

    // The pre-patch DTO the classifier value-diffs against. Read through the
    // ordinary reader so a full-object patch from a client that projects
    // everything still diffs down to what actually changed.
    let index = page.annotation_index(&annotation);
    if index < 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("the annotation is not in the page's /Annots (index {index})"),
        ));
    }
    let listing_before = read_page_annotations(
        document,
        page_index,
        page_object_number,
        &state_before.revision,
        &mut ActionReadBudgetTracker::default(),
        cancel,
    )?;
    let current = listing_before
        .annotations
        .get(index as usize)
        .ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the annotation is at index {index} and the listing is shorter"),
            )
        })?
        .clone();

    apply_base_patch(&annotation, &patch.base)?;
    apply_kind_patch(document, page, &annotation, patch, context)?;

    let modified = format_pdf_timestamp(clock.now()).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the clock produced no PDF date")
    })?;
    write(annotation.set_string_value("M", &modified), "/M")?;

    let impact = appearance_impact_of(
        &serde_json::to_value(&current).expect("serialize"),
        &serde_json::to_value(patch).expect("serialize"),
    );
    let appearance = settle_appearance(
        &annotation,
        page,
        impact,
        patch.base.blend_mode.unwrap_or(previous_blend),
    );

    // Closed before the read-back. PDFium caches the parsed appearance form
    // on the annotation context, so a handle that saw the old stream can go
    // on reporting its blend mode after a regeneration; reopening is what
    // makes the returned DTO describe the document rather than the cache.
    drop(annotation);
    cancel.check()?;

    let listing_after = read_page_annotations(
        document,
        page_index,
        page_object_number,
        &state_after.revision,
        &mut ActionReadBudgetTracker::default(),
        cancel,
    )?;
    let updated = listing_after.annotations.into_iter().nth(index as usize).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "the annotation vanished from the page during its own update",
        )
    })?;

    let meta = compute_mutation_impact(
        AnnotationMutationKind::Update,
        state_before,
        state_after,
        vec![stable_id],
    )
    .map_err(|e| EngineError::with_message(EngineErrorCode::InvalidArg, e.to_string()))?;

    Ok(AnnotationUpdateResult { updated, appearance, meta })
}

/// Preserve or re-bake, and say which.
///
/// An `/AP` is content: a foreign producer's stream cannot be recomputed, so
/// it survives an inert patch and a VERIFIED rigid translation. With no
/// stream to preserve there is nothing to protect, so any appearance-relevant
/// write bakes one, because the alternative is an annotation that renders as
/// nothing.
fn settle_appearance(
    annotation: &pdfium::Annotation<'_>,
    page: &PageView<'_>,
    impact: AppearanceImpact,
    blend: BlendMode,
) -> AppearanceOutcome {
    let preserve = match impact {
        AppearanceImpact::Inert => true,
        // With no stream to preserve there is nothing to protect, so a
        // translation bakes one: the alternative is an annotation that moved
        // and still renders as nothing.
        //
        // Unexercised. Every annotation these writers create ships with an
        // appearance, so no case in the corpus reaches a translation with
        // none, and a version that preserved unconditionally passes. It
        // becomes reachable with a subtype whose appearance the generic
        // generator cannot produce.
        AppearanceImpact::Translation => annotation.has_appearance_stream(AppearanceMode::Normal),
        AppearanceImpact::Regenerate => false,
    };
    if preserve {
        return AppearanceOutcome { action: AppearanceAction::Preserved, changed: false };
    }

    let ok = annotation.generate_appearance(Some(blend.code()));
    page.generate_content();
    if ok {
        AppearanceOutcome { action: AppearanceAction::Regenerated, changed: true }
    } else {
        AppearanceOutcome {
            action: AppearanceAction::GenerationUnavailable,
            // The dictionary changed even though no stream was written, so a
            // renderer that draws from the dictionary may draw differently.
            changed: impact == AppearanceImpact::Regenerate,
        }
    }
}

/// Everything that can refuse a patch, before anything is written.
fn preflight(
    annotation: &pdfium::Annotation<'_>,
    patch: &AnnotationPatch,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let subtype = crate::features::annotation::subtype::AnnotationSubtype::from_code(
        annotation.subtype_code(),
    );
    // re-port (Task 8): refused here, before the identity stamp writes, until
    // the engine writes `/RD` again. See rect_differences_not_ported.
    let rect_differences = match &patch.kind {
        AnnotationPatchKind::Square { fields } | AnnotationPatchKind::Circle { fields } => {
            &fields.rect_differences
        }
        AnnotationPatchKind::Caret { fields } => &fields.rect_differences,
        AnnotationPatchKind::FreeText { fields } => &fields.rect_differences,
        _ => &Patch::Keep,
    };
    if matches!(rect_differences, Patch::Set(_)) {
        return Err(rect_differences_not_ported());
    }
    if subtype != patch.kind.subtype() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "a {} patch cannot be applied to a {} annotation",
                patch.kind.subtype().as_str(),
                subtype.as_str()
            ),
        ));
    }

    if let AnnotationPatchKind::Highlight { fields }
    | AnnotationPatchKind::Underline { fields }
    | AnnotationPatchKind::Squiggly { fields }
    | AnnotationPatchKind::Strikeout { fields, .. } = &patch.kind
        && let Some(quads) = &fields.quad_points
    {
        refuse_shrinking_quads(annotation, quads, "a text-markup")?;
    }

    // A redaction's quads share the text-markup rule for the same reason:
    // PDFium appends to `/QuadPoints` and cannot truncate it.
    if let AnnotationPatchKind::Redact { fields } = &patch.kind
        && let Some(quads) = &fields.quad_points
    {
        refuse_shrinking_quads(annotation, quads, "a redact")?;
    }
    match &patch.kind {
        // A box kind's rotation needs the box it turns, and unlike a draft
        // the box may already be on the annotation. That is the one case
        // `write_box_transform` reads for, so preflight asks the same
        // question here rather than letting the write find out.
        AnnotationPatchKind::Caret { fields } => {
            box_rotation_satisfiable(annotation, &fields.rotation, &fields.unrotated_rect)?;
        }
        AnnotationPatchKind::FreeText { fields } => {
            box_rotation_satisfiable(annotation, &fields.rotation, &fields.unrotated_rect)?;
            if let Some(family) = fields.font_family.as_deref() {
                super::legacy::font_resolvable(family, context)?;
            }
            if let Some(size) = fields.font_size
                && size <= 0.0
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("a free-text patch's fontSize must be positive, not {size}"),
                ));
            }
            crate::features::annotation::rich_text::assert_agreement(
                patch.base.contents.as_set().map(String::as_str),
                fields.rich_text.as_deref(),
            )?;
        }
        AnnotationPatchKind::Stamp { fields } => {
            box_rotation_satisfiable(annotation, &fields.rotation, &fields.unrotated_rect)?;
            if let Some(source) = &fields.source {
                let resource = context.resource(&source.resource)?;
                if crate::features::binary::sniff(resource.bytes).is_none() {
                    return Err(EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        "a stamp's source must be PNG, JPEG or PDF bytes",
                    ));
                }
            }
            if let Some(name) = fields.name.as_deref()
                && !crate::features::annotation::vocabulary::is_standard_stamp_name(name)
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("'{name}' is not one of the standard stamp labels"),
                ));
            }
        }
        AnnotationPatchKind::Redact { fields } => {
            if let Some(family) = fields.font_family.as_deref() {
                super::legacy::font_resolvable(family, context)?;
            }
        }
        AnnotationPatchKind::Link { fields } => {
            if let Patch::Set(super::draft::LinkTargetDraft::Uri { uri }) = &fields.target
                && uri.is_empty()
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "a link's uri is empty: send target null to clear it instead",
                ));
            }
        }
        _ => {}
    }

    if let Some(opacity) = opacity_of(&patch.kind)
        && !(0.0..=1.0).contains(&opacity)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("opacity {opacity} is outside 0..=1"),
        ));
    }
    Ok(())
}

/// `/QuadPoints` can grow and cannot shrink.
///
/// PDFium appends to the list and has no way to truncate it, so a shorter
/// one would leave stale quads behind: a markup marking text nobody selected,
/// or a redaction declaring a region nobody asked to remove. An EMPTY list is
/// refused separately, because it reads as deleting the attachment points
/// rather than as supplying none.
fn refuse_shrinking_quads(
    annotation: &pdfium::Annotation<'_>,
    quads: &[crate::geom::PdfQuad],
    what: &str,
) -> Result<(), EngineError> {
    if quads.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "{what} patch's quadPoints, when present, must be non-empty: \
                 an empty list would be read as deleting the attachment points"
            ),
        ));
    }
    let existing = annotation
        .quad_points()
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading /QuadPoints failed: {e}"),
            )
        })?
        .len();
    if quads.len() < existing {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "{what} patch cannot shrink quadPoints: the annotation has {existing} \
                 and the patch supplies {}",
                quads.len()
            ),
        ));
    }
    Ok(())
}

/// A box kind's rotation needs the box it turns, here or already on the
/// annotation.
///
/// The difference from the draft rule is the second half. A patch that
/// rotates an annotation which already carries an `/UnrotatedRect` is
/// satisfiable and common: it is what rotating a box a second time looks
/// like.
fn box_rotation_satisfiable(
    annotation: &pdfium::Annotation<'_>,
    rotation: &Patch<f64>,
    unrotated_rect: &Patch<crate::geom::PdfRect>,
) -> Result<(), EngineError> {
    let Patch::Set(degrees) = rotation else {
        return Ok(());
    };
    if *degrees == 0.0 {
        return Ok(());
    }
    let has_box = match unrotated_rect {
        Patch::Set(_) => true,
        Patch::Clear => false,
        Patch::Keep => super::super::read::transform::read_unrotated_rect(annotation)?.is_some(),
    };
    if !has_box {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "rotation requires unrotatedRect: supply the pair together, or rely on one \
             already on the annotation",
        ));
    }
    Ok(())
}

fn opacity_of(kind: &AnnotationPatchKind) -> Option<f64> {
    match kind {
        AnnotationPatchKind::Highlight { fields }
        | AnnotationPatchKind::Underline { fields }
        | AnnotationPatchKind::Squiggly { fields }
        | AnnotationPatchKind::Strikeout { fields, .. } => fields.opacity,
        AnnotationPatchKind::Square { fields } | AnnotationPatchKind::Circle { fields } => {
            fields.opacity
        }
        AnnotationPatchKind::Polygon { fields, .. }
        | AnnotationPatchKind::Polyline { fields, .. } => fields.geometry.opacity,
        AnnotationPatchKind::Line { fields, .. } => fields.opacity,
        AnnotationPatchKind::Ink { fields, .. } => fields.opacity,
        AnnotationPatchKind::Text { fields } => fields.opacity,
        AnnotationPatchKind::Caret { fields } => fields.opacity,
        AnnotationPatchKind::FreeText { fields } => fields.opacity,
        AnnotationPatchKind::Redact { fields } => fields.opacity,
        AnnotationPatchKind::FileAttachment { fields } => fields.opacity,
        // A link, a stamp and a widget have no `/CA`, for the same reasons
        // their drafts do not.
        AnnotationPatchKind::Link { .. }
        | AnnotationPatchKind::Stamp { .. }
        | AnnotationPatchKind::Widget { .. } => None,
    }
}

fn apply_base_patch(
    annotation: &pdfium::Annotation<'_>,
    base: &AnnotationPatchBase,
) -> Result<(), EngineError> {
    super::legacy::write_patch_string(annotation, "Contents", &base.contents)?;
    super::legacy::write_patch_string(annotation, "Subj", &base.subject)?;
    if let Some(flags) = base.flags {
        // Onto the live word, which is what makes `{ hidden: true }` hide
        // without also un-printing.
        if !annotation.set_flags(flags.fold(annotation.flags())) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "FPDFAnnot_SetFlags refused the /F word",
            ));
        }
    }
    Ok(())
}

fn apply_kind_patch(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    patch: &AnnotationPatch,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    match &patch.kind {
        AnnotationPatchKind::Highlight { fields }
        | AnnotationPatchKind::Underline { fields }
        | AnnotationPatchKind::Squiggly { fields } => apply_text_markup_patch(annotation, fields),
        AnnotationPatchKind::Strikeout { fields, intent } => {
            apply_text_markup_patch(annotation, fields)?;
            if let Some(intent) = intent {
                write(annotation.set_intent(intent.name()), "/IT")?;
            }
            Ok(())
        }
        AnnotationPatchKind::Square { fields } | AnnotationPatchKind::Circle { fields } => {
            apply_shape_patch(annotation, fields)
        }
        AnnotationPatchKind::Polygon { fields, cloudy_intensity } => {
            apply_vertex_patch(annotation, fields)?;
            // Only a polygon takes `/BE`, and a clear or a non-positive
            // value both remove it: never leave a degenerate entry the read
            // side would normalize away.
            match cloudy_intensity {
                Patch::Keep => {}
                Patch::Set(value) if *value > 0.0 => {
                    if !annotation.set_border_effect(*value as f32) {
                        return Err(EngineError::with_message(
                            EngineErrorCode::Unknown,
                            "EPDFAnnot_SetBorderEffect refused the intensity",
                        ));
                    }
                }
                _ => {
                    annotation.clear_border_effect();
                }
            }
            Ok(())
        }
        AnnotationPatchKind::Polyline { fields, line_endings } => {
            apply_vertex_patch(annotation, fields)?;
            if let Some(endings) = line_endings {
                set_line_endings(annotation, *endings)?;
            }
            Ok(())
        }
        AnnotationPatchKind::Line { fields, line_points, line_endings } => {
            apply_geometry_style_patch(annotation, fields)?;
            if let Some(points) = line_points
                && !annotation.set_line(point_to_native(points.start), point_to_native(points.end))
            {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "EPDFAnnot_SetLine refused the endpoints",
                ));
            }
            if let Some(endings) = line_endings {
                set_line_endings(annotation, *endings)?;
            }
            Ok(())
        }
        AnnotationPatchKind::Ink { fields, ink_list, intent } => {
            apply_stroke_style_patch(annotation, fields)?;
            if let Some(intent) = intent {
                write(annotation.set_intent(intent.name()), "/IT")?;
            }
            if let Some(strokes) = ink_list {
                // There is no way to replace ONE stroke, so a patch that
                // changes the pen path removes the whole list and appends
                // the new one. Half of that would leave a drawing with the
                // old strokes under the new ones.
                if !annotation.remove_ink_list() {
                    return Err(EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "FPDFAnnot_RemoveInkList refused",
                    ));
                }
                for (index, stroke) in strokes.iter().enumerate() {
                    let points: Vec<pdfium::PointF> =
                        stroke.iter().copied().map(point_to_native).collect();
                    if annotation.add_ink_stroke(&points) < 0 {
                        return Err(EngineError::with_message(
                            EngineErrorCode::Unknown,
                            format!("FPDFAnnot_AddInkStroke refused stroke {index}"),
                        ));
                    }
                }
            }
            Ok(())
        }
        AnnotationPatchKind::Text { fields } => apply_text_patch(annotation, fields),
        AnnotationPatchKind::Caret { fields } => apply_caret_patch(annotation, fields),
        AnnotationPatchKind::FreeText { fields } => {
            apply_free_text_patch(annotation, fields, &patch.base.contents, context)
        }
        AnnotationPatchKind::Redact { fields } => apply_redact_patch(annotation, fields, context),
        AnnotationPatchKind::Link { fields } => apply_link_patch(document, annotation, fields),
        AnnotationPatchKind::FileAttachment { fields } => {
            apply_file_attachment_patch(annotation, fields)
        }
        AnnotationPatchKind::Stamp { fields } => {
            apply_stamp_patch(document, page, annotation, fields, context)
        }
        AnnotationPatchKind::Widget { fields } => apply_widget_patch(annotation, fields),
    }
}

/// A polygon's or polyline's own geometry, plus the styling it shares.
fn apply_vertex_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::VertexPatchFields,
) -> Result<(), EngineError> {
    apply_geometry_style_patch(annotation, &fields.geometry)?;
    // Unlike `/QuadPoints`, a vertex list can shrink: `EPDFAnnot_SetVertices`
    // replaces the array rather than appending to it, so there is no stale
    // tail to leave behind and no preflight to refuse one.
    if let Some(vertices) = &fields.vertices {
        let points: Vec<pdfium::PointF> = vertices.iter().copied().map(point_to_native).collect();
        if !annotation.set_vertices(&points) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetVertices refused the point list",
            ));
        }
    }
    Ok(())
}

/// The styling a filled geometric patch may change.
fn apply_geometry_style_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::GeometryPatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    patch_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;
    match &fields.interior_color {
        Patch::Keep => {}
        Patch::Set(color) => set_color(annotation, AnnotationColorType::InteriorColor, *color)?,
        Patch::Clear => {
            annotation.clear_color(AnnotationColorType::InteriorColor as i32);
        }
    }
    super::transform::write_vertex_transform(annotation, fields.rotation.clone())
}

/// The same without a fill.
fn apply_stroke_style_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::StrokePatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    patch_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;
    super::transform::write_vertex_transform(annotation, fields.rotation.clone())
}

fn set_opacity_patch(
    annotation: &pdfium::Annotation<'_>,
    opacity: Option<f64>,
) -> Result<(), EngineError> {
    if let Some(opacity) = opacity
        && !annotation.set_opacity(opacity)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOpacity refused the value",
        ));
    }
    Ok(())
}

/// `/BS` and `/D`, read-modify-write.
///
/// The native call takes the style AND the width together, so a patch naming
/// either has to READ the other rather than defaulting it: defaulting would
/// reset a width the caller never mentioned.
fn patch_border(
    annotation: &pdfium::Annotation<'_>,
    style: Option<crate::features::annotation::vocabulary::AnnotationBorderStyle>,
    width: Option<f64>,
    dashes: &Option<Vec<f64>>,
) -> Result<(), EngineError> {
    if style.is_some() || width.is_some() {
        let current = annotation.border_style();
        let style = style.unwrap_or(
            crate::features::annotation::vocabulary::AnnotationBorderStyle::from_code(
                current.style_code,
            ),
        );
        let width = width.unwrap_or(f64::from(current.width));
        if !annotation.set_border_style(style.code(), width as f32) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetBorderStyle refused the border",
            ));
        }
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

/// `/LE`, both ends.
fn set_line_endings(
    annotation: &pdfium::Annotation<'_>,
    endings: crate::features::annotation::vocabulary::LineEndings,
) -> Result<(), EngineError> {
    if !annotation.set_line_endings(endings.start.code(), endings.end.code()) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetLineEndings refused the pair",
        ));
    }
    Ok(())
}

fn point_to_native(point: crate::geom::PdfPoint) -> pdfium::PointF {
    pdfium::PointF { x: point.x as f32, y: point.y as f32 }
}

fn apply_text_markup_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::TextMarkupPatchFields,
) -> Result<(), EngineError> {
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    if let Some(opacity) = fields.opacity
        && !annotation.set_opacity(opacity)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOpacity refused the value",
        ));
    }
    if let Some(quads) = &fields.quad_points {
        // Overwrite what is there, then append the rest. Preflight has
        // already refused a list shorter than the existing one.
        let existing = annotation
            .quad_points()
            .map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("reading /QuadPoints failed: {e}"),
                )
            })?
            .len();
        for (index, quad) in quads.iter().enumerate() {
            let native = quad_to_native(*quad);
            let ok = if index < existing {
                annotation.set_quad_point(index, native)
            } else {
                annotation.append_quad_point(native)
            };
            if !ok {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    format!("PDFium refused the quad at index {index}"),
                ));
            }
        }
        // `/Rect` is recomputed so it stays the smallest enclosing box.
        set_rect(annotation, super::legacy::enclosing_rect(quads))?;
    }
    Ok(())
}

fn apply_shape_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::ShapePatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    if let Some(opacity) = fields.opacity
        && !annotation.set_opacity(opacity)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOpacity refused the value",
        ));
    }

    // `/BS` is one native call taking both the style and the width, so a
    // patch naming either has to read the other rather than defaulting it:
    // defaulting would reset a width the caller never mentioned.
    if fields.border_style.is_some() || fields.stroke_width.is_some() {
        let current = annotation.border_style();
        let style = fields.border_style.unwrap_or(
            crate::features::annotation::vocabulary::AnnotationBorderStyle::from_code(
                current.style_code,
            ),
        );
        let width = fields.stroke_width.unwrap_or(f64::from(current.width));
        if !annotation.set_border_style(style.code(), width as f32) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetBorderStyle refused the border",
            ));
        }
    }
    if let Some(dashes) = &fields.dash_array
        && !dashes.is_empty()
    {
        let dashes: Vec<f32> = dashes.iter().map(|d| *d as f32).collect();
        if !annotation.set_border_dash_pattern(&dashes) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_SetBorderDashPattern refused the pattern",
            ));
        }
    }

    match &fields.interior_color {
        Patch::Keep => {}
        Patch::Set(color) => set_color(annotation, AnnotationColorType::InteriorColor, *color)?,
        Patch::Clear => {
            annotation.clear_color(AnnotationColorType::InteriorColor as i32);
        }
    }
    {
        match &fields.cloudy_intensity {
            Patch::Keep => {}
            Patch::Set(value) if *value > 0.0 => {
                if !annotation.set_border_effect(*value as f32) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "EPDFAnnot_SetBorderEffect refused the intensity",
                    ));
                }
            }
            // A clear clears, and so does a non-positive value: never write
            // a degenerate `/BE` the read side would normalize away.
            _ => {
                annotation.clear_border_effect();
            }
        }
    }
    {
        match &fields.rect_differences {
            Patch::Keep => {}
            // re-port (Task 8): preflight refuses a set.
            Patch::Set(_) => {}
            Patch::Clear => {
                annotation.clear_rectangle_differences();
            }
        }
    }
    Ok(())
}

fn quad_to_native(quad: crate::geom::PdfQuad) -> pdfium::QuadF {
    pdfium::QuadF {
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
    let ok = annotation.set_rect(pdfium::RectF {
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

/// A sticky note. The icon and the review state; the picture is the
/// generator's, baked from `/C` and `/Name` after this returns.
fn apply_text_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::TextPatchFields,
) -> Result<(), EngineError> {
    use crate::features::annotation::state::{state_model_to_pdf, state_to_pdf};

    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    if let Some(icon) = fields.icon
        && !annotation.set_name(icon.pdf_name())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetName refused the icon",
        ));
    }
    // No cross-field rule here, unlike the draft. A patch touches what it
    // states, and the other half of the pair may already be on the
    // annotation.
    write_patch_string_mapped(annotation, "StateModel", &fields.state_model, state_model_to_pdf)?;
    write_patch_string_mapped(annotation, "State", &fields.state, state_to_pdf)
}

/// A tri-state string entry whose wire spelling is not its PDF spelling.
fn write_patch_string_mapped(
    annotation: &pdfium::Annotation<'_>,
    key: &str,
    value: &Patch<String>,
    to_pdf: impl Fn(&str) -> String,
) -> Result<(), EngineError> {
    match value {
        Patch::Keep => Ok(()),
        Patch::Clear => super::legacy::write_patch_string(annotation, key, &Patch::Clear),
        Patch::Set(value) => {
            super::legacy::write_patch_string(annotation, key, &Patch::Set(to_pdf(value)))
        }
    }
}

/// A caret: a box, an inset and an `/IT`.
fn apply_caret_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::CaretPatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    // Before the styling, so the bake at the end of the mutation sees it.
    write_box_transform_patch(annotation, &fields.rotation, &fields.unrotated_rect)?;
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    if let Some(intent) = fields.intent {
        write(annotation.set_intent(intent.name()), "/IT")?;
    }
    patch_rect_differences(annotation, &fields.rect_differences)
}

/// The box-family rotation pair, as a patch.
fn write_box_transform_patch(
    annotation: &pdfium::Annotation<'_>,
    rotation: &Patch<f64>,
    unrotated_rect: &Patch<crate::geom::PdfRect>,
) -> Result<(), EngineError> {
    super::transform::write_box_transform(
        annotation,
        &super::transform::AnnotationTransform {
            rotation: rotation.clone(),
            unrotated_rect: unrotated_rect.clone(),
        },
        // Preflight already asked this question and refused the
        // unsatisfiable case, so by here the answer only matters for the
        // path that reads it, and it is the same read.
        || Ok(super::super::read::transform::read_unrotated_rect(annotation)?.is_some()),
    )
}

/// `/RD`, tri-state.
fn patch_rect_differences(
    annotation: &pdfium::Annotation<'_>,
    value: &Patch<crate::features::annotation::read::shape::PdfRectDifferences>,
) -> Result<(), EngineError> {
    match value {
        Patch::Keep => {}
        Patch::Clear => {
            annotation.clear_rectangle_differences();
        }
        // re-port (Task 8): preflight refuses a set.
        Patch::Set(_) => {}
    }
    Ok(())
}

/// A free text or a callout. `applyFreeTextPatch` in
/// `writeFreeTextAnnotation.ts`.
///
/// Every text write goes through the rich document: the body style lives
/// there and the fork derives `/DA` from it. So the `/DA` read-modify-write
/// here carries only the `/DA` COLOUR (the border and the leader), and the
/// font, size, text colour and alignment are body-style changes.
fn apply_free_text_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::FreeTextPatchFields,
    contents: &Patch<String>,
    context: &super::context::AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    // `/C` here is the BOX BACKGROUND, not the mark.
    match &fields.interior_color {
        Patch::Keep => {}
        Patch::Set(color) => set_color(annotation, AnnotationColorType::Color, *color)?,
        Patch::Clear => {
            annotation.clear_color(AnnotationColorType::Color as i32);
        }
    }
    set_opacity_patch(annotation, fields.opacity)?;
    patch_border(annotation, fields.border_style, fields.stroke_width, &fields.dash_array)?;

    use crate::features::annotation::rich_text;
    let current = rich_text::read_rich_text(annotation);
    if let Some(color) = fields.color {
        let da = annotation.default_appearance();
        let family = da
            .map_or(crate::features::annotation::vocabulary::DEFAULT_STANDARD_FONT, |da| {
                crate::features::annotation::vocabulary::StandardFont::from_code(da.font_code)
            })
            .as_str();
        let size = da
            .map(|da| f64::from(da.font_size))
            .filter(|size| *size > 0.0)
            .unwrap_or(crate::features::annotation::read::free_text::DEFAULT_FONT_SIZE);
        super::legacy::set_default_appearance(annotation, family, size, color, context)?;
    }
    // After the `/DA`, on the same terms as the draft.
    if let Some(font_color) = fields.font_color {
        set_color(annotation, AnnotationColorType::TextColor, font_color)?;
    }
    let describe = |key: &str| context.describe_font(key);
    if let Some(rich) = &fields.rich_text {
        // Rich replacement: everything regenerated. A `contents` given too
        // was checked against its projection in preflight.
        rich_text::write_rich_text(annotation, rich, Some(&describe))?;
    } else if !contents.is_keep() {
        // Plain-text replacement: body-style paragraphs, one per line break.
        // Run formatting is lost by design: a plain-text client cannot
        // preserve what it cannot see.
        let input = rich_text::RichTextDocumentInput {
            body: None,
            paragraphs: rich_text::paragraphs_from_plain_text(
                contents.as_set().map_or("", String::as_str),
            ),
        };
        rich_text::write_rich_text(annotation, &input, Some(&describe))?;
    } else if let Some(current) = current
        && (fields.font_family.is_some()
            || fields.font_size.is_some()
            || fields.font_color.is_some()
            || fields.text_align.is_some())
    {
        // Default-style change: the body moves, and since runs are deltas,
        // every run that did not override the property follows. Alignment
        // is a body property too: the `/RC` body's alignment wins over `/Q`
        // on regeneration, so `/Q` alone (set below) would not move it.
        let mut body = current.body;
        if let Some(family) = fields.font_family.as_deref() {
            let face = rich_text::face_for_free_text_font(family, Some(&describe));
            body.family = face.family;
            if let Some(weight) = face.weight {
                body.weight = weight;
            }
            if let Some(italic) = face.italic {
                body.italic = italic;
            }
        }
        if let Some(size) = fields.font_size {
            body.size = size;
        }
        if let Some(color) = fields.font_color {
            body.color = rich_text::hex_color(color.r, color.g, color.b);
        }
        if let Some(align) = fields.text_align {
            body.align = align.as_str().to_owned();
        }
        let body = match serde_json::to_value(&body).expect("a rich body serializes") {
            serde_json::Value::Object(body) => body,
            _ => unreachable!("a struct serializes to an object"),
        };
        let input =
            rich_text::RichTextDocumentInput { body: Some(body), paragraphs: current.paragraphs };
        rich_text::write_rich_text(annotation, &input, Some(&describe))?;
    }
    if let Some(align) = fields.text_align
        && !annotation.set_text_alignment(align.code())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetTextAlignment refused the alignment",
        ));
    }
    if let Some(intent) = fields.intent {
        write(annotation.set_intent(intent.name()), "/IT")?;
    }
    patch_rect_differences(annotation, &fields.rect_differences)?;
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
    if let Some(ending) = fields.line_ending {
        set_line_endings(
            annotation,
            crate::features::annotation::vocabulary::LineEndings {
                start: crate::features::annotation::vocabulary::LineEnding::None,
                end: ending,
            },
        )?;
    }
    write_box_transform_patch(annotation, &fields.rotation, &fields.unrotated_rect)
}

/// A redaction marking.
fn apply_redact_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::RedactPatchFields,
    context: &super::context::AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(quads) = &fields.quad_points {
        write_quads(annotation, quads)?;
        // Derived only when the patch did not say. A redaction's box can
        // carry a margin the quads do not describe, which is why the DRAFT
        // takes one outright; a patch that moves the regions and says
        // nothing about the box is asking for the box to follow, and one
        // that supplies both has already decided.
        if fields.rect.is_none() {
            set_rect(annotation, super::legacy::enclosing_rect(quads))?;
        }
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    match &fields.interior_color {
        Patch::Keep => {}
        Patch::Set(color) => set_color(annotation, AnnotationColorType::InteriorColor, *color)?,
        Patch::Clear => {
            annotation.clear_color(AnnotationColorType::InteriorColor as i32);
        }
    }
    match &fields.overlay_text {
        Patch::Keep => {}
        // There is no call that removes `/OverlayText`, so an empty string
        // is how a label is cleared. PDFium reports an empty one as absent,
        // which is what makes the two the same thing here.
        Patch::Clear => {
            if !annotation.set_overlay_text("") {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "EPDFAnnot_SetOverlayText refused the clear",
                ));
            }
        }
        Patch::Set(text) => {
            if !annotation.set_overlay_text(text) {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "EPDFAnnot_SetOverlayText refused the label",
                ));
            }
        }
    }
    if let Some(repeat) = fields.repeat
        && !annotation.set_overlay_text_repeat(repeat)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetOverlayTextRepeat refused",
        ));
    }
    // A redaction's `/DA` members are DEFAULTED rather than read, unlike a
    // free text's. §12.5.6.14 ties the entry to `/OverlayText` rather than
    // to the annotation, so it is written whole whenever the label's styling
    // is touched at all, and a patch naming a colour alone resets the size.
    // `redact-label-colour-only` records the twelve points that result from
    // patching a nine-point label, on both engines.
    if fields.font_family.is_some() || fields.font_size.is_some() || fields.font_color.is_some() {
        let family = fields.font_family.clone().unwrap_or_else(|| {
            crate::features::annotation::vocabulary::DEFAULT_STANDARD_FONT.as_str().to_owned()
        });
        let size = fields
            .font_size
            .unwrap_or(crate::features::annotation::read::redact::DEFAULT_FONT_SIZE);
        let color = fields
            .font_color
            .unwrap_or(crate::features::annotation::read::redact::DEFAULT_LABEL_COLOR);
        super::legacy::set_default_appearance(annotation, &family, size, color, context)?;
    }
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

/// Overwrite what is there and append the rest.
///
/// Preflight has already refused a list shorter than the existing one, so
/// every index below the existing count is a replacement and the rest are
/// appends.
fn write_quads(
    annotation: &pdfium::Annotation<'_>,
    quads: &[crate::geom::PdfQuad],
) -> Result<(), EngineError> {
    let existing = annotation
        .quad_points()
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading /QuadPoints failed: {e}"),
            )
        })?
        .len();
    for (index, quad) in quads.iter().enumerate() {
        let native = quad_to_native(*quad);
        let ok = if index < existing {
            annotation.set_quad_point(index, native)
        } else {
            annotation.append_quad_point(native)
        };
        if !ok {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("PDFium refused the quad at index {index}"),
            ));
        }
    }
    Ok(())
}

/// A link: its hit rectangle and where it goes.
fn apply_link_patch(
    document: &pdfium::Document,
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::LinkPatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    match &fields.target {
        Patch::Keep => Ok(()),
        // Both entries go. The model treats the target as ONE concept with
        // two spellings, so removing only `/A` would resurrect a stale
        // direct `/Dest` as the live target.
        //
        // The `/Dest` half is unexercised: nothing in this engine writes a
        // direct `/Dest`, so every link the vectors build carries an `/A`
        // alone and a build that removed only that one passes. It would
        // take a fixture link written by something else to see it, and the
        // case it guards is exactly the foreign file this engine did not
        // write.
        Patch::Clear => {
            if !annotation.remove_action() || !annotation.remove_destination() {
                return Err(EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the link's target could not be cleared",
                ));
            }
            Ok(())
        }
        Patch::Set(target) => super::legacy::write_link_target(document, annotation, target),
    }
}

/// A file-attachment marker. Presentation only: the FILE is create-only.
fn apply_file_attachment_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::FileAttachmentPatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    if let Some(color) = fields.color {
        set_color(annotation, AnnotationColorType::Color, color)?;
    }
    set_opacity_patch(annotation, fields.opacity)?;
    if let Some(icon) = fields.icon
        && !annotation.set_name(icon.pdf_name())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFAnnot_SetName refused the icon",
        ));
    }
    Ok(())
}

/// A stamp.
///
/// Two paths, and which one runs depends on whether the bytes changed. With
/// a `source` the picture is rebuilt from scratch; without one the existing
/// appearance is re-fitted into the new geometry, which is the call that
/// makes moving a stamp cheap.
fn apply_stamp_patch(
    document: &pdfium::Document,
    page: &PageView<'_>,
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::StampPatchFields,
    context: &super::context::AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if let Some(name) = fields.name.as_deref() {
        super::legacy::set_stamp_name(annotation, name)?;
    }
    let fit = fields.fit.unwrap_or_default();

    if let Some(source) = &fields.source {
        let resource = context.resource(&source.resource)?;
        // The box to author into is the patch's, or the one already there.
        let rect = match fields.rect {
            Some(rect) => rect,
            None => read_rect(annotation)?,
        };
        let logical = match &fields.unrotated_rect {
            Patch::Set(logical) => Some(*logical),
            Patch::Clear => None,
            Patch::Keep => super::super::read::transform::read_unrotated_rect(annotation)?,
        };
        let rotation_is_live = match &fields.rotation {
            Patch::Set(degrees) => *degrees != 0.0,
            Patch::Clear => false,
            Patch::Keep => super::super::read::transform::read_rotation(annotation)?.is_some(),
        };
        return super::legacy::author_stamp_appearance(
            document,
            page,
            annotation,
            resource.bytes,
            fit,
            rect,
            logical.filter(|_| rotation_is_live),
            &fields.rotation,
            &fields.unrotated_rect,
        );
    }

    // No new bytes: geometry only.
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    write_box_transform_patch(annotation, &fields.rotation, &fields.unrotated_rect)?;
    if fields.rect.is_some()
        || fields.fit.is_some()
        || !fields.rotation.is_keep()
        || !fields.unrotated_rect.is_keep()
    {
        // Re-fit the appearance that is already there into the new box. The
        // fork recorded the content's own rect on the last write, which is
        // what stops this from scaling an already-scaled picture again.
        if !annotation.update_appearance_to_rect(fit.native()) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFAnnot_UpdateAppearanceToRect refused the fit",
            ));
        }
    }
    Ok(())
}

/// The annotation's current `/Rect`.
fn read_rect(annotation: &pdfium::Annotation<'_>) -> Result<crate::geom::PdfRect, EngineError> {
    let rect = annotation.rect();
    Ok(crate::geom::normalize_pdf_rect(crate::geom::PdfRect {
        left: f64::from(rect.left),
        bottom: f64::from(rect.bottom),
        right: f64::from(rect.right),
        top: f64::from(rect.top),
    }))
}

/// A widget: move it and restyle it.
fn apply_widget_patch(
    annotation: &pdfium::Annotation<'_>,
    fields: &super::patch::WidgetPatchFields,
) -> Result<(), EngineError> {
    if let Some(rect) = fields.rect {
        set_rect(annotation, rect)?;
    }
    super::legacy::set_mk_color_patch(annotation, 0, &fields.color)?;
    super::legacy::set_mk_color_patch(annotation, 1, &fields.interior_color)?;
    if fields.touches_border() {
        patch_border(annotation, fields.border_style, fields.stroke_width, &None)?;
    }
    if fields.touches_text_style() {
        // A widget's `/DA` members are DEFAULTED rather than read, which is
        // the opposite of the free-text rule above. One writer serves this
        // and `forms.createField`'s inline placements, so a widget's styling
        // is a whole description rather than an edit: a patch naming a size
        // alone resets the family. `widget-font-size-only` records the
        // helvetica that results from patching a times-roman widget, on both
        // engines.
        let family = fields
            .font_family
            .unwrap_or(crate::features::annotation::vocabulary::DEFAULT_STANDARD_FONT);
        let size = fields
            .font_size
            .unwrap_or(crate::features::annotation::read::redact::DEFAULT_FONT_SIZE);
        let color = fields
            .font_color
            .unwrap_or(crate::features::annotation::read::free_text::DEFAULT_FREE_TEXT_COLOR);
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
    // Best-effort, as on the draft: PDFium declines `/Q` on a widget with no
    // `/FT`, and an inert widget has none.
    if let Some(align) = fields.text_align {
        let _ = annotation.set_text_alignment(align.code());
    }
    // A widget attached to a field re-bakes the family-correct appearance;
    // on an inert one this does nothing, which is the right answer for it.
    annotation.generate_form_field_appearance();
    Ok(())
}

/// re-port (Task 8): the error a `/RD` write returns until the engine owns
/// `/RD` the way upstream does. `EPDFAnnot_SetRectangleDifferences` was
/// removed in runtime 5e9241945.
fn rect_differences_not_ported() -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotImplemented,
        "writing /RD is not ported to the runtime 1461b0c51 API",
    )
}
