//! The stage plugin through uniffi: the viewport lens as one handle.
//!
//! Stage-core types shared with other plugins (the camera, the zoom spec,
//! the anchor, alignment) are `epdf-ffi`'s. Stage's own types are mirrored
//! here with a `Stage` prefix: most carry an `epdf-geometry` or
//! `epdf-stage-core` field, a `usize` index, or a JSON page row, none of
//! which uniffi can carry. A value that does not fit its record (a column
//! count that is not whole, an index past `u32`, a page row without its
//! size) is an `invalid-state` error, never a guess.
//!
//! A page argument is a `PageRef`; the `_index` twins take a display index
//! (the TypeScript's `PageRef | number`). Motion runs on the kernel's host
//! clock: a host that paints gives the kernel frames, and without them every
//! navigation jumps.
//!
//! Every method hops to the kernel thread. `set_camera`, `pan_by`,
//! `scroll_by` and `zoom_around` fire without a reply; their errors go to
//! the kernel's report sink.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    Camera, HostError, KernelBound, KernelPost, PageFrameInsets, PageInfo, PageRef, Point, Rect,
    RegistrationHandle, Size, StageAlignValue, StageAlignment, StageAlignmentValue, StageDirection,
    StageScrollMetrics, StageSizingMode, StageSpreadMode, Subscription, ViewAnchor, ViewerError,
    ZoomMode, ZoomSpec, mirror_enum, to_u32,
};
use epdf_geometry::PageRotation;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_interaction::{InteractionCapability, RegisterHandlerOptions};
use serde::Deserialize;
use serde_json::Value;

use crate::settings::SettingKey;
use crate::{
    ArrivalAlignValue, ArrivalAlignment, BoxQuery, ColumnsPolicy, FlowMode, Gap, GestureOptions,
    GoToDestinationOptions, GoToOptions, GridColumns, Keep, LayoutKind, Orientation,
    PageDestination, PageHit, PageTarget, PartialArrivalAlignment, PartialPageFrame,
    ResponsiveRule, RevealAnchor, RevealAnchorValue, RevealOptions, RevealWord, RevealZoom,
    RevealZoomWord, RuleWhen, ScrollBehaviorKind, ScrollHandlerOptions, StageCapability,
    StageZoomMode as CoreZoomMode, Viewpoint, VisiblePage, ZoomTarget, ZoomToOptions,
    create_scroll_handler,
};

/// A page argument by its ref.
fn page_target(page: PageRef) -> PageTarget {
    PageTarget::Ref(page.into())
}

fn invalid(what: String) -> Error {
    Error::named("InvalidState", format!("[stage] {what}")).with("code", "invalid-state")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageFlow {
    Continuous,
    Paged,
}
mirror_enum!(StageFlow <=> FlowMode { Continuous, Paged });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageLayout {
    Vertical,
    Horizontal,
    Grid,
}
mirror_enum!(StageLayout <=> LayoutKind { Vertical, Horizontal, Grid });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageScrollBehavior {
    Smooth,
    Instant,
}
mirror_enum!(StageScrollBehavior <=> ScrollBehaviorKind { Smooth, Instant });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageOrientation {
    Portrait,
    Landscape,
}
mirror_enum!(StageOrientation <=> Orientation { Portrait, Landscape });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageRevealWord {
    Keep,
    Start,
    Center,
    End,
}
mirror_enum!(StageRevealWord <=> RevealWord { Keep, Start, Center, End });

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageSettingKey {
    Flow,
    Layout,
    Spread,
    Sizing,
    Columns,
    Bounded,
    Padding,
    Gap,
    PageFrame,
    Direction,
    FitAlign,
    ArrivalAlign,
    ZoomAlign,
    AnchorAlign,
    ViewRotation,
    Zoom,
    ScrollBehavior,
    Interaction,
    PanFallback,
    ZoomGestures,
    ViewUnitsPerPoint,
}
mirror_enum!(StageSettingKey <=> SettingKey {
    Flow, Layout, Spread, Sizing, Columns, Bounded, Padding, Gap, PageFrame, Direction,
    FitAlign, ArrivalAlign, ZoomAlign, AnchorAlign, ViewRotation, Zoom, ScrollBehavior,
    Interaction, PanFallback, ZoomGestures, ViewUnitsPerPoint,
});

/// Grid columns: about the square root of the item count, as many as fit,
/// or a fixed count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageGridColumns {
    Square,
    Auto,
    Count { count: u32 },
}

impl TryFrom<GridColumns> for StageGridColumns {
    type Error = Error;
    fn try_from(c: GridColumns) -> Result<Self, Error> {
        Ok(match c {
            GridColumns::Policy(ColumnsPolicy::Square) => StageGridColumns::Square,
            GridColumns::Policy(ColumnsPolicy::Auto) => StageGridColumns::Auto,
            GridColumns::Count(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => {
                StageGridColumns::Count { count: n as u32 }
            }
            GridColumns::Count(n) => return Err(invalid(format!("a column count of {n}"))),
        })
    }
}

impl From<StageGridColumns> for GridColumns {
    fn from(c: StageGridColumns) -> Self {
        match c {
            StageGridColumns::Square => GridColumns::Policy(ColumnsPolicy::Square),
            StageGridColumns::Auto => GridColumns::Policy(ColumnsPolicy::Auto),
            StageGridColumns::Count { count } => GridColumns::Count(f64::from(count)),
        }
    }
}

/// Space between items: world units (scale with zoom) or screen points.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageGap {
    World { units: f64 },
    Px { px: f64 },
}

impl From<Gap> for StageGap {
    fn from(g: Gap) -> Self {
        match g {
            Gap::World(units) => StageGap::World { units },
            Gap::Px { px } => StageGap::Px { px },
        }
    }
}

impl From<StageGap> for Gap {
    fn from(g: StageGap) -> Self {
        match g {
            StageGap::World { units } => Gap::World(units),
            StageGap::Px { px } => Gap::Px { px },
        }
    }
}

