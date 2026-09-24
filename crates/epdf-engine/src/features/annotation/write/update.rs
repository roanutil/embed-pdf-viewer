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
use super::create::write;
use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use super::patch::{AnnotationPatch, AnnotationPatchBase, AnnotationPatchKind};
use super::resolve::{capture_or_stamp_stable_id, resolve_annotation};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_date};
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
    state_before: &PageState,
    state_after: &PageState,
    clock: &dyn Clock,
    ids: &dyn IdSource,
    cancel: &CancelToken,
) -> Result<AnnotationUpdateResult, EngineError> {
    cancel.check()?;
    let annotation = resolve_annotation(page, annotation_ref)?;

    // Everything that can refuse comes first. After the identity stamp below
    // this function has written to the document and cannot take it back.
    preflight(&annotation, patch)?;
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
    apply_kind_patch(&annotation, &patch.kind)?;

    let modified = format_pdf_date(clock.now()).ok_or_else(|| {
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
) -> Result<(), EngineError> {
    let subtype = crate::features::annotation::subtype::AnnotationSubtype::from_code(
        annotation.subtype_code(),
    );
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
        if quads.is_empty() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "a text-markup patch's quadPoints, when present, must be non-empty: \
                 an empty list would be read as deleting the attachment points",
            ));
        }
        // PDFium can append to `/QuadPoints` and has no way to truncate it,
        // so a shorter list would leave stale quads behind and the
        // annotation would mark text nobody selected.
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
                    "a text-markup patch cannot shrink quadPoints: the annotation has \
                     {existing} and the patch supplies {}",
                    quads.len()
                ),
            ));
        }
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

fn opacity_of(kind: &AnnotationPatchKind) -> Option<f64> {
    match kind {
        AnnotationPatchKind::Highlight { fields }
        | AnnotationPatchKind::Underline { fields }
        | AnnotationPatchKind::Squiggly { fields }
        | AnnotationPatchKind::Strikeout { fields, .. } => fields.opacity,
        AnnotationPatchKind::Square { fields } | AnnotationPatchKind::Circle { fields } => {
            fields.opacity
        }
    }
}

fn apply_base_patch(
    annotation: &pdfium::Annotation<'_>,
    base: &AnnotationPatchBase,
) -> Result<(), EngineError> {
    super::create::write_patch_string(annotation, "Contents", &base.contents)?;
    super::create::write_patch_string(annotation, "Subj", &base.subject)?;
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
    annotation: &pdfium::Annotation<'_>,
    kind: &AnnotationPatchKind,
) -> Result<(), EngineError> {
    match kind {
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
    }
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
        set_rect(annotation, super::create::enclosing_rect(quads))?;
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
            Patch::Set(rd) => {
                if !annotation.set_rectangle_differences(
                    rd.left as f32,
                    rd.bottom as f32,
                    rd.right as f32,
                    rd.top as f32,
                ) {
                    return Err(EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "EPDFAnnot_SetRectangleDifferences refused the margins",
                    ));
                }
            }
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
