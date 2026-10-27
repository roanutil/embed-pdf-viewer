//! `contract.ts`: the public stage vocabulary. Settings, the camera and
//! view model, navigation and reveal options, events and the registration
//! options. The capability itself is [`crate::StageCapability`]; the token
//! is [`crate::STAGE`].

use std::fmt;
use std::rc::Rc;

use epdf_geometry::{PageFrame, PageRotation, PageTransform, Point, Rect, Size};
use epdf_kernel::Token;
use epdf_kernel::geometry::PageRef;
use epdf_stage_core::{
    Align, AlignValue, Alignment, AlignmentValue, Anchor, Camera, Direction, PageBox, SizingMode,
    SpreadMode, ZoomMode, ZoomSpec,
};
use serde::{Deserialize, Serialize, Serializer};

use crate::controller::StageCapability;
use crate::model::CameraOrder;
use crate::settings::SettingKey;

/// `PageInfo`: one row of the kernel's page registry, as the engine
/// reported it. The kernel keeps the registry as JSON, so the stage hands
/// rows back as the same JSON.
pub type PageInfo = serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutKind {
    Vertical,
    Horizontal,
    Grid,
}

/// Navigation either tweens the camera or jumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollBehaviorKind {
    Smooth,
    Instant,
}

/// Continuous scroll over the whole document, or one item at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowMode {
    Continuous,
    Paged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnsPolicy {
    /// About the square root of the item count.
    Square,
    /// Wrapped: as many columns as fit the viewport line at the current zoom.
    Auto,
}

/// Grid column policy: `'square'`, `'auto'` or a fixed count.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GridColumns {
    Policy(ColumnsPolicy),
    Count(f64),
}

/// Space between items. A number is world units (scales with zoom);
/// `{px}` is screen px (zoom-stable).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Gap {
    World(f64),
    Px { px: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// The one environmental fact a headless stage has: the box it was told
/// about with `set_viewport_size`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageBox {
    pub width: f64,
    pub height: f64,
    /// Of the container. A square box is portrait.
    pub orientation: Orientation,
}

/// A declarative box query: bounds inclusive, every field optional, AND-ed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<Orientation>,
}

/// A rule's `when`: a query, or a predicate for anything the box can answer.
#[derive(Clone)]
pub enum RuleWhen {
    Query(BoxQuery),
    Predicate(Rc<dyn Fn(&StageBox) -> bool>),
}

impl fmt::Debug for RuleWhen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleWhen::Query(q) => f.debug_tuple("Query").field(q).finish(),
            RuleWhen::Predicate(_) => f.write_str("Predicate(fn)"),
        }
    }
}

/// One `@container` block for the settings bag: when the box matches,
/// assert this patch. Rules apply in source order, later winning per key.
#[derive(Clone, Debug)]
pub struct ResponsiveRule {
    pub name: Option<String>,
    pub when: RuleWhen,
    /// Absent for a pure named query.
    pub settings: Option<StageSettingsPatch>,
}

/// The navigation-only `'keep'`: this axis does not move on arrival.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keep {
    Keep,
}

/// One axis of the arrival policy: an `AlignValue` or `'keep'`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArrivalAlignValue {
    Keep(Keep),
    Align(AlignValue),
}

impl ArrivalAlignValue {
    pub const KEEP: ArrivalAlignValue = ArrivalAlignValue::Keep(Keep::Keep);

    pub fn named(align: Align) -> Self {
        ArrivalAlignValue::Align(AlignValue::Named(align))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArrivalAlignment {
    pub x: ArrivalAlignValue,
    pub y: ArrivalAlignValue,
}

/// `Partial<ArrivalAlignment>`, the per-call override.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartialArrivalAlignment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<ArrivalAlignValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<ArrivalAlignValue>,
}

/// The stage's orthogonal settings, one view's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSettings {
    pub flow: FlowMode,
    pub layout: LayoutKind,
    pub spread: SpreadMode,
    pub sizing: SizingMode,
    pub columns: GridColumns,
    pub bounded: bool,
    pub padding: f64,
    pub gap: Gap,
    pub page_frame: PageFrame,
    pub direction: Direction,
    pub fit_align: Alignment,
    pub arrival_align: ArrivalAlignment,
    pub zoom_align: AlignmentValue,
    pub anchor_align: AlignmentValue,
    pub view_rotation: PageRotation,
    pub zoom: ZoomSpec,
    pub scroll_behavior: ScrollBehaviorKind,
    /// Whether this view's pointer input reaches the interaction hub.
    pub interaction: bool,
    /// With `interaction`: a drag between pages pans whatever the tool.
    pub pan_fallback: bool,
    /// Whether a pinch or a zoom wheel zooms this view.
    pub zoom_gestures: bool,
    pub view_units_per_point: f64,
}

