//! Creating an annotation.
//!
//! G11. Ports `services/src/features/annotations/AnnotationMutator.ts`
//! (`create`) and the one-create path of `AnnotationBatchApplier.ts`, with
//! the draft writers of `internal/write/` (`annotationWriterRegistry.ts`
//! `applyDraft`, `writeAnnotationBase.ts`, `writeStyle.ts` and each kind's
//! `write*Annotation.ts`), `stampCreation.ts`, `generateAppearance.ts` and
//! the links of `writeAnnotationRelationship.ts` (`linkReply`, `linkPopup`).
//! Operation: `annotations.create`, a single verb and a change op.
//!
//! # Checked before the first write
//!
//! Everything that can refuse a create happens before the annotation
//! exists ([`super::prepare::prepare_create`], the name claim, the object
//! number claim, the reply and parent lookups). After that the only
//! failures are PDFium's own, and the change the create runs in takes
//! them back.
//!
//! # Born on its page's `/Annots`, without loading the page
//!
//! `EPDFPage_CreateAnnotRaw` makes the annotation an indirect object at the
//! end of its page's `/Annots`, so a new annotation is named by its object
//! number. It is opened again by that place for each pass: its data and
//! appearance, its links, then its attribution, last, so no later write
//! touches `/M`. A create stamped as the session's (`stamp`) with no `nm` of
//! its own is given a fresh UUIDv7 name.

use pdfium::{Annotation, Document};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::context::AnnotationWriteContext;
use super::draft::AnnotationActor;
use super::prepare::{
    DraftFields, DraftResources, LinkTargetWrite, PreparedDraft, Rgb, StampFit, intent_name,
    prepare_create, text_alignment_of,
};
use super::primitives::{
    self as p, set_color, set_opacity, write_annotation_box, write_new_points,
};
use super::stamp::{DrawingIndex, author_stamp_appearance, write_stamp_fit};
use crate::cancel::CancelToken;
use crate::clock::{Clock, format_pdf_timestamp};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::kinds::{AnnotationRecord, read_annotation_record_kind};
use crate::features::annotation::read::record::read_annotation_record_base;
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::features::annotation::vocabulary::{
    AnnotationBorderStyle, AnnotationColorType as C, AnnotationReplyType, FileAttachmentIcon,
    LineEnding, LineEndings, NoteIcon, StandardFont,
};
use crate::geom::PdfPoint;
use crate::identity::{AnnotationRef, PageObjectNumber, PageRef};
use crate::ids::IdSource;
use crate::mutation::MutationMeta;
use crate::session::document::WriteStamp;

/// The `meta` of an annotation write (`AnnotationListMutationMeta`): the
/// write's meta, and the annotations it changed, the written one first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationMutationMeta {
    #[serde(flatten)]
    pub meta: MutationMeta,
    pub changed: Vec<AnnotationRef>,
}

impl AnnotationMutationMeta {
    /// `annotationMutationMeta`: one page touched, these annotations changed.
    pub fn of(stamp: &WriteStamp, page: PageObjectNumber, changed: Vec<AnnotationRef>) -> Self {
        Self { meta: MutationMeta::local(vec![PageRef::new(page)], stamp), changed }
    }
}

/// What a create answers (`AnnotationCreateResult`), in the file's
/// coordinates: the new annotation read back as a listing reads it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnnotationCreateResult {
    pub annotation: AnnotationRecord,
    pub meta: AnnotationMutationMeta,
    /// A popup's parent as it was before the create linked it, for the
    /// create's reverse to put back. Not on the wire.
    #[serde(skip)]
    pub parent_before: Option<ParentBefore>,
}

/// What a popup's create changed on the annotation it shows: its `/Popup`
/// and, on a note, its `/Open` (`linkPopup`), as they were before.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentBefore {
    #[serde(rename = "ref")]
    pub reference: AnnotationRef,
    /// The object number its `/Popup` named, if any.
    pub popup: Option<u32>,
    pub open: Option<bool>,
}

/// One create (`BatchCreate`, as `AnnotationMutator.create` builds it).
#[derive(Debug)]
pub struct CreateRequest<'a> {
    pub page: PageObjectNumber,
    /// The draft as the caller sent it, in the file's coordinates, with its
    /// `reply` and a popup's `parent` still in it.
    pub draft: Map<String, Value>,
    pub resources: DraftResources<'a>,
    /// The object number it gets, claimed by the caller
    /// ([`crate::session::document::DocumentSession::claim_object_number`]);
    /// the next free one when absent.
    pub object_number: Option<u32>,
    /// Who creates it, stamped as its author; `None` stamps no identity.
    pub actor: Option<AnnotationActor>,
}

