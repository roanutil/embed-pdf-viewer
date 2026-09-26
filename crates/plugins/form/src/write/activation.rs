//! `write/activation.ts`: widget activation and the widget DOM-event feed
//! into the actions plane. A click first offers the widget's `/A` tree to
//! the dispatcher; only a tree-less widget falls back to the form's own
//! scripted activation.

use epdf_annotation_core::AnnotationRef as TypedAnnotationRef;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_plugin_actions::{
    ACTIONS, ActionOrigin, ActionResultStatus, ActionSource, ActionTrigger, ActionsCapability,
    HoverEvents, HoverPump, HoverTarget, PdfAnnotationEventKind, create_hover_pump,
};
use serde_json::Value;

use crate::contract::{FormCommitResult, FormCommitStatus, WidgetActivationResult};
use crate::controller::FormCapability;
use crate::core::model::field_for_widget;
use crate::promise::await_engine;
use crate::read::fields::widget_object_of;
use crate::value::{get, str_of, strict_eq, truthy};

/// `pageRefsEqual(a, b)` over two JSON page refs.
fn same_page(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (Some(left), Some(right)) = (left, right) else { return false };
    str_of(left, "kind") == str_of(right, "kind")
        && strict_eq(get(left, "pageObjectNumber"), get(right, "pageObjectNumber"))
}

/// `sameAnnotationRef(left, right)`. An index ref's `revision` is compared
/// with `===`, which never holds for a revision object read from JSON.
fn same_annotation_ref(left: &Value, right: &Value) -> bool {
    let eq = |k: &str| strict_eq(get(left, k), get(right, k));
    if !eq("kind") || !same_page(get(left, "page"), get(right, "page")) {
        return false;
    }
    match (str_of(left, "kind"), str_of(right, "kind")) {
        (Some("objectNumber"), Some("objectNumber")) => eq("annotObjectNumber"),
        (Some("nm"), Some("nm")) => eq("nm"),
        (l, r) => l == Some("index") && r == Some("index") && eq("index") && eq("revision"),
    }
}

/// The page of a JSON annotation ref.
fn page_of_ref(annotation_ref: &Value) -> PageRef {
    get(annotation_ref, "page").and_then(PageRef::from_json).unwrap_or(PageRef::new(0))
}

fn widget_source(field: &Value, annotation_ref: &Value) -> ActionSource {
    ActionSource::Widget {
        field: field.clone(),
        annotation: annotation_ref.clone(),
        page: page_of_ref(annotation_ref),
    }
}

impl FormCapability {
    /// The annotation plugin's committed DTO for a ref, when it has one. A
    /// ref that doesn't decode counts as not loaded.
    fn loaded(&self, annotation_ref: &Value) -> Option<Value> {
        let annotation = self.0.annotation.as_ref()?;
        let typed: TypedAnnotationRef = epdf_kernel::js::from_value(annotation_ref).ok()?;
        annotation.get_raw(&typed)
    }

    /// `annotationActivation(ref)`: the widget's `/A` tree, from the loaded
    /// annotation when there is one, else from a page read.
    async fn annotation_activation(&self, annotation_ref: &Value) -> Result<Option<Value>, Error> {
        let Some(doc) = self.doc() else { return Ok(None) };
        let activate = |annotation: &Value| {
            get(annotation, "actions")
                .and_then(|a| get(a, "activate"))
                .filter(|t| !t.is_null())
                .cloned()
        };
        if let Some(loaded) = self.loaded(annotation_ref)
            && str_of(&loaded, "subtype") == Some("widget")
        {
            return Ok(activate(&loaded));
        }
        let page = page_of_ref(annotation_ref);
        doc.page_index(page.page_object_number)?;
        let list = await_engine(doc.call(
            "page.annotations.list",
            Some(page.page_object_number),
            Vec::new(),
        ))
        .await?;
        let found = get(&list, "annotations").and_then(Value::as_array).into_iter().flatten().find(
            |candidate| {
                same_annotation_ref(get(candidate, "ref").unwrap_or(&Value::Null), annotation_ref)
            },
        );
        Ok(found.filter(|a| str_of(a, "subtype") == Some("widget")).and_then(activate))
    }