/// `Partial<PageFrame>`: the sides a `pageFrame` change names. The sides
/// it leaves out are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartialPageFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
}

impl From<PageFrame> for PartialPageFrame {
    fn from(frame: PageFrame) -> Self {
        PartialPageFrame {
            top: Some(frame.top),
            right: Some(frame.right),
            bottom: Some(frame.bottom),
            left: Some(frame.left),
        }
    }
}

/// `StageSettingsPatch`: any subset of the settings, each replacing the
/// current value whole. `None` is an absent (or `undefined`) key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<FlowMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<SpreadMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizing: Option<SizingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<GridColumns>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Gap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_frame: Option<PartialPageFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_align: Option<Alignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrival_align: Option<ArrivalAlignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_align: Option<AlignmentValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_align: Option<AlignmentValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_rotation: Option<PageRotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<ZoomSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_behavior: Option<ScrollBehaviorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interaction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan_fallback: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom_gestures: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_units_per_point: Option<f64>,
}

impl StageSettingsPatch {
    /// A patch that sets only the zoom intent.
    pub fn zoom(zoom: ZoomSpec) -> Self {
        StageSettingsPatch { zoom: Some(zoom), ..Default::default() }
    }
}

impl From<StageSettings> for StageSettingsPatch {
    fn from(s: StageSettings) -> Self {
        StageSettingsPatch {
            flow: Some(s.flow),
            layout: Some(s.layout),
            spread: Some(s.spread),
            sizing: Some(s.sizing),
            columns: Some(s.columns),
            bounded: Some(s.bounded),
            padding: Some(s.padding),
            gap: Some(s.gap),
            page_frame: Some(s.page_frame.into()),
            direction: Some(s.direction),
            fit_align: Some(s.fit_align),
            arrival_align: Some(s.arrival_align),
            zoom_align: Some(s.zoom_align),
            anchor_align: Some(s.anchor_align),
            view_rotation: Some(s.view_rotation),
            zoom: Some(s.zoom),
            scroll_behavior: Some(s.scroll_behavior),
            interaction: Some(s.interaction),
            pan_fallback: Some(s.pan_fallback),
            zoom_gestures: Some(s.zoom_gestures),
            view_units_per_point: Some(s.view_units_per_point),
        }
    }
}

/// A laid-out page handed to the shell: the layout box plus identity, the
/// device-snapped screen origin, the page transform and the on-screen
/// region in un-rotated page points.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisiblePage {
    #[serde(flatten)]
    pub page: PageBox,
    #[serde(rename = "ref")]
    pub page_ref: PageRef,
    pub screen_x: f64,
    pub screen_y: f64,
    pub transform: PageTransform,
    pub visible_rect: Rect,
}

/// A page-relative view memento: what I'm looking at and how zoomed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewpoint {
    pub anchor: Anchor,
    pub zoom: ZoomSpec,
}

/// Durable, serializable view state: the unit of session persistence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageViewState {
    #[serde(flatten)]
    pub settings: StageSettings,
    /// `None` restores page 0 (`view.cursor ?? 0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
    pub anchor: Anchor,
}

/// A view to restore: [`StageViewState`] with each setting optional, as
/// `applyViewState` reads one (`mergeSettings` skips an absent setting, so
/// a partial view keeps the others).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageViewPatch {
    #[serde(flatten)]
    pub settings: StageSettingsPatch,
    /// `None` restores page 0 (`view.cursor ?? 0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
    pub anchor: Anchor,
}

