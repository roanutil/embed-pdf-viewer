//! `packages/plugin/annotation` in Rust: the pure `epdf-annotation-core`
//! model wired to the engine (optimistic create, patch and delete with
//! rollback, whole-document hydration with a cursor-ordered replay of
//! remote events) and to the interaction hub (ambient editing, drawing,
//! click placement, the footprint ghost, marquee selection). Text markup
//! lights up when a selection plugin is present; the hover feed when an
//! actions plugin is.
//!
//! The TypeScript package has five entry points, and this crate exports
//! what each does:
//!
//! - the root and `/contract`: [`annotation_plugin`], [`ANNOTATION`] and the
//!   public half of [`AnnotationCapability`], the repository's DTO helpers
//!   ([`from_dto`], [`to_create_draft`], [`to_patch`], [`ref_key`],
//!   [`style_from_dto`]), [`DEFAULT_CHROME`](reducer::default_chrome) and
//!   [`default_tools`];
//! - `/contract/host`: the same capability's host half (render
//!   projections, pointer intents, hydration, behaviors, the script commit
//!   door) and [`AnnotationState`] / [`AnnotationAction`];
//! - `/authoring`: [`widget_appearance_from_props`] and the core's
//!   placement layer, re-exported;
//! - `/internal`: [`create_annotation_capability`] and
//!   [`build_tool_registry`].
//!
//! DTOs stay `serde_json::Value`: the model carries each one verbatim
//! (`Annot.data`), and the engine calls send JSON.

pub mod binary;
pub mod capability;
pub mod effects;
pub mod handler;
pub mod hover_feed;
pub mod markup;
pub mod placement;
pub mod promise;
pub mod reducer;
pub mod repository;
pub mod text_item;
pub mod threads;
pub mod tools;
pub mod types;
pub mod value;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_interaction::{INTERACTION, Tool};
use epdf_plugin_selection::SELECTION;

pub use capability::{AnnotationCapability, HoverAt, create_annotation_capability, view_env};
pub use effects::register_annotation_effects;
pub use epdf_annotation_core::{MIN_DRAG, ClickPlacement, props_for, resolve_click_placement};
pub use handler::{
    create_draw_handler, create_edit_handler, create_ghost_handler, create_marquee_handler,
    create_place_handler,
};
pub use markup::wire_markup;
pub use reducer::{annotation_reducer, default_chrome, initial_annotation_state, merge_chrome};
pub use repository::seam::{color_to_css, css_to_color, widget_appearance_from_props};
pub use repository::{from_dto, ref_key, style_from_dto, to_create_draft, to_patch, to_scoped_patch};
pub use threads::{CommentThread, CommentThreadReview, ReviewStatus, build_comment_threads};
pub use tools::{
    AnnotationToolDef, GhostPolicy, InkAuthoringOptions, PromptSourceSpec, ResolvedTool,
    SelectionAuthoring, StampSourceSpec, ToolRegistry, build_tool_registry, default_tools,
    is_touch_direct,
};
pub use types::*;

/// `annotationPlugin(config)`: document-scoped, requires the interaction
/// hub, and uses the selection and actions plugins when present.
pub fn annotation_plugin(config: AnnotationConfig) -> AnyPlugin {
    let initial = config.clone();
    let for_capability = config.clone();
    define::<AnnotationState, AnnotationAction>("annotation")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(SELECTION.any())
        .optional(ACTIONS.any())
        .initial_state(move || initial_annotation_state(&initial))
        .reduce(annotation_reducer)
        .capability(&ANNOTATION, move |ctx| create_annotation_capability(ctx, &for_capability))
        .effects(register_annotation_effects)
        .init(|ctx| {
            let interaction = ctx.get(&INTERACTION)?;
            let annotation = ctx.get(&ANNOTATION)?;
            let selection = ctx.try_get(&SELECTION)?;

            if let Some(actions) = ctx.try_get(&ACTIONS)? {
                let sink_annotation = annotation.clone();
                let dispose = actions.register_annot_commit_sink(Rc::new(move |entries| {
                    Ok(sink_annotation.commit_script_effects(entries))
                }));
                ctx.cleanup_fn(move || dispose());
            }

            for tool in annotation.tools() {
                if tool.enables("text-select") && selection.is_none() {
                    continue;
                }
                let mut hub_tool =
                    Tool::new(tool.id.clone(), tool.cursor.clone(), tool.enables.iter().map(String::as_str));
                hub_tool.touch_direct = Some(is_touch_direct(&tool.enables));
                interaction.register_tool(hub_tool);
                if let Some(defaults) = tool.defaults.clone() {
                    annotation.set_defaults(&tool.preset, defaults);
                }
            }

            interaction.register_handler(Rc::new(create_place_handler(annotation.clone())), None);
            interaction.register_handler(
                Rc::new(create_ghost_handler(annotation.clone(), interaction.clone())),
                None,
            );
            interaction.register_handler(
                Rc::new(create_edit_handler(annotation.clone(), interaction.clone())),
                None,
            );
            interaction.register_handler(Rc::new(create_marquee_handler(annotation.clone())), None);
            interaction.register_handler(
                Rc::new(create_draw_handler(annotation.clone(), interaction.clone())),
                None,
            );
            let (a, ix) = (annotation.clone(), interaction.clone());
            interaction.on_tool_change(move || {
                a.cancel();
                a.clear_ghost();
                a.prune_engaged_selection();
                if !ix.active_tool().enables("annotation-stamp") {
                    a.disarm_stamp();
                }
            });

            if let Some(selection) = selection {
                wire_markup(annotation, selection, interaction);
            }
            Ok(None)
        })
        .build()
}
