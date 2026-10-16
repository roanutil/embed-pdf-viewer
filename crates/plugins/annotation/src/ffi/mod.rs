//! The annotation plugin through uniffi: five handles over the one
//! capability, typed records decoded from the plugin's JSON, one event enum.
//!
//! Not exported (see the plan's Decisions): the hub's pointer and chrome
//! members (`editPointer`, `marqueePointer`, `createPointer`, `hoverAt`,
//! `getHitKind`, `claimsTouchAt`, `getCursorAt`, `listChromeNodes`,
//! `getSelectionAnchor`), the web render projections (`listPageItems`,
//! `listTextItems`, `listLinkItems`, `getAppearanceEpoch`, `getBakeScale`,
//! `renderAppearances`, `pdfToPageRect`, `getCssFontFamily`, `getToolGhost`,
//! `getArmedStampPreview`, `getStampArmEpoch`, `getChromeSettings`,
//! `updateChromeSettings`), the raw DTO members (`listRaw`, `getRaw`,
//! `createRaw`, `updateRaw`, `getSelectionFlags`), the rich-text editing
//! members (`setRichText`, `draftRichText`), and the hooks other plugins call
//! (`registerBehavior`, `pruneEngagedSelection`, `getBehaviorFor`,
//! `commitScriptEffects`, `deliverRemoteEvent`, `setPageViewports`,
//! `remeasurePage`, `distanceCreationPage`, `onDraftCaptured`,
//! `registerTool`, `getResolvedTool`, `listResolvedTools`, `getToolSubtype`).

mod drafts;
mod handle;
mod picker;
mod selection;
mod threads;
mod tools;

use epdf_annotation_core as core;
use epdf_ffi::{
    AnnotationCallout, AnnotationFlags, AnnotationFlagsPatch, AnnotationProps,
    AnnotationPropsPatch, AnnotationRef, CreationDraftAnchor, PageRef, PluginErrorInfo, Point,
    Rect, TextQuad,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::AnnotationCapability;

pub use drafts::{
    AnnotationAttachmentFile, AnnotationDraftsHandle, AnnotationPageQuads,
    AnnotationStampPlacement, AnnotationStampSource, AnnotationTextEndAnchor,
};
pub use handle::AnnotationHandle;
pub use picker::{AnnotationFilePicker, AnnotationFilePromptRequest, AnnotationPickedFile};
pub use selection::{
    AnnotationQuarterTurn, AnnotationSelectionHandle, AnnotationSelectionProps,
    AnnotationTextFormat,
};
pub use threads::{
    AnnotationComment, AnnotationCommentPermissions, AnnotationCommentThread,
    AnnotationThreadDeleteFailure, AnnotationThreadDeleteResult, AnnotationThreadStatus,
    AnnotationThreadsHandle,
};
pub use tools::{AnnotationPropSpec, AnnotationTool, AnnotationToolsHandle};

pub(crate) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[annotation] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

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

pub(crate) fn ref_out(r: core::AnnotationRef) -> Result<AnnotationRef, Error> {
    AnnotationRef::try_from(r)
}

pub(crate) fn refs_out(refs: Vec<core::AnnotationRef>) -> Result<Vec<AnnotationRef>, Error> {
    refs.into_iter().map(ref_out).collect()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

/// Subtype-specific geometry in page space.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationGeometry {
    Rect { bounds: Rect, rotation: f64 },
    Line { from: Point, to: Point },
    Polygon { vertices: Vec<Point> },
    Polyline { vertices: Vec<Point> },
    Ink { strokes: Vec<Vec<Point>> },
    Markup { quads: Vec<TextQuad> },
    Caret { bounds: Rect },
    Text { bounds: Rect, rotation: f64, callout: Option<AnnotationCallout> },
}

fn points(v: Vec<core::Point>) -> Vec<Point> {
    v.into_iter().map(Into::into).collect()
}

impl From<crate::AnnotationGeometry> for AnnotationGeometry {
    fn from(g: crate::AnnotationGeometry) -> Self {
        use crate::AnnotationGeometry as G;
        match g {
            G::Rect { bounds, rotation } => {
                AnnotationGeometry::Rect { bounds: bounds.into(), rotation }
            }
            G::Line { from, to } => AnnotationGeometry::Line { from: from.into(), to: to.into() },
            G::Polygon { vertices } => AnnotationGeometry::Polygon { vertices: points(vertices) },
            G::Polyline { vertices } => AnnotationGeometry::Polyline { vertices: points(vertices) },
            G::Ink { strokes } => {
                AnnotationGeometry::Ink { strokes: strokes.into_iter().map(points).collect() }
            }
            G::Markup { quads } => {
                AnnotationGeometry::Markup { quads: quads.into_iter().map(Into::into).collect() }
            }
            G::Caret { bounds } => AnnotationGeometry::Caret { bounds: bounds.into() },
            G::Text { bounds, rotation, callout } => AnnotationGeometry::Text {
                bounds: bounds.into(),
                rotation,
                callout: callout.map(Into::into),
            },
        }
    }
}

/// The page-space record every read returns.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationRecord {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub subtype: String,
    pub bounds: Rect,
    pub geometry: AnnotationGeometry,
    pub props: AnnotationProps,
    pub flags: AnnotationFlags,
    pub contents: Option<String>,
    pub author: Option<String>,
    /// ISO 8601, as the engine reports it.
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub group: Option<AnnotationRef>,
    pub in_reply_to: Option<AnnotationRef>,
    pub can_update: bool,
    pub can_delete: bool,
}

