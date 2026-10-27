//! `settings.ts`: the defaults, the default responsive rules, and the
//! registry that classifies how a change to each setting affects the view.
//!
//! The TypeScript derives everything from `SETTINGS_EFFECT`, a record keyed
//! by setting name, and `SETTING_KEYS`, its key order. Here the keys are the
//! [`SettingKey`] enum in that order, and [`for_each_setting!`] expands one
//! arm per field wherever the TypeScript loops over the keys, so a new
//! setting added to [`StageSettings`] without a row here fails to compile.

use epdf_geometry::{NO_FRAME, PageRotation};
use epdf_stage_core::{
    Align, AlignValue, Alignment, AlignmentValue, Direction, SizingMode, SpreadMode, ZoomMode,
    ZoomSpec,
};
use serde::{Deserialize, Serialize};

use crate::contract::*;
use crate::model::object_is;

pub mod area;

/// `DEFAULT_SETTINGS`: a document-reading feel.
pub fn default_settings() -> StageSettings {
    StageSettings {
        flow: FlowMode::Continuous,
        layout: LayoutKind::Vertical,
        spread: SpreadMode::None,
        sizing: SizingMode::Intrinsic,
        columns: GridColumns::Policy(ColumnsPolicy::Square),
        bounded: true,
        padding: 24.0,
        gap: Gap::World(16.0),
        page_frame: NO_FRAME,
        direction: Direction::Ltr,
        fit_align: Alignment { x: Align::Center, y: Align::Center },
        arrival_align: ArrivalAlignment {
            x: ArrivalAlignValue::named(Align::Start),
            y: ArrivalAlignValue::named(Align::Start),
        },
        zoom_align: AlignmentValue {
            x: AlignValue::Named(Align::Center),
            y: AlignValue::Named(Align::Center),
        },
        anchor_align: AlignmentValue {
            x: AlignValue::Named(Align::Start),
            y: AlignValue::Named(Align::Start),
        },
        view_rotation: PageRotation::DEG_0,
        zoom: ZoomSpec::Mode { mode: ZoomMode::Automatic },
        scroll_behavior: ScrollBehaviorKind::Smooth,
        interaction: true,
        pan_fallback: true,
        zoom_gestures: true,
        // Web: 1 PDF point is 96/72 CSS px.
        view_units_per_point: 96.0 / 72.0,
    }
}

/// `DEFAULT_RESPONSIVE`: a compact container (at most 600 px wide) gets the
/// thin phone gutter, and the rule is queryable as `'compact'`.
pub fn default_responsive() -> Vec<ResponsiveRule> {
    vec![ResponsiveRule {
        name: Some("compact".into()),
        when: RuleWhen::Query(BoxQuery { max_width: Some(600.0), ..Default::default() }),
        settings: Some(StageSettingsPatch { padding: Some(4.0), ..Default::default() }),
    }]
}

/// How a change to a setting affects the view, weakest to strongest in
/// the rank `syncResponsive` escalates by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingEffect {
    /// Guides future verbs and the surface binding only.
    None,
    /// Pure clamp policy: re-clamp the current camera in place.
    Reclamp,
    /// Re-resolves zoom: re-applies the anchor without invalidating.
    Refit,
    /// A layout input: invalidates the scene, then re-applies the anchor.
    Scene,
    /// Crosses the flow boundary: re-place onto the cursor's page.
    Reflow,
}

/// Expand `$m!(key_variant, field, effect)` once per setting, in
/// `SETTING_KEYS` order.
#[macro_export]
macro_rules! for_each_setting {
    ($m:ident) => {
        $m!(Flow, flow, Reflow);
        $m!(Layout, layout, Scene);
        $m!(Spread, spread, Scene);
        $m!(Sizing, sizing, Scene);
        $m!(Columns, columns, Scene);
        $m!(Bounded, bounded, Reclamp);
        $m!(Padding, padding, Reclamp);
        $m!(Gap, gap, Scene);
        $m!(PageFrame, page_frame, Scene);
        $m!(Direction, direction, Scene);
        $m!(FitAlign, fit_align, Reclamp);
        $m!(ArrivalAlign, arrival_align, None);
        $m!(ZoomAlign, zoom_align, None);
        $m!(AnchorAlign, anchor_align, None);
        $m!(ViewRotation, view_rotation, Scene);
        $m!(Zoom, zoom, Refit);
        $m!(ScrollBehavior, scroll_behavior, None);
        $m!(Interaction, interaction, None);
        $m!(PanFallback, pan_fallback, None);
        $m!(ZoomGestures, zoom_gestures, None);
        $m!(ViewUnitsPerPoint, view_units_per_point, Scene);
    };
}

/// A setting's name, in `SETTING_KEYS` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingKey {
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

