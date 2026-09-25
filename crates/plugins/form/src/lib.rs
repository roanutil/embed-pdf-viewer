//! `packages/plugin/form` in Rust: the field plane. Document-scoped and
//! requiring the interaction hub, it keeps a reactive model of the engine's
//! form snapshot, projects fill controls for each page, commits value
//! writes, resets, interchange and field design through `doc.forms.*`, and
//! registers the palette tools and the draw-to-place handler. Fill mode works
//! with no annotation plugin; with one, a behavior keeps widgets inert while
//! the active tool carries `form-fill`. Beside the actions plugin it
//! registers the `reset-form` executor, the submit dataset resolver and the
//! script commit sink, and runs value commits through the K/V/C/F
//! transaction in the actions plugin's script realm.
//!
//! The TypeScript package has five entry points. The root and `/contract`
//! are [`form_plugin`], [`FORM`], [`FormCapability`] and the types;
//! `/contract/host` and `/internal` are the same token and capability (the
//! host members say so in their docs); `/scripting` is
//! [`scripting::create_form_scripting_controller`] and
//! [`scripting::FormScriptingController`].
//!
//! Engine DTOs (the snapshot, fields, effects, action trees) stay
//! `serde_json::Value`, carried and printed as the engine wrote them.

#![allow(clippy::type_complexity)]

pub mod capability;
pub mod core;
pub mod effects;
pub mod field_selection;
pub mod handler;
pub mod promise;
pub mod reducer;
pub mod scripting;
pub mod tools;
pub mod types;
pub mod value;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_annotation_core::AnnotationRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, Ctx, Error, define};
use epdf_plugin_actions::{ACTIONS, ActionExecutorResult, FormEffectsResult};
use epdf_plugin_annotation::{ANNOTATION, AnnotationToolDef, Behavior};
use epdf_plugin_interaction::{INTERACTION, InteractionCapability, Tool};
use serde_json::{Value, json};

pub use capability::{FormCapability, create_form_capability, ref_key_of};
pub use core::fill_items::{
    ChoiceKind, FillControl, FillItem, ToggleKind, fill_item_for_widget, fill_items, project_widget,
};
pub use core::model::{
    Box, FieldKey, Model, Msg, field_by_key, field_for_widget, field_key_of, initial_model, update,
};
pub use effects::register_form_effects;
pub use field_selection::{FieldSelection, build_submit_entries, resolve_field_selection};
pub use handler::{PlaceHandler, create_place_handler};
pub use reducer::{form_reducer, initial_form_state};
pub use scripting::{
    FormScriptingController, FormScriptingControllerOptions, FormScriptingHost, TransactionBody,
    TransactionPort, create_form_scripting_controller, create_form_scripting_host,
};
pub use tools::{FORM_TOOLS, FormToolDef, PLACE_TAGS, form_tool_by_id};
pub use types::*;

/// The `form-widgets` behavior: widgets are geometry-inert while the active
/// tool carries `form-fill` (the built-in pointer and pan tools do), and
/// ordinary annotations otherwise.
struct FormWidgets {
    interaction: InteractionCapability,
}

impl Behavior for FormWidgets {
    fn id(&self) -> &str {
        "form-widgets"
    }
    fn matches(&self, subtype: &str, _ref: Option<&AnnotationRef>) -> bool {
        subtype.starts_with("widget")
    }
    fn engaged(&self) -> bool {
        self.interaction.active_tool().enables("form-fill")
    }
}

/// A JSON promise read as a typed one in the same moment, as a JavaScript
/// value needs no conversion.
fn typed_effects(promise: Promise<Value>) -> Promise<FormEffectsResult> {
    let (typed, resolver) = Promise::pending();
    promise.on_settle(move |result| {
        resolver.settle(result.clone().and_then(epdf_kernel::engine::decode));
    });
    typed
}

fn all_failed(effects: &[Value]) -> FormEffectsResult {
    let results: Vec<Value> = (0..effects.len())
        .map(|index| json!({ "index": index, "status": "failed", "fields": [], "changedWidgets": [] }))
        .collect();
    epdf_kernel::engine::decode(json!({ "results": results }))
        .unwrap_or(FormEffectsResult { results: Vec::new() })
}

