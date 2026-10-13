//! `contract.ts` and `token.ts`: the public protocol's types (the page-space
//! record, patches, filters, events, the selection-chrome settings, the
//! render projections) and the token.
//!
//! The TypeScript types the one capability twice (the public
//! `AnnotationCapability` and the host `AnnotationHostCapability`, two lenses
//! over one token). Rust has one type, [`crate::AnnotationCapability`], and
//! marks the host members in their docs.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_annotation_core::js::Nullable;
use epdf_annotation_core::richtext::RichTextParagraph;
use epdf_annotation_core::{
    AnnotationFlags, AnnotationFlagsPatch, AnnotationPropsPatch, AnnotationRef, Callout,
    CreationDraftAnchor, Geom, PageRef, PropKey, PropSpec, Rect, SelectionAnchor,
    SnapSettingsPatch, TextQuad,
};
use epdf_geometry::{Point, Size};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::controller::AnnotationCapability;
use crate::rich_text::FontHandle;
use crate::tools::definitions::AnnotationToolDef;

pub use crate::rich_text::{Face, TextFormat, TextSelection};
pub use crate::tools::definitions::ARMED_STAMP_TOOL_ID;

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
    pub axis_opacity: f64,
    pub indicator_opacity: f64,
    // `DEFAULT_CHROME.guides` has no colours: a patch's spread appends them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicator_color: Option<String>,
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

// ── hydration and the tool ghost ──────────────────────────────────────────────

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
    pub page: PageRef,
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
    /// Painted through `listPageItems`.
    Vector { tool_id: String, geom: Geom },
}

// ── configuration and host inputs ─────────────────────────────────────────────

/// What the TypeScript reads from its global environment. The controller
/// never touches the DOM; these are the ambient values it does use.
#[derive(Clone, Default)]
pub struct AnnotationHost {
    /// `console.error(message, error?)`: every failure the plugin swallows.
    /// `None` drops them.
    pub console_error: Option<Rc<dyn Fn(&str, Option<&Error>)>>,
    /// `crypto.randomUUID()`: the `/NM` seeded on every repository create.
    /// `None` counts up from `00000000-0000-4000-8000-000000000001`.
    pub random_uuid: Option<Rc<dyn Fn() -> String>>,
    /// `ctx.engine.fonts.list()`: the registered fonts. The Rust engine
    /// contract has no fonts service, so the host supplies the list; `None`
    /// is no registered fonts, as on the cloud engine.
    pub fonts: Option<Rc<dyn Fn() -> Vec<FontHandle>>>,
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

// ── render projections ────────────────────────────────────────────────────────

/// `LinkNavItem`: one clickable link area on a page. It deserializes too:
/// the link plugin's engine-backed page cache keeps items in its slice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkNavItem {
    pub id: String,
    pub bounds: Rect,
    pub target: Value,
    pub attached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activate: Option<Value>,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<AnnotationRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_events: Option<HoverFlags>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// `90 | -90`: the quarter turn `rotateSelectionBy` takes. The TypeScript
/// types the delta as one of the two literals, so no other angle can be
/// asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuarterTurn {
    /// `90`: the core's `rotate90` step over the selection.
    Plus90,
    /// `-90`: each member's rotation less 90, through `setRotation`.
    Minus90,
}

impl QuarterTurn {
    /// The TypeScript's runtime test, `delta === 90`: anything else takes
    /// the `-90` branch, which is what an untyped caller passing `45` gets.
    pub fn from_delta(delta: f64) -> Self {
        if delta == 90.0 { QuarterTurn::Plus90 } else { QuarterTurn::Minus90 }
    }

    /// The literal this turn stands for.
    pub fn delta(self) -> f64 {
        match self {
            QuarterTurn::Plus90 => 90.0,
            QuarterTurn::Minus90 => -90.0,
        }
    }
}

/// `SelectionFlags`: per flag, `true`/`false` when every member agrees,
/// `null` when they differ. Keys in `FLAG_KEYS` order.
pub type SelectionFlags = Map<String, Value>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextItemCss {
    pub font_family: String,
    pub font_size: f64,
    pub color: String,
    pub font_weight: f64,
    pub font_style: &'static str,
    pub text_decoration: &'static str,
    pub align: epdf_annotation_core::TextAlign,
    pub padding: f64,
}

/// `TextItem`: a free-text annotation projected for the editable layer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextItem {
    pub id: String,
    #[serde(rename = "ref")]
    pub ref_: Option<AnnotationRef>,
    #[serde(rename = "box")]
    pub box_: Rect,
    pub contents: Value,
    pub rich_text: RichTextParagraphs,
    pub editing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rot: Option<f64>,
    pub css: TextItemCss,
}

