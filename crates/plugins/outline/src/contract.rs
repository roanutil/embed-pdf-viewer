//! `contract.ts`: the public outline vocabulary.
//!
//! Items, refs, drafts, patches and positions are the engine's records as
//! JSON (`Value`), as the plugin carries them: `OutlineItem` is `{ref,
//! parent, title, target, action, open, color, bold, italic}`, an
//! `OutlineItemRef` is `{kind: 'objectNumber', objectNumber}`, and an
//! `OutlinePosition` is `'start'`, `'end'`, `{before: ref}` or `{after: ref}`.

use epdf_kernel::signal::AbortSignal;
use serde::Serialize;
use serde_json::Value;

pub use crate::model::OutlineRow;
pub use epdf_kernel::mirror::ResourceStatus;
pub use epdf_plugin_link::LinkActivation;

/// `OutlineItem`, as the engine reads it.
pub type OutlineItem = Value;
/// `OutlineItemRef`: `{kind: 'objectNumber', objectNumber}`.
pub type OutlineItemRef = Value;
/// `OutlineItemPatch`: the fields to change; `target: null` removes the
/// target and `color: null` the colour.
pub type OutlineItemPatch = Value;
/// `OutlinePosition`: `'start'`, `'end'`, `{before: ref}` or `{after: ref}`.
pub type OutlinePosition = Value;
/// `OutlineItemInput`: a new item's fields (`title`, `target`, `color`,
/// `bold`, `italic`), `parent` (the top level when left out or `null`) and
/// `position` (the end when left out). Everything left out has a default.
pub type OutlineItemInput = Value;
/// `EventOrigin`, as the confirmed event carried it.
pub type EventOrigin = Value;
/// `PdfLinkTargetWritable`: `{kind: 'goto', destination}`, a website or a
/// named verb.
pub type PdfLinkTargetWritable = Value;

/// `OperationOptions`: what the reads and writes take.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `OutlineCreateOptions`.
#[derive(Clone, Default)]
pub struct OutlineCreateOptions {
    pub signal: Option<AbortSignal>,
    /// Select the new item and expand the items it sits under, as making it
    /// in a panel does.
    pub select: bool,
}

/// `OutlineChangedEvent`: a confirmed change of the outline, whoever made
/// it: this session, the document's scripts or another session. Fires after
/// the reads show it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineChangedEvent {
    /// `created`, `updated`, `moved`, `deleted` or `restored`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// The items it touched; for a delete, what went: the item first, then
    /// the items under it.
    pub items: Vec<OutlineItemRef>,
    pub origin: EventOrigin,
}

/// `OutlineResyncedEvent`: the outline was read from the engine, first or
/// again. `outline` is `{items, incomplete}`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineResyncedEvent {
    pub outline: Value,
}
