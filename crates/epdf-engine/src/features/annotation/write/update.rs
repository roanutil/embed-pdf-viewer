//! Changing an annotation.
//!
//! G11. Ports `services/src/features/annotations/AnnotationMutator.ts`
//! (`update`) with the patch writers of `internal/write/`
//! (`annotationWriterRegistry.ts` `applyPatch`, `preflightPatch`, and each
//! kind's `apply*Patch`), `writeAnnotationBox.ts` (`applyAnnotationBoxPatch`),
//! `writeAnnotationPoints.ts` (`updatePoints`),
//! `writeAnnotationTransformMetadata.ts` (`settleAnnotationTurn`),
//! `writeMeasurementFields.ts` (as far as 8.25 leaves it),
//! `writeAnnotationRelationship.ts` (`writeAnnotationRelationship`,
//! `writePopupParent`, `writeLinkedOpen`) and `writeEmbedMetadata.ts`
//! (`applyEmbedMetadataOnUpdate`). Operation: `annotations.update`, a single
//! verb and a change op.
//!
//! # An update is not structural
//!
//! It does not touch `/Annots`, so the annotation keeps its place and its
//! name through the write, and is read back where it was.
//!
//! # The appearance decision is made against the annotation as it was
//!
//! The patch is resolved against the read taken before anything is written
//! ([`super::patch::pdf_resolve_annotation_patch`]), and the appearance
//! verdict value-diffs the resolved patch against that read. A client that
//! sends a whole read back with one change is classified by that change.
//!
//! # Blend mode is captured before the re-bake
//!
//! It lives in the appearance stream's graphics state rather than in the
//! dictionary, so re-baking without carrying it forward would silently reset
//! it on every unrelated edit.

use pdfium::{Annotation, Document};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::appearance::{AppearanceImpact, appearance_impact_of};
use super::context::AnnotationWriteContext;
use super::create::{
    AnnotationMutationMeta, Link, link_popup, link_reply, link_target, open_existing,
    set_default_appearance, write_link_target,
};
use super::draft::AnnotationActor;
use super::patch::{ResolvedPatch, pdf_resolve_annotation_patch, touches_caption};
use super::prepare::{
    DraftFields, DraftResources, Rgb, StampFit, assert_resources, intent_name, text_alignment_of,
};
use super::primitives::{
    self as p, CurrentPoints, apply_annotation_box_patch, read_current_points, set_color,
    set_opacity, shift_annot_rect, shift_between, update_points,
};
use super::stamp::{DrawingIndex, author_stamp_appearance, write_stamp_fit};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_timestamp};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::kinds::{AnnotationRecord, read_annotation_record_kind};
use crate::features::annotation::read::record::read_annotation_record_base;
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationColorType as C, BlendMode, LineEnding, LineEndings,
    RT_UNKNOWN, StandardFont,
};
use crate::geom::{NamedPdfQuad, PdfPoint};
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::session::document::WriteStamp;

/// What happened to the appearance stream (`AppearanceAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceAction {
    Preserved,
    Regenerated,
    /// The kind has no generator, so nothing was written.
    GenerationUnavailable,
    /// In an undo: put back as it was before the change being undone.
    Restored,
}

/// The engine's appearance verdict (`AppearanceOutcome`).
///
/// `changed` rather than `action` is the raster-invalidation signal: a
/// regeneration that could not happen still changed the dictionary, so a
/// renderer that draws from it may draw differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceOutcome {
    pub action: AppearanceAction,
    pub changed: bool,
}

/// What an update answers (`AnnotationUpdateResult`), in the file's
/// coordinates: the annotation after the patch, read back as a listing reads
/// it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationUpdateResult {
    /// As a listing reads it; a widget as the form reads its row.
    pub annotation: Map<String, Value>,
    pub appearance: AppearanceOutcome,
    pub meta: AnnotationMutationMeta,
    /// The patch as it was written, resolved. Not on the wire: the change
    /// op's undo reads which fields it named.
    #[serde(skip)]
    pub resolved: Map<String, Value>,
}

/// One update (`AnnotationMutator.update`'s arguments).
#[derive(Debug)]
pub struct UpdateRequest<'a> {
    pub reference: AnnotationRef,
    /// The patch as the caller sent it, in the file's coordinates.
    pub patch: Map<String, Value>,
    pub resources: DraftResources<'a>,
    /// Who updates it, as the authority check answered: stamped as
    /// `UpdatedBy`, and a new group.
    pub actor: Option<AnnotationActor>,
}

/// What an update writes with besides the request.
pub struct UpdateServices<'a> {
    pub context: &'a AnnotationWriteContext<'a>,
    pub drawings: &'a std::cell::RefCell<DrawingIndex>,
    pub clock: &'a dyn Clock,
}

