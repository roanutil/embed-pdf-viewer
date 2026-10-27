//! The annotation plugin through uniffi: five handles over the one
//! capability, records decoded from the engine's JSON, one event enum.
//!
//! Migrated by hand onto the re-ported capability (Task 13.17); compiled and
//! tested since Task 13.33. The capability now
//! speaks the engine's records (`Annotation` is the engine DTO), so the typed
//! projection the old plugin made (`AnnotationGeometry`, `AnnotationProps`,
//! `AnnotationFlags`) is gone from this surface: a record carries its ref,
//! page, subtype, rect, text members and authority typed, and the rest of the
//! DTO as JSON (`fields_json`). Patches, create fields, tool defaults and
//! selection values are engine field bags, passed as JSON objects.
//!
//! Not exported: the hub's pointer and chrome members (`editPointer`,
//! `marqueePointer`, `createPointer`, `hoverAt`, `getHitKind`,
//! `claimsTouchAt`, `getCursorAt`, `listChromeNodes`, `selection.getAnchor`),
//! the web render projections (`listPageItems`, `listTextItems`,
//! `listLinkItems`, `getAppearanceEpoch`, `renderAppearances`,
//! `getCssFontFamily`, `getImageGhost`, `renderArmedStampPreview`), the
//! rich-text editing members (`draftRichText`, `setTextSelection` ranges
//! beyond a caret), import and export, and the hooks other plugins call
//! (`registerBehavior`, `pruneEngagedSelection`, `getBehaviorFor`,
//! `commitScriptEffects`, `lend`, `setPageViewports`, `remeasurePage`,
//! `distanceCreationPage`, `tools.register`).

mod drafts;
mod handle;
mod picker;
mod selection;
mod threads;
mod tools;

use epdf_ffi::{AnnotationRef, PageRef, PluginErrorInfo, Point, Rect};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Map, Value, json};

use crate::AnnotationCapability;

pub use drafts::{
    AnnotationAttachmentFile, AnnotationDraftsHandle, AnnotationPageQuads,
    AnnotationStampPlacement, AnnotationStampSource, AnnotationTextEndAnchor,
};
pub use handle::AnnotationHandle;
pub use picker::{AnnotationFilePicker, AnnotationFilePromptRequest, AnnotationPickedFile};
pub use selection::{AnnotationSelectionHandle, AnnotationSelectionProps, AnnotationTextFormat};
pub use threads::{
    AnnotationComment, AnnotationCommentPermissions, AnnotationCommentThread,
    AnnotationThreadDeleteFailure, AnnotationThreadDeleteResult, AnnotationThreadStatus,
    AnnotationThreadsHandle,
};
pub use tools::{AnnotationProperty, AnnotationTool, AnnotationToolsHandle};

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    plugin_error(code::INVALID_INPUT, "annotation", message.into(), None, None)
}

/// The capability, if its document is still open.
pub(crate) fn open(cap: &AnnotationCapability) -> Result<&AnnotationCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "annotation",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// A JSON object handed over the boundary as text (`fields_json`).
pub(crate) fn object_of(text: &str) -> Result<Map<String, Value>, Error> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(invalid("a field bag must be a JSON object")),
        Err(e) => Err(invalid(format!("a field bag is not JSON: {e}"))),
    }
}

// ── refs, pages and geometry as the engine's JSON ───────────────────────────

pub(crate) fn page_json(page: PageRef) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": page.page_object_number })
}

fn whole(value: Option<&Value>, what: &str) -> Result<u32, Error> {
    value
        .and_then(Value::as_f64)
        .filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX))
        .map(|n| n as u32)
        .ok_or_else(|| invalid(format!("{what} is not a whole number")))
}

pub(crate) fn page_out(page: &Value) -> Result<PageRef, Error> {
    Ok(PageRef { page_object_number: whole(page.get("objectNumber"), "a page's object number")? })
}

