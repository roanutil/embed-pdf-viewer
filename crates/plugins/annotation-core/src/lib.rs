//! `packages/core/annotation` in Rust: the pure annotation model. The kind
//! table, the flat property vocabulary, the reducer (`update(model, msg)`
//! returning the next model and effects), per-kind content-space geometry,
//! cloudy borders, hit testing, the painted scene and the view selectors.
//! No DOM, engine or framework.
//!
//! Modules mirror the TypeScript files and names (`geomHit` is
//! [`geometry::geom_hit`]). Every value type serializes to the JSON the
//! TypeScript object prints, so the vectors under
//! `tooling/plugin-conformance/vectors/core/annotation-*.json` compare them
//! directly. JavaScript numerics go through [`js`] (`Math.min`/`max` NaN
//! propagation, `Math.round`, V8's `hypot`, `toFixed`, `Number(string)`,
//! `Number#toString`).
//!
//! Where the TypeScript returns the model it was given, [`update::update`]
//! returns `None`: a store skips its change notification for that branch.

pub mod anchor;
pub mod cloudy;
pub mod endings;
pub mod flags;
pub mod geometry;
pub mod group;
pub mod hit;
pub mod ink;
pub mod js;
pub mod kinds;
pub mod links;
pub mod placement;
pub mod plane;
pub mod props;
pub mod scene;
pub mod snap;
pub mod types;
pub mod update;
pub mod view;

#[cfg(feature = "harness")]
pub mod harness;

pub use anchor::{
    AnchorMode, AnchoredBox, anchor_mode_of, anchor_of, anchored_box, anchored_geom,
    anchored_stroke_width, unanchored_geom,
};
pub use cloudy::{cloudy_border_extent, cloudy_path, cloudy_poly_path};
pub use flags::{
    DRAWN_FLAGS, FLAG_KEYS, FlagBearer, FlagFields, NO_ANNOTATION_FLAGS, annot_contents_editable,
    annot_deletable, annot_interactive, annot_transformable, flags_equal, interactive, merge_flags,
    viewable,
};
pub use geometry::*;
pub use group::{GroupCaps, expand_groups, group_caps, group_key_of, group_members};
pub use hit::{
    Target, can_move, cursor_at, group_union_bounds, hit_test, is_selectable, paint_order,
};
pub use ink::straighten_ink_stroke;
pub use kinds::{AnnotationKind, KINDS, KindCaps, PropSpec, caps_for, is_markup, props_for};
pub use links::{link_children_of, link_of};
pub use placement::{
    ClickPlacement, PlacementOptions, clamp_rect_to_box, click_create_geom, resolve_click_placement,
};
pub use plane::{PlaneFields, is_attached_link, is_conversation_only, is_substrate_only};
pub use props::{
    PropValue, apply_props, initial_text_style, kind_takes_link, read_prop, shared_props,
    style_from_props, text_style_from_props,
};
pub use scene::{blend_for, layout_redact_label, scene};
pub use snap::{SnapResult, compute_move_snap};
pub use types::*;
pub use update::{
    CalloutDraft, MIN_DRAG, RotateDelta, annots_in_box, apply, callout_box, callout_upright_rot,
    defaults_for, initial_model, initial_style, rotate_draft_delta, update,
};
pub use view::{
    SelectionAnchor, TextBox, chrome, creation_draft_anchor, page_items, selected_items,
    selection_anchor, selection_bounds_on_page, selection_knob, text_boxes,
};