fn unknown(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

/// The annotation at `index` on the page, as a listing reads it, in the
/// file's coordinates.
pub fn read_record(
    document: &Document,
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
) -> Result<AnnotationRecord, EngineError> {
    let mut budget = crate::features::actions::vocabulary::ActionReadBudgetTracker::default();
    let base = read_annotation_record_base(document, annotation, page, index, &mut budget)?;
    let kind = read_annotation_record_kind(document, annotation, &base, annotation.subtype_code())?;
    Ok(AnnotationRecord { base, kind })
}

/// The annotation at `index`, as the JSON object the resolution reads: a
/// widget as the form reads its row, joined to its field
/// (`joinWidgetFieldNumbers`), every other kind as a listing reads it.
pub fn read_record_json(
    document: &Document,
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
) -> Result<Map<String, Value>, EngineError> {
    if annotation.subtype_code() == AnnotationSubtype::Widget.code() {
        let row =
            crate::features::forms::widgets::read_widget_row(document, annotation, page, index)?;
        return match serde_json::to_value(row) {
            Ok(Value::Object(fields)) => Ok(fields),
            Ok(_) => Err(unknown("a widget row serialized to no object")),
            Err(e) => Err(unknown(format!("a widget row did not serialize: {e}"))),
        };
    }
    record_json(&read_record(document, annotation, page, index)?)
}

/// A record as the JSON object the resolution reads.
pub fn record_json(record: &AnnotationRecord) -> Result<Map<String, Value>, EngineError> {
    match serde_json::to_value(record) {
        Ok(Value::Object(fields)) => Ok(fields),
        Ok(_) => Err(unknown("a record serialized to no object")),
        Err(e) => Err(unknown(format!("a record did not serialize: {e}"))),
    }
}

/// `annotations.update`: edit an annotation in place, checked whole before
/// the first write (`AnnotationMutator.update`).
pub fn update_annotation(
    document: &Document,
    page_index: u32,
    stamp: &WriteStamp,
    request: UpdateRequest<'_>,
    services: &UpdateServices<'_>,
    cancel: &CancelToken,
) -> Result<AnnotationUpdateResult, EngineError> {
    cancel.check()?;
    let UpdateRequest { reference, patch, resources, actor } = request;
    let page = reference.page();
    // An update edits in place, so the annotation keeps its position
    // throughout: opened again there after its appearance is baked.
    let index = super::resolve::resolve_annotation_index_raw(document, page_index, &reference)?;
    let open = || -> Result<Annotation<'_>, EngineError> {
        document
            .raw_annotation(page_index, index)
            .ok_or_else(|| unknown("the annotation is not where it was"))
    };
    let annotation = open()?;
    let previous_blend = BlendMode::from_code(annotation.blend_mode_code());
    let current = read_record_json(document, &annotation, page, index)?;

    assert_caption_metadata_writable(&annotation, &current, &patch)?;
    let describe = |key: &str| services.context.describe_font(key);
    let resolved = pdf_resolve_annotation_patch(&current, patch, Some(&describe))?;
    assert_resources(resolved.subtype, &resources, false)?;
    preflight_patch(document, &resolved, &resources, services.context)?;
    // The apply boundary: everything below writes.
    cancel.check()?;

    // The appearance decision depends only on the read and the patch. A
    // write that redraws or re-places the drawing starts from the turn the
    // annotation reads: our keys record it first.
    let impact = if resources.appearance.is_some() {
        AppearanceImpact::Regenerate
    } else {
        appearance_impact_of(&Value::Object(current.clone()), &Value::Object(resolved.data.clone()))
    };
    if impact != AppearanceImpact::Inert {
        p::settle_annotation_turn(&annotation)?;
    }

    apply_patch(document, &annotation, &current, &resolved, &resources, services)?;
    // A derived plain label supersedes imported rich contents: a stale `/RC`
    // would show a different value where rich text is preferred.
    if let Some(contents) = resolved.data.get("contents")
        && Some(contents) != current.get("contents")
        && is_dimension(&current, &resolved.data)
    {
        p::remove_key(&annotation, "RC")?;
    }

    // `/IRT` and `/RT`: only this annotation is written. A popup relink and
    // a shared `/Open` also write the other annotation, which `meta.changed`
    // names after this one.
    if let Some(reply) = resolved.data.get("reply")
        && !same_reply(reply, current.get("reply").unwrap_or(&Value::Null))
    {
        write_relationship(document, page_index, page, &annotation, &resolved.fields)?;
    }
    let mut linked: Vec<AnnotationRef> = Vec::new();
    let mut relinked = false;
    if resolved.subtype == AnnotationSubtype::Popup
        && let Some(parent) = &resolved.parent
        && !same_ref(Some(parent), ref_of(current.get("parent")).as_ref())
    {
        relinked = true;
        let previous = ref_of(current.get("parent"));
        if let Some(name) = write_popup_parent(
            document,
            page_index,
            page,
            &annotation,
            Some(parent),
            previous.as_ref(),
        )? {
            linked.push(name);
        }
    }
    // A note and its popup hold one `/Open`: writing it on one writes the
    // other (a relink has made them equal already).
    let open_value = match resolved.subtype {
        AnnotationSubtype::Text => resolved.fields.open,
        AnnotationSubtype::Popup => resolved.fields.open_popup,
        _ => None,
    };
    if let Some(open_value) = open_value
        && !relinked
    {
        let other = match resolved.subtype {
            AnnotationSubtype::Text => ref_of(current.get("popup")),
            _ => ref_of(current.get("parent")),
        };
        if let Some(other) = other
            && let Some(name) = write_linked_open(
                document,
                page_index,
                page,
                &other,
                open_value,
                resolved.subtype == AnnotationSubtype::Popup,
            )?
        {
            linked.push(name);
        }
    }

    // `/M` on every update, whatever the patch touched; then `UpdatedBy`
    // (and a new group) when there is someone to name.
    let modified = format_pdf_timestamp(services.clock.now())
        .ok_or_else(|| unknown("the clock gave no PDF date"))?;
    p::set_string(&annotation, "M", &modified)?;
    apply_embed_metadata_on_update(&annotation, actor.as_ref())?;

    // The appearance (`appearanceImpactOf`): an `/AP` is content, not
    // cache. An inert patch never touches it, and a verified rigid
    // translation keeps it as it is (a missing one stays missing: the
    // renderer draws it in memory where the annotation now is). Everything
    // else bakes one. New appearance bytes are a new drawing.
    let appearance = match impact {
        AppearanceImpact::Inert | AppearanceImpact::Translation => {
            AppearanceOutcome { action: AppearanceAction::Preserved, changed: false }
        }
        AppearanceImpact::Regenerate => {
            let blend = resolved.fields.blend_mode.unwrap_or(previous_blend);
            // A widget is drawn as its field's family draws it; a widget in
            // no field has no generator.
            let drawn = if resolved.subtype == AnnotationSubtype::Widget {
                annotation.generate_form_field_appearance()
            } else {
                annotation.generate_appearance(Some(blend.code()))
            };
            if drawn {
                AppearanceOutcome { action: AppearanceAction::Regenerated, changed: true }
            } else {
                // No generator for this kind (a stamp keeps the appearance
                // its writer placed). The dictionary still changed.
                AppearanceOutcome { action: AppearanceAction::GenerationUnavailable, changed: true }
            }
        }
    };

    // PDFium caches the parsed appearance form on an annotation's handle;
    // reopening is what makes the read describe the new stream.
    drop(annotation);
    let after = open()?;
    let record = read_record_json(document, &after, page, index)?;
    let own: AnnotationRef = serde_json::from_value(record.get("ref").cloned().unwrap_or_default())
        .map_err(|e| unknown(format!("an annotation read back with no ref: {e}")))?;
    let mut changed = vec![own];
    for other in linked {
        if !changed.iter().any(|seen| same_ref(Some(seen), Some(&other))) {
            changed.push(other);
        }
    }
    Ok(AnnotationUpdateResult {
        annotation: record,
        appearance,
        meta: AnnotationMutationMeta::of(stamp, page, changed),
        resolved: resolved.data,
    })
}

