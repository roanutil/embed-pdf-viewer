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
//! The files follow the TypeScript package: [`contract`] (with `token.ts`
//! and `host-contract.ts`), [`model`], [`controller`] (the composition
//! root), and the areas under [`services`], [`read`], [`write`], [`sync`],
//! [`scripting`] and [`tools`], each an `impl` block on the one
//! [`FormCapability`]. `/scripting` is
//! [`scripting::controller::create_form_scripting_controller`].
//!
//! Engine DTOs (the snapshot, fields, effects, action trees, refs) stay
//! `serde_json::Value`, carried and printed as the engine wrote them.

#![allow(clippy::type_complexity)]

pub mod contract;
pub mod controller;
pub mod core;
pub mod field_selection;
pub mod model;
pub mod promise;
pub mod read;
pub mod scripting;
pub mod services;
pub mod sync;
pub mod tools;
pub mod value;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

use std::rc::Rc;

use epdf_annotation_core::AnnotationRef;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Promise;
use epdf_kernel::scope::Teardown;
use epdf_kernel::{AnyPlugin, Controller, Error, define};
use epdf_plugin_actions::{ACTIONS, ActionExecutorResult, FormEffectsResult};
use epdf_plugin_annotation::{ANNOTATION, AnnotationToolDef, Behavior};
use epdf_plugin_interaction::{
    INTERACTION, InteractionCapability, RegisterHandlerOptions, RegisterToolOptions, Tool,
};
use serde_json::{Value, json};

pub use contract::*;
pub use controller::{FormCapability, FormHostCapability, create_form_controller, key_of};
pub use core::fill_items::{
    ChoiceKind, FillControl, FillItem, ToggleKind, fill_item_for_widget, fill_items, project_widget,
};
pub use core::model::{
    Box, FieldKey, Model, Msg, WidgetHit, field_by_key, field_for_widget, field_key_of,
    initial_model, update, widget_at,
};
pub use field_selection::{FieldSelection, build_submit_entries, resolve_field_selection};
pub use model::{FormAction, FormState, form_reducer, initial_form_state};
pub use scripting::controller::{
    FormScriptingController, FormScriptingControllerOptions, TransactionBody, TransactionPort,
    create_form_scripting_controller,
};
pub use tools::definitions::{FORM_TOOLS, FormToolDef, PLACE_TAGS, form_tool_by_id};
pub use tools::handlers::{PlaceHandler, create_place_handler};

/// `createSerialMutationQueue`: the core serial queue under its historical
/// form-plugin name.
pub use epdf_kernel::serial_queue::SerialQueue as SerialMutationQueue;

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
        self.interaction.get_active_tool().enables("form-fill")
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

fn defer(cx: &Controller<FormState, FormAction>, f: impl FnOnce() + 'static) {
    let teardown: Teardown = std::boxed::Box::new(move || {
        f();
        Ok(None)
    });
    cx.defer(teardown);
}

/// `wireHub(ctx, form)`: the fill and edit tools, the palette through the
/// annotation registry when present, the place handler, the widget
/// behavior, and the actions-plane seams.
fn wire_hub(cx: &Controller<FormState, FormAction>, form: FormCapability) -> Result<(), Error> {
    let interaction = cx.get(&INTERACTION)?;
    let options = RegisterToolOptions::default;
    // Fill-only mode; the built-in pointer and pan tools carry 'form-fill'
    // themselves. `link-nav` keeps links navigating while filling.
    interaction
        .register_tool(Tool::new("form-fill", "default", ["form-fill", "link-nav"]), options())?;
    // Design mode's resting state: widgets select, move and resize.
    interaction.register_tool(
        Tool::new("form-edit", "default", ["annotation-edit", "annotation-marquee"]),
        options(),
    )?;
    let annotation = cx.try_get(&ANNOTATION)?;
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
                interaction.register_tool(Tool::new(t.id, t.cursor, PLACE_TAGS), options())?;
            }
        }
    }
    interaction.register_handler(
        Rc::new(create_place_handler(form.clone(), interaction.clone(), annotation.clone())),
        RegisterHandlerOptions::default(),
    );
    if let Some(annotation) = &annotation {
        annotation.register_behavior(Rc::new(FormWidgets { interaction: interaction.clone() }));
    }

    let Some(actions) = cx.try_get(&ACTIONS)? else { return Ok(()) };
    // The document-commit sink: script form effects land through the owner,
    // engine write and model reconciliation in one place.
    let sink_cx = cx.clone();
    let dispose = actions.register_form_commit_sink(Rc::new(move |effects: Vec<Value>| {
        Ok(match sink_cx.try_get(&FORM)? {
            None => typed_effects(Promise::resolved(write::script_effects::failed_batch(
                &effects, None,
            ))),
            Some(form) => typed_effects(form.commit_script_form_effects(effects)),
        })
    }));
    defer(cx, move || dispose());

    let resolver_cx = cx.clone();
    let dispose = actions.register_submit_resolver(Rc::new(move |intent, action_ctx, diagnose| {
        let Some(form) = resolver_cx.try_get(&FORM)? else {
            return Err(Error::new("form plugin unavailable"));
        };
        Ok(form.resolve_submit_dataset(intent.clone(), action_ctx.clone(), diagnose))
    }));
    defer(cx, move || dispose());

    // The executor threads its ActionContext: a lifecycle ResetForm's
    // recalculation surfaces as lifecycle.
    let executor_cx = cx.clone();
    let dispose = actions.register_executor(
        "reset-form",
        Rc::new(move |node: &Value, action_ctx| {
            let (cx, node, origin) = (executor_cx.clone(), node.clone(), action_ctx.origin);
            Ok(Promise::spawn(async move {
                if node.get("type").and_then(Value::as_str) != Some("reset-form") {
                    return Ok(ActionExecutorResult::Inert {
                        reason: "not a reset-form node".into(),
                    });
                }
                let Some(form) = cx.try_get(&FORM)? else {
                    return Ok(ActionExecutorResult::Inert {
                        reason: "form plugin unavailable".into(),
                    });
                };
                let fields = node.get("fields").and_then(Value::as_array).cloned();
                let exclude = value::truthy(node.get("exclude"));
                let result = form.reset_form_action(fields, exclude, Some(origin)).await?;
                if result.status == FormCommitStatus::Failed {
                    let error = result.error.map_or_else(|| "reset failed".into(), |e| e.message);
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
    defer(cx, move || dispose());
    Ok(())
}

/// `formPlugin(config)`: document-scoped, requires the interaction hub, and
/// uses the annotation and actions plugins when present. `connect` runs once
/// every capability exists: it subscribes to the document events, hydrates,
/// and registers this plugin's tools and handlers with the hub.
pub fn form_plugin(config: FormConfig) -> AnyPlugin {
    define::<FormState, FormAction>("form")
        .document_scoped()
        .requires(INTERACTION.any())
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_form_state)
        .reduce(form_reducer)
        .create(&FORM, move |cx| {
            let api = create_form_controller(cx.clone(), &config)?;
            let form = api.clone();
            let connect: ConnectFn = Rc::new(move || {
                form.connect();
                wire_hub(&cx, form.clone())
            });
            Ok((api, Some(connect)))
        })
        .build()
}
