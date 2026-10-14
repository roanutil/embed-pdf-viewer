//! `packages/plugin/annotation` in Rust: the pure `epdf-annotation-core`
//! model wired to the engine (optimistic create, patch and delete with
//! rollback, whole-document hydration with a cursor-ordered replay of
//! remote events) and to the interaction hub (ambient editing, drawing,
//! click placement, the footprint ghost, marquee selection). Text markup
//! lights up when a selection plugin is present; the hover feed when an
//! actions plugin is.
//!
//! The files follow the TypeScript package: [`contract`] (with `token.ts`),
//! [`host_contract`] (with `priorities.ts`), [`model`], [`controller`] (the
//! composition root), and the areas under [`services`], [`read`],
//! [`write`], [`sync`], [`comments`] and [`tools`], each an `impl` block on
//! the one [`AnnotationCapability`]. The TypeScript's five entry points
//! (`.`, `/contract`, `/contract/host`, `/authoring`, `/internal`) are the
//! public items here.
//!
//! DTOs stay `serde_json::Value`: the model carries each one verbatim
//! (`Annot.data`), and the engine calls send JSON.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod binary;
pub mod comments;
pub mod contract;
pub mod controller;
pub mod create_input;
pub mod host_contract;
pub mod model;
pub mod promise;
pub mod read;
pub mod repository;
pub mod rich_text;
pub mod services;
pub mod sync;
pub mod text_item;
pub mod tools;
pub mod value;
pub mod write;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{AnyPlugin, Controller, Error, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_interaction::{INTERACTION, RegisterHandlerOptions, RegisterToolOptions, Tool};
use epdf_plugin_selection::SELECTION;

pub use comments::build::{
    CommentThread, CommentThreadReview, ReviewStatus, build_comment_threads,
};
pub use comments::comments::CommentsLens;
pub use contract::*;
pub use controller::{AnnotationCapability, create_annotation_controller};
pub use create_input::{CreateAnnotationInput, geometry_from_input};
pub use epdf_annotation_core::{ClickPlacement, MIN_DRAG, props_for, resolve_click_placement};
pub use host_contract::*;
pub use model::{
    AnnotationAction, AnnotationState, annotation_reducer, default_chrome,
    initial_annotation_state, merge_chrome,
};
pub use read::chrome::HitView;
pub use repository::seam::{color_to_css, css_to_color, widget_appearance_from_props};
pub use repository::{
    annotation_key, from_dto, style_from_dto, to_create_draft, to_patch, to_scoped_patch,
};
pub use services::geometry::view_env;
pub use tools::definitions::{
    AnnotationToolDef, GhostPolicy, InkAuthoringOptions, PromptSourceSpec, ResolvedTool,
    SelectionAuthoring, StampSourceSpec, ToolRegistry, build_tool_registry, default_tools,
    is_touch_direct,
};
pub use tools::handlers::{
    create_draw_handler, create_edit_handler, create_ghost_handler, create_marquee_handler,
    create_place_handler,
};
pub use tools::markup_bridge::wire_markup;
pub use write::links::LinksLens;
pub use write::measurement::PageMeasurementViewport;

/// `AnnotationHostCapability`: the same object as the public capability.
pub type AnnotationHostCapability = AnnotationCapability;

/// `annotationPlugin(config)`: document-scoped, requires the interaction
/// hub, and uses the selection and actions plugins when present. `connect`
/// runs once every capability exists: it folds in confirmed document events
/// and registers this plugin's tools and pointer handlers with the hub.
pub fn annotation_plugin(config: AnnotationConfig) -> AnyPlugin {
    let initial = config.clone();
    define::<AnnotationState, AnnotationAction>("annotation")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(SELECTION.any())
        .optional(ACTIONS.any())
        .initial_state(move || initial_annotation_state(&initial))
        .reduce(annotation_reducer)
        .create(&ANNOTATION, move |cx| {
            let api = create_annotation_controller(cx.clone(), &config)?;
            let annotation = api.clone();
            let connect: ConnectFn = Rc::new(move || {
                sync::document_events::subscribe_document_events(&cx, annotation.clone());
                wire_hub(&cx, annotation.clone())
            });
            Ok((api, Some(connect)))
        })
        .build()
}

/// `wireHub(ctx)`: register every resolved tool and the five handlers, and
/// reset the gesture state on every tool change.
fn wire_hub(
    cx: &Controller<AnnotationState, AnnotationAction>,
    annotation: AnnotationCapability,
) -> Result<(), Error> {
    let interaction = cx.get(&INTERACTION)?;
    let selection = cx.try_get(&SELECTION)?;

    // The actions plane's commit sink.
    if let Some(actions) = cx.try_get(&ACTIONS)? {
        let sink_annotation = annotation.clone();
        let dispose = actions.register_annot_commit_sink(Rc::new(move |entries| {
            Ok(sink_annotation.commit_script_effects(entries))
        }));
        cx.defer(Box::new(move || {
            dispose();
            Ok(None)
        }));
    }

    for tool in annotation.list_resolved_tools() {
        if tool.enables("text-select") && selection.is_none() {
            continue;
        }
        // A sibling that added its tools through `registerTool` already
        // announced them to the hub.
        if interaction.has_tool(&tool.id) {
            continue;
        }
        let mut hub_tool = Tool::new(
            tool.id.clone(),
            tool.cursor.clone(),
            tool.enables.iter().map(String::as_str),
        );
        hub_tool.touch_direct = Some(is_touch_direct(&tool.enables));
        interaction.register_tool(hub_tool, RegisterToolOptions::default())?;
        if let Some(defaults) = tool.defaults.clone() {
            annotation.set_tool_defaults(&tool.id, defaults);
        }
    }

    let options = RegisterHandlerOptions::default;
    interaction.register_handler(Rc::new(create_place_handler(annotation.clone())), options());
    interaction.register_handler(
        Rc::new(create_ghost_handler(annotation.clone(), interaction.clone())),
        options(),
    );
    interaction.register_handler(
        Rc::new(create_edit_handler(annotation.clone(), interaction.clone())),
        options(),
    );
    interaction.register_handler(Rc::new(create_marquee_handler(annotation.clone())), options());
    interaction.register_handler(
        Rc::new(create_draw_handler(annotation.clone(), interaction.clone())),
        options(),
    );
    let (a, ix) = (annotation.clone(), interaction.clone());
    let _ = interaction.on_tool_changed(move |_| {
        a.cancel();
        a.clear_ghost();
        a.prune_engaged_selection();
        // Leaving the armed stamp's own tool drops the payload; the legacy
        // `annotation-stamp` tag still holds one.
        let active = ix.get_active_tool();
        if active.id != ARMED_STAMP_TOOL_ID && !active.enables("annotation-stamp") {
            a.disarm_stamp();
        }
        Ok(())
    });

    if let Some(selection) = selection {
        wire_markup(annotation, selection, interaction);
    }
    Ok(())
}