/// An `AnnotationRef` as the engine writes it. The engine has no `nm` ref
/// any more: a ref is an object number or a base index on its page.
pub(crate) fn ref_json(reference: &AnnotationRef) -> Result<Value, Error> {
    match reference {
        AnnotationRef::ObjectNumber { page, annot_object_number } => Ok(json!({
            "kind": "objectNumber", "page": page_json(*page), "objectNumber": annot_object_number
        })),
        AnnotationRef::Index { page, index } => Ok(json!({
            "kind": "baseIndex", "page": page_json(*page), "baseIndex": index
        })),
    }
}

pub(crate) fn refs_json(refs: &[AnnotationRef]) -> Result<Vec<Value>, Error> {
    refs.iter().map(ref_json).collect()
}

/// An engine ref as the foreign enum.
pub(crate) fn ref_out(reference: &Value) -> Result<AnnotationRef, Error> {
    let page = page_out(reference.get("page").unwrap_or(&Value::Null))?;
    match reference.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => Ok(AnnotationRef::ObjectNumber {
            page,
            annot_object_number: whole(
                reference.get("objectNumber"),
                "an annotation's object number",
            )?,
        }),
        Some("baseIndex") => Ok(AnnotationRef::Index {
            page,
            index: whole(reference.get("baseIndex"), "an annotation's base index")?,
        }),
        _ => Err(invalid("a ref of a kind the engine does not write")),
    }
}

pub(crate) fn refs_out(refs: &Value) -> Result<Vec<AnnotationRef>, Error> {
    refs.as_array().map(|list| list.iter().map(ref_out).collect()).unwrap_or_else(|| Ok(Vec::new()))
}

pub(crate) fn point_json(point: Point) -> Value {
    json!({ "x": point.x, "y": point.y })
}

pub(crate) fn rect_out(rect: &Value) -> Rect {
    let n = |k: &str| rect.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    Rect { x: n("x"), y: n("y"), width: n("width"), height: n("height") }
}

pub(crate) fn geometry_point(point: Point) -> epdf_geometry::Point {
    epdf_geometry::Point { x: point.x, y: point.y }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

// ── records ─────────────────────────────────────────────────────────────────

/// An annotation as the engine reads it: the members every caller needs,
/// typed, and the whole record as JSON.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationRecord {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub subtype: String,
    /// The annotation's `/Rect`, page space.
    pub rect: Rect,
    pub contents: Option<String>,
    pub author: Option<String>,
    /// ISO 8601, as the engine reports it.
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub group_id: Option<String>,
    /// `reply.to`: the annotation this one answers.
    pub in_reply_to: Option<AnnotationRef>,
    pub can_update: bool,
    pub can_delete: bool,
    /// The engine's record, every member.
    pub fields_json: String,
}

pub(crate) fn record_out(
    cap: &AnnotationCapability,
    dto: &Value,
) -> Result<AnnotationRecord, Error> {
    let reference = dto.get("ref").cloned().unwrap_or(Value::Null);
    let in_reply_to = match dto.get("reply").and_then(|r| r.get("to")) {
        None | Some(Value::Null) => None,
        Some(to) => Some(ref_out(to)?),
    };
    Ok(AnnotationRecord {
        annotation_ref: ref_out(&reference)?,
        page: page_out(dto.get("page").unwrap_or(&Value::Null))?,
        subtype: text(dto.get("subtype")).unwrap_or_default(),
        rect: rect_out(dto.get("rect").unwrap_or(&Value::Null)),
        contents: text(dto.get("contents")),
        author: text(dto.get("author")),
        created_at: text(dto.get("createdAt")),
        modified_at: text(dto.get("modifiedAt")),
        group_id: text(dto.get("groupId")),
        in_reply_to,
        can_update: cap.can_update(&reference),
        can_delete: cap.can_delete(&reference),
        fields_json: dto.to_string(),
    })
}

pub(crate) fn records_out(
    cap: &AnnotationCapability,
    list: &[Value],
) -> Result<Vec<AnnotationRecord>, Error> {
    list.iter().map(|dto| record_out(cap, dto)).collect()
}