impl TryFrom<crate::Annotation> for AnnotationRecord {
    type Error = Error;

    fn try_from(a: crate::Annotation) -> Result<Self, Error> {
        // `props` is `Partial<AnnotationProps>`: only the keys the subtype
        // has (`props_of` in `read/annotations.rs`). The core's patch type
        // parses that shape and keeps unknown keys in `extra`, which the
        // record drops.
        let props: core::AnnotationPropsPatch = decode(Value::Object(a.props))?;
        let in_reply_to = match a.in_reply_to {
            Value::Null => None,
            other => Some(ref_out(decode(other)?)?),
        };
        Ok(AnnotationRecord {
            annotation_ref: ref_out(a.ref_)?,
            page: a.page.into(),
            subtype: a.subtype,
            bounds: a.bounds.into(),
            geometry: a.geometry.into(),
            props: props.into(),
            flags: a.flags.into(),
            contents: text(&a.contents),
            author: text(&a.author),
            created_at: text(&a.created_at),
            modified_at: text(&a.modified_at),
            group: a.group.map(ref_out).transpose()?,
            in_reply_to,
            can_update: a.authority.update,
            can_delete: a.authority.delete,
        })
    }
}

pub(crate) fn annotations_out(
    list: Vec<crate::Annotation>,
) -> Result<Vec<AnnotationRecord>, Error> {
    list.into_iter().map(AnnotationRecord::try_from).collect()
}

/// A geometry write. `rotation: None` keeps the current rotation.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationGeometryPatch {
    Rect {
        bounds: Rect,
        rotation: Option<f64>,
    },
    Line {
        from: Point,
        to: Point,
    },
    Polygon {
        vertices: Vec<Point>,
    },
    Polyline {
        vertices: Vec<Point>,
    },
    Ink {
        strokes: Vec<Vec<Point>>,
    },
    Markup {
        quads: Vec<TextQuad>,
    },
    /// `remove_callout` drops the leader; `callout` sets one; neither leaves it.
    Text {
        bounds: Rect,
        rotation: Option<f64>,
        callout: Option<AnnotationCallout>,
        remove_callout: bool,
    },
}

fn core_points(v: Vec<Point>) -> Vec<core::Point> {
    v.into_iter().map(Into::into).collect()
}

impl TryFrom<AnnotationGeometryPatch> for crate::AnnotationGeometryPatch {
    type Error = Error;

    fn try_from(g: AnnotationGeometryPatch) -> Result<Self, Error> {
        use crate::AnnotationGeometryPatch as G;
        use core::js::Nullable;
        Ok(match g {
            AnnotationGeometryPatch::Rect { bounds, rotation } => {
                G::Rect { bounds: bounds.into(), rotation }
            }
            AnnotationGeometryPatch::Line { from, to } => {
                G::Line { from: from.into(), to: to.into() }
            }
            AnnotationGeometryPatch::Polygon { vertices } => {
                G::Polygon { vertices: core_points(vertices) }
            }
            AnnotationGeometryPatch::Polyline { vertices } => {
                G::Polyline { vertices: core_points(vertices) }
            }
            AnnotationGeometryPatch::Ink { strokes } => {
                G::Ink { strokes: strokes.into_iter().map(core_points).collect() }
            }
            AnnotationGeometryPatch::Markup { quads } => {
                G::Markup { quads: quads.into_iter().map(Into::into).collect() }
            }
            AnnotationGeometryPatch::Text { bounds, rotation, callout, remove_callout } => {
                let callout = match (callout, remove_callout) {
                    (Some(_), true) => {
                        return Err(invalid(
                            "a text geometry patch both sets and removes its callout",
                        ));
                    }
                    (Some(c), false) => Nullable::Value(c.into()),
                    (None, true) => Nullable::Null,
                    (None, false) => Nullable::Undefined,
                };
                G::Text { bounds: bounds.into(), rotation, callout }
            }
        })
    }
}

