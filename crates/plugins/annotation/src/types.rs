//! `types.ts`: the slice, its actions, the selection-chrome settings, the
//! render projections the capability hands out, and the token.
//!
//! The TypeScript types the one capability twice (the public
//! `AnnotationCapability` and the host `AnnotationHostCapability`, two lenses
//! over one token). Rust has one type, [`crate::AnnotationCapability`], and
//! marks the host members in their docs.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::{
    AnnotationRef, Geom, Model, PageObjectNumber, PropKey, PropSpec, Rect, SnapSettingsPatch,
};
use epdf_geometry::{Point, Size};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::capability::AnnotationCapability;
use crate::tools::AnnotationToolDef;

// ── selection chrome ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokeStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChromeOutline {
    pub style: StrokeStyle,
    pub width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeHandles {
    pub size: f64,
    pub hit_size: f64,
    pub fill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeKnob {
    pub size: f64,
    pub hit_size: f64,
    pub offset: f64,
    pub stalk: bool,
    pub fill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeGuides {
    pub enabled: bool,
    pub style: StrokeStyle,
    pub width: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_color: Option<String>,
    pub axis_opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator_color: Option<String>,
    pub indicator_opacity: f64,
}

/// `ChromeSettings`: every length is CSS px; every colour falls back to
/// `accent`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChromeSettings {
    pub accent: String,
    pub outline: ChromeOutline,
    pub handles: ChromeHandles,
    pub knob: ChromeKnob,
    pub guides: ChromeGuides,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeOutlinePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StrokeStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeHandlesPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeKnobPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stalk: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeGuidesPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<StrokeStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_opacity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator_opacity: Option<f64>,
}

/// `ChromeSettingsPatch`: deep-partial, one level per piece.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChromeSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<ChromeOutlinePatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handles: Option<ChromeHandlesPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knob: Option<ChromeKnobPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guides: Option<ChromeGuidesPatch>,
}

// ── the slice ─────────────────────────────────────────────────────────────────

/// `AnnotationHydration`: whole-document hydration status.
#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationHydration {
    Loading,
    Complete,
    Error(Error),
    /// The scope lacks `doc.annotate.read`.
    Forbidden,
}

impl Serialize for AnnotationHydration {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self {
            AnnotationHydration::Loading => map.serialize_entry("status", "loading")?,
            AnnotationHydration::Complete => map.serialize_entry("status", "complete")?,
            AnnotationHydration::Forbidden => map.serialize_entry("status", "forbidden")?,
            AnnotationHydration::Error(error) => {
                map.serialize_entry("status", "error")?;
                map.serialize_entry("error", &error.to_canonical())?;
            }
        }
        map.end()
    }
}

/// `ToolGhost`: the armed tool's would-be placement under the cursor.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolGhost {
    pub pon: PageObjectNumber,
    #[serde(rename = "box")]
    pub box_: Rect,
    pub rot: f64,
    #[serde(flatten)]
    pub kind: ToolGhostKind,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum ToolGhostKind {
    /// The armed stamp raster; the framework blits it.
    Image,
    /// Painted through `pageItems`.
    Vector { tool_id: String, geom: Geom },
}

/// `AnnotationState`. The model and chrome sit behind `Rc` so the memoized
/// selectors can key on identity, as the TypeScript's `===` does.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationState {
    pub model: Rc<Model>,
    pub hydration: AnnotationHydration,
    pub chrome: Rc<ChromeSettings>,
    pub tool_ghost: Option<Rc<ToolGhost>>,
    pub stamp_arm_epoch: f64,
}

/// The hydration error makes one variant large; actions are transient.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum AnnotationAction {
    #[serde(rename = "SET_MODEL")]
    SetModel { model: Rc<Model> },
    #[serde(rename = "SET_HYDRATION")]
    SetHydration { hydration: AnnotationHydration },
    #[serde(rename = "SET_CHROME")]
    SetChrome { patch: ChromeSettingsPatch },
    #[serde(rename = "SET_TOOL_GHOST")]
    SetToolGhost { ghost: Option<Rc<ToolGhost>> },
    #[serde(rename = "STAMP_ARM_CHANGED")]
    StampArmChanged,
}

// ── host inputs ───────────────────────────────────────────────────────────────