/// `SETTING_KEYS`.
pub const SETTING_KEYS: [SettingKey; 21] = [
    SettingKey::Flow,
    SettingKey::Layout,
    SettingKey::Spread,
    SettingKey::Sizing,
    SettingKey::Columns,
    SettingKey::Bounded,
    SettingKey::Padding,
    SettingKey::Gap,
    SettingKey::PageFrame,
    SettingKey::Direction,
    SettingKey::FitAlign,
    SettingKey::ArrivalAlign,
    SettingKey::ZoomAlign,
    SettingKey::AnchorAlign,
    SettingKey::ViewRotation,
    SettingKey::Zoom,
    SettingKey::ScrollBehavior,
    SettingKey::Interaction,
    SettingKey::PanFallback,
    SettingKey::ZoomGestures,
    SettingKey::ViewUnitsPerPoint,
];

/// `SETTINGS_EFFECT[key]`.
pub fn settings_effect(key: SettingKey) -> SettingEffect {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if key == SettingKey::$key {
                return SettingEffect::$effect;
            }
        };
    }
    for_each_setting!(arm);
    unreachable!("every SettingKey has a row")
}

/// Whether a patch carries `key` (`patch[key] !== undefined`).
pub fn patch_has(patch: &StageSettingsPatch, key: SettingKey) -> bool {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if key == SettingKey::$key {
                return patch.$field.is_some();
            }
        };
    }
    for_each_setting!(arm);
    false
}

/// The keys a patch carries, in `SETTING_KEYS` order.
pub fn patch_keys(patch: &StageSettingsPatch) -> Vec<SettingKey> {
    SETTING_KEYS.iter().copied().filter(|&k| patch_has(patch, k)).collect()
}

/// `!Object.is(current, value)` for one setting value. A setting that is a
/// JavaScript object is never the same as the one a patch carries.
pub(crate) trait Identity {
    fn same_object(&self, other: &Self) -> bool;
}

macro_rules! primitive_identity {
    ($($t:ty),*) => {$(
        impl Identity for $t {
            fn same_object(&self, other: &Self) -> bool {
                self == other
            }
        }
    )*};
}
primitive_identity!(
    FlowMode,
    LayoutKind,
    SpreadMode,
    SizingMode,
    Direction,
    ScrollBehaviorKind,
    bool
);

impl Identity for f64 {
    fn same_object(&self, other: &Self) -> bool {
        object_is(*self, *other)
    }
}

impl Identity for PageRotation {
    fn same_object(&self, other: &Self) -> bool {
        object_is(self.0, other.0)
    }
}

impl Identity for GridColumns {
    fn same_object(&self, other: &Self) -> bool {
        match (self, other) {
            (GridColumns::Count(a), GridColumns::Count(b)) => object_is(*a, *b),
            (GridColumns::Policy(a), GridColumns::Policy(b)) => a == b,
            _ => false,
        }
    }
}

impl Identity for Gap {
    fn same_object(&self, other: &Self) -> bool {
        match (self, other) {
            (Gap::World(a), Gap::World(b)) => object_is(*a, *b),
            _ => false,
        }
    }
}

macro_rules! object_identity {
    ($($t:ty),*) => {$(
        impl Identity for $t {
            fn same_object(&self, _other: &Self) -> bool {
                false
            }
        }
    )*};
}
object_identity!(
    epdf_geometry::PageFrame,
    PartialPageFrame,
    Alignment,
    ArrivalAlignment,
    AlignmentValue,
    ZoomSpec
);

/// Whether `patchSettings(state, patch)` lands anything: some carried key
/// is not `Object.is` the current value.
pub(crate) fn patch_changes(current: &StageSettings, patch: &StageSettingsPatch) -> bool {
    macro_rules! arm {
        (PageFrame, $field:ident, $effect:ident) => {
            if patch.$field.is_some() {
                return true;
            }
        };
        ($key:ident, $field:ident, $effect:ident) => {
            if let Some(value) = &patch.$field
                && !current.$field.same_object(value)
            {
                return true;
            }
        };
    }
    for_each_setting!(arm);
    false
}

/// `settingsEqual`: field by field, one level of structural equality, which
/// for these types is `PartialEq` (see [`crate::responsive::eq_setting`]).
pub fn settings_equal(a: &StageSettings, b: &StageSettings) -> bool {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if !crate::responsive::eq_setting(&a.$field, &b.$field) {
                return false;
            }
        };
    }
    for_each_setting!(arm);
    true
}

/// `eqSetting(a[key], b[key])` for one key.
pub fn setting_eq(a: &StageSettings, b: &StageSettings, key: SettingKey) -> bool {
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if key == SettingKey::$key {
                return crate::responsive::eq_setting(&a.$field, &b.$field);
            }
        };
    }
    for_each_setting!(arm);
    true
}

/// `pickSettings`: the settings slice of a larger object. The typed state
/// keeps its settings as a field, so this is a copy.
pub fn pick_settings(src: &StageSettings) -> StageSettings {
    src.clone()
}