/// One axis of the arrival policy: an alignment, or keep this axis still.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageArrivalAlignValue {
    Keep,
    Align { value: StageAlignValue },
}

impl From<ArrivalAlignValue> for StageArrivalAlignValue {
    fn from(v: ArrivalAlignValue) -> Self {
        match v {
            ArrivalAlignValue::Keep(Keep::Keep) => StageArrivalAlignValue::Keep,
            ArrivalAlignValue::Align(value) => {
                StageArrivalAlignValue::Align { value: value.into() }
            }
        }
    }
}

impl From<StageArrivalAlignValue> for ArrivalAlignValue {
    fn from(v: StageArrivalAlignValue) -> Self {
        match v {
            StageArrivalAlignValue::Keep => ArrivalAlignValue::KEEP,
            StageArrivalAlignValue::Align { value } => ArrivalAlignValue::Align(value.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageArrivalAlignment {
    pub x: StageArrivalAlignValue,
    pub y: StageArrivalAlignValue,
}

impl From<ArrivalAlignment> for StageArrivalAlignment {
    fn from(a: ArrivalAlignment) -> Self {
        StageArrivalAlignment { x: a.x.into(), y: a.y.into() }
    }
}

impl From<StageArrivalAlignment> for ArrivalAlignment {
    fn from(a: StageArrivalAlignment) -> Self {
        ArrivalAlignment { x: a.x.into(), y: a.y.into() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StagePartialArrivalAlignment {
    #[uniffi(default = None)]
    pub x: Option<StageArrivalAlignValue>,
    #[uniffi(default = None)]
    pub y: Option<StageArrivalAlignValue>,
}

impl From<StagePartialArrivalAlignment> for PartialArrivalAlignment {
    fn from(a: StagePartialArrivalAlignment) -> Self {
        PartialArrivalAlignment { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

/// Every stage setting. `view_rotation` is degrees clockwise.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageSettings {
    pub flow: StageFlow,
    pub layout: StageLayout,
    pub spread: StageSpreadMode,
    pub sizing: StageSizingMode,
    pub columns: StageGridColumns,
    pub bounded: bool,
    pub padding: f64,
    pub gap: StageGap,
    pub page_frame: PageFrameInsets,
    pub direction: StageDirection,
    pub fit_align: StageAlignment,
    pub arrival_align: StageArrivalAlignment,
    pub zoom_align: StageAlignmentValue,
    pub anchor_align: StageAlignmentValue,
    pub view_rotation: f64,
    pub zoom: ZoomSpec,
    pub scroll_behavior: StageScrollBehavior,
    /// Whether this view's pointer input reaches the interaction hub.
    pub interaction: bool,
    /// With `interaction`: a drag between pages pans whatever the tool.
    pub pan_fallback: bool,
    /// Whether a pinch or a zoom wheel zooms this view.
    pub zoom_gestures: bool,
    pub view_units_per_point: f64,
}

impl TryFrom<crate::StageSettings> for StageSettings {
    type Error = Error;
    fn try_from(s: crate::StageSettings) -> Result<Self, Error> {
        Ok(StageSettings {
            flow: s.flow.into(),
            layout: s.layout.into(),
            spread: s.spread.into(),
            sizing: s.sizing.into(),
            columns: s.columns.try_into()?,
            bounded: s.bounded,
            padding: s.padding,
            gap: s.gap.into(),
            page_frame: s.page_frame.into(),
            direction: s.direction.into(),
            fit_align: s.fit_align.into(),
            arrival_align: s.arrival_align.into(),
            zoom_align: s.zoom_align.into(),
            anchor_align: s.anchor_align.into(),
            view_rotation: s.view_rotation.0,
            zoom: s.zoom.into(),
            scroll_behavior: s.scroll_behavior.into(),
            interaction: s.interaction,
            pan_fallback: s.pan_fallback,
            zoom_gestures: s.zoom_gestures,
            view_units_per_point: s.view_units_per_point,
        })
    }
}

impl TryFrom<StageSettings> for crate::StageSettings {
    type Error = Error;
    fn try_from(s: StageSettings) -> Result<Self, Error> {
        Ok(crate::StageSettings {
            flow: s.flow.into(),
            layout: s.layout.into(),
            spread: s.spread.into(),
            sizing: s.sizing.into(),
            columns: s.columns.into(),
            bounded: s.bounded,
            padding: s.padding,
            gap: s.gap.into(),
            page_frame: s.page_frame.into(),
            direction: s.direction.into(),
            fit_align: s.fit_align.into(),
            arrival_align: s.arrival_align.into(),
            zoom_align: s.zoom_align.into(),
            anchor_align: s.anchor_align.into(),
            view_rotation: PageRotation(s.view_rotation),
            zoom: s.zoom.into(),
            scroll_behavior: s.scroll_behavior.into(),
            interaction: s.interaction,
            pan_fallback: s.pan_fallback,
            zoom_gestures: s.zoom_gestures,
            view_units_per_point: s.view_units_per_point,
        })
    }
}

/// Some settings: `None` leaves a key alone.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageSettingsPatch {
    #[uniffi(default = None)]
    pub flow: Option<StageFlow>,
    #[uniffi(default = None)]
    pub layout: Option<StageLayout>,
    #[uniffi(default = None)]
    pub spread: Option<StageSpreadMode>,
    #[uniffi(default = None)]
    pub sizing: Option<StageSizingMode>,
    #[uniffi(default = None)]
    pub columns: Option<StageGridColumns>,
    #[uniffi(default = None)]
    pub bounded: Option<bool>,
    #[uniffi(default = None)]
    pub padding: Option<f64>,
    #[uniffi(default = None)]
    pub gap: Option<StageGap>,
    #[uniffi(default = None)]
    pub page_frame: Option<PageFrameInsets>,
    #[uniffi(default = None)]
    pub direction: Option<StageDirection>,
    #[uniffi(default = None)]
    pub fit_align: Option<StageAlignment>,
    #[uniffi(default = None)]
    pub arrival_align: Option<StageArrivalAlignment>,
    #[uniffi(default = None)]
    pub zoom_align: Option<StageAlignmentValue>,
    #[uniffi(default = None)]
    pub anchor_align: Option<StageAlignmentValue>,
    #[uniffi(default = None)]
    pub view_rotation: Option<f64>,
    #[uniffi(default = None)]
    pub zoom: Option<ZoomSpec>,
    #[uniffi(default = None)]
    pub scroll_behavior: Option<StageScrollBehavior>,
    #[uniffi(default = None)]
    pub interaction: Option<bool>,
    #[uniffi(default = None)]
    pub pan_fallback: Option<bool>,
    #[uniffi(default = None)]
    pub zoom_gestures: Option<bool>,
    #[uniffi(default = None)]
    pub view_units_per_point: Option<f64>,
}

impl TryFrom<StageSettingsPatch> for crate::StageSettingsPatch {
    type Error = Error;
    fn try_from(p: StageSettingsPatch) -> Result<Self, Error> {
        Ok(crate::StageSettingsPatch {
            flow: p.flow.map(Into::into),
            layout: p.layout.map(Into::into),
            spread: p.spread.map(Into::into),
            sizing: p.sizing.map(Into::into),
            columns: p.columns.map(Into::into),
            bounded: p.bounded,
            padding: p.padding,
            gap: p.gap.map(Into::into),
            page_frame: p
                .page_frame
                .map(|frame| PartialPageFrame::from(epdf_geometry::PageFrame::from(frame))),
            direction: p.direction.map(Into::into),
            fit_align: p.fit_align.map(Into::into),
            arrival_align: p.arrival_align.map(Into::into),
            zoom_align: p.zoom_align.map(Into::into),
            anchor_align: p.anchor_align.map(Into::into),
            view_rotation: p.view_rotation.map(PageRotation),
            zoom: p.zoom.map(Into::into),
            scroll_behavior: p.scroll_behavior.map(Into::into),
            interaction: p.interaction,
            pan_fallback: p.pan_fallback,
            zoom_gestures: p.zoom_gestures,
            view_units_per_point: p.view_units_per_point,
        })
    }
}

/// A container query: bounds inclusive, every field optional, all AND-ed.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageBoxQuery {
    #[uniffi(default = None)]
    pub min_width: Option<f64>,
    #[uniffi(default = None)]
    pub max_width: Option<f64>,
    #[uniffi(default = None)]
    pub min_height: Option<f64>,
    #[uniffi(default = None)]
    pub max_height: Option<f64>,
    #[uniffi(default = None)]
    pub orientation: Option<StageOrientation>,
}

impl From<StageBoxQuery> for BoxQuery {
    fn from(q: StageBoxQuery) -> Self {
        BoxQuery {
            min_width: q.min_width,
            max_width: q.max_width,
            min_height: q.min_height,
            max_height: q.max_height,
            orientation: q.orientation.map(Into::into),
        }
    }
}

/// When the viewport matches `when`, assert `settings`. Later rules win
/// per key. A rule without settings is a named query (`matches_rule`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageResponsiveRule {
    #[uniffi(default = None)]
    pub name: Option<String>,
    pub when: StageBoxQuery,
    #[uniffi(default = None)]
    pub settings: Option<StageSettingsPatch>,
}

impl TryFrom<StageResponsiveRule> for ResponsiveRule {
    type Error = Error;
    fn try_from(r: StageResponsiveRule) -> Result<Self, Error> {
        Ok(ResponsiveRule {
            name: r.name,
            when: RuleWhen::Query(r.when.into()),
            settings: r.settings.map(crate::StageSettingsPatch::try_from).transpose()?,
        })
    }
}

/// A page's content, view and device sizes and scales, in points.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StagePageTransform {
    pub rotation: f64,
    pub view_width: f64,
    pub view_height: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub device_width: f64,
    pub device_height: f64,
    /// Device pixels per PDF point: the scale to render this page at.
    pub render_scale: f64,
    pub view_scale: f64,
    pub base_scale: f64,
    pub zoom: f64,
}

impl From<&epdf_geometry::PageTransform> for StagePageTransform {
    fn from(t: &epdf_geometry::PageTransform) -> Self {
        StagePageTransform {
            rotation: t.rotation.0,
            view_width: t.view_width,
            view_height: t.view_height,
            content_width: t.content_width,
            content_height: t.content_height,
            device_width: t.device_width,
            device_height: t.device_height,
            render_scale: t.render_scale,
            view_scale: t.view_scale,
            base_scale: t.base_scale,
            zoom: t.zoom,
        }
    }
}

/// A laid-out page: where it sits on screen and how it is scaled.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageVisiblePage {
    pub page: PageRef,
    pub page_index: u32,
    /// World-space layout box.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub content_scale: f64,
    /// Device-snapped screen origin, in viewport points.
    pub screen_x: f64,
    pub screen_y: f64,
    pub transform: StagePageTransform,
    /// The on-screen region in un-rotated page points.
    pub visible_rect: Rect,
}

impl TryFrom<&VisiblePage> for StageVisiblePage {
    type Error = Error;
    fn try_from(v: &VisiblePage) -> Result<Self, Error> {
        Ok(StageVisiblePage {
            page: v.page_ref.into(),
            page_index: to_u32(v.page.page_index, "a visible page's index")?,
            x: v.page.x,
            y: v.page.y,
            width: v.page.width,
            height: v.page.height,
            rotation: v.page.rotation.0,
            content_scale: v.page.content_scale,
            screen_x: v.screen_x,
            screen_y: v.screen_y,
            transform: (&v.transform).into(),
            visible_rect: v.visible_rect.into(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageViewpoint {
    pub anchor: ViewAnchor,
    pub zoom: ZoomSpec,
}

impl TryFrom<Viewpoint> for StageViewpoint {
    type Error = Error;
    fn try_from(v: Viewpoint) -> Result<Self, Error> {
        Ok(StageViewpoint { anchor: v.anchor.try_into()?, zoom: v.zoom.into() })
    }
}

impl From<StageViewpoint> for Viewpoint {
    fn from(v: StageViewpoint) -> Self {
        Viewpoint { anchor: v.anchor.into(), zoom: v.zoom.into() }
    }
}

/// Durable view state: settings plus where you are. Save it, then
/// `apply_view_state` or an initial-view provider restores it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageViewState {
    pub settings: StageSettings,
    /// `None` restores page 0.
    pub cursor: Option<u32>,
    pub anchor: ViewAnchor,
}

impl TryFrom<StageViewState> for crate::StageViewState {
    type Error = Error;
    fn try_from(v: StageViewState) -> Result<Self, Error> {
        Ok(crate::StageViewState {
            settings: v.settings.try_into()?,
            cursor: v.cursor.map(|c| c as usize),
            anchor: v.anchor.into(),
        })
    }
}

impl TryFrom<crate::StageViewState> for StageViewState {
    type Error = Error;
    fn try_from(v: crate::StageViewState) -> Result<Self, Error> {
        Ok(StageViewState {
            settings: v.settings.try_into()?,
            cursor: v.cursor.map(|c| to_u32(c, "a view state's cursor")).transpose()?,
            anchor: v.anchor.try_into()?,
        })
    }
}

/// Absolute offsets into the scroll range; an omitted axis stays.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageScrollToOptions {
    #[uniffi(default = None)]
    pub left: Option<f64>,
    #[uniffi(default = None)]
    pub top: Option<f64>,
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
}

impl From<StageScrollToOptions> for crate::StageScrollToOptions {
    fn from(o: StageScrollToOptions) -> Self {
        crate::StageScrollToOptions {
            left: o.left,
            top: o.top,
            behavior: o.behavior.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageGoToOptions {
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
    /// Restore this viewpoint instead of a fresh placement.
    #[uniffi(default = None)]
    pub viewpoint: Option<StageViewpoint>,
    #[uniffi(default = None)]
    pub arrival_align: Option<StagePartialArrivalAlignment>,
}

impl From<StageGoToOptions> for GoToOptions {
    fn from(o: StageGoToOptions) -> Self {
        GoToOptions {
            behavior: o.behavior.map(Into::into),
            viewpoint: o.viewpoint.map(Into::into),
            arrival_align: o.arrival_align.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageRevealAnchorValue {
    Word { word: StageRevealWord },
    Fraction { value: f64 },
}

impl From<RevealAnchorValue> for StageRevealAnchorValue {
    fn from(v: RevealAnchorValue) -> Self {
        match v {
            RevealAnchorValue::Word(word) => StageRevealAnchorValue::Word { word: word.into() },
            RevealAnchorValue::Fraction(value) => StageRevealAnchorValue::Fraction { value },
        }
    }
}

impl From<StageRevealAnchorValue> for RevealAnchorValue {
    fn from(v: StageRevealAnchorValue) -> Self {
        match v {
            StageRevealAnchorValue::Word { word } => RevealAnchorValue::Word(word.into()),
            StageRevealAnchorValue::Fraction { value } => RevealAnchorValue::Fraction(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageRevealAnchor {
    #[uniffi(default = None)]
    pub x: Option<StageRevealAnchorValue>,
    #[uniffi(default = None)]
    pub y: Option<StageRevealAnchorValue>,
}

impl From<RevealAnchor> for StageRevealAnchor {
    fn from(a: RevealAnchor) -> Self {
        StageRevealAnchor { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

impl From<StageRevealAnchor> for RevealAnchor {
    fn from(a: StageRevealAnchor) -> Self {
        RevealAnchor { x: a.x.map(Into::into), y: a.y.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageRevealZoom {
    Keep,
    Fit,
    FitWidth,
    FitHeight,
    Level { level: f64 },
}

impl From<RevealZoom> for StageRevealZoom {
    fn from(z: RevealZoom) -> Self {
        match z {
            RevealZoom::Word(RevealZoomWord::Keep) => StageRevealZoom::Keep,
            RevealZoom::Word(RevealZoomWord::Fit) => StageRevealZoom::Fit,
            RevealZoom::Word(RevealZoomWord::FitWidth) => StageRevealZoom::FitWidth,
            RevealZoom::Word(RevealZoomWord::FitHeight) => StageRevealZoom::FitHeight,
            RevealZoom::Level { level } => StageRevealZoom::Level { level },
        }
    }
}

impl From<StageRevealZoom> for RevealZoom {
    fn from(z: StageRevealZoom) -> Self {
        match z {
            StageRevealZoom::Keep => RevealZoom::Word(RevealZoomWord::Keep),
            StageRevealZoom::Fit => RevealZoom::Word(RevealZoomWord::Fit),
            StageRevealZoom::FitWidth => RevealZoom::Word(RevealZoomWord::FitWidth),
            StageRevealZoom::FitHeight => RevealZoom::Word(RevealZoomWord::FitHeight),
            StageRevealZoom::Level { level } => RevealZoom::Level { level },
        }
    }
}

/// A reveal. With none of `rect`, `zoom` and `anchor` set it is the
/// minimal movement that brings the page into view.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageRevealOptions {
    #[uniffi(default = None)]
    pub behavior: Option<StageScrollBehavior>,
    /// Page space; `None` is the whole page.
    #[uniffi(default = None)]
    pub rect: Option<Rect>,
    #[uniffi(default = None)]
    pub zoom: Option<StageRevealZoom>,
    #[uniffi(default = None)]
    pub anchor: Option<StageRevealAnchor>,
}

impl From<RevealOptions> for StageRevealOptions {
    fn from(o: RevealOptions) -> Self {
        StageRevealOptions {
            behavior: o.behavior.map(Into::into),
            rect: o.rect.map(Into::into),
            zoom: o.zoom.map(Into::into),
            anchor: o.anchor.map(Into::into),
        }
    }
}

impl From<StageRevealOptions> for RevealOptions {
    fn from(o: StageRevealOptions) -> Self {
        RevealOptions {
            behavior: o.behavior.map(Into::into),
            rect: o.rect.map(Into::into),
            zoom: o.zoom.map(Into::into),
            anchor: o.anchor.map(Into::into),
        }
    }
}

/// The page under a viewport point and its page-space point.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StagePageHit {
    pub page: PageRef,
    pub point: Point,
    pub scale: f64,
    pub rotation: f64,
    pub zoom: f64,
}

impl From<PageHit> for StagePageHit {
    fn from(h: PageHit) -> Self {
        StagePageHit {
            page: h.page_ref.into(),
            point: h.point.into(),
            scale: h.scale,
            rotation: h.rotation.0,
            zoom: h.zoom,
        }
    }
}

/// The active zoom intent's mode; `Custom` for a level or a page fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StageZoomMode {
    Mode { mode: ZoomMode },
    Custom,
}

impl From<CoreZoomMode> for StageZoomMode {
    fn from(m: CoreZoomMode) -> Self {
        match m {
            CoreZoomMode::Mode(mode) => StageZoomMode::Mode { mode: mode.into() },
            CoreZoomMode::Custom => StageZoomMode::Custom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StageZoomTarget {
    Level { level: f64 },
    Spec { spec: ZoomSpec },
}

impl From<StageZoomTarget> for ZoomTarget {
    fn from(t: StageZoomTarget) -> Self {
        match t {
            StageZoomTarget::Level { level } => ZoomTarget::Level(level),
            StageZoomTarget::Spec { spec } => ZoomTarget::Spec(spec.into()),
        }
    }
}

// One row of the page registry crosses as `epdf_ffi::PageInfo` (umbrella
// Task 0d), the record `ViewerHost::pages` already returns, so Swift has one
// page type.

#[derive(Deserialize)]
struct Row {
    index: u32,
    #[serde(rename = "ref")]
    page: RowRef,
    size: RowSize,
    rotation: f64,
}

#[derive(Deserialize)]
struct RowRef {
    #[serde(rename = "objectNumber")]
    page_object_number: u32,
}

#[derive(Deserialize)]
struct RowSize {
    width: f64,
    height: f64,
}

fn page_info(row: &Value) -> Result<PageInfo, Error> {
    let decoded: Row = serde_json::from_value(row.clone())
        .map_err(|e| invalid(format!("a page row that does not decode ({e}): {row}")))?;
    let rotation = decoded.rotation;
    if !(rotation.fract() == 0.0 && (0.0..360.0).contains(&rotation)) {
        return Err(invalid(format!("a page row whose rotation is not 0, 90, 180 or 270: {row}")));
    }
    Ok(PageInfo {
        page: PageRef { page_object_number: decoded.page.page_object_number },
        index: decoded.index,
        width: decoded.size.width,
        height: decoded.size.height,
        rotation: rotation as u32,
    })
}

/// `stagePlugin({ config })`'s options, as `ViewerConfig` carries them.
/// Tweens and flings run on the kernel's host clock; a host clock without
/// frames makes every navigation jump.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct StageConfig {
    #[uniffi(default = None)]
    pub settings: Option<StageSettingsPatch>,
    /// `None` keeps the stage's default rules; an empty list opts out.
    #[uniffi(default = None)]
    pub responsive: Option<Vec<StageResponsiveRule>>,
}

impl StageConfig {
    /// On the kernel thread: a rule is an `Rc`.
    pub fn into_config(self) -> Result<crate::StageConfig, Error> {
        Ok(crate::StageConfig {
            settings: self
                .settings
                .map(crate::StageSettingsPatch::try_from)
                .transpose()?
                .unwrap_or_default(),
            responsive: self
                .responsive
                .map(|rules| rules.into_iter().map(ResponsiveRule::try_from).collect())
                .transpose()?,
        })
    }
}

/// A destination in page space (`PageDestination`): the page it goes to
/// and where on it, from the page's top-left, y down. `None` keeps the
/// viewer's current value; an `xyz` zoom of 0 means the same.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum StagePageDestination {
    Xyz { page: PageRef, x: Option<f64>, y: Option<f64>, zoom: Option<f64> },
    Fit { page: PageRef },
    FitH { page: PageRef, y: Option<f64> },
    FitV { page: PageRef, x: Option<f64> },
    FitR { page: PageRef, x: f64, y: f64, width: f64, height: f64 },
    FitB { page: PageRef },
    FitBh { page: PageRef, y: Option<f64> },
    FitBv { page: PageRef, x: Option<f64> },
}

impl From<StagePageDestination> for PageDestination {
    fn from(d: StagePageDestination) -> Self {
        use StagePageDestination as D;
        match d {
            D::Xyz { page, x, y, zoom } => PageDestination::Xyz { page: page.into(), x, y, zoom },
            D::Fit { page } => PageDestination::Fit { page: page.into() },
            D::FitH { page, y } => PageDestination::FitH { page: page.into(), y },
            D::FitV { page, x } => PageDestination::FitV { page: page.into(), x },
            D::FitR { page, x, y, width, height } => {
                PageDestination::FitR { page: page.into(), x, y, width, height }
            }
            D::FitB { page } => PageDestination::FitB { page: page.into() },
            D::FitBh { page, y } => PageDestination::FitBh { page: page.into(), y },
            D::FitBv { page, x } => PageDestination::FitBv { page: page.into(), x },
        }
    }
}

impl From<PageDestination> for StagePageDestination {
    fn from(d: PageDestination) -> Self {
        use StagePageDestination as D;
        match d {
            PageDestination::Xyz { page, x, y, zoom } => D::Xyz { page: page.into(), x, y, zoom },
            PageDestination::Fit { page } => D::Fit { page: page.into() },
            PageDestination::FitH { page, y } => D::FitH { page: page.into(), y },
            PageDestination::FitV { page, x } => D::FitV { page: page.into(), x },
            PageDestination::FitR { page, x, y, width, height } => {
                D::FitR { page: page.into(), x, y, width, height }
            }
            PageDestination::FitB { page } => D::FitB { page: page.into() },
            PageDestination::FitBh { page, y } => D::FitBh { page: page.into(), y },
            PageDestination::FitBv { page, x } => D::FitBv { page: page.into(), x },
        }
    }
}

/// Supplies the first view of a document (a saved position, say). Called
/// on the kernel thread when the stage first places, highest priority
/// first; `None` passes to the next provider.
#[uniffi::export(with_foreign)]
pub trait StageInitialViewProvider: Send + Sync {
    fn initial_view(&self) -> Result<Option<StageViewState>, HostError>;
}

fn open(cap: &StageCapability) -> Result<&StageCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "stage",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

struct Bound {
    cap: StageCapability,
    /// The document's pointer hub, when the interaction plugin is installed.
    hub: Option<InteractionCapability>,
    report: Rc<dyn Fn(&Error)>,
}

impl Bound {
    /// For a fire method: run `f` if the document is open, report its error.
    fn fire(&self, f: impl FnOnce(&StageCapability) -> Result<(), Error>) {
        if let Ok(cap) = open(&self.cap)
            && let Err(error) = f(cap)
        {
            (self.report)(&error);
        }
    }
}

#[derive(uniffi::Object)]
pub struct StageHandle {
    bound: KernelBound<Bound>,
    post: KernelPost,
}

impl StageHandle {
    /// On the kernel thread. `report` receives a fire method's errors.
    pub fn new(cap: StageCapability, post: KernelPost, report: Rc<dyn Fn(&Error)>) -> Arc<Self> {
        Self::with_hub(cap, None, post, report)
    }

    /// `new`, with the document's pointer hub for
    /// [`StageHandle::register_scroll_handler`].
    pub fn with_hub(
        cap: StageCapability,
        hub: Option<InteractionCapability>,
        post: KernelPost,
        report: Rc<dyn Fn(&Error)>,
    ) -> Arc<Self> {
        Arc::new(StageHandle {
            bound: KernelBound::new(Bound { cap, hub, report }, post.clone()),
            post,
        })
    }

    async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&StageCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.bound.call(move |b| f(open(&b.cap)?)).await
    }
}

#[uniffi::export]
impl StageHandle {
    // ── read ──

    pub async fn camera(&self) -> Result<Camera, ViewerError> {
        self.read(|c| Ok(c.get_camera().into())).await
    }

    pub async fn viewport_size(&self) -> Result<Size, ViewerError> {
        self.read(|c| Ok(c.get_viewport_size().into())).await
    }

    pub async fn scroll_metrics(&self) -> Result<StageScrollMetrics, ViewerError> {
        self.read(|c| Ok(c.get_scroll_metrics().into())).await
    }

    pub async fn visible_pages(&self) -> Result<Vec<StageVisiblePage>, ViewerError> {
        self.read(|c| c.list_visible_pages().iter().map(StageVisiblePage::try_from).collect()).await
    }

    pub async fn is_page_visible(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.read(move |c| Ok(c.is_page_visible(page_target(page)))).await
    }

    pub async fn current_page_index(&self) -> Result<u32, ViewerError> {
        self.read(|c| to_u32(c.get_current_page_index(), "the current page index")).await
    }

    pub async fn current_page(&self) -> Result<Option<PageRef>, ViewerError> {
        self.read(|c| Ok(c.get_current_page().map(Into::into))).await
    }

    pub async fn page_count(&self) -> Result<u32, ViewerError> {
        self.read(|c| to_u32(c.get_page_count(), "the page count")).await
    }

    /// Where the view is, as a destination; `None` before a page is on
    /// screen.
    pub async fn current_destination(&self) -> Result<Option<StagePageDestination>, ViewerError> {
        self.read(|c| Ok(c.get_current_destination().map(Into::into))).await
    }

    /// The pages of the current item (two in a spread).
    pub async fn current_item_pages(&self) -> Result<Vec<PageInfo>, ViewerError> {
        self.read(|c| c.list_current_item_pages().iter().map(page_info).collect()).await
    }

    pub async fn page_frame(&self, page: PageRef) -> Result<Option<StageVisiblePage>, ViewerError> {
        self.read(move |c| {
            c.get_page_frame(page_target(page)).as_ref().map(StageVisiblePage::try_from).transpose()
        })
        .await
    }

    pub async fn page_at(&self, point: Point) -> Result<Option<StagePageHit>, ViewerError> {
        self.read(move |c| Ok(c.get_page_at(point.into()).map(Into::into))).await
    }

    pub async fn viewport_to_page(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.viewport_to_page(page_target(page), point.into()).map(Into::into)))
            .await
    }

    pub async fn page_to_world(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.page_to_world(page_target(page), point.into()).map(Into::into)))
            .await
    }

    pub async fn page_rect_to_viewport(
        &self,
        page: PageRef,
        rect: Rect,
    ) -> Result<Option<Rect>, ViewerError> {
        self.read(move |c| {
            Ok(c.page_rect_to_viewport(page_target(page), rect.into()).map(Into::into))
        })
        .await
    }

    pub async fn world_to_viewport(&self, point: Point) -> Result<Point, ViewerError> {
        self.read(move |c| Ok(c.world_to_viewport(point.into()).into())).await
    }

    pub async fn viewport_to_world(&self, point: Point) -> Result<Point, ViewerError> {
        self.read(move |c| Ok(c.viewport_to_world(point.into()).into())).await
    }

    pub async fn page_to_viewport(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<Point>, ViewerError> {
        self.read(move |c| Ok(c.page_to_viewport(page_target(page), point.into()).map(Into::into)))
            .await
    }

    /// Degrees clockwise.
    pub async fn view_rotation(&self) -> Result<f64, ViewerError> {
        self.read(|c| Ok(c.get_view_rotation().0)).await
    }

    pub async fn zoom_level(&self) -> Result<f64, ViewerError> {
        self.read(|c| Ok(c.get_zoom_level())).await
    }

    pub async fn zoom_mode(&self) -> Result<StageZoomMode, ViewerError> {
        self.read(|c| Ok(c.get_zoom_mode().into())).await
    }

    pub async fn viewpoint(&self) -> Result<StageViewpoint, ViewerError> {
        self.read(|c| c.get_viewpoint()?.try_into()).await
    }

    // ── motion ──

    /// Momentum from a release velocity in viewport points per second.
    pub async fn fling(&self, velocity_x: f64, velocity_y: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.fling(velocity_x, velocity_y)).await
    }

    pub async fn is_moving(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.is_moving())).await
    }

    pub async fn stop_motion(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.stop_motion();
            Ok(())
        })
        .await
    }

    /// Fire: no reply; an error goes to the report sink.
    pub fn set_camera(&self, camera: Camera) {
        self.bound.fire(move |b| b.fire(|c| c.set_camera(camera.into())));
    }

    /// Fire, in viewport points.
    pub fn pan_by(&self, dx: f64, dy: f64) {
        self.bound.fire(move |b| b.fire(|c| c.pan_by(dx, dy)));
    }

    pub async fn scroll_to(&self, options: StageScrollToOptions) -> Result<(), ViewerError> {
        self.read(move |c| c.scroll_to(options.into())).await
    }

    /// Fire: relative offsets.
    pub fn scroll_by(&self, options: StageScrollToOptions) {
        self.bound.fire(move |b| b.fire(|c| c.scroll_by(options.into())));
    }

    /// Fire: hold `point` (viewport) fixed while zooming by `factor`.
    pub fn zoom_around(&self, point: Point, factor: f64) {
        self.bound.fire(move |b| b.fire(|c| c.zoom_around(point.into(), factor)));
    }

    /// Open a direct-manipulation bracket. `elastic` turns on rubber-band
    /// overscroll for it.
    pub async fn begin_gesture(&self, elastic: Option<bool>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.begin_gesture(Some(GestureOptions { elastic }));
            Ok(())
        })
        .await
    }

    pub async fn end_gesture(&self) -> Result<(), ViewerError> {
        self.read(|c| c.end_gesture()).await
    }

    // ── navigation ──

    /// A past-the-end index lands on the last page.
    pub async fn go_to_page_index(
        &self,
        index: u32,
        options: Option<StageGoToOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_page(index as usize, options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_page(
        &self,
        page: PageRef,
        options: Option<StageGoToOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_page(page_target(page), options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_first_page(
        &self,
        options: Option<StageGoToOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_first_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn go_to_last_page(
        &self,
        options: Option<StageGoToOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_last_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn can_go_next(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.can_go_next())).await
    }

    pub async fn can_go_previous(&self) -> Result<bool, ViewerError> {
        self.read(|c| Ok(c.can_go_previous())).await
    }

    pub async fn next_page(&self, options: Option<StageGoToOptions>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.next_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn previous_page(
        &self,
        options: Option<StageGoToOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.previous_page(options.map(Into::into));
            Ok(())
        })
        .await
    }

    pub async fn reset_view(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.reset_view();
            Ok(())
        })
        .await
    }

    pub async fn reveal(
        &self,
        page: PageRef,
        options: Option<StageRevealOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.reveal(page_target(page), options.map(Into::into));
            Ok(())
        })
        .await
    }

    /// Go where a destination says. It may change the zoom, as the
    /// destination asks.
    pub async fn go_to_destination(
        &self,
        destination: StagePageDestination,
        behavior: Option<StageScrollBehavior>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.go_to_destination(
                &destination.into(),
                Some(GoToDestinationOptions { behavior: behavior.map(Into::into) }),
            );
            Ok(())
        })
        .await
    }

    pub async fn reveal_index(
        &self,
        index: u32,
        options: Option<StageRevealOptions>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.reveal(index as usize, options.map(Into::into));
            Ok(())
        })
        .await
    }

    // ── settings ──

    pub async fn settings(&self) -> Result<StageSettings, ViewerError> {
        self.read(|c| c.get_settings().try_into()).await
    }

    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.read(|c| c.reset_settings()).await
    }

    pub async fn update_settings(&self, patch: StageSettingsPatch) -> Result<(), ViewerError> {
        self.read(move |c| c.update_settings(patch.try_into()?)).await
    }

    pub async fn set_responsive_rules(
        &self,
        rules: Vec<StageResponsiveRule>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            let rules =
                rules.into_iter().map(ResponsiveRule::try_from).collect::<Result<_, _>>()?;
            c.set_responsive_rules(rules)
        })
        .await
    }

    pub async fn matches_rule(&self, name: String) -> Result<bool, ViewerError> {
        self.read(move |c| Ok(c.matches_rule(&name))).await
    }

    pub async fn active_rules(&self) -> Result<Vec<String>, ViewerError> {
        self.read(|c| Ok(c.list_active_rules())).await
    }

    pub async fn apply_view_state(&self, view: StageViewState) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.apply_view_state(crate::StageViewState::try_from(view)?);
            Ok(())
        })
        .await
    }

    // ── zoom ──

    pub async fn double_tap_zoom(&self, point: Point) -> Result<(), ViewerError> {
        self.read(move |c| c.double_tap_zoom(point.into())).await
    }

    pub async fn zoom_in(&self) -> Result<(), ViewerError> {
        self.read(|c| c.zoom_in()).await
    }

    pub async fn zoom_out(&self) -> Result<(), ViewerError> {
        self.read(|c| c.zoom_out()).await
    }

    /// `around` (viewport) stays fixed; `None` is the zoom-align point.
    pub async fn zoom_to(
        &self,
        target: StageZoomTarget,
        around: Option<Point>,
    ) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.zoom_to(target.into(), Some(ZoomToOptions { around: around.map(Into::into) }))
        })
        .await
    }

    pub async fn zoom_by(&self, factor: f64, around: Option<Point>) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.zoom_by(factor, Some(ZoomToOptions { around: around.map(Into::into) }))
        })
        .await
    }

    pub async fn fit_width(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_width()).await
    }

    pub async fn fit_page(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_page()).await
    }

    pub async fn fit_all(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_all()).await
    }

    pub async fn fit_automatic(&self) -> Result<(), ViewerError> {
        self.read(|c| c.fit_automatic()).await
    }

    /// Degrees clockwise: 0, 90, 180 or 270.
    pub async fn set_view_rotation(&self, degrees: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.set_view_rotation(PageRotation(degrees))).await
    }

    pub async fn rotate_view_by(&self, delta: f64) -> Result<(), ViewerError> {
        self.read(move |c| c.rotate_view_by(delta)).await
    }

    // ── lifecycle ──

    /// Report the viewport in points. The first non-zero size places the
    /// document.
    pub async fn set_viewport_size(&self, size: Size) -> Result<(), ViewerError> {
        self.read(move |c| c.set_viewport_size(size.into())).await
    }

    pub async fn set_device_pixel_ratio(&self, ratio: f64) -> Result<(), ViewerError> {
        self.read(move |c| {
            c.set_device_pixel_ratio(ratio);
            Ok(())
        })
        .await
    }

    pub async fn refit(&self) -> Result<(), ViewerError> {
        self.read(|c| c.refit()).await
    }

    pub async fn view_state(&self) -> Result<StageViewState, ViewerError> {
        self.read(|c| c.get_view_state()?.try_into()).await
    }

    pub async fn lens_id(&self) -> Result<String, ViewerError> {
        self.read(|c| Ok(c.get_lens_id())).await
    }

    /// The provider is asked on the kernel thread when the stage first
    /// places. A thrown error is reported and counts as `None`.
    pub async fn provide_initial_view(
        &self,
        priority: f64,
        provider: Arc<dyn StageInitialViewProvider>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let report = b.report.clone();
                let off = open(&b.cap)?.provide_initial_view(priority, move || {
                    match provider.initial_view() {
                        Ok(view) => view.and_then(|v| match crate::StageViewState::try_from(v) {
                            Ok(v) => Some(v),
                            Err(error) => {
                                report(&error);
                                None
                            }
                        }),
                        Err(error) => {
                            report(&error.into());
                            None
                        }
                    }
                });
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    /// Register this lens's pan-scroll handler with the document's pointer
    /// hub, scoped to this lens, as the web's `<Stage>` does when it forwards
    /// its samples to the hub. A pan tool then drags the stage. `pan_fallback`
    /// (`None` is true) lets any other tool pan over the gaps between pages.
    /// `unsupported` without the interaction plugin.
    pub async fn register_scroll_handler(
        &self,
        pan_fallback: Option<bool>,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.bound
            .call(move |b| {
                let cap = open(&b.cap)?;
                let Some(hub) = b.hub.as_ref() else {
                    return Err(plugin_error(
                        code::UNSUPPORTED,
                        "stage",
                        "the scroll handler needs the interaction plugin",
                        None,
                        None,
                    ));
                };
                let handler = create_scroll_handler(
                    cap.clone(),
                    hub.clone(),
                    ScrollHandlerOptions { pan_fallback },
                );
                let off = hub.register_handler(
                    handler,
                    RegisterHandlerOptions { source: Some(cap.get_lens_id()) },
                );
                Ok(RegistrationHandle::new(post, move || off.call()))
            })
            .await
    }

    pub async fn place_initial(&self) -> Result<(), ViewerError> {
        self.read(|c| {
            c.place_initial();
            Ok(())
        })
        .await
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StagePageChangedEvent {
    pub page: Option<PageRef>,
    pub page_index: u32,
    pub previous_page_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageZoomChangedEvent {
    pub level: f64,
    pub previous_level: f64,
    pub mode: StageZoomMode,
}

/// Coalesced per camera write, not per frame.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageCameraChangedEvent {
    pub camera: Camera,
}

/// A tween or fling settled, or was stopped.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageMotionEndedEvent {
    pub camera: Camera,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StageSettingsChangedEvent {
    pub settings: StageSettings,
    pub changed: Vec<StageSettingKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StageViewportChangedEvent {
    pub size: Size,
}

// One event per call; boxing a variant would change the foreign enum.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum StageEvent {
    PageChanged { event: StagePageChangedEvent },
    ZoomChanged { event: StageZoomChangedEvent },
    CameraChanged { event: StageCameraChangedEvent },
    MotionEnded { event: StageMotionEndedEvent },
    SettingsChanged { event: StageSettingsChangedEvent },
    ViewportChanged { event: StageViewportChangedEvent },
}

#[uniffi::export(with_foreign)]
pub trait StageEventListener: Send + Sync {
    fn on_event(&self, event: StageEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

#[uniffi::export]
impl StageHandle {
    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already
    /// has). An event that does not convert is reported, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn StageEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.bound.fire(move |b| {
            let c = &b.cap;
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    macro_rules! relay {
                        ($hook:ident, |$e:ident| $event:expr) => {{
                            let listener = listener.clone();
                            c.$hook(move |$e| {
                                listener.on_event($event);
                                Ok(())
                            })
                        }};
                    }
                    let offs: Vec<Unsubscribe> = vec![
                        relay!(on_page_changed, |e| StageEvent::PageChanged {
                            event: StagePageChangedEvent {
                                page: e.page.map(Into::into),
                                page_index: to_u32(e.page_index, "a page index")?,
                                previous_page_index: to_u32(e.previous_page_index, "a page index")?,
                            },
                        }),
                        relay!(on_zoom_changed, |e| StageEvent::ZoomChanged {
                            event: StageZoomChangedEvent {
                                level: e.level,
                                previous_level: e.previous_level,
                                mode: e.mode.into(),
                            },
                        }),
                        relay!(on_camera_changed, |e| StageEvent::CameraChanged {
                            event: StageCameraChangedEvent { camera: e.camera.into() },
                        }),
                        relay!(on_motion_ended, |e| StageEvent::MotionEnded {
                            event: StageMotionEndedEvent { camera: e.camera.into() },
                        }),
                        relay!(on_settings_changed, |e| StageEvent::SettingsChanged {
                            event: StageSettingsChangedEvent {
                                settings: e.settings.clone().try_into()?,
                                changed: e.changed.iter().map(|k| (*k).into()).collect(),
                            },
                        }),
                        relay!(on_viewport_changed, |e| StageEvent::ViewportChanged {
                            event: StageViewportChangedEvent { size: e.size.into() },
                        }),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