/// The key two refs to one annotation share (`annotationKey`).
fn key_of(reference: &AnnotationRef) -> String {
    match reference {
        AnnotationRef::ObjectNumber { page, annot } => format!("{}:o{annot}", page.get()),
        AnnotationRef::BaseIndex { page, base_index } => format!("{}:b{base_index}", page.get()),
        AnnotationRef::Nm { page, nm } => format!("{}:n{nm}", page.get()),
        AnnotationRef::Index { page, index, .. } => format!("{}:i{index}", page.get()),
    }
}

/// `sameRef`.
fn same_ref(left: Option<&AnnotationRef>, right: Option<&AnnotationRef>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => key_of(left) == key_of(right),
        (None, None) => true,
        _ => false,
    }
}

fn ref_of(value: Option<&Value>) -> Option<AnnotationRef> {
    serde_json::from_value(value?.clone()).ok()
}

/// `sameReply`: the same annotation, the same type (`reply` when left out).
fn same_reply(next: &Value, current: &Value) -> bool {
    if next.is_null() || current.is_null() {
        return next.is_null() == current.is_null();
    }
    let kind =
        |reply: &Value| reply.get("type").and_then(Value::as_str).unwrap_or("reply").to_owned();
    same_ref(ref_of(next.get("to")).as_ref(), ref_of(current.get("to")).as_ref())
        && kind(next) == kind(current)
}

/// `isDimension`, of the annotation as the patch leaves it.
fn is_dimension(current: &Map<String, Value>, patch: &Map<String, Value>) -> bool {
    let subtype = current.get("subtype").and_then(Value::as_str);
    let intent = patch.get("intent").or_else(|| current.get("intent")).and_then(Value::as_str);
    matches!(
        (subtype, intent),
        (Some("line"), Some("line-dimension"))
            | (Some("polyline"), Some("polyline-dimension"))
            | (Some("polygon"), Some("polygon-dimension"))
    )
}

/// `assertCaptionMetadataWritable`: a polygon or polyline keeps its caption
/// in `/EMBD_Metadata`; when that entry is there and isn't a dictionary, a
/// patch that touches the caption has nowhere to write it.
fn assert_caption_metadata_writable(
    annotation: &Annotation<'_>,
    current: &Map<String, Value>,
    patch: &Map<String, Value>,
) -> Result<(), EngineError> {
    let subtype = current.get("subtype").and_then(Value::as_str);
    if !matches!(subtype, Some("polygon" | "polyline")) || !touches_caption(patch) {
        return Ok(());
    }
    let present = annotation.has_key("EMBD_Metadata").unwrap_or(false);
    // `FPDF_OBJECT_DICTIONARY`.
    if present && annotation.value_type("EMBD_Metadata").unwrap_or(0) != 6 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "Malformed annotation metadata cannot store a caption",
        ));
    }
    Ok(())
}

/// `preflightPatch`: each kind's own checks of what the schema can't see,
/// before the first write (`preflightStampPatch`).
fn preflight_patch(
    document: &Document,
    resolved: &ResolvedPatch,
    resources: &DraftResources<'_>,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let f = &resolved.fields;
    if resolved.subtype == AnnotationSubtype::Stamp {
        if let Some(Some(name)) = &f.name
            && name.is_empty()
        {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "stamp name must be non-empty",
            ));
        }
        if resources.appearance.is_some() {
            super::stamp::require_stamp_content(document, resources.appearance)?;
        }
    }
    // A font that isn't one of the fourteen is a key this thread must have
    // registered: found out now rather than after the first write.
    if let Some(family) = f.font_family.as_deref()
        && StandardFont::from_family(family).is_none()
        && matches!(resolved.subtype, AnnotationSubtype::FreeText | AnnotationSubtype::Redact)
    {
        context.resolve_font(family)?;
    }
    Ok(())
}

// ── The writers ─────────────────────────────────────────────────────────────