/// `{ paragraphs: RichTextParagraph[] }`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextParagraphs {
    pub paragraphs: Vec<RichTextParagraph>,
}

// ── comments ──────────────────────────────────────────────────────────────────

/// `CommentPermissions`: per-thread action gates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentPermissions {
    pub can_reply: bool,
    pub can_set_status: bool,
    pub can_edit_text: bool,
    pub can_delete: bool,
    pub can_delete_thread: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadDeleteFailure {
    pub ref_: AnnotationRef,
    pub error: Error,
}

/// `ThreadDeleteResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadDeleteResult {
    pub deleted: Vec<AnnotationRef>,
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

/// `CommentThreadChangedEvent['change']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadChange {
    Reply,
    Text,
    Status,
    Marked,
    Deleted,
}

/// `CommentThreadChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentThreadChangedEvent {
    pub root_ref: AnnotationRef,
    pub change: ThreadChange,
}

// ── the page-space record ─────────────────────────────────────────────────────

/// `AnnotationGeometry`: subtype-specific geometry in page space.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AnnotationGeometry {
    Rect { bounds: Rect, rotation: f64 },
    Line { from: Point, to: Point },
    Polygon { vertices: Vec<Point> },
    Polyline { vertices: Vec<Point> },
    Ink { strokes: Vec<Vec<Point>> },
    Markup { quads: Vec<TextQuad> },
    Caret { bounds: Rect },
    Text { bounds: Rect, rotation: f64, callout: Option<Callout> },
}

/// `{ update, delete }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RecordAuthority {
    pub update: bool,
    pub delete: bool,
}

/// `Annotation`: the page-space record every read returns, projected once
/// per model entry.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    #[serde(rename = "ref")]
    pub ref_: AnnotationRef,
    pub page: PageRef,
    pub subtype: String,
    pub bounds: Rect,
    pub geometry: AnnotationGeometry,
    pub props: Map<String, Value>,
    pub flags: AnnotationFlags,
    pub contents: Value,
    pub author: Value,
    pub created_at: Value,
    pub modified_at: Value,
    pub group: Option<AnnotationRef>,
    pub in_reply_to: Value,
    pub authority: RecordAuthority,
    pub raw: Option<Value>,
}

/// `AnnotationGeometryPatch`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AnnotationGeometryPatch {
    Rect {
        bounds: Rect,
        #[serde(default)]
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
    Text {
        bounds: Rect,
        #[serde(default)]
        rotation: Option<f64>,
        /// Absent leaves the callout, `null` removes it.
        #[serde(default)]
        callout: Nullable<Callout>,
    },
}

impl AnnotationGeometryPatch {
    pub fn kind(&self) -> &'static str {
        match self {
            AnnotationGeometryPatch::Rect { .. } => "rect",
            AnnotationGeometryPatch::Line { .. } => "line",
            AnnotationGeometryPatch::Polygon { .. } => "polygon",
            AnnotationGeometryPatch::Polyline { .. } => "polyline",
            AnnotationGeometryPatch::Ink { .. } => "ink",
            AnnotationGeometryPatch::Markup { .. } => "markup",
            AnnotationGeometryPatch::Text { .. } => "text",
        }
    }
}

/// `AnnotationPatch`: a page-space patch. Omitted parts are untouched.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationPatch {
    #[serde(default)]
    pub bounds: Option<Rect>,
    #[serde(default)]
    pub geometry: Option<AnnotationGeometryPatch>,
    #[serde(default)]
    pub props: Option<AnnotationPropsPatch>,
    #[serde(default)]
    pub flags: Option<AnnotationFlagsPatch>,
    #[serde(default)]
    pub contents: Option<String>,
    #[serde(default)]
    pub rich_text: Option<RichTextParagraphs>,
}

/// `AnnotationFilter`: every field narrows.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AnnotationFilter {
    #[serde(default)]
    pub page: Option<PageRef>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub group: Option<AnnotationRef>,
}

/// `AnnotationTool`: the public shape of an authoring tool.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationTool {
    pub id: String,
    pub subtype: String,
    pub preset: String,
    pub cursor: String,
    pub enables: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<AnnotationPropsPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<AnnotationFlagsPatch>,
    pub upright: bool,
}

/// `AnnotationSelectionAnchor`.
pub type AnnotationSelectionAnchor = SelectionAnchor;