/// Which annotations `list` returns. Every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationFilter {
    #[uniffi(default = None)]
    pub pages: Option<Vec<PageRef>>,
    #[uniffi(default = None)]
    pub subtype: Option<String>,
}

impl From<AnnotationFilter> for crate::read::annotations::AnnotationFilter {
    fn from(f: AnnotationFilter) -> Self {
        crate::read::annotations::AnnotationFilter {
            pages: f.pages.map(|pages| pages.into_iter().map(page_json).collect()),
            subtype: f.subtype,
        }
    }
}

/// A `PluginErrorInfo` the plugin already wrote as JSON.
pub(crate) fn plugin_error_info_of(info: &Value) -> PluginErrorInfo {
    let s = |k: &str, or: &str| info.get(k).and_then(Value::as_str).unwrap_or(or).to_string();
    PluginErrorInfo {
        code: s("code", "unknown"),
        message: s("message", ""),
        capability: s("capability", "annotation"),
        permission: info.get("permission").and_then(Value::as_str).map(str::to_string),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchFailure {
    pub annotation_ref: AnnotationRef,
    pub error: PluginErrorInfo,
}

/// What a batch did, best effort per entry (`BatchResult`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationBatchFailure>,
}

pub(crate) fn batch_out(result: &Value) -> Result<AnnotationBatchResult, Error> {
    Ok(AnnotationBatchResult {
        applied: refs_out(result.get("applied").unwrap_or(&Value::Null))?,
        failed: result
            .get("failed")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|f| {
                        Ok(AnnotationBatchFailure {
                            annotation_ref: ref_out(f.get("ref").unwrap_or(&Value::Null))?,
                            error: plugin_error_info_of(f.get("error").unwrap_or(&Value::Null)),
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

/// A creation draft (`draft.get()`): a polygon or polyline being drawn.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCreationDraft {
    pub page: PageRef,
    /// The draft as the plugin reads it (`CreationDraft`).
    pub draft_json: String,
}

pub(crate) fn draft_out(draft: &Value) -> Result<Option<AnnotationCreationDraft>, Error> {
    if draft.is_null() {
        return Ok(None);
    }
    Ok(Some(AnnotationCreationDraft {
        page: page_out(draft.get("page").unwrap_or(&Value::Null))?,
        draft_json: draft.to_string(),
    }))
}

// ── events ──────────────────────────────────────────────────────────────────

/// A confirmed creation or update, whoever caused it. The `origin` is
/// dropped: no foreign caller acts on it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationChangedEvent {
    pub annotation: AnnotationRecord,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationDeletedEvent {
    /// Everything that went, the annotation first.
    pub refs: Vec<AnnotationRef>,
    pub page: PageRef,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationResyncedPages {
    All,
    Pages { pages: Vec<PageRef> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationThreadChange {
    Reply,
    Text,
    Status,
    Marked,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationEvent {
    Resynced {
        pages: AnnotationResyncedPages,
    },
    Created {
        event: AnnotationChangedEvent,
    },
    Updated {
        event: AnnotationChangedEvent,
    },
    Deleted {
        event: AnnotationDeletedEvent,
    },
    Reordered {
        page: PageRef,
        order: Vec<AnnotationRef>,
    },
    SelectionChanged {
        refs: Vec<AnnotationRef>,
        previous_refs: Vec<AnnotationRef>,
    },
    DraftChanged {
        draft: Option<AnnotationCreationDraft>,
    },
    EditingChanged {
        annotation_ref: Option<AnnotationRef>,
    },
    HoverChanged {
        annotation_ref: Option<AnnotationRef>,
    },
    ThreadChanged {
        root_ref: AnnotationRef,
        change: AnnotationThreadChange,
    },
    /// The engine refused a change the user made; it is already gone from the view.
    WriteFailed {
        refs: Vec<AnnotationRef>,
        error: PluginErrorInfo,
    },
}

#[uniffi::export(with_foreign)]
pub trait AnnotationEventListener: Send + Sync {
    fn on_event(&self, event: AnnotationEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}