/// `applyPatch`: the kind's writer.
fn apply_patch(
    document: &Document,
    annotation: &Annotation<'_>,
    current: &Map<String, Value>,
    resolved: &ResolvedPatch,
    resources: &DraftResources<'_>,
    services: &UpdateServices<'_>,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    let f = &resolved.fields;
    apply_base(annotation, f)?;
    match resolved.subtype {
        S::Highlight | S::Underline | S::Squiggly | S::Strikeout => {
            if let Some(color) = f.color {
                set_color(annotation, C::Color, color)?;
            }
            if let Some(opacity) = f.opacity {
                set_opacity(annotation, opacity)?;
            }
            if resolved.subtype == S::Strikeout {
                write_intent_patch(annotation, f)?;
            }
            if let Some(quads) = &f.quad_points {
                let shift = shift_between(&current_quad_corners(annotation)?, &corners(quads));
                replace_quad_points(annotation, quads)?;
                match shift {
                    Some(shift) => shift_annot_rect(annotation, shift)?,
                    None => p::set_rect_from_quad_points(annotation, quads)?,
                }
            }
        }
        S::Square | S::Circle => {
            apply_annotation_box_patch(annotation, f.r#box, f.rotation)?;
            apply_filled_style_patch(annotation, f)?;
            apply_cloudy_patch(annotation, f)?;
        }
        S::Polygon | S::Polyline => {
            let points = read_current_points(annotation);
            let placed = update_points(
                annotation,
                &points,
                f.vertices.as_ref().map(std::slice::from_ref),
                f.rotation,
            )?;
            write_measurement_fields(annotation, resolved.subtype, f)?;
            if let Some(drawn) = placed {
                p::set_vertices(annotation, drawn.first().map(Vec::as_slice).unwrap_or_default())?;
            }
            apply_filled_style_patch(annotation, f)?;
            if resolved.subtype == S::Polygon {
                apply_cloudy_patch(annotation, f)?;
            } else if let Some(endings) = f.line_endings {
                p::set_line_endings(annotation, endings)?;
            }
        }
        S::Line => {
            write_measurement_fields(annotation, resolved.subtype, f)?;
            apply_filled_style_patch(annotation, f)?;
            let points: Option<Vec<Vec<PdfPoint>>> =
                f.line_points.map(|line| vec![vec![line.start, line.end]]);
            let current_points: CurrentPoints = read_current_points(annotation);
            if let Some(drawn) =
                update_points(annotation, &current_points, points.as_deref(), f.rotation)?
                && let Some([start, end]) = drawn
                    .first()
                    .map(Vec::as_slice)
                    .and_then(|ends| <&[PdfPoint; 2]>::try_from(ends).ok())
            {
                p::set_line(annotation, *start, *end)?;
            }
            if let Some(endings) = f.line_endings {
                p::set_line_endings(annotation, endings)?;
            }
        }
        S::Ink => {
            write_intent_patch(annotation, f)?;
            apply_geometry_style_patch(annotation, f)?;
            let points = read_current_points(annotation);
            if let Some(drawn) =
                update_points(annotation, &points, f.ink_list.as_deref(), f.rotation)?
            {
                p::set_ink_list(annotation, &drawn)?;
            }
        }
        S::Text => {
            use crate::features::annotation::state::{state_model_to_pdf, state_to_pdf};
            if let Some(rect) = f.rect {
                p::set_rect(annotation, rect)?;
            }
            if let Some(color) = f.color {
                set_color(annotation, C::Color, color)?;
            }
            if let Some(opacity) = f.opacity {
                set_opacity(annotation, opacity)?;
            }
            if let Some(icon) = f.note_icon {
                p::set_name(annotation, icon.pdf_name())?;
            }
            // The popup's `/Open` follows in the mutator, which can reach it.
            if let Some(open) = f.open {
                p::set_boolean(annotation, "Open", open)?;
            }
            if let Some(model) = &f.state_model {
                let model = model.as_deref().map(state_model_to_pdf);
                p::set_string_or_clear(annotation, "StateModel", model.as_deref())?;
            }
            if let Some(state) = &f.state {
                let state = state.as_deref().map(state_to_pdf);
                p::set_string_or_clear(annotation, "State", state.as_deref())?;
            }
        }
        S::Caret => {
            apply_annotation_box_patch(annotation, f.r#box, f.rotation)?;
            if let Some(color) = f.color {
                set_color(annotation, C::Color, color)?;
            }
            if let Some(opacity) = f.opacity {
                set_opacity(annotation, opacity)?;
            }
            write_intent_patch(annotation, f)?;
        }
        S::FreeText => apply_free_text_patch(annotation, f, services.context)?,
        S::Redact => apply_redact_patch(annotation, f, services.context)?,
        S::Link => {
            if let Some(rect) = f.rect {
                p::set_rect(annotation, rect)?;
            }
            match &f.target {
                None => {}
                // Both entries go: removing only `/A` would bring a stale
                // direct `/Dest` back as the live target.
                Some(None) => {
                    if !annotation.remove_action() || !annotation.remove_destination() {
                        return Err(unknown("failed to clear link target"));
                    }
                }
                Some(Some(target)) => write_link_target(document, annotation, target)?,
            }
        }
        S::FileAttachment => {
            if let Some(rect) = f.rect {
                p::set_rect(annotation, rect)?;
            }
            if let Some(color) = f.color {
                set_color(annotation, C::Color, color)?;
            }
            if let Some(opacity) = f.opacity {
                set_opacity(annotation, opacity)?;
            }
            if let Some(icon) = f.file_icon {
                p::set_name(annotation, icon.pdf_name())?;
            }
            write_file_change(document, annotation, f.file.as_ref(), resources.file, services)?;
        }
        S::Stamp => apply_stamp_patch(document, annotation, current, f, resources, services)?,
        S::Popup => {
            if let Some(rect) = f.rect {
                p::set_rect(annotation, rect)?;
            }
            if let Some(open) = f.open_popup {
                p::set_boolean(annotation, "Open", open)?;
            }
        }
        // `applyWidgetPatch`: its place, its look and its actions.
        S::Widget => {
            super::widget::apply_widget_box_patch(annotation, f.r#box, f.widget_rotation)?;
            super::widget::apply_widget_style(annotation, &f.widget, services.context)?;
            if let Some(actions) = &f.widget_actions {
                crate::features::actions::write::write_widget_actions(
                    document,
                    annotation,
                    actions.as_ref(),
                )?;
            }
        }
        S::Unsupported => {
            return Err(EngineError::with_message(
                EngineErrorCode::NotImplemented,
                format!("no writer registered for subtype='{}'", resolved.subtype.as_str()),
            ));
        }
    }
    Ok(())
}

/// `applyAnnotationBasePatch`: contents and subject (tri-state), and the
/// flags the patch names.
fn apply_base(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    if let Some(contents) = &f.contents {
        p::set_string_or_clear(annotation, "Contents", contents.as_deref())?;
    }
    if let Some(subject) = &f.subject {
        p::set_string_or_clear(annotation, "Subj", subject.as_deref())?;
    }
    if f.flags.iter().any(Option::is_some) {
        p::set_flags(annotation, &f.flags)?;
    }
    Ok(())
}

/// `/IT` from a patch: a value, or `null` to remove it.
fn write_intent_patch(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    match &f.intent {
        None => Ok(()),
        Some(None) => p::set_intent_or_clear(annotation, None),
        Some(Some(intent)) => {
            let name = intent_name(intent).ok_or_else(|| unknown("an intent with no /IT name"))?;
            p::set_intent_or_clear(annotation, Some(name))
        }
    }
}

/// `applyBorderPatch`: `/BS` style and width in one call, the one the patch
/// leaves out read so it is kept; then the dash (`null` or empty removes
/// it).
fn apply_border_patch(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    if f.border_style.is_some() || f.stroke_width.is_some() {
        let current = annotation.border_style();
        let style =
            f.border_style.unwrap_or_else(|| AnnotationBorderStyle::from_code(current.style_code));
        let width = f.stroke_width.unwrap_or(f64::from(current.width));
        p::set_border_style(annotation, style.code(), width)?;
    }
    match &f.dash_array {
        None => {}
        Some(None) => p::clear_border_dash_pattern(annotation)?,
        Some(Some(dashes)) if dashes.is_empty() => p::clear_border_dash_pattern(annotation)?,
        Some(Some(dashes)) => p::set_border_dash_pattern(annotation, dashes)?,
    }
    Ok(())
}

/// `applyGeometryStylePatch`: `/C`, `/CA` and the border, each when named.
fn apply_geometry_style_patch(
    annotation: &Annotation<'_>,
    f: &DraftFields,
) -> Result<(), EngineError> {
    if let Some(color) = f.color {
        set_color(annotation, C::Color, color)?;
    }
    if let Some(opacity) = f.opacity {
        set_opacity(annotation, opacity)?;
    }
    apply_border_patch(annotation, f)
}

/// `applyFilledStylePatch`: `/IC` (`null` removes it), then the geometry
/// style.
fn apply_filled_style_patch(
    annotation: &Annotation<'_>,
    f: &DraftFields,
) -> Result<(), EngineError> {
    match f.interior_color {
        None => {}
        Some(None) => p::clear_color(annotation, C::InteriorColor),
        Some(Some(fill)) => set_color(annotation, C::InteriorColor, fill)?,
    }
    apply_geometry_style_patch(annotation, f)
}

/// `/BE`, tri-state: a value sets it, `null` (or a value that isn't
/// positive) removes it, never leaving a degenerate entry.
fn apply_cloudy_patch(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    match f.cloudy_intensity {
        None => Ok(()),
        Some(Some(intensity)) if intensity > 0.0 => p::set_border_effect(annotation, intensity),
        Some(_) => {
            p::clear_border_effect(annotation);
            Ok(())
        }
    }
}

/// `writeMeasurementFields`, as far as 8.25 leaves it: the intent, and a
/// caption hidden, inline and unplaced, or removed. The patch reached here
/// with its caption complete ([`super::patch`]'s `measurementFollows`).
fn write_measurement_fields(
    annotation: &Annotation<'_>,
    subtype: AnnotationSubtype,
    f: &DraftFields,
) -> Result<(), EngineError> {
    write_intent_patch(annotation, f)?;
    match (subtype, f.caption_enabled) {
        (_, None) => {}
        (AnnotationSubtype::Line, Some(None)) => p::remove_key(annotation, "Cap")?,
        (AnnotationSubtype::Line, Some(Some(enabled))) => {
            if !annotation.set_line_caption(enabled, false) {
                return Err(unknown("EPDFAnnot_SetLineCaption returned false"));
            }
        }
        (_, Some(None)) => {
            for key in ["MeasurementCaption", "MeasurementCaptionCenter"] {
                annotation.clear_embed_metadata_key(key).map_err(|e| {
                    EngineError::with_message(
                        EngineErrorCode::RuntimeUnavailable,
                        format!("EPDFAnnot_ClearEmbedMetadataKey failed: {e}"),
                    )
                })?;
            }
        }
        (_, Some(Some(enabled))) => {
            if !annotation.set_shape_caption(enabled) {
                return Err(unknown("EPDFAnnot_SetShapeCaption returned false"));
            }
        }
    }
    Ok(())
}

fn corners(quads: &[NamedPdfQuad]) -> Vec<PdfPoint> {
    quads.iter().flat_map(|q| [q.upper_left, q.upper_right, q.lower_left, q.lower_right]).collect()
}

/// The quads the annotation holds, as corners in the order [`corners`]
/// gives them (`readQuadPoints`, `cornersOf`).
fn current_quad_corners(annotation: &Annotation<'_>) -> Result<Vec<PdfPoint>, EngineError> {
    let quads = annotation.quad_points().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading /QuadPoints failed: {e}"),
        )
    })?;
    let point = |x: f32, y: f32| PdfPoint { x: f64::from(x), y: f64::from(y) };
    Ok(quads
        .into_iter()
        .flat_map(|q| [point(q.x1, q.y1), point(q.x2, q.y2), point(q.x3, q.y3), point(q.x4, q.y4)])
        .collect())
}