fn init(ctx: Ctx<types::FormState, types::FormAction>) -> Result<Option<Promise<()>>, Error> {
    let interaction = ctx.get(&INTERACTION)?;
    let form = ctx.get(&FORM)?;
    // Fill-only mode; the built-in pointer and pan tools carry 'form-fill'
    // themselves. `link-nav` keeps links navigating while filling.
    interaction.register_tool(Tool::new("form-fill", "default", ["form-fill", "link-nav"]));
    // Design mode's resting state: widgets select, move and resize.
    interaction.register_tool(Tool::new(
        "form-edit",
        "default",
        ["annotation-edit", "annotation-marquee"],
    ));
    let annotation = ctx.try_get(&ANNOTATION)?;
    for t in FORM_TOOLS.iter() {
        match &annotation {
            Some(annotation) => {
                let def: AnnotationToolDef = epdf_kernel::js::from_value(&json!({
                    "id": t.id,
                    "subtype": t.visual_kind,
                    "cursor": t.cursor,
                    "enables": PLACE_TAGS,
                    "clickCreate": t.click_create,
                    "defaults": t.defaults,
                }))?;
                annotation.register_tool(def)?;
            }
            None => {
                interaction.register_tool(Tool::new(t.id, t.cursor, PLACE_TAGS));
            }
        }
    }
    interaction.register_handler(
        Rc::new(create_place_handler(form.clone(), interaction.clone(), annotation.clone())),
        None,
    );
    if let Some(annotation) = &annotation {
        annotation.register_behavior(Rc::new(FormWidgets { interaction: interaction.clone() }));
    }

    if let Some(actions) = ctx.try_get(&ACTIONS)? {
        // The document-commit sink: script form effects land through the
        // owner, engine write and model reconciliation in one place.
        let sink_ctx = ctx.clone();
        let dispose = actions.register_form_commit_sink(Rc::new(move |effects: Vec<Value>| {
            Ok(match sink_ctx.try_get(&FORM)? {
                None => Promise::resolved(all_failed(&effects)),
                Some(form) => typed_effects(form.commit_script_form_effects(effects)),
            })
        }));
        ctx.cleanup_fn(move || dispose());

        let resolver_ctx = ctx.clone();
        let dispose =
            actions.register_submit_resolver(Rc::new(move |intent, action_ctx, diagnose| {
                let Some(form) = resolver_ctx.try_get(&FORM)? else {
                    return Err(Error::new("form plugin unavailable"));
                };
                Ok(form.resolve_submit_dataset(intent.clone(), action_ctx.clone(), diagnose))
            }));
        ctx.cleanup_fn(move || dispose());

        // The executor threads its ActionContext: a lifecycle ResetForm's
        // recalculation surfaces as lifecycle.
        let executor_ctx = ctx.clone();
        let dispose = actions.register_executor(
            "reset-form",
            Rc::new(move |node: &Value, action_ctx| {
                let (ctx, node, origin) = (executor_ctx.clone(), node.clone(), action_ctx.origin);
                Ok(Promise::spawn(async move {
                    if node.get("type").and_then(Value::as_str) != Some("reset-form") {
                        return Ok(ActionExecutorResult::Inert {
                            reason: "not a reset-form node".into(),
                        });
                    }
                    let Some(form) = ctx.try_get(&FORM)? else {
                        return Ok(ActionExecutorResult::Inert {
                            reason: "form plugin unavailable".into(),
                        });
                    };
                    let fields = node.get("fields").and_then(Value::as_array).cloned();
                    let exclude = value::truthy(node.get("exclude"));
                    let result = form.reset_form_action(fields, exclude, Some(origin)).await?;
                    if result.status == FormCommitStatus::Failed {
                        let error =
                            result.error.map_or_else(|| "reset failed".into(), |e| e.message);
                        return Ok(ActionExecutorResult::Failed { error });
                    }
                    if result.effects_result.is_none() {
                        let reason = result.diagnostics.first().map_or_else(
                            || "reset-form resolved no fields".into(),
                            |d| d.message.clone(),
                        );
                        return Ok(ActionExecutorResult::Inert { reason });
                    }
                    Ok(ActionExecutorResult::Executed)
                }))
            }),
        );
        ctx.cleanup_fn(move || dispose());
    }
    Ok(None)
}

/// `formPlugin(options)`: document-scoped, requires the interaction hub,
/// and uses the annotation and actions plugins when present.
pub fn form_plugin(config: FormPluginConfig) -> AnyPlugin {
    define::<FormState, FormAction>("form")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_form_state)
        .reduce(form_reducer)
        .capability(&FORM, move |ctx| create_form_capability(ctx, &config))
        .effects(register_form_effects)
        .init(init)
        .build()
}
