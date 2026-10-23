//! `packages/core/ui` in Rust: the structure-only bar schema, the toolbar fit
//! solver, the overflow projection, the viewer's layout with its edits and
//! its validation, and the shortcut parser. DOM-free, kernel-free and
//! framework-free, like the TypeScript; it depends on `epdf-kernel` only for
//! its `Error` shape and JavaScript number printing.
//!
//! Modules mirror the TypeScript files. Every value type serializes to the
//! JSON the TypeScript object prints, so the vectors under
//! `tooling/plugin-conformance/vectors/core/ui-*.json` compare them directly.
//! [`js`] holds the JavaScript semantics the port reproduces: object key
//! order, `Map` order, non-finite numbers, `trim` and UTF-16 lengths.
//! [`value`] is a whole JavaScript value, which the layout's plain data is.

pub mod js;
pub mod layout;
pub mod layout_compile;
pub mod layout_validate;
pub mod projection;
pub mod schema;
pub mod shortcuts;
pub mod solver;
pub mod suggestions;
pub mod value;

#[cfg(feature = "harness")]
pub mod harness;

pub use js::{JsMap, Record};
pub use layout::{
    AddItemOptions, BarEdge, BarPlace, EXPANDED_MIN_WIDTH_PX, FloatPosition, Layout, LayoutEnv,
    LayoutFunction, LayoutInput, LayoutOption, LayoutResult, MEDIUM_MIN_WIDTH_PX,
    PanelPresentation, RemoveItemOptions, SelectionTarget, TabsVisibility, TextDirection,
    ViewerSize, create_layout, is_layout, item_command_of, item_id_of, layout_warnings_of,
    panel_presentations_of, panel_width_fields_of, resolve_layout, same_layout_env, size_of,
    with_layout_warning,
};
pub use layout_compile::{
    VIEW_MODE_COMMAND, edge_of, host_of, mode_command_of, modes_of, panel_command_of,
};
pub use layout_validate::{HasCommand, LayoutKnownIds, validate_layout};
pub use projection::{
    MenuTarget, OverflowRow, OverflowSection, ResolveMenuTarget, SectionRole, StripGroup,
    project_overflow, project_shed, project_strip,
};
pub use schema::{
    BarChild, BarGroup, BarItem, BarSchema, BarSections, Collapse, CustomItem, CustomOpts,
    DEFAULT_IMPORTANCE, GroupOpts, GroupRole, Importance, ItemOpts, NormalizedBar, NormalizedGroup,
    NormalizedSection, NormalizedUnit, PINNED, SectionName, Variant, command_of_unit, custom,
    filter_bar, group, has_visible_units, is_standalone_unit, item, normalize_bar,
};
pub use shortcuts::{
    KeyStroke, ParsedShortcut, ShortcutOptions, format_shortcut, match_shortcut, parse_shortcut,
    same_shortcut,
};
pub use solver::{FitMetrics, FitResult, GroupAssignment, UnitAssignment, solve};
pub use suggestions::{closest_match, did_you_mean, edit_distance};
pub use value::JsValue;