/// `replaceQuadPoints`: `/QuadPoints` removed, then the new list appended,
/// so a list can shrink.
fn replace_quad_points(
    annotation: &Annotation<'_>,
    quads: &[NamedPdfQuad],
) -> Result<(), EngineError> {
    p::remove_key(annotation, "QuadPoints")?;
    p::append_quad_points(annotation, quads)
}

/// `applyFreeTextPatch`. Colour model, as Acrobat draws it: `color` is the
/// `/DA` colour (the border and a callout's line), `interiorColor` `/C`, the
/// box's background; the text's style is the rich body's, which the patch
/// reached here stating whole when it moves.
fn apply_free_text_patch(
    annotation: &Annotation<'_>,
    f: &DraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    apply_annotation_box_patch(annotation, f.r#box, f.rotation)?;
    match f.interior_color {
        None => {}
        Some(None) => p::clear_color(annotation, C::Color),
        Some(Some(background)) => set_color(annotation, C::Color, background)?,
    }
    if let Some(opacity) = f.opacity {
        set_opacity(annotation, opacity)?;
    }
    apply_border_patch(annotation, f)?;
    match f.cloudy_intensity {
        None => {}
        Some(None) => p::clear_border_effect(annotation),
        Some(Some(intensity)) => p::set_border_effect(annotation, intensity)?,
    }
    if let Some(Rgb(r, g, b)) = f.color {
        // A border colour change keeps the font `/DA` names and its size.
        match annotation.default_appearance() {
            Some(da) => {
                let size = if da.font_size > 0.0 { da.font_size } else { 12.0 };
                if !annotation.set_default_appearance(KEEP_FONT_CODE, size, (r, g, b)) {
                    return Err(unknown("EPDFAnnot_SetDefaultAppearance returned false"));
                }
            }
            None => set_default_appearance(annotation, "helvetica", 12.0, Rgb(r, g, b), context)?,
        }
    }
    if let Some(rich) = &f.rich_text {
        // Everything regenerated: `/RC`, `/DS`, `/DA`, `/Contents` and the
        // appearance.
        let describe = |key: &str| context.describe_font(key);
        crate::features::annotation::rich_text::write_rich_text(annotation, rich, Some(&describe))?;
    }
    if let Some(align) = f.text_align.as_deref() {
        p::set_text_alignment(annotation, text_alignment_of(align).code())?;
    }
    if let Some(Some(intent)) = &f.intent {
        let name = intent_name(intent).ok_or_else(|| unknown("an intent with no /IT name"))?;
        p::set_intent(annotation, name)?;
    }
    if let Some(align) = f.vertical_align.as_deref() {
        let code = match align {
            "middle" => 1.0,
            "bottom" => 2.0,
            _ => 0.0,
        };
        p::written(
            annotation.set_embed_metadata_number("VerticalAlignment", code),
            "EPDFAnnot_SetEmbedMetadataNumber",
        )?;
    }
    match &f.callout_line {
        None => {}
        Some(None) => p::remove_key(annotation, "CL")?,
        Some(Some(callout)) => {
            let points: Vec<PdfPoint> = match callout {
                crate::geom::CalloutLine::Straight(points) => points.to_vec(),
                crate::geom::CalloutLine::Kneed(points) => points.to_vec(),
            };
            p::set_callout_line(annotation, &points)?;
        }
    }
    match f.line_ending {
        None => {}
        Some(None) => p::remove_key(annotation, "LE")?,
        Some(Some(ending)) => {
            p::set_line_endings(annotation, LineEndings { start: LineEnding::None, end: ending })?;
        }
    }
    Ok(())
}

/// `KEEP_FONT_CODE`: a `/DA` write that keeps the font it names.
const KEEP_FONT_CODE: i32 = -1;

/// `applyRedactPatch`. `/DA` packs the label's font, size and colour into
/// one string: a patch that names some keeps the others as they are (a size
/// of 0 fits the label to the region, so it is kept too).
fn apply_redact_patch(
    annotation: &Annotation<'_>,
    f: &DraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    if let Some(rect) = f.rect {
        p::set_rect(annotation, rect)?;
    }
    if let Some(quads) = &f.quad_points {
        replace_quad_points(annotation, quads)?;
        if f.rect.is_none() {
            p::set_rect_from_quad_points(annotation, quads)?;
        }
    }
    if let Some(color) = f.color {
        set_color(annotation, C::Color, color)?;
    }
    if let Some(opacity) = f.opacity {
        set_opacity(annotation, opacity)?;
    }
    match f.interior_color {
        None => {}
        Some(None) => p::clear_color(annotation, C::InteriorColor),
        Some(Some(fill)) => set_color(annotation, C::InteriorColor, fill)?,
    }
    if let Some(text) = &f.overlay_text {
        // The native setter removes the key for an empty string, so `null`
        // (clear the label) is written as `""`.
        if !annotation.set_overlay_text(text.as_deref().unwrap_or("")) {
            return Err(unknown("EPDFAnnot_SetOverlayText returned false"));
        }
    }
    if let Some(repeat) = f.repeat
        && !annotation.set_overlay_text_repeat(repeat)
    {
        return Err(unknown("EPDFAnnot_SetOverlayTextRepeat returned false"));
    }
    let touches_label = matches!(f.overlay_text, Some(Some(_)))
        || f.font_family.is_some()
        || f.font_size.is_some()
        || f.font_color.is_some();
    if touches_label {
        let current = annotation.default_appearance();
        let family = match (&f.font_family, current) {
            (Some(family), _) => family.clone(),
            (None, Some(da)) => StandardFont::from_code(da.font_code).as_str().to_owned(),
            (None, None) => "helvetica".to_owned(),
        };
        let size = f.font_size.or(current.map(|da| f64::from(da.font_size))).unwrap_or(12.0);
        let color = f
            .font_color
            .or(current.map(|da| Rgb(da.color.0, da.color.1, da.color.2)))
            .unwrap_or(Rgb::BLACK);
        set_default_appearance(annotation, &family, size, color, context)?;
    }
    if let Some(align) = f.text_align.as_deref() {
        p::set_text_alignment(annotation, text_alignment_of(align).code())?;
    }
    Ok(())
}

/// `writeFileChange`: a file attachment's file, the name, type and
/// description replaced whole when the patch names it, and new bytes when
/// the `file` resource comes with it. Its file can't be removed.
fn write_file_change(
    document: &Document,
    annotation: &Annotation<'_>,
    metadata: Option<&Option<super::prepare::AttachmentFileWrite>>,
    bytes: Option<&[u8]>,
    services: &UpdateServices<'_>,
) -> Result<(), EngineError> {
    if metadata.is_none() && bytes.is_none() {
        return Ok(());
    }
    let refused = |message: &str| EngineError::with_message(EngineErrorCode::InvalidArg, message);
    let mut attachment = annotation.file_attachment_for_write();
    if let Some(None) = metadata {
        if attachment.is_some() {
            return Err(refused("a file attachment's file can't be removed"));
        }
        // The `null` a read returns for an annotation without a file, sent
        // back: nothing changes.
        if bytes.is_none() {
            return Ok(());
        }
    }
    let metadata = metadata.and_then(Option::as_ref);
    // `readAttachmentFileInfo`: a read that fails is an error, not a
    // missing value.
    let read_error = |what: &str, e: pdfium::PdfiumError| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the attachment's {what} failed: {e}"),
        )
    };
    let current = match attachment.as_ref() {
        Some(a) => Some((
            a.name().map_err(|e| read_error("name", e))?,
            a.mime_type().map_err(|e| read_error("type", e))?,
            a.description().map_err(|e| read_error("description", e))?,
        )),
        None => None,
    };
    let had_file = current.is_some();
    if attachment.is_none() {
        // No file specification yet: writing one needs the name and the bytes.
        let (Some(file), Some(_)) = (metadata, bytes) else {
            return Err(refused(
                "the file attachment has no file: send the file's name as `file` and its bytes as the `file` resource",
            ));
        };
        attachment = Some(
            annotation
                .add_file_attachment(&file.name)
                .ok_or_else(|| unknown("FPDFAnnot_AddFileAttachment returned NULL"))?,
        );
    }
    let attachment = attachment.ok_or_else(|| unknown("the file attachment has no file"))?;
    let (current_name, current_type, current_description) = current.unwrap_or((None, None, None));
    // The type the file ends with: the data's when it gives one (none if it
    // names none), else the one the file has.
    let mime_type = match metadata {
        Some(file) => file.mime_type.clone(),
        None => current_type.clone(),
    };
    if let Some(bytes) = bytes {
        // New bytes get a new embedded file stream, without the old one's type.
        crate::features::files::attachments::write_file_payload(
            document,
            &attachment,
            bytes,
            // Only the type: the name and description are the file
            // specification's, written below.
            &crate::features::files::attachments::AttachmentFileBase {
                name: String::new(),
                mime_type: mime_type.clone(),
                description: None,
            },
            services.clock,
        )?;
    } else if mime_type != current_type
        && !matches!(attachment.set_mime_type(mime_type.as_deref().unwrap_or("")), Ok(true))
    {
        return Err(refused("the file attachment's file has no embedded data to give a MIME type"));
    }
    let Some(file) = metadata else { return Ok(()) };
    // A file just added is named already (`current !== null`).
    if had_file && Some(&file.name) != current_name.as_ref() && !attachment.set_name(&file.name) {
        return Err(unknown("EPDFAttachment_SetName returned false"));
    }
    if file.description != current_description
        && !attachment.set_description(file.description.as_deref().unwrap_or(""))
    {
        return Err(unknown("EPDFAttachment_SetDescription returned false"));
    }
    Ok(())
}