/// What a create writes with besides the request.
pub struct CreateServices<'a> {
    pub context: &'a AnnotationWriteContext<'a>,
    pub drawings: &'a std::cell::RefCell<DrawingIndex>,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdSource,
}

fn unknown(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

/// `annotations.create`: one annotation, checked whole before the first
/// write, made without loading its page (`AnnotationMutator.create`).
pub fn create_annotation(
    document: &Document,
    page_index: u32,
    stamp: &WriteStamp,
    request: CreateRequest<'_>,
    services: &CreateServices<'_>,
    cancel: &CancelToken,
) -> Result<AnnotationCreateResult, EngineError> {
    cancel.check()?;
    let CreateRequest { page, mut draft, resources, object_number, actor } = request;
    if draft.get("subtype").and_then(Value::as_str) == Some("widget") {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a widget is the form's: create one with doc.forms.create or doc.forms.addWidget",
        )
        .with_details(serde_json::json!({ "field": "subtype" })));
    }

    // The links come out of the data: the create links them itself.
    let reply = match draft.get("reply") {
        Some(value) if !value.is_null() => Some(read_reply(value)?),
        _ => None,
    };
    let is_popup = draft.get("subtype").and_then(Value::as_str) == Some("popup");
    let parent: Option<AnnotationRef> = match draft.get("parent").filter(|_| is_popup) {
        Some(value) if !value.is_null() => {
            Some(serde_json::from_value(value.clone()).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("popup create field 'parent': {e}"),
                )
                .with_details(serde_json::json!({ "field": "parent" }))
            })?)
        }
        _ => None,
    };
    draft.remove("reply");

    let mut prepared = prepare_create(draft, &resources, services.context, document)?;
    claim_name(document, page_index, page, prepared.nm())?;
    // A stamped create is a new annotation: without a name of its own it
    // gets a fresh one. A fresh name is never taken.
    if prepared.nm().is_none() {
        let fresh = uuid_v7(services.clock.now().epoch_millis, services.ids);
        prepared.fields.nm = Some(Some(fresh));
    }
    let reply_to = reply
        .map(|(to, reply_type)| {
            Ok((link_target(document, page_index, page, &to, Link::Reply)?, reply_type))
        })
        .transpose()?;
    if prepared.subtype == AnnotationSubtype::Popup && parent.is_none() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "create: a popup needs the annotation it shows (parent)",
        )
        .with_details(serde_json::json!({ "field": "parent" })));
    }
    let parent_at = parent
        .as_ref()
        .map(|parent| link_target(document, page_index, page, parent, Link::Popup))
        .transpose()?;
    // The apply boundary: everything below writes.
    cancel.check()?;

    let annotation = document
        .create_annotation_raw(page_index, prepared.subtype.code(), object_number.unwrap_or(0))
        .ok_or_else(|| match object_number {
            Some(number) => crate::session::document::object_number_unavailable(number, "taken"),
            None => unknown("create: EPDFPage_CreateAnnotRaw returned NULL"),
        })?;
    apply_draft(document, &annotation, &prepared, &resources, services)?;
    generate_appearance(&annotation, prepared.fields.blend_mode.map(|mode| mode.code()));
    let object_number = annotation.object_number();
    if object_number == 0 {
        return Err(unknown("create: EPDFPage_CreateAnnotRaw made a direct object"));
    }
    drop(annotation);
    let index = document
        .raw_annotation_count(page_index)
        .and_then(|count| count.checked_sub(1))
        .ok_or_else(|| unknown("a created annotation is not where it was"))?;
    let open = || -> Result<Annotation<'_>, EngineError> {
        document
            .raw_annotation(page_index, index)
            .filter(|created| created.object_number() == object_number)
            .ok_or_else(|| unknown("a created annotation is not where it was"))
    };

    // A reply writes only itself. A popup's parent gains its `/Popup`: one
    // the document has already is named in the change too.
    let mut linked: Vec<AnnotationRef> = Vec::new();
    if let Some((target, reply_type)) = reply_to {
        let created = open()?;
        let parent = open_existing(document, page_index, target)?;
        link_reply(&created, &parent, reply_type)?;
    }
    let mut parent_before = None;
    if let Some(target) = parent_at {
        let created = open()?;
        let parent = open_existing(document, page_index, target)?;
        let reference = crate::features::annotation::read::record::annotation_ref_of(
            document, &parent, page, target,
        );
        parent_before = Some(ParentBefore {
            reference: reference.clone(),
            popup: parent.linked_annotation("Popup").ok().flatten().map(|p| p.object_number()),
            open: parent.boolean_value("Open").ok().flatten(),
        });
        link_popup(&created, &parent)?;
        linked.push(reference);
    }

    // Attributed last, so no later write touches `/M`.
    stamp_creation(&open()?, actor.as_ref(), services.clock)?;

    let created = open()?;
    let mut budget = crate::features::actions::vocabulary::ActionReadBudgetTracker::default();
    let base = read_annotation_record_base(document, &created, page, index, &mut budget)?;
    let kind = read_annotation_record_kind(document, &created, &base, created.subtype_code())?;
    let mut changed = vec![AnnotationRef::ObjectNumber { page, annot: object_number }];
    changed.extend(linked);
    Ok(AnnotationCreateResult {
        annotation: AnnotationRecord { base, kind },
        meta: AnnotationMutationMeta::of(stamp, page, changed),
        parent_before,
    })
}