// ── events ────────────────────────────────────────────────────────────────────

/// `ChangeOrigin`, as JSON: `{locality, trigger, sessionId, actorId}`.
pub type ChangeOrigin = Value;

/// `AnnotationChangedEvent`: a confirmed creation or update.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationChangedEvent {
    #[serde(rename = "ref")]
    pub ref_: AnnotationRef,
    pub page: PageRef,
    pub subtype: Value,
    pub annotation: Option<Annotation>,
    pub raw: Value,
    pub origin: ChangeOrigin,
}

pub type AnnotationCreatedEvent = AnnotationChangedEvent;
pub type AnnotationUpdatedEvent = AnnotationChangedEvent;

/// `AnnotationDeletedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationDeletedEvent {
    #[serde(rename = "ref")]
    pub ref_: AnnotationRef,
    pub page: PageRef,
    pub origin: ChangeOrigin,
}

/// `AnnotationResyncedEvent`: `pages` is `'all'` or the reloaded pages.
#[derive(Clone, Debug, PartialEq)]
pub enum ResyncedPages {
    All,
    Pages(Vec<PageRef>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationResyncedEvent {
    pub pages: ResyncedPages,
}

impl Serialize for AnnotationResyncedEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        match &self.pages {
            ResyncedPages::All => map.serialize_entry("pages", "all")?,
            ResyncedPages::Pages(pages) => map.serialize_entry("pages", pages)?,
        }
        map.end()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationSelectionChangedEvent {
    pub refs: Vec<AnnotationRef>,
    pub previous_refs: Vec<AnnotationRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationDraftChangedEvent {
    pub draft: Option<CreationDraftAnchor>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AnnotationEditingChangedEvent {
    #[serde(rename = "ref")]
    pub ref_: Option<AnnotationRef>,
}

// ── batches ───────────────────────────────────────────────────────────────────

/// `BatchResult<AnnotationRef, R>`: what applied, and what failed with its
/// `PluginErrorInfo`. `skipped` is always empty here.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchResult<R> {
    pub applied: Vec<AnnotationRef>,
    pub failed: Vec<(R, Error)>,
}

impl<R: Serialize> BatchResult<R> {
    pub fn to_json(&self) -> Value {
        json!({
            "applied": self.applied,
            "skipped": [],
            "failed": self
                .failed
                .iter()
                .map(|(r, e)| json!({ "ref": r, "error": e.plugin_error_info() }))
                .collect::<Vec<_>>(),
        })
    }
}

// ── stamps and the file-picker port ───────────────────────────────────────────

/// `StampPreviewProvider`: a render at a device pixel width, or `null`.
pub type StampPreviewProvider = Rc<dyn Fn(f64) -> Promise<Option<ArmedStampPreview>>>;

/// `StampToolInput['preview']`: fixed bytes, or a resolution-aware provider.
#[derive(Clone)]
pub enum StampPreview {
    /// A `BinarySource` as canonical JSON; a falsy one is no preview.
    Source(Value),
    Provider(StampPreviewProvider),
}

/// `StampToolInput`. `source` is a `BinarySource` as canonical JSON.
#[derive(Clone, Default)]
pub struct StampToolInput {
    pub source: Value,
    pub target_width: Option<f64>,
    pub name: Option<String>,
    pub subject: Option<String>,
    pub preview: Option<StampPreview>,
    pub intrinsic_size: Option<Size>,
}

/// `ArmedStampPreview`: the ghost's paintable bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct ArmedStampPreview {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
}

/// `StampPlacement`: where a programmatic stamp placement lands.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampPlacement {
    pub page: PageRef,
    pub at: Point,
    #[serde(default)]
    pub target_width: Option<f64>,
    #[serde(default)]
    pub rotation: Option<f64>,
}

/// `FilePromptRequest`: what the file-picker port is asked for.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePromptRequest {
    pub tool_id: String,
    pub subtype: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    pub page: PageRef,
    pub point: Point,
}

/// `FilePickerProvider`: given a click, a promise of the file to place (an
/// `AttachmentFileSource` as canonical JSON), or of a falsy value (`null`)
/// to cancel.
pub type FilePickerProvider = Rc<dyn Fn(FilePromptRequest) -> Promise<Value>>;

/// The annotation capability token. The TypeScript's public and host tokens
/// are one runtime object; Rust has one token and one type.
pub static ANNOTATION: LazyLock<Token<AnnotationCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "annotation",
        "add annotationPlugin() from '@embedpdf/plugin-annotation' to your plugins list",
    )
});