/// A page-space patch. `None` parts are untouched.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationPatch {
    #[uniffi(default = None)]
    pub bounds: Option<Rect>,
    #[uniffi(default = None)]
    pub geometry: Option<AnnotationGeometryPatch>,
    #[uniffi(default = None)]
    pub props: Option<AnnotationPropsPatch>,
    #[uniffi(default = None)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[uniffi(default = None)]
    pub contents: Option<String>,
}

impl TryFrom<AnnotationPatch> for crate::AnnotationPatch {
    type Error = Error;

    fn try_from(p: AnnotationPatch) -> Result<Self, Error> {
        Ok(crate::AnnotationPatch {
            bounds: p.bounds.map(Into::into),
            geometry: p.geometry.map(TryInto::try_into).transpose()?,
            props: p.props.map(TryInto::try_into).transpose()?,
            flags: p.flags.map(Into::into),
            contents: p.contents,
            rich_text: None,
        })
    }
}

/// What `create` takes. `subtype` is the PDF subtype in kebab case
/// (`square`, `ink`, `free-text`); the geometry fields that subtype reads
/// must be set, as `geometry_from_input` checks.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreateAnnotationInput {
    pub page: PageRef,
    pub subtype: String,
    #[uniffi(default = None)]
    pub tool: Option<String>,
    #[uniffi(default = None)]
    pub props: Option<AnnotationPropsPatch>,
    #[uniffi(default = None)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[uniffi(default = None)]
    pub select: Option<bool>,
    #[uniffi(default = None)]
    pub bounds: Option<Rect>,
    #[uniffi(default = None)]
    pub rotation: Option<f64>,
    #[uniffi(default = None)]
    pub from: Option<Point>,
    #[uniffi(default = None)]
    pub to: Option<Point>,
    #[uniffi(default = None)]
    pub vertices: Option<Vec<Point>>,
    #[uniffi(default = None)]
    pub strokes: Option<Vec<Vec<Point>>>,
    #[uniffi(default = None)]
    pub quads: Option<Vec<TextQuad>>,
    #[uniffi(default = None)]
    pub callout: Option<AnnotationCallout>,
}

/// Page object number 0 and no geometry: a base for `..Default::default()`.
impl Default for CreateAnnotationInput {
    fn default() -> Self {
        CreateAnnotationInput {
            page: PageRef { page_object_number: 0 },
            subtype: String::new(),
            tool: None,
            props: None,
            flags: None,
            select: None,
            bounds: None,
            rotation: None,
            from: None,
            to: None,
            vertices: None,
            strokes: None,
            quads: None,
            callout: None,
        }
    }
}

impl TryFrom<CreateAnnotationInput> for crate::CreateAnnotationInput {
    type Error = Error;

    fn try_from(i: CreateAnnotationInput) -> Result<Self, Error> {
        Ok(crate::CreateAnnotationInput {
            page: i.page.into(),
            subtype: i.subtype,
            tool: i.tool,
            props: i.props.map(TryInto::try_into).transpose()?,
            flags: i.flags.map(Into::into),
            select: i.select,
            bounds: i.bounds.map(|r| serde_json::to_value(core::Rect::from(r)).expect("a rect")),
            rotation: i.rotation,
            from: i.from.map(Into::into),
            to: i.to.map(Into::into),
            vertices: i.vertices.map(core_points),
            strokes: i.strokes.map(|s| s.into_iter().map(core_points).collect()),
            quads: i.quads.map(|q| q.into_iter().map(Into::into).collect()),
            callout: i.callout.map(Into::into),
        })
    }
}

/// Every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationFilter {
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    #[uniffi(default = None)]
    pub subtype: Option<String>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = None)]
    pub group: Option<AnnotationRef>,
}

impl From<AnnotationFilter> for crate::AnnotationFilter {
    fn from(f: AnnotationFilter) -> Self {
        crate::AnnotationFilter {
            page: f.page.map(Into::into),
            subtype: f.subtype,
            author: f.author,
            group: f.group.map(Into::into),
        }
    }
}

/// Whole-document hydration.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AnnotationHydration {
    Loading,
    Complete,
    Failed {
        error: PluginErrorInfo,
    },
    /// The scope lacks `doc.annotate.read`.
    Forbidden,
}

impl From<crate::AnnotationHydration> for AnnotationHydration {
    fn from(h: crate::AnnotationHydration) -> Self {
        match h {
            crate::AnnotationHydration::Loading => AnnotationHydration::Loading,
            crate::AnnotationHydration::Complete => AnnotationHydration::Complete,
            crate::AnnotationHydration::Forbidden => AnnotationHydration::Forbidden,
            crate::AnnotationHydration::Error(e) => {
                AnnotationHydration::Failed { error: plugin_error_info(&e) }
            }
        }
    }
}