fn read_reply(value: &Value) -> Result<(AnnotationRef, AnnotationReplyType), EngineError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        to: AnnotationRef,
        #[serde(default, rename = "type")]
        reply_type: Option<String>,
    }
    let invalid = |message: String| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("create field 'reply': {message}"),
        )
        .with_details(serde_json::json!({ "field": "reply" }))
    };
    let reply: Reply = serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
    let reply_type = match reply.reply_type.as_deref() {
        None | Some("reply") => AnnotationReplyType::Reply,
        Some("group") => AnnotationReplyType::Group,
        Some(other) => return Err(invalid(format!("unknown reply type '{other}'"))),
    };
    Ok((reply.to, reply_type))
}

/// A UUIDv7 (RFC 9562 §5.7): 48 bits of milliseconds, then random bits
/// under the version and variant nibbles (`generateUuidV7`).
pub fn uuid_v7(epoch_millis: i64, ids: &dyn IdSource) -> String {
    let mut bytes = [0u8; 16];
    ids.fill_random(&mut bytes);
    let mut time = epoch_millis.max(0) as u64;
    for at in (0..6).rev() {
        bytes[at] = (time % 256) as u8;
        time /= 256;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// A name must be free on its page (ISO 32000-2 §12.5.2, `claimName`).
fn claim_name(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    nm: Option<&str>,
) -> Result<(), EngineError> {
    let Some(nm) = nm else { return Ok(()) };
    let count = document.raw_annotation_count(page_index).unwrap_or(0);
    let taken = (0..count)
        .filter_map(|index| document.raw_annotation(page_index, index))
        .any(|annotation| annotation.string_value("NM").ok().flatten().as_deref() == Some(nm));
    if taken {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("nm '{nm}' is already used on page {}", page.get()),
        )
        .with_details(serde_json::json!({ "field": "nm" })));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Reply,
    Popup,
}

/// A link's target, checked before the first write: an annotation the
/// document has, on the same page (ISO 32000-2 §12.5.6.2). Answers its
/// place in the page's `/Annots`; creates only append, so it holds.
fn link_target(
    document: &Document,
    page_index: u32,
    page: PageObjectNumber,
    target: &AnnotationRef,
    link: Link,
) -> Result<u32, EngineError> {
    if target.page() != page {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            match link {
                Link::Reply => format!(
                    "/IRT parent must be on the same page as the reply (parent page {}, reply page {})",
                    target.page().get(),
                    page.get()
                ),
                Link::Popup => format!(
                    "a popup's parent must be on the same page (parent page {}, popup page {})",
                    target.page().get(),
                    page.get()
                ),
            },
        ));
    }
    super::resolve::resolve_annotation_index_raw(document, page_index, target)
}

fn open_existing(
    document: &Document,
    page_index: u32,
    index: u32,
) -> Result<Annotation<'_>, EngineError> {
    document
        .raw_annotation(page_index, index)
        .ok_or_else(|| unknown("an annotation to link to is not where it was"))
}