    /// `activateThroughScripts(ref, annotationRef)`.
    async fn activate_through_scripts(
        &self,
        field_ref: Value,
        annotation_ref: Value,
    ) -> Result<FormCommitResult, Error> {
        self.doc().ok_or_else(|| Error::new("no document"))?;
        let Some(scripting) = &self.0.scripting else {
            return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, false));
        };
        let reading = {
            let (this, annotation_ref) = (self.clone(), annotation_ref.clone());
            Promise::spawn(async move { this.annotation_activation(&annotation_ref).await })
        };
        let Some(action) = reading.await?.filter(|a| truthy(Some(a))) else {
            return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, true));
        };
        let result = scripting.activate(field_ref, action).await?;
        self.surface_via_actions(&result, ActionOrigin::User)?;
        if result.effects_result.is_some() {
            self.refresh().await?;
        }
        Ok(result)
    }

    /// `activateWidget(widget)`: run a widget's `/A` action as a click
    /// would. With the actions plugin the whole tree is dispatched there;
    /// only a tree-less result (inert, no steps, no diagnostics) falls back
    /// to the form path. `not-found` when no field owns the widget.
    pub fn activate_widget(&self, annotation_ref: Value) -> Promise<WidgetActivationResult> {
        let this = self.clone();
        Promise::spawn(async move {
            let Some(field) = field_for_widget(&this.model(), widget_object_of(&annotation_ref))
            else {
                return Err(plugin_error(
                    code::NOT_FOUND,
                    "form",
                    "no form field owns this widget",
                    None,
                    None,
                ));
            };
            let field_ref = get(&field, "ref").cloned().unwrap_or(Value::Null);
            if let Some(actions) = this.cx().try_get(&ACTIONS)? {
                let result = actions
                    .dispatch(ActionTrigger::Activate {
                        annotation_ref: annotation_ref.clone(),
                        page: page_of_ref(&annotation_ref),
                        source: Some(widget_source(&field_ref, &annotation_ref)),
                    })
                    .await?;
                let no_tree = result.status == ActionResultStatus::Inert
                    && result.steps.is_empty()
                    && result.diagnostics.is_empty();
                if !no_tree {
                    return Ok(WidgetActivationResult::Dispatched(result));
                }
            }
            let now = this.clone();
            let result = this
                .0
                .queue
                .run(move || {
                    Ok(Promise::spawn(async move {
                        now.activate_through_scripts(field_ref, annotation_ref).await
                    }))
                })
                .await?;
            Ok(WidgetActivationResult::Form(result))
        })
    }

    fn widget_hover_pump(&self, actions: &ActionsCapability) -> HoverPump {
        let mut pump = self.0.widget_pump.borrow_mut();
        pump.get_or_insert_with(|| {
            let actions = actions.clone();
            create_hover_pump(move |trigger| actions.dispatch(trigger))
        })
        .clone()
    }

    /// Tree presence through the folded annotation model, when available.
    fn widget_hover_flags(&self, annotation_ref: &Value) -> Option<(bool, bool)> {
        let loaded = self.loaded(annotation_ref)?;
        if str_of(&loaded, "subtype") != Some("widget") {
            return None;
        }
        let root = |event: &str| {
            truthy(get(&loaded, "actions").and_then(|a| get(a, event)).and_then(|t| get(t, "root")))
        };
        Some((root("cursorEnter"), root("cursorExit")))
    }

    /// Host lens: `notifyWidgetEvent(field, widget, event)`, the widget
    /// DOM-event feed. With the actions plugin the matching `/AA` tree
    /// dispatches (hover through the shared pump); without it a no-op.
    pub fn notify_widget_event(
        &self,
        field_ref: &Value,
        annotation_ref: &Value,
        event: PdfAnnotationEventKind,
    ) -> Result<(), Error> {
        let Some(actions) = self.cx().try_get(&ACTIONS)? else { return Ok(()) };
        let source = widget_source(field_ref, annotation_ref);
        let page = page_of_ref(annotation_ref);
        match event {
            PdfAnnotationEventKind::CursorExit => self.widget_hover_pump(&actions).hover(None),
            PdfAnnotationEventKind::CursorEnter => {
                let flags = self.widget_hover_flags(annotation_ref);
                if flags == Some((false, false)) {
                    return Ok(());
                }
                self.widget_hover_pump(&actions).hover(Some(HoverTarget {
                    annotation_ref: annotation_ref.clone(),
                    page,
                    source: Some(source),
                    events: flags
                        .map(|(enter, exit)| HoverEvents { enter: Some(enter), exit: Some(exit) }),
                }));
            }
            _ => actions
                .dispatch(ActionTrigger::Annotation {
                    event,
                    annotation_ref: annotation_ref.clone(),
                    page,
                    source: Some(source),
                })
                .ignore(),
        }
        Ok(())
    }
}