/// `applyStampPatch`. New bytes replace the drawing, authored in the
/// unturned box; without them the box and its turn move, and the drawing
/// it already has is re-fit (with a new opacity, its opacity layer
/// replaced).
fn apply_stamp_patch(
    document: &Document,
    annotation: &Annotation<'_>,
    current: &Map<String, Value>,
    f: &DraftFields,
    resources: &DraftResources<'_>,
    services: &UpdateServices<'_>,
) -> Result<(), EngineError> {
    match &f.name {
        None => {}
        // The generic removal: the reader then reports the default; the
        // artwork is untouched.
        Some(None) => p::remove_key(annotation, "Name")?,
        Some(Some(name)) => p::set_name(annotation, name)?,
    }
    // The fit the drawing is (re-)fit with: the patch's, else the recorded
    // one, else `fill`, the way a PDF places any appearance in its `/Rect`.
    let recorded = match current.get("fit").and_then(Value::as_str) {
        Some("contain") => StampFit::Contain,
        Some("cover") => StampFit::Cover,
        _ => StampFit::Fill,
    };
    let fit = match f.fit {
        Some(fit) => fit.unwrap_or(StampFit::Fill),
        None => recorded,
    };
    match f.fit {
        None => {}
        Some(None) => {
            annotation.clear_embed_metadata_key("AppearanceFit").map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("EPDFAnnot_ClearEmbedMetadataKey failed: {e}"),
                )
            })?;
        }
        Some(Some(fit)) => write_stamp_fit(annotation, fit)?,
    }
    if let Some(bytes) = resources.appearance {
        // New bytes replace the drawing, so they only need the new value.
        if let Some(opacity) = f.opacity {
            set_opacity(annotation, opacity)?;
        }
        let shape = crate::features::annotation::read::turn::read_annotation_box(annotation);
        let rotation = match f.rotation {
            Some(rotation) => rotation,
            None => shape.rotation,
        };
        return author_stamp_appearance(
            document,
            &mut services.drawings.borrow_mut(),
            annotation,
            bytes,
            fit,
            f.r#box.unwrap_or(shape.content_box),
            rotation,
        );
    }
    // No new bytes: the box and its turn only.
    let moved = apply_annotation_box_patch(annotation, f.r#box, f.rotation)?;
    if let Some(opacity) = f.opacity {
        if !annotation.set_stamp_opacity(fit.native(), opacity as f32) {
            return Err(unknown("EPDFAnnot_SetStampOpacity returned false"));
        }
    } else if (moved || f.fit.is_some()) && !annotation.update_appearance_to_rect(fit.native()) {
        return Err(unknown("EPDFAnnot_UpdateAppearanceToRect returned false"));
    }
    Ok(())
}