/// `linkReply`: `/IRT` and `/RT`.
fn link_reply(
    annotation: &Annotation<'_>,
    parent: &Annotation<'_>,
    reply_type: AnnotationReplyType,
) -> Result<(), EngineError> {
    if !matches!(annotation.set_linked_annotation("IRT", parent), Ok(true)) {
        return Err(unknown("failed to set /IRT linked annotation"));
    }
    annotation.set_reply_type(reply_type.code());
    Ok(())
}

/// `linkPopup`: `/Parent` and `/Popup`, both ways. An annotation has one
/// popup: a second is refused, naming the first. A note and its popup hold
/// one `/Open`: on linking the popup's wins when it has one, else it takes
/// the note's.
fn link_popup(popup: &Annotation<'_>, parent: &Annotation<'_>) -> Result<(), EngineError> {
    if let Ok(Some(existing)) = parent.linked_annotation("Popup") {
        let existing = existing.object_number();
        if existing != popup.object_number() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the annotation already has a popup (object {existing}); delete it first"),
            )
            .with_details(
                serde_json::json!({ "field": "parent", "popupObjectNumber": existing }),
            ));
        }
    }
    let linked = matches!(popup.set_linked_annotation("Parent", parent), Ok(true))
        && matches!(parent.set_linked_annotation("Popup", popup), Ok(true));
    if !linked {
        return Err(unknown("failed to link a popup to its parent"));
    }
    if parent.subtype_code() == crate::features::annotation::subtype::code::TEXT {
        match popup.boolean_value("Open").ok().flatten() {
            Some(open) => p::set_boolean(parent, "Open", open)?,
            None => {
                if let Some(open) = parent.boolean_value("Open").ok().flatten() {
                    p::set_boolean(popup, "Open", open)?;
                }
            }
        }
    }
    Ok(())
}

/// `stampCreation`: `/T` from the actor's name, `/CreationDate` and `/M` at
/// now, and the owner in `/EMBD_Metadata`.
fn stamp_creation(
    annotation: &Annotation<'_>,
    actor: Option<&AnnotationActor>,
    clock: &dyn Clock,
) -> Result<(), EngineError> {
    if let Some(name) = actor.and_then(|a| a.display_name.as_deref()).filter(|n| !n.is_empty()) {
        p::set_string(annotation, "T", name)?;
    }
    let now =
        format_pdf_timestamp(clock.now()).ok_or_else(|| unknown("the clock gave no PDF date"))?;
    p::set_string(annotation, "CreationDate", &now)?;
    p::set_string(annotation, "M", &now)?;
    // `applyEmbedMetadataOnCreate`.
    let Some(actor) = actor else { return Ok(()) };
    let user = actor.user_id.as_deref().filter(|s| !s.is_empty());
    let group = actor.group_id.as_deref().filter(|s| !s.is_empty());
    if user.is_none() && group.is_none() {
        return Ok(());
    }
    p::written(
        annotation.set_embed_metadata_number("SchemaVersion", p::EMBD_METADATA_SCHEMA_VERSION),
        "EPDFAnnot_SetEmbedMetadataNumber",
    )?;
    if let Some(user) = user {
        for key in ["UserID", "CreatedBy", "UpdatedBy"] {
            p::set_embed_string(annotation, key, user)?;
        }
    }
    if let Some(group) = group {
        p::set_embed_string(annotation, "GroupID", group)?;
    }
    Ok(())
}

/// `generateAppearance`: bake `/AP /N` from the dictionary. `false` for a
/// kind with no generator (a stamp keeps the appearance its writer made).
fn generate_appearance(annotation: &Annotation<'_>, blend: Option<i32>) -> bool {
    annotation.generate_appearance(blend)
}

// ── The writers ─────────────────────────────────────────────────────────────

/// What a create that leaves a field out reads back (`ANNOTATION_DEFAULTS`):
/// the colour of each kind.
fn default_color(subtype: AnnotationSubtype) -> Rgb {
    use AnnotationSubtype as S;
    match subtype {
        S::Highlight | S::Text | S::FileAttachment => Rgb::YELLOW,
        S::Underline | S::Squiggly | S::Strikeout | S::FreeText => Rgb::BLACK,
        _ => Rgb::RED,
    }
}

