//! The shell every annotation DTO carries.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readAnnotationBase.ts`.
//!
//! Identity, flags, rect, the markup strings, the dates, the blend mode, the
//! reply edge, the EmbedPDF fields and the eleven action slots. No
//! subtype-specific field is here and none ever will be: a subtype reader
//! extends this with its own, so the shell does not grow when a subtype lands.

use pdfium::{Annotation, AnnotationActionEvent, Document};
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::embed_metadata::read_embed_metadata;
use super::identity::read_annotation_identity;
use super::relationship::read_annotation_relationship;
use crate::clock::pdf_date_to_iso;
use crate::error::EngineError;
use crate::features::actions::reader::{ActionTree, read_action_model};
use crate::features::actions::vocabulary::ActionReadBudgetTracker;
use crate::features::annotation::flags::AnnotationFlags;
use crate::features::annotation::vocabulary::{AnnotationReplyType, BlendMode};
use crate::geom::PdfRect;
use crate::identity::{AnnotationIdentityQuality, AnnotationRef, PageObjectNumber};
use crate::revision::RevisionToken;

/// `/A` and the ten `/AA` entries, with only the slots the annotation
/// actually fills.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationActions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activate: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_enter: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_exit: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_down: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_up: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blur: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_open: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_close: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_visible: Option<ActionTree>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_invisible: Option<ActionTree>,
}

impl AnnotationActions {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Identity and provenance, shared by every subtype.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationBase {
    #[serde(rename = "ref")]
    pub annotation_ref: AnnotationRef,
    pub page_object_number: PageObjectNumber,
    /// Position in `/Annots`, zero based. Display order, not identity.
    pub index: u32,
    pub identity_quality: AnnotationIdentityQuality,
    pub nm: Option<String>,
    pub flags: AnnotationFlags,
    /// `/Rect` as the file wrote it, NOT normalized.
    ///
    /// Deliberate, and it differs from every other rect this engine reports.
    /// A `/Rect` whose corners are the other way round is a fact about the
    /// document that a writer has to preserve, and normalizing on read would
    /// make a read-modify-write silently reorder a rectangle the caller never
    /// touched. `annotation_base.pdf` carries one.
    pub rect: PdfRect,
    /// `/Contents`. `None` iff the key is absent.
    pub contents: Option<String>,
    /// `/Subj`.
    pub subject: Option<String>,
    /// `/T`, the author.
    pub author: Option<String>,
    /// `/CreationDate`, as ISO 8601.
    pub created: Option<String>,
    /// `/M`, as ISO 8601.
    pub modified: Option<String>,
    pub blend_mode: BlendMode,
    pub in_reply_to: Option<AnnotationRef>,
    /// `Some` exactly when `in_reply_to` is.
    pub reply_type: Option<AnnotationReplyType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<AnnotationActions>,
}

/// Read the shell.
///
/// `budget` spans a whole page listing rather than one annotation, for the
/// reason it spans a whole page listing in `pages.list`: PDFium's limits are
/// per model, and a page with an action on every annotation could otherwise
/// build an unbounded snapshot out of individually reasonable ones.
pub fn read_annotation_base(
    document: &Document,
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
    revision: &RevisionToken,
    budget: &mut ActionReadBudgetTracker,
) -> Result<AnnotationBase, EngineError> {
    let identity = read_annotation_identity(annotation, page, index, revision)?;
    let relationship = read_annotation_relationship(annotation, page)?;
    let embed = read_embed_metadata(annotation)?.unwrap_or_default();
    let actions = read_annotation_actions(document, annotation, budget)?;

    let rect = annotation.rect();
    let created = dictionary_read("CreationDate", annotation.string_value("CreationDate"))?;
    let modified = dictionary_read("M", annotation.string_value("M"))?;

    Ok(AnnotationBase {
        annotation_ref: identity.annotation_ref,
        page_object_number: page,
        index,
        identity_quality: identity.quality,
        nm: identity.nm,
        flags: AnnotationFlags::from_bits(annotation.flags()),
        rect: PdfRect {
            left: f64::from(rect.left),
            bottom: f64::from(rect.bottom),
            right: f64::from(rect.right),
            top: f64::from(rect.top),
        },
        contents: dictionary_read("Contents", annotation.string_value("Contents"))?,
        subject: dictionary_read("Subj", annotation.string_value("Subj"))?,
        author: dictionary_read("T", annotation.string_value("T"))?,
        // A date the parser refuses becomes `None` rather than an error. A
        // malformed `/M` is common in the wild and is not a reason to refuse
        // to report an annotation's geometry.
        created: created.as_deref().and_then(pdf_date_to_iso),
        modified: modified.as_deref().and_then(pdf_date_to_iso),
        blend_mode: BlendMode::from_code(annotation.blend_mode_code()),
        in_reply_to: relationship.in_reply_to,
        reply_type: relationship.reply_type,
        user_id: embed.user_id,
        group_id: embed.group_id,
        created_by: embed.created_by,
        updated_by: embed.updated_by,
        actions,
    })
}

/// The eleven slots, with the whole struct dropped when every one is empty.
fn read_annotation_actions(
    document: &Document,
    annotation: &Annotation<'_>,
    budget: &mut ActionReadBudgetTracker,
) -> Result<Option<AnnotationActions>, EngineError> {
    let mut read = |event: AnnotationActionEvent| -> Result<Option<ActionTree>, EngineError> {
        match document.annotation_action_model(annotation, event) {
            Some(model) => read_action_model(&model, budget).map(Some),
            None => Ok(None),
        }
    };

    let actions = AnnotationActions {
        activate: read(AnnotationActionEvent::Activate)?,
        cursor_enter: read(AnnotationActionEvent::CursorEnter)?,
        cursor_exit: read(AnnotationActionEvent::CursorExit)?,
        mouse_down: read(AnnotationActionEvent::MouseDown)?,
        mouse_up: read(AnnotationActionEvent::MouseUp)?,
        focus: read(AnnotationActionEvent::Focus)?,
        blur: read(AnnotationActionEvent::Blur)?,
        page_open: read(AnnotationActionEvent::PageOpen)?,
        page_close: read(AnnotationActionEvent::PageClose)?,
        page_visible: read(AnnotationActionEvent::PageVisible)?,
        page_invisible: read(AnnotationActionEvent::PageInvisible)?,
    };
    Ok((!actions.is_empty()).then_some(actions))
}
