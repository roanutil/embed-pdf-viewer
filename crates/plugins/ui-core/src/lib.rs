//! `packages/core/ui` in Rust: the structure-only chrome schema, the toolbar
//! fit solver, the overflow projection, the schema transforms and the
//! shortcut parser. DOM-free, kernel-free and framework-free, like the
//! TypeScript; it depends on `epdf-kernel` only for its `Error` shape.
//!
//! Modules mirror the TypeScript files. Every value type serializes to the
//! JSON the TypeScript object prints, so the vectors under
//! `tooling/plugin-conformance/vectors/core/ui-*.json` compare them directly.
//! [`js`] holds the JavaScript semantics the port reproduces: object key
//! order, `Map` order, non-finite numbers, `trim` and UTF-16 lengths.

pub mod helpers;
pub mod js;
pub mod projection;
pub mod schema;
pub mod shortcuts;
pub mod solver;
pub mod transforms;

#[cfg(feature = "harness")]
pub mod harness;

pub use helpers::{CHROME_HELPER_KEYS, CHROME_HELPERS, ChromeHelpers};
pub use js::{JsMap, Record};
pub use projection::{
    MenuTarget, OverflowRow, OverflowSection, ResolveMenuTarget, SectionRole, StripGroup,
    project_overflow, project_shed, project_strip,
};
pub use schema::{
    BarChild, BarGroup, BarItem, BarSchema, BarSections, ChromeSchema, Collapse, CustomItem,
    CustomOpts, DEFAULT_IMPORTANCE, FrameSchema, GroupOpts, GroupOptsOrItems, GroupRole,
    Importance, ItemOpts, MenuSchema, MenuSection, NormalizedBar, NormalizedGroup,
    NormalizedSection, NormalizedUnit, PINNED, SectionName, TabsVisibility, ToolbarPosition,
    Variant, custom, define_chrome, filter_bar, group, item, normalize_bar, validate_chrome,
};
pub use shortcuts::{
    KeyStroke, ParsedShortcut, ShortcutOptions, format_shortcut, match_shortcut, parse_shortcut,
};
pub use solver::{FitMetrics, FitResult, GroupAssignment, UnitAssignment, solve};
pub use transforms::{AddItemSpec, add_item, remove_items, replace_item};