/// `applyDraft`: the kind's writer.
fn apply_draft(
    document: &Document,
    annotation: &Annotation<'_>,
    draft: &PreparedDraft,
    resources: &DraftResources<'_>,
    services: &CreateServices<'_>,
) -> Result<(), EngineError> {
    use AnnotationSubtype as S;
    let f = &draft.fields;
    apply_base(annotation, draft)?;
    match draft.subtype {
        S::Highlight | S::Underline | S::Squiggly | S::Strikeout => {
            let quads = f.quad_points.as_deref().unwrap_or_default();
            p::append_quad_points(annotation, quads)?;
            p::set_rect_from_quad_points(annotation, quads)?;
            set_color(annotation, C::Color, f.color.unwrap_or(default_color(draft.subtype)))?;
            set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
            if draft.subtype == S::Strikeout {
                write_intent(annotation, f)?;
            }
        }
        S::Square | S::Circle => {
            write_annotation_box(annotation, placed_box(f)?, rotation(f))?;
            apply_filled_style(annotation, f)?;
            write_cloudy(annotation, f)?;
        }
        S::Polygon | S::Polyline => {
            let vertices = f.vertices.clone().unwrap_or_default();
            let drawn = write_new_points(annotation, &[vertices], rotation(f))?;
            write_intent(annotation, f)?;
            write_caption(annotation, draft.subtype, f)?;
            apply_filled_style(annotation, f)?;
            p::set_vertices(annotation, drawn.first().map(Vec::as_slice).unwrap_or_default())?;
            if draft.subtype == S::Polygon {
                write_cloudy(annotation, f)?;
            } else {
                p::set_line_endings(annotation, f.line_endings.unwrap_or(NO_ENDINGS))?;
            }
        }
        S::Line => {
            write_intent(annotation, f)?;
            write_caption(annotation, draft.subtype, f)?;
            apply_filled_style(annotation, f)?;
            let line = f.line_points.ok_or_else(|| unknown("a line create has no linePoints"))?;
            let drawn = write_new_points(annotation, &[vec![line.start, line.end]], rotation(f))?;
            let ends = drawn.first().cloned().unwrap_or_default();
            if let [start, end] = ends.as_slice() {
                p::set_line(annotation, *start, *end)?;
            }
            p::set_line_endings(annotation, f.line_endings.unwrap_or(NO_ENDINGS))?;
        }
        S::Ink => {
            write_intent(annotation, f)?;
            apply_geometry_style(annotation, f)?;
            let ink = f.ink_list.clone().unwrap_or_default();
            let drawn = write_new_points(annotation, &ink, rotation(f))?;
            p::set_ink_list(annotation, &drawn)?;
        }
        S::Link => {
            p::set_rect(annotation, f.rect.ok_or_else(|| unknown("a link create has no rect"))?)?;
            // A new link draws no border: `/Border [0 0 0]`, where PDF apps
            // would draw a black one.
            if !annotation.set_border(0.0, 0.0, 0.0) {
                return Err(unknown("FPDFAnnot_SetBorder returned false"));
            }
            if let Some(Some(target)) = &f.target {
                write_link_target(document, annotation, target)?;
            }
        }
        S::Text => {
            p::set_rect(annotation, f.rect.ok_or_else(|| unknown("a note create has no rect"))?)?;
            set_color(annotation, C::Color, f.color.unwrap_or(Rgb::YELLOW))?;
            set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
            p::set_name(annotation, f.note_icon.unwrap_or(NoteIcon::Note).pdf_name())?;
            if let Some(open) = f.open {
                p::set_boolean(annotation, "Open", open)?;
            }
            use crate::features::annotation::state::{state_model_to_pdf, state_to_pdf};
            if let Some(Some(model)) = &f.state_model {
                p::set_string(annotation, "StateModel", &state_model_to_pdf(model))?;
            }
            if let Some(Some(state)) = &f.state {
                p::set_string(annotation, "State", &state_to_pdf(state))?;
            }
        }
        S::Caret => {
            write_annotation_box(annotation, placed_box(f)?, rotation(f))?;
            set_color(annotation, C::Color, f.color.unwrap_or(Rgb::RED))?;
            set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
            write_intent(annotation, f)?;
        }
        S::FreeText => apply_free_text(annotation, f, services.context)?,
        S::Stamp => {
            if let Some(Some(name)) = &f.name {
                p::set_name(annotation, name)?;
            }
            // Before the drawing is placed: the wrapper paints `/CA`.
            if let Some(opacity) = f.opacity {
                set_opacity(annotation, opacity)?;
            }
            // A create records the fit it used; data that records none is
            // fit as such a stamp is shown, `fill`.
            let fit = match f.fit {
                Some(None) => StampFit::Fill,
                Some(Some(fit)) => fit,
                None => StampFit::Contain,
            };
            let bytes = resources.appearance.ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "creating a stamp annotation needs its 'appearance' resource",
                )
            })?;
            author_stamp_appearance(
                document,
                &mut services.drawings.borrow_mut(),
                annotation,
                bytes,
                fit,
                placed_box(f)?,
                rotation(f),
            )?;
            if !matches!(f.fit, Some(None)) {
                write_stamp_fit(annotation, fit)?;
            }
        }
        S::FileAttachment => {
            p::set_rect(annotation, f.rect.ok_or_else(|| unknown("an attachment has no rect"))?)?;
            set_color(annotation, C::Color, f.color.unwrap_or(Rgb::YELLOW))?;
            set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
            p::set_name(
                annotation,
                f.file_icon.unwrap_or(FileAttachmentIcon::Paperclip).pdf_name(),
            )?;
            let file = f.file.clone().flatten().unwrap_or_default();
            let attachment = annotation
                .add_file_attachment(&file.name)
                .ok_or_else(|| unknown("FPDFAnnot_AddFileAttachment returned NULL"))?;
            let bytes = resources.file.ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "creating a file-attachment annotation needs its 'file' resource",
                )
            })?;
            crate::features::files::attachments::write_file_payload(
                document,
                &attachment,
                bytes,
                &crate::features::files::attachments::AttachmentFileBase {
                    name: file.name.clone(),
                    mime_type: file.mime_type.clone(),
                    description: file.description.clone(),
                },
                services.clock,
            )?;
        }
        S::Popup => {
            p::set_rect(annotation, f.rect.ok_or_else(|| unknown("a popup has no rect"))?)?;
            if let Some(open) = f.open_popup {
                p::set_boolean(annotation, "Open", open)?;
            }
        }
        S::Redact => apply_redact(annotation, f, services.context)?,
        S::Widget | S::Unsupported => {
            return Err(EngineError::with_message(
                EngineErrorCode::NotImplemented,
                format!("no writer registered for draft.subtype='{}'", draft.subtype.as_str()),
            ));
        }
    }
    Ok(())
}