impl From<StageViewState> for StageViewPatch {
    fn from(view: StageViewState) -> Self {
        StageViewPatch { settings: view.settings.into(), cursor: view.cursor, anchor: view.anchor }
    }
}

/// `StageScrollToOptions`: absolute offsets in screen px into the scroll
/// range; an omitted axis does not move. `behavior` defaults to instant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StageScrollToOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<ScrollBehaviorKind>,
}

/// Options for `go_to_destination`: the destination itself says where and
/// how close.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GoToDestinationOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<ScrollBehaviorKind>,
}

/// Options for the navigation intents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoToOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<ScrollBehaviorKind>,
    /// Restore this viewpoint instead of a fresh placement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<Viewpoint>,
    /// Override the landing for this navigation only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrival_align: Option<PartialArrivalAlignment>,
}

impl GoToOptions {
    pub fn behavior(behavior: ScrollBehaviorKind) -> Self {
        GoToOptions { behavior: Some(behavior), ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevealWord {
    Keep,
    Start,
    Center,
    End,
}

/// One axis of a reveal arrival: `'keep' | 'start' | 'center' | 'end'` or a
/// viewport fraction.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RevealAnchorValue {
    Word(RevealWord),
    Fraction(f64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RevealAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<RevealAnchorValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<RevealAnchorValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevealZoomWord {
    Keep,
    Fit,
    FitWidth,
    FitHeight,
}

/// What happens to zoom on a reveal, relative to its target rect.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RevealZoom {
    Word(RevealZoomWord),
    Level { level: f64 },
}

/// Options for `reveal`. With none of `rect`, `zoom` and `anchor` set it is
/// a bare reveal (minimal movement, cursor untouched).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RevealOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior: Option<ScrollBehaviorKind>,
    /// Page space (points from the page's top-left, y down); `None`
    /// (absent or `null`) is the whole page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<RevealZoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<RevealAnchor>,
}

/// `PageDestination` (`@embedpdf/core`, from engine-core): a destination
/// (ISO 32000-1 §12.3.2.2) in page space, measured from the top-left of the
/// page it goes to, y down. `None` keeps the viewer's current value (`null`
/// or absent in the file); an `xyz` zoom of 0 means the same.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum PageDestination {
    #[serde(rename = "xyz")]
    Xyz {
        page: PageRef,
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        #[serde(default)]
        zoom: Option<f64>,
    },
    #[serde(rename = "fit")]
    Fit { page: PageRef },
    #[serde(rename = "fitH")]
    FitH {
        page: PageRef,
        #[serde(default)]
        y: Option<f64>,
    },
    #[serde(rename = "fitV")]
    FitV {
        page: PageRef,
        #[serde(default)]
        x: Option<f64>,
    },
    #[serde(rename = "fitR")]
    FitR { page: PageRef, x: f64, y: f64, width: f64, height: f64 },
    #[serde(rename = "fitB")]
    FitB { page: PageRef },
    #[serde(rename = "fitBH")]
    FitBh {
        page: PageRef,
        #[serde(default)]
        y: Option<f64>,
    },
    #[serde(rename = "fitBV")]
    FitBv {
        page: PageRef,
        #[serde(default)]
        x: Option<f64>,
    },
}

impl PageDestination {
    /// The page it goes to.
    pub fn page(&self) -> PageRef {
        match *self {
            PageDestination::Xyz { page, .. }
            | PageDestination::Fit { page }
            | PageDestination::FitH { page, .. }
            | PageDestination::FitV { page, .. }
            | PageDestination::FitR { page, .. }
            | PageDestination::FitB { page }
            | PageDestination::FitBh { page, .. }
            | PageDestination::FitBv { page, .. } => page,
        }
    }
}

/// A page argument as the stage takes it, `PageRef | number`: a page's
/// durable ref, or its zero-based display index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageTarget {
    Ref(PageRef),
    /// A display index. Only a whole number names a page.
    Index(f64),
}

impl From<PageRef> for PageTarget {
    fn from(page: PageRef) -> Self {
        PageTarget::Ref(page)
    }
}