// ── Links between annotations ───────────────────────────────────────────────

/// `writeAnnotationRelationship`: `null` makes the annotation top-level;
/// a reply links it to an annotation on its own page (ISO 32000-2
/// §12.5.6.2).
fn write_relationship(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    annotation: &Annotation<'_>,
    f: &DraftFields,
) -> Result<(), EngineError> {
    match &f.reply {
        None => Ok(()),
        Some(None) => {
            p::remove_key(annotation, "IRT")?;
            annotation.set_reply_type(RT_UNKNOWN);
            Ok(())
        }
        Some(Some(reply)) => {
            let at = link_target(document, page_index, page, &reply.to, Link::Reply)?;
            let parent = open_existing(document, page_index, at)?;
            link_reply(annotation, &parent, reply.reply_type)
        }
    }
}

/// `writePopupParent`: a popup and the annotation it shows linked both ways;
/// a previous parent loses its `/Popup` when it named this popup. Answers
/// the parent's name, as the annotation the link wrote.
fn write_popup_parent(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    popup: &Annotation<'_>,
    parent: Option<&AnnotationRef>,
    previous: Option<&AnnotationRef>,
) -> Result<Option<AnnotationRef>, EngineError> {
    if let Some(previous) = previous
        && let Ok(at) = super::resolve::resolve_annotation_index_raw(document, page_index, previous)
        && let Some(earlier) = document.raw_annotation(page_index, at)
    {
        let points_here = earlier
            .linked_annotation("Popup")
            .ok()
            .flatten()
            .is_some_and(|linked| linked.object_number() == popup.object_number());
        if points_here {
            p::remove_key(&earlier, "Popup")?;
        }
    }
    let Some(parent) = parent else {
        p::remove_key(popup, "Parent")?;
        return Ok(None);
    };
    let at = link_target(document, page_index, page, parent, Link::Popup)?;
    let target = open_existing(document, page_index, at)?;
    link_popup(popup, &target)?;
    Ok(Some(crate::features::annotation::read::record::annotation_ref_of(
        document, &target, page, at,
    )))
}