const NO_ENDINGS: LineEndings = LineEndings { start: LineEnding::None, end: LineEnding::None };

fn placed_box(f: &DraftFields) -> Result<crate::geom::PdfRect, EngineError> {
    f.r#box.ok_or_else(|| unknown("a box kind reached its writer without its box"))
}

fn rotation(f: &DraftFields) -> Option<f64> {
    f.rotation.flatten()
}

/// `applyAnnotationBaseDraft`: contents, subject, nm, and the flags, each
/// one the draft leaves out its kind's default (a new annotation prints,
/// except a popup).
fn apply_base(annotation: &Annotation<'_>, draft: &PreparedDraft) -> Result<(), EngineError> {
    let f = &draft.fields;
    if let Some(contents) = &f.contents {
        p::set_string_or_clear(annotation, "Contents", contents.as_deref())?;
    }
    if let Some(subject) = &f.subject {
        p::set_string_or_clear(annotation, "Subj", subject.as_deref())?;
    }
    if let Some(nm) = draft.nm() {
        p::set_string(annotation, "NM", nm)?;
    }
    let mut flags: [Option<bool>; 10] = [Some(false); 10];
    // `print`, bit 2, is the one default that is on.
    flags[2] = Some(draft.subtype != AnnotationSubtype::Popup);
    for (bit, given) in f.flags.iter().enumerate() {
        if given.is_some() {
            flags[bit] = *given;
        }
    }
    p::set_flags(annotation, &flags)
}

fn write_intent(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    if let Some(Some(intent)) = &f.intent {
        let name = intent_name(intent).ok_or_else(|| unknown("an intent with no /IT name"))?;
        p::set_intent(annotation, name)?;
    }
    Ok(())
}