impl From<usize> for PageTarget {
    fn from(index: usize) -> Self {
        PageTarget::Index(index as f64)
    }
}

/// `getPageAt`'s hit: the page under a viewport point and its page-space
/// point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageHit {
    #[serde(rename = "ref")]
    pub page_ref: PageRef,
    pub point: Point,
    pub scale: f64,
    /// Total display rotation (document `/Rotate` plus view rotation).
    pub rotation: PageRotation,
    /// Zoom relative to the page's 100% baseline.
    pub zoom: f64,
}

/// `StageConfig`: the settings this lens registers with, plus the
/// responsive rules.
#[derive(Clone, Default)]
pub struct StageConfig {
    pub settings: StageSettingsPatch,
    /// Container queries; `None` is [`crate::settings::default_responsive`],
    /// an empty list opts out.
    pub responsive: Option<Vec<ResponsiveRule>>,
}

/// `StagePluginOptions`: a lens's registration. A document may be viewed
/// through several stages at once, each with its own id and token.
#[derive(Clone, Default)]
pub struct StagePluginOptions {
    /// Plugin id; `'stage'` for the main lens.
    pub id: Option<String>,
    /// The lens's token; [`crate::STAGE`] by default.
    pub token: Option<Token<StageCapability>>,
    pub config: StageConfig,
}

/// `ViewportPoint`: this stage's container px, top-left origin.
pub type ViewportPoint = Point;
/// `ViewportRect`.
pub type ViewportRect = Rect;

/// `ZoomModeValue | 'custom'`: the active zoom intent's mode, `Custom` for
/// a fixed level or a page-width or page-height spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageZoomMode {
    Mode(ZoomMode),
    Custom,
}

impl StageZoomMode {
    /// The mode `spec` names, as `'mode' in zoom ? zoom.mode : 'custom'`.
    pub fn of(spec: ZoomSpec) -> Self {
        match spec {
            ZoomSpec::Mode { mode } => StageZoomMode::Mode(mode),
            _ => StageZoomMode::Custom,
        }
    }
}

impl Serialize for StageZoomMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            StageZoomMode::Mode(mode) => mode.serialize(s),
            StageZoomMode::Custom => s.serialize_str("custom"),
        }
    }
}

/// `zoomTo`'s first argument: a bare number is a fixed level.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ZoomTarget {
    Level(f64),
    Spec(ZoomSpec),
}

/// `ZoomToOptions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ZoomToOptions {
    /// Hold this viewport point fixed while the level changes (default: the
    /// `zoomAlign` point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub around: Option<ViewportPoint>,
}

/// `beginGesture`'s options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GestureOptions {
    /// Rubber-band overscroll for this gesture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elastic: Option<bool>,
}

// ── events ──

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagePageChangedEvent {
    /// The new current page, `None` when the document has no page there.
    pub page: Option<PageRef>,
    pub page_index: usize,
    pub previous_page_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageZoomChangedEvent {
    pub level: f64,
    pub previous_level: f64,
    pub mode: StageZoomMode,
}

/// Coalesced per camera write, not per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageCameraChangedEvent {
    pub camera: Camera,
    /// The member order of the camera object the state holds.
    pub(crate) order: CameraOrder,
}

/// A tween or fling settled, or was stopped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageMotionEndedEvent {
    pub camera: Camera,
    pub(crate) order: CameraOrder,
}

impl Serialize for StageCameraChangedEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry("camera", &self.order.view(&self.camera))?;
        map.end()
    }
}

impl Serialize for StageMotionEndedEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry("camera", &self.order.view(&self.camera))?;
        map.end()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StageSettingsChangedEvent {
    pub settings: StageSettings,
    /// The keys whose value changed, in `SETTING_KEYS` order.
    pub changed: Vec<SettingKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StageViewportChangedEvent {
    pub size: Size,
}

/// `stageState`'s fields (`state.ts`): what a framework binding reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageStateView {
    pub zoom_level: f64,
    pub zoom_mode: StageZoomMode,
    pub current_page_index: usize,
    pub current_page: Option<PageRef>,
    pub page_count: usize,
    pub view_rotation: PageRotation,
    pub active_rules: Vec<String>,
}
