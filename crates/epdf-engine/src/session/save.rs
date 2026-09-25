//! G10. Mutation finalization (Stage B).
//!
//! Replaces `WorkerHost.finishMutation` and `saveLayerArtifact`
//! (`WorkerHost.ts:1380-1414`), and the event vocabulary in
//! `core/src/events/DocumentEvent.ts`. The saves themselves are
//! [`crate::features::save`]; `services/src/events/EventHub.ts` stays
//! TypeScript and DELIVERS what this produces.
//!
//! # Every successful mutation funnels through one place
//!
//! Two things have to happen after a write, and both are easy to forget at
//! a call site. The session's mutation sequence bumps, which is what
//! invalidates the version-keyed caches; and a LAYER session serializes its
//! artifact, because a layer that is not persisted after a write is a write
//! nobody kept.
//!
//! Forgetting either is silent. A stale form model answers questions about
//! a field that changed, and an unpersisted layer looks fine until the
//! session closes. So the finalizer exists, every mutation goes through it,
//! and what it does is decided by the mutation's KIND rather than by the
//! caller remembering.
//!
//! # Three families bump their own sequence
//!
//! The form mutators, the page flattener and the redaction applier all
//! re-read through a model or a listing before they return, and a read that
//! happened before the bump would be the stale one. So they bump first and
//! this skips them, rather than bumping twice per write and rebuilding
//! every version-keyed cache for nothing.

use serde::{Deserialize, Serialize};

use super::document::DocumentSession;
use crate::error::EngineError;
use crate::features::save::{LayerArtifactKind, save_layer_to_buffer};

/// What kind of mutation just finished.
///
/// Named rather than a boolean, because the thing a caller gets wrong is
/// which family a mutation belongs to, and a boolean called
/// `already_bumped` would put that judgement at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationFamily {
    /// The annotation, page, metadata, piece-info and attachment writers.
    /// They do not re-read through a version-keyed cache, so the sequence
    /// bumps here.
    Ordinary,
    /// The form writers, the page flattener and the redaction applier.
    /// Each re-reads after its own bump, so bumping again would rebuild
    /// every version-keyed cache for nothing.
    SelfSequenced,
}

/// What a finished mutation produced beyond its own result.
#[derive(Debug, Clone, PartialEq)]
pub struct MutationFinalization {
    /// The session's sequence after the mutation. A consumer keying a
    /// cache on it re-reads when it moves.
    pub mutation_sequence: u64,
    /// The layer artifact, for a layer session. `None` for a base session,
    /// which has nothing to persist separately: its writes are in the
    /// document and go out with the next save.
    pub artifact: Option<Vec<u8>>,
}

/// Finish a mutation.
pub fn finish_mutation(
    session: &DocumentSession,
    family: MutationFamily,
    cancel: &crate::cancel::CancelToken,
) -> Result<MutationFinalization, EngineError> {
    if family == MutationFamily::Ordinary {
        session.note_mutation();
    }

    let artifact = match session.kind() {
        Some(super::document::OpenKind::Layer) => {
            Some(save_layer_to_buffer(session, LayerArtifactKind::Artifact, cancel)?)
        }
        _ => None,
    };

    Ok(MutationFinalization { mutation_sequence: session.mutation_sequence(), artifact })
}

/// Who caused a mutation.
///
/// Provenance, not transport: transport is invisible by design. `Local`
/// means THIS ENGINE INSTANCE rather than this user, which is the
/// distinction a consumer needs: the same person in two tabs is two
/// sessions, and tab A's edit reaches tab B as remote with the same
/// subject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventOrigin {
    pub kind: OriginKind,
    /// The engine instance that caused it.
    pub session_id: String,
    /// The authenticated subject, for a cloud engine. `None` locally.
    pub sub: Option<String>,
    /// When, in epoch milliseconds.
    pub ts: i64,
    /// The server audit-log row id, which is the resume cursor and is
    /// monotonic per document. `None` until the mutation has a server
    /// identity, which for a local engine is never.
    pub server_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OriginKind {
    Local,
    Remote,
}

