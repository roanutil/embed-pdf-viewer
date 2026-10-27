//! `write/activation.ts`: widget activation and the widget event feed into
//! the actions plane. A click first offers the widget's `/A` tree to the
//! dispatcher; only a tree-less widget falls back to the form's own
//! scripted activation.

use epdf_kernel::Error;
use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_plugin_actions::{
    ActionOrigin, ActionResultStatus, ActionSource, ActionTrigger, ActionsCapability, HoverEvents,
    HoverPump, HoverTarget, PdfAnnotationEventKind, create_hover_pump,
};
use serde_json::Value;

use crate::contract::{FormCommitResult, FormCommitStatus, WidgetActivationResult};
use crate::model::{field_for_widget, widget_row_of};
use crate::read::fields::widget_object_of;
use crate::value::{get, str_of, truthy};
use crate::write::fill::{FillRequest, FillScripts};

fn page_of_ref(reference: &Value) -> Result<PageRef, Error> {
    get(reference, "page")
        .and_then(PageRef::from_json)
        .ok_or_else(|| Error::named("TypeError", "the widget's ref names no page"))
}

/// `widgetSource(field, annotationRef)`.
fn widget_source(field: &Value, annotation: &Value, page: PageRef) -> ActionSource {
    ActionSource::Widget { field: field.clone(), annotation: annotation.clone(), page }
}

impl crate::controller::Core {
    /// `annotationActivation(ref)`: the widget's activation action, from its
    /// row in the form.
    fn annotation_activation(&self, reference: &Value) -> Promise<Option<Value>> {
        let me = self.rc();
        let reference = reference.clone();
        Promise::spawn(async move {
            me.fields.settled().await?;
            let row = if str_of(&reference, "kind") == Some("objectNumber") {
                let n = get(&reference, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
                widget_row_of(&me.view(), n).cloned()
            } else {
                None
            };
            Ok(row
                .and_then(|r| get(&r, "actions").and_then(|a| get(a, "activate")).cloned())
                .filter(|a| !a.is_null()))
        })
    }

    /// `activateThroughScripts(ref, annotationRef, options)`.
    fn activate_through_scripts(
        &self,
        field: Value,
        annotation: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<FormCommitResult> {
        let me = self.rc();
        Promise::spawn(async move {
            if me.script_controller().is_none() {
                return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, false));
            }
            let Some(action) = me.annotation_activation(&annotation).await? else {
                return Ok(FormCommitResult::bare(FormCommitStatus::Unchanged, true));
            };
            let request = FillRequest {
                label: None,
                ops: Vec::new(),
                scripts: Some(FillScripts::Activate { field, action }),
                origin: ActionOrigin::User,
            };
            Ok(me.fill(request, signal).await?.result)
        })
    }

    /// `activateWidget(widget, options)`: do what a click on a widget does.
    pub fn activate_widget(
        &self,
        annotation: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<WidgetActivationResult> {
        let me = self.rc();
        Promise::spawn(async move {
            let Some(field) = field_for_widget(&me.view(), widget_object_of(&annotation)).cloned()
            else {
                return Err(me.error(code::NOT_FOUND, "no form field owns this widget"));
            };
            let field_ref = get(&field, "ref").cloned().unwrap_or(Value::Null);
            if let Some(actions) = me.actions() {
                let page = page_of_ref(&annotation)?;
                let trigger = ActionTrigger::Activate {
                    annotation_ref: annotation.clone(),
                    page,
                    source: Some(widget_source(&field_ref, &annotation, page)),
                };
                let result =
                    me.cancellable(signal.clone(), actions.dispatch(trigger, None)).await?;
                let no_tree = result.status == ActionResultStatus::Inert
                    && result.steps.is_empty()
                    && result.diagnostics.is_empty();
                if !no_tree {
                    return Ok(WidgetActivationResult::Dispatched(result));
                }
            }
            let result = me.activate_through_scripts(field_ref, annotation, signal).await?;
            Ok(WidgetActivationResult::Form(result))
        })
    }

    /// One shared hover pump, made on first use.
    fn widget_hover_pump(&self, actions: &ActionsCapability) -> HoverPump {
        if let Some(pump) = &*self.widget_pump.borrow() {
            return pump.clone();
        }
        let dispatcher = actions.clone();
        let pump = create_hover_pump(move |trigger| dispatcher.dispatch(trigger, None));
        *self.widget_pump.borrow_mut() = Some(pump.clone());
        pump
    }

    /// `widgetHoverFlags(ref)`: whether the widget has enter and exit trees.
    fn widget_hover_flags(&self, annotation: &Value) -> Option<HoverEvents> {
        if str_of(annotation, "kind") != Some("objectNumber") {
            return None;
        }
        let n = get(annotation, "objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let index = self.view();
        let row = widget_row_of(&index, n)?;
        let tree = |key: &str| {
            truthy(get(row, "actions").and_then(|a| get(a, key)).and_then(|t| get(t, "root")))
        };
        Some(HoverEvents { enter: Some(tree("cursorEnter")), exit: Some(tree("cursorExit")) })
    }

    /// `notifyWidgetEvent(field, widget, event)`: a widget's pointer or focus
    /// event to the actions plugin, which runs its `/AA` actions.
    pub fn notify_widget_event(
        &self,
        field: &Value,
        annotation: &Value,
        event: PdfAnnotationEventKind,
    ) -> Result<(), Error> {
        let Some(actions) = self.actions() else { return Ok(()) };
        let page = page_of_ref(annotation)?;
        let source = widget_source(field, annotation, page);
        match event {
            PdfAnnotationEventKind::CursorExit => {
                self.widget_hover_pump(&actions).hover(None);
            }
            PdfAnnotationEventKind::CursorEnter => {
                // Check for an enter/exit tree first, so a widget without one
                // costs no dispatch.
                let flags = self.widget_hover_flags(annotation);
                if flags.as_ref().is_some_and(|f| f.enter != Some(true) && f.exit != Some(true)) {
                    return Ok(());
                }
                self.widget_hover_pump(&actions).hover(Some(HoverTarget {
                    annotation_ref: annotation.clone(),
                    page,
                    source: Some(source),
                    events: flags,
                }));
            }
            _ => {
                // The first field to take focus starts the realm.
                if event == PdfAnnotationEventKind::Focus {
                    self.warm_scripts(&self.view());
                }
                // Fire-and-forget: dispatch never rejects, and results
                // surface through the actions events.
                actions
                    .dispatch(
                        ActionTrigger::Annotation {
                            event,
                            annotation_ref: annotation.clone(),
                            page,
                            source: Some(source),
                        },
                        None,
                    )
                    .ignore();
            }
        }
        Ok(())
    }
}