/// `writeLinkedOpen`: `/Open` on the other half of a note and its popup.
/// Only a note shares it: `onlyNote` skips another parent, which keeps its
/// own. Answers the name of the annotation it wrote.
fn write_linked_open(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    target: &AnnotationRef,
    open: bool,
    only_note: bool,
) -> Result<Option<AnnotationRef>, EngineError> {
    let at = super::resolve::resolve_annotation_index_raw(document, page_index, target)?;
    let other = open_existing(document, page_index, at)?;
    if only_note && other.subtype_code() != crate::features::annotation::subtype::code::TEXT {
        return Ok(None);
    }
    p::set_boolean(&other, "Open", open)?;
    Ok(Some(crate::features::annotation::read::record::annotation_ref_of(
        document, &other, page, at,
    )))
}

/// `applyEmbedMetadataOnUpdate`: `UpdatedBy`, and the group the update
/// gives it, when there is someone to name. `UserID`, `CreatedBy` and the
/// rest stay.
fn apply_embed_metadata_on_update(
    annotation: &Annotation<'_>,
    actor: Option<&AnnotationActor>,
) -> Result<(), EngineError> {
    let Some(actor) = actor else { return Ok(()) };
    let user = actor.user_id.as_deref().filter(|s| !s.is_empty());
    let group = actor.group_id.as_deref().filter(|s| !s.is_empty());
    if user.is_none() && group.is_none() {
        return Ok(());
    }
    p::ensure_schema_version(annotation)?;
    if let Some(user) = user {
        p::set_embed_string(annotation, "UpdatedBy", user)?;
    }
    if let Some(group) = group {
        p::set_embed_string(annotation, "GroupID", group)?;
    }
    Ok(())
}