/// A mutation event before its provenance is stamped.
///
/// What a mutation path hands to its publisher. The publisher adds the
/// origin, because only it knows which engine instance it is.
///
/// The RESULT rides verbatim: an event carries exactly what the operation
/// returned, so a consumer that acted on the return value and a consumer
/// that acted on the event acted on the same thing. That is why each
/// variant carries its own result type rather than a summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DocumentEventInit {
    #[serde(rename = "annotation.created")]
    AnnotationCreated(serde_json::Value),
    #[serde(rename = "annotation.updated")]
    AnnotationUpdated(serde_json::Value),
    #[serde(rename = "annotation.deleted")]
    AnnotationDeleted(serde_json::Value),
    #[serde(rename = "annotation.moved")]
    AnnotationMoved(serde_json::Value),
    #[serde(rename = "pages.moved")]
    PagesMoved(serde_json::Value),
    #[serde(rename = "pages.rotated")]
    PagesRotated(serde_json::Value),
    #[serde(rename = "pages.deleted")]
    PagesDeleted(serde_json::Value),
    #[serde(rename = "pages.inserted")]
    PagesInserted(serde_json::Value),
    #[serde(rename = "pages.flattened")]
    PagesFlattened(serde_json::Value),
    #[serde(rename = "redaction.applied")]
    RedactionApplied(serde_json::Value),
    #[serde(rename = "metadata.updated")]
    MetadataUpdated(serde_json::Value),
    #[serde(rename = "attachment.created")]
    AttachmentCreated(serde_json::Value),
    #[serde(rename = "attachment.deleted")]
    AttachmentDeleted(serde_json::Value),
    #[serde(rename = "form.valueChanged")]
    FormValueChanged(serde_json::Value),
    #[serde(rename = "form.imported")]
    FormImported(serde_json::Value),
    #[serde(rename = "form.repaired")]
    FormRepaired(serde_json::Value),
    #[serde(rename = "form.fieldCreated")]
    FormFieldCreated(serde_json::Value),
    #[serde(rename = "form.fieldUpdated")]
    FormFieldUpdated(serde_json::Value),
    #[serde(rename = "form.fieldDeleted")]
    FormFieldDeleted(serde_json::Value),
    #[serde(rename = "form.widgetAttached")]
    FormWidgetAttached(serde_json::Value),
    #[serde(rename = "form.widgetDetached")]
    FormWidgetDetached(serde_json::Value),
}

impl DocumentEventInit {
    /// The operation's result, unchanged.
    ///
    /// Every variant carries one and they all carry it the same way, which
    /// is the invariant the event stream rests on: a consumer that acted on
    /// the operation's return value and one that acted on its event acted on
    /// the same thing.
    pub fn result(&self) -> &serde_json::Value {
        match self {
            Self::AnnotationCreated(v)
            | Self::AnnotationUpdated(v)
            | Self::AnnotationDeleted(v)
            | Self::AnnotationMoved(v)
            | Self::PagesMoved(v)
            | Self::PagesRotated(v)
            | Self::PagesDeleted(v)
            | Self::PagesInserted(v)
            | Self::PagesFlattened(v)
            | Self::RedactionApplied(v)
            | Self::MetadataUpdated(v)
            | Self::AttachmentCreated(v)
            | Self::AttachmentDeleted(v)
            | Self::FormValueChanged(v)
            | Self::FormImported(v)
            | Self::FormRepaired(v)
            | Self::FormFieldCreated(v)
            | Self::FormFieldUpdated(v)
            | Self::FormFieldDeleted(v)
            | Self::FormWidgetAttached(v)
            | Self::FormWidgetDetached(v) => v,
        }
    }

    /// The wire `type`, which is also the tag a consumer switches on.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::AnnotationCreated(_) => "annotation.created",
            Self::AnnotationUpdated(_) => "annotation.updated",
            Self::AnnotationDeleted(_) => "annotation.deleted",
            Self::AnnotationMoved(_) => "annotation.moved",
            Self::PagesMoved(_) => "pages.moved",
            Self::PagesRotated(_) => "pages.rotated",
            Self::PagesDeleted(_) => "pages.deleted",
            Self::PagesInserted(_) => "pages.inserted",
            Self::PagesFlattened(_) => "pages.flattened",
            Self::RedactionApplied(_) => "redaction.applied",
            Self::MetadataUpdated(_) => "metadata.updated",
            Self::AttachmentCreated(_) => "attachment.created",
            Self::AttachmentDeleted(_) => "attachment.deleted",
            Self::FormValueChanged(_) => "form.valueChanged",
            Self::FormImported(_) => "form.imported",
            Self::FormRepaired(_) => "form.repaired",
            Self::FormFieldCreated(_) => "form.fieldCreated",
            Self::FormFieldUpdated(_) => "form.fieldUpdated",
            Self::FormFieldDeleted(_) => "form.fieldDeleted",
            Self::FormWidgetAttached(_) => "form.widgetAttached",
            Self::FormWidgetDetached(_) => "form.widgetDetached",
        }
    }

    /// Which family the mutation behind this event belongs to.
    ///
    /// Derived from the event rather than passed alongside it, so a caller
    /// cannot finalize a form write as an ordinary one.
    pub fn family(&self) -> MutationFamily {
        match self {
            Self::PagesFlattened(_)
            | Self::RedactionApplied(_)
            | Self::FormValueChanged(_)
            | Self::FormImported(_)
            | Self::FormRepaired(_)
            | Self::FormFieldCreated(_)
            | Self::FormFieldUpdated(_)
            | Self::FormFieldDeleted(_)
            | Self::FormWidgetAttached(_)
            | Self::FormWidgetDetached(_) => MutationFamily::SelfSequenced,
            _ => MutationFamily::Ordinary,
        }
    }
}

/// A stamped event, ready to publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEvent {
    #[serde(flatten)]
    pub init: DocumentEventInit,
    pub origin: EventOrigin,
}

/// Stamp an event with its provenance.
pub fn publish(init: DocumentEventInit, origin: EventOrigin) -> DocumentEvent {
    DocumentEvent { init, origin }
}