pub(crate) fn plugin_error_info(error: &Error) -> PluginErrorInfo {
    let info = error.plugin_error_info();
    PluginErrorInfo {
        code: info.get("code").and_then(Value::as_str).unwrap_or("unknown").into(),
        message: info.get("message").and_then(Value::as_str).unwrap_or(&error.message).into(),
        capability: info.get("capability").and_then(Value::as_str).unwrap_or("annotation").into(),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchFailure {
    pub annotation_ref: AnnotationRef,
    pub error: PluginErrorInfo,
}

/// What a batch did, best effort per entry.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationBatchFailure>,
}

impl TryFrom<crate::BatchResult<core::AnnotationRef>> for AnnotationBatchResult {
    type Error = Error;

    fn try_from(b: crate::BatchResult<core::AnnotationRef>) -> Result<Self, Error> {
        Ok(AnnotationBatchResult {
            applied: refs_out(b.applied)?,
            failed: b
                .failed
                .into_iter()
                .map(|(r, e)| {
                    Ok(AnnotationBatchFailure {
                        annotation_ref: ref_out(r)?,
                        error: plugin_error_info(&e),
                    })
                })
                .collect::<Result<_, Error>>()?,
        })
    }
}

/// `createMany`'s batch: each entry is its input's index, since a failed
/// create has no ref.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCreateFailure {
    pub index: u32,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCreateBatchResult {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationCreateFailure>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationSnapSettings {
    pub guides: bool,
    pub guide_threshold: f64,
    pub rotation: bool,
    pub rotation_angles: Vec<f64>,
    pub rotation_threshold: f64,
}

impl From<core::SnapSettings> for AnnotationSnapSettings {
    fn from(s: core::SnapSettings) -> Self {
        AnnotationSnapSettings {
            guides: s.guides,
            guide_threshold: s.guide_threshold,
            rotation: s.rotation,
            rotation_angles: s.rotation_angles,
            rotation_threshold: s.rotation_threshold,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationSnapSettingsPatch {
    #[uniffi(default = None)]
    pub guides: Option<bool>,
    #[uniffi(default = None)]
    pub guide_threshold: Option<f64>,
    #[uniffi(default = None)]
    pub rotation: Option<bool>,
    #[uniffi(default = None)]
    pub rotation_angles: Option<Vec<f64>>,
    #[uniffi(default = None)]
    pub rotation_threshold: Option<f64>,
}

impl From<AnnotationSnapSettingsPatch> for core::SnapSettingsPatch {
    fn from(p: AnnotationSnapSettingsPatch) -> Self {
        core::SnapSettingsPatch {
            guides: p.guides,
            guide_threshold: p.guide_threshold,
            rotation: p.rotation,
            rotation_angles: p.rotation_angles,
            rotation_threshold: p.rotation_threshold,
        }
    }
}

/// `annotationPlugin(config)`, as Swift sets it. Tool overrides and the
/// chrome look stay Rust-side (see the plan's Decisions).
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct AnnotationConfig {
    #[uniffi(default = None)]
    pub snap: Option<AnnotationSnapSettingsPatch>,
}

impl From<AnnotationConfig> for crate::AnnotationConfig {
    fn from(c: AnnotationConfig) -> Self {
        crate::AnnotationConfig { snap: c.snap.map(Into::into), ..Default::default() }
    }
}

/// A confirmed creation or update, whoever caused it. `annotation` is
/// `None` when the plugin could not project the DTO (an unsupported
/// subtype). The plugin's `origin` is dropped: no foreign caller acts on it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationChangedEvent {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    pub annotation: Option<AnnotationRecord>,
}

impl TryFrom<&crate::AnnotationChangedEvent> for AnnotationChangedEvent {
    type Error = Error;

    fn try_from(e: &crate::AnnotationChangedEvent) -> Result<Self, Error> {
        Ok(AnnotationChangedEvent {
            annotation_ref: ref_out(e.ref_.clone())?,
            page: e.page.into(),
            annotation: e.annotation.clone().map(AnnotationRecord::try_from).transpose()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationDeletedEvent {
    pub annotation_ref: AnnotationRef,
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
    Resynced { pages: AnnotationResyncedPages },
    Created { event: AnnotationChangedEvent },
    Updated { event: AnnotationChangedEvent },
    Deleted { event: AnnotationDeletedEvent },
    SelectionChanged { refs: Vec<AnnotationRef>, previous_refs: Vec<AnnotationRef> },
    DraftChanged { draft: Option<CreationDraftAnchor> },
    EditingChanged { annotation_ref: Option<AnnotationRef> },
    ThreadChanged { root_ref: AnnotationRef, change: AnnotationThreadChange },
}

#[uniffi::export(with_foreign)]
pub trait AnnotationEventListener: Send + Sync {
    fn on_event(&self, event: AnnotationEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}