/// What the TypeScript reads from its global environment. The capability
/// never touches the DOM; these are the two ambient values it does use.
#[derive(Clone, Default)]
pub struct AnnotationHost {
    /// `console.error(message, error?)`: the placement, file-picker, link
    /// sync, grouped-create and flags-write failures. `None` drops them.
    pub console_error: Option<Rc<dyn Fn(&str, Option<&Error>)>>,
    /// `crypto.randomUUID()`: the `/NM` seeded on every repository create.
    /// `None` uses a process-wide counter (`00000000-0000-4000-8000-...`).
    pub random_uuid: Option<Rc<dyn Fn() -> String>>,
}

/// `AnnotationConfig`: the initial snap and chrome settings and the tools.
#[derive(Clone, Default, Deserialize)]
pub struct AnnotationConfig {
    #[serde(default)]
    pub snap: Option<SnapSettingsPatch>,
    #[serde(default)]
    pub chrome: Option<ChromeSettingsPatch>,
    #[serde(default)]
    pub tools: Option<Vec<AnnotationToolDef>>,
    #[serde(skip)]
    pub host: AnnotationHost,
}

// ── projections ───────────────────────────────────────────────────────────────

/// `LinkNavItem`: one clickable link area on a page.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkNavItem {
    pub id: String,
    pub rect: Rect,
    pub target: Value,
    pub attached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activate: Option<Value>,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<AnnotationRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover_events: Option<HoverFlags>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct HoverFlags {
    pub enter: bool,
    pub exit: bool,
}

/// `Behavior`: a sibling plugin (forms, links) marks some annotations as
/// interactive; while engaged they render their own DOM and are not
/// geometry-editable.
pub trait Behavior {
    fn id(&self) -> &str;
    fn matches(&self, subtype: &str, ref_: Option<&AnnotationRef>) -> bool;
    fn engaged(&self) -> bool;
}

/// `SelectionProps`: the shared specs, the first member's values, and the
/// keys that differ across members.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelectionProps {
    pub specs: Vec<PropSpec>,
    pub values: Map<String, Value>,
    pub mixed: Vec<PropKey>,
}

/// `SelectionFlags`: per flag, `true`/`false` when every member agrees,
/// `null` when they differ. Keys in `FLAG_KEYS` order.
pub type SelectionFlags = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextItemCss {
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub color: String,
    pub align: epdf_annotation_core::TextAlign,
    pub padding: f64,
    pub background: Option<String>,
}

/// `TextItem`: a free-text annotation projected for the editable layer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextItem {
    pub id: String,
    #[serde(rename = "ref")]
    pub ref_: Option<AnnotationRef>,
    #[serde(rename = "box")]
    pub box_: Rect,
    pub contents: Value,
    pub editing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rot: Option<f64>,
    pub css: TextItemCss,
}

/// `CommentPermissions`: per-thread action gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentPermissions {
    pub can_reply: bool,
    pub can_edit_text: bool,
    pub can_delete: bool,
    pub can_set_status: bool,
    pub can_delete_thread: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadDeleteFailure {
    pub ref_: Value,
    pub error: Error,
}

/// `ThreadDeleteResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadDeleteResult {
    pub deleted: Vec<Value>,
    pub failed: Vec<ThreadDeleteFailure>,
}

impl ThreadDeleteResult {
    pub fn to_json(&self) -> Value {
        json!({
            "deleted": self.deleted,
            "failed": self
                .failed
                .iter()
                .map(|f| json!({ "ref": f.ref_, "error": f.error.to_canonical() }))
                .collect::<Vec<_>>(),
        })
    }
}

/// `StampToolInput`. `source` and `preview` are `BinarySource`s as
/// canonical JSON.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampToolInput {
    pub source: Value,
    #[serde(default)]
    pub target_width: Option<f64>,
    #[serde(default)]
    pub preview: Option<Value>,
    #[serde(default)]
    pub intrinsic_size: Option<Size>,
}

/// `ArmedStampPreview`: the ghost's paintable bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct ArmedStampPreview {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
}

/// `FilePromptRequest`: what the file-picker port is asked for.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePromptRequest {
    pub tool_id: String,
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    pub pon: PageObjectNumber,
    pub point: Point,
}

/// `FilePickerProvider`: given a click, a promise of the file to place (an
/// `AttachmentFileSource` as canonical JSON), or of a falsy value (`null`)
/// to cancel.
pub type FilePickerProvider = Rc<dyn Fn(FilePromptRequest) -> Promise<Value>>;

/// The annotation capability token.
pub static ANNOTATION: LazyLock<Token<AnnotationCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "annotation",
        "add annotationPlugin() from '@embedpdf/plugin-annotation' to your plugins list",
    )
});