/// The caption half of `writeMeasurementFields`, for the captions a create
/// can write before 8.25: hidden, inline, with no offset or centre. A line
/// that names any caption field states the caption whole, hidden unless it
/// says `null` (`resolveMeasurementDraft`); a shape writes a hidden caption
/// when it says so.
fn write_caption(
    annotation: &Annotation<'_>,
    subtype: AnnotationSubtype,
    f: &DraftFields,
) -> Result<(), EngineError> {
    if !f.caption_touched {
        return Ok(());
    }
    let enabled = match subtype {
        AnnotationSubtype::Line => f.caption_enabled.unwrap_or(Some(false)),
        _ => f.caption_enabled.flatten(),
    };
    // No caption flag: nothing to write on a new annotation.
    if let Some(enabled) = enabled {
        let ok = if subtype == AnnotationSubtype::Line {
            annotation.set_line_caption(enabled, false)
        } else {
            annotation.set_shape_caption(enabled)
        };
        if !ok {
            return Err(unknown("the caption could not be written"));
        }
    }
    Ok(())
}

fn write_cloudy(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    if let Some(Some(intensity)) = f.cloudy_intensity
        && intensity > 0.0
    {
        p::set_border_effect(annotation, intensity)?;
    }
    Ok(())
}

/// `applyBorderDraft`: `/BS` (a 1 pt solid border when left out), then the
/// dash.
fn apply_border(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    p::set_border_style(
        annotation,
        f.border_style.unwrap_or(AnnotationBorderStyle::Solid).code(),
        f.stroke_width.unwrap_or(1.0),
    )?;
    if let Some(Some(dashes)) = &f.dash_array
        && !dashes.is_empty()
    {
        p::set_border_dash_pattern(annotation, dashes)?;
    }
    Ok(())
}

/// `applyGeometryStyleDraft`: `/C`, `/CA` and the border.
fn apply_geometry_style(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    set_color(annotation, C::Color, f.color.unwrap_or(Rgb::RED))?;
    set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
    apply_border(annotation, f)
}

/// `applyFilledStyleDraft`: `/IC` set, or cleared when left out or `null`,
/// then the geometry style.
fn apply_filled_style(annotation: &Annotation<'_>, f: &DraftFields) -> Result<(), EngineError> {
    match f.interior_color.flatten() {
        Some(fill) => set_color(annotation, C::InteriorColor, fill)?,
        None => p::clear_color(annotation, C::InteriorColor),
    }
    apply_geometry_style(annotation, f)
}

/// `applyDefaultAppearance`: `/DA` from a standard font, or from a font this
/// thread registered under its key.
fn set_default_appearance(
    annotation: &Annotation<'_>,
    family: &str,
    size: f64,
    Rgb(r, g, b): Rgb,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let ok = match StandardFont::from_family(family) {
        Some(standard) => {
            annotation.set_default_appearance(standard.code(), size as f32, (r, g, b))
        }
        None => annotation.set_default_appearance_registered_font(
            context.resolve_font(family)?,
            size as f32,
            (r, g, b),
        ),
    };
    if !ok {
        return Err(unknown("EPDFAnnot_SetDefaultAppearance returned false"));
    }
    Ok(())
}

/// `applyFreeTextDraft`. Colour model, as Acrobat draws it: `color` is the
/// `/DA` colour (the border and a callout's line), `fontColor` the rich
/// body's (the text), `interiorColor` `/C`, the box's background.
fn apply_free_text(
    annotation: &Annotation<'_>,
    f: &DraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    write_annotation_box(annotation, placed_box(f)?, rotation(f))?;
    match f.interior_color.flatten() {
        Some(background) => set_color(annotation, C::Color, background)?,
        None => p::clear_color(annotation, C::Color),
    }
    set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
    apply_border(annotation, f)?;
    if let Some(Some(intensity)) = f.cloudy_intensity {
        p::set_border_effect(annotation, intensity)?;
    }
    // The rich text writer below sets the `/DA` font and size from the body
    // and keeps this colour: the border's.
    set_default_appearance(
        annotation,
        f.font_family.as_deref().unwrap_or("helvetica"),
        f.font_size.unwrap_or(12.0),
        f.color.unwrap_or(Rgb::BLACK),
        context,
    )?;
    let rich = f.rich_text.clone().ok_or_else(|| unknown("a free text without its rich text"))?;
    let align = rich
        .body
        .as_ref()
        .and_then(|body| body.get("align"))
        .and_then(Value::as_str)
        .unwrap_or("left");
    p::set_text_alignment(annotation, text_alignment_of(align).code())?;
    write_intent(annotation, f)?;
    if let Some(align) = f.vertical_align.as_deref().filter(|align| *align != "top") {
        let code = if align == "middle" { 1.0 } else { 2.0 };
        p::written(
            annotation.set_embed_metadata_number("VerticalAlignment", code),
            "EPDFAnnot_SetEmbedMetadataNumber",
        )?;
    }
    if let Some(Some(callout)) = &f.callout_line {
        let points: Vec<PdfPoint> = match callout {
            crate::geom::CalloutLine::Straight(points) => points.to_vec(),
            crate::geom::CalloutLine::Kneed(points) => points.to_vec(),
        };
        p::set_callout_line(annotation, &points)?;
    }
    if let Some(Some(ending)) = f.line_ending {
        p::set_line_endings(annotation, LineEndings { start: LineEnding::None, end: ending })?;
    }
    // Rich text last, always: a box is born with all four forms (`/RC`,
    // `/DS`, `/DA`, `/Contents`) and its appearance, after the geometry the
    // layout needs.
    let describe = |key: &str| context.describe_font(key);
    crate::features::annotation::rich_text::write_rich_text(annotation, &rich, Some(&describe))
}

