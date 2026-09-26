//! `contract.ts`: the public stage vocabulary. Settings, the camera and
//! view model, navigation and reveal options, events, and the host frame
//! seam. The capability itself is [`crate::StageCapability`]; the token is
//! [`crate::STAGE`].

use std::fmt;
use std::rc::Rc;

use epdf_geometry::{PageFrame, PageRotation, PageTransform, Point, Rect, Size};
use epdf_kernel::geometry::PageRef;
use epdf_plugin_actions::page_ref_json;
use epdf_stage_core::{
    Align, AlignValue, Alignment, AlignmentValue, Anchor, Camera, Direction, PageBox, SizingMode,
    SpreadMode, ZoomMode, ZoomSpec,
};
use serde::{Deserialize, Serialize, Serializer};

use crate::settings::SettingKey;

/// `PageObjectNumber`, the number inside a [`PageRef`].
pub type PageObjectNumber = u32;

/// `PageInfo` (`PageLayout` in `engine-core`): one row of the kernel's page
/// registry, as the engine reported it. The kernel keeps the registry as
/// JSON, so the stage hands rows back as the same JSON.
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
/// about with `set_viewport`.
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
    pub settings: Option<PartialSettings>,
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

/// The stage's orthogonal, independently settable primitives.
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
    pub view_units_per_point: f64,
}

/// `Partial<StageSettings>`. `None` is an absent (or `undefined`) key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialSettings {
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
    pub page_frame: Option<PageFrame>,
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
    pub view_units_per_point: Option<f64>,
}

impl PartialSettings {
    /// A patch that sets only the zoom intent.
    pub fn zoom(zoom: ZoomSpec) -> Self {
        PartialSettings { zoom: Some(zoom), ..Default::default() }
    }
}

impl From<StageSettings> for PartialSettings {
    fn from(s: StageSettings) -> Self {
        PartialSettings {
            flow: Some(s.flow),
            layout: Some(s.layout),
            spread: Some(s.spread),
            sizing: Some(s.sizing),
            columns: Some(s.columns),
            bounded: Some(s.bounded),
            padding: Some(s.padding),
            gap: Some(s.gap),
            page_frame: Some(s.page_frame),
            direction: Some(s.direction),
            fit_align: Some(s.fit_align),
            arrival_align: Some(s.arrival_align),
            zoom_align: Some(s.zoom_align),
            anchor_align: Some(s.anchor_align),
            view_rotation: Some(s.view_rotation),
            zoom: Some(s.zoom),
            scroll_behavior: Some(s.scroll_behavior),
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
    #[serde(rename = "ref", with = "page_ref_json")]
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
    /// `undefined` restores page 0 (`view.cursor ?? 0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
    pub anchor: Anchor,
}

/// The host frame seam: `requestAnimationFrame` and `cancelAnimationFrame`.
///
/// The TypeScript binds `globalThis.requestAnimationFrame` when the host has
/// one and otherwise runs frameless (navigation jumps, no rest detector, no
/// momentum). Rust has no ambient frame clock, so the host passes one in
/// [`StageConfig::scheduler`]; without it the stage is frameless. A handle
/// is never 0, which the capability reads as "nothing scheduled".
pub trait Scheduler {
    /// Run the callback on the next frame with the frame's timestamp in ms.
    fn raf(&self, callback: Box<dyn FnOnce(f64)>) -> u64;
    /// Cancel a scheduled callback.
    fn caf(&self, handle: u64);
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
    /// Viewer coordinates on the page; `None` (absent or `null`) is the
    /// whole page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<RevealZoom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<RevealAnchor>,
}

/// `getPageAt`'s hit: the page under a viewport point and its page-space
/// point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageHit {
    #[serde(rename = "ref", with = "page_ref_json")]
    pub page_ref: PageRef,
    pub point: Point,
    pub scale: f64,
    /// Total display rotation (document `/Rotate` plus view rotation).
    pub rotation: PageRotation,
    /// Zoom relative to the page's 100% baseline.
    pub zoom: f64,
}

/// `StageConfig`: settings overrides plus the capability's own config.
#[derive(Clone, Default)]
pub struct StageConfig {
    pub settings: PartialSettings,
    /// The host frame seam; `None` is frameless. See [`Scheduler`].
    pub scheduler: Option<Rc<dyn Scheduler>>,
    /// Container queries; `None` is [`crate::settings::default_responsive`],
    /// an empty list opts out.
    pub responsive: Option<Vec<ResponsiveRule>>,
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

// ── events ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagePageChangedEvent {
    /// The registry row of the new cursor page, `None` when there is none.
    pub page: Option<PageInfo>,
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StageCameraChangedEvent {
    pub camera: Camera,
}

/// A tween or fling settled, or was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StageMotionEndedEvent {
    pub camera: Camera,
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