/// `applyRedactDraft`. `color` is `/C`, the marking's outline; `interiorColor`
/// `/IC`, the fill after the apply (left out, never cleared: ISO leaves the
/// region transparent); `fontColor` the label's `/DA` colour.
fn apply_redact(
    annotation: &Annotation<'_>,
    f: &DraftFields,
    context: &AnnotationWriteContext<'_>,
) -> Result<(), EngineError> {
    let quads = f.quad_points.as_deref().unwrap_or_default();
    if !quads.is_empty() {
        p::append_quad_points(annotation, quads)?;
    }
    match f.rect {
        Some(rect) => p::set_rect(annotation, rect)?,
        None => p::set_rect_from_quad_points(annotation, quads)?,
    }
    set_color(annotation, C::Color, f.color.unwrap_or(Rgb::RED))?;
    set_opacity(annotation, f.opacity.unwrap_or(1.0))?;
    if let Some(Some(fill)) = f.interior_color {
        set_color(annotation, C::InteriorColor, fill)?;
    }
    if let Some(Some(text)) = &f.overlay_text
        && !text.is_empty()
        && !annotation.set_overlay_text(text)
    {
        return Err(unknown("EPDFAnnot_SetOverlayText returned false"));
    }
    if f.repeat == Some(true) && !annotation.set_overlay_text_repeat(true) {
        return Err(unknown("EPDFAnnot_SetOverlayTextRepeat returned false"));
    }
    // ISO 32000-2 requires a `/DA` beside `/OverlayText`: a label, or any of
    // its style, writes one, defaults filling the rest.
    let touches_label = matches!(f.overlay_text, Some(Some(_)))
        || f.font_family.is_some()
        || f.font_size.is_some()
        || f.font_color.is_some();
    if touches_label {
        set_default_appearance(
            annotation,
            f.font_family.as_deref().unwrap_or("helvetica"),
            f.font_size.unwrap_or(12.0),
            f.font_color.unwrap_or(Rgb::BLACK),
            context,
        )?;
    }
    if let Some(align) = f.text_align.as_deref() {
        p::set_text_alignment(annotation, text_alignment_of(align).code())?;
    }
    Ok(())
}

/// `applyLinkTarget` (`createLinkTargetAction`): a new action of the
/// document, set as `/A`.
fn write_link_target(
    document: &Document,
    annotation: &Annotation<'_>,
    target: &LinkTargetWrite,
) -> Result<(), EngineError> {
    let action = match target {
        LinkTargetWrite::Uri(uri) => document.create_uri_action(uri),
        LinkTargetWrite::Named(name) => document.create_named_action(name),
        LinkTargetWrite::Goto(destination) => {
            document.create_goto_action(super::legacy::build_destination(document, destination)?)
        }
    }
    .map_err(|e| unknown(format!("failed to create the link's action: {e}")))?;
    if !annotation.set_action(action) {
        return Err(unknown("EPDFAnnot_SetAction failed for link"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_name_is_a_uuid_v7_of_its_moment() {
        let ids = crate::ids::SeededIdSource::new(7);
        let name = uuid_v7(0x0123_4567_89ab, &ids);
        assert_eq!(&name[..13], "01234567-89ab");
        assert_eq!(&name[14..15], "7", "the version nibble");
        assert!(matches!(&name[19..20], "8" | "9" | "a" | "b"), "the variant: {name}");
        assert_eq!(name.len(), 36);
    }
}
