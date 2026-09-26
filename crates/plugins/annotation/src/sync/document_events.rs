//! `sync/document-events.ts`: makes annotations reactive to the document
//! event stream. It kicks whole-document hydration once, hands remote
//! `annotation.*` events to the hydration-aware delivery, rehydrates on
//! `stream.desynced` and `document.versioned`, reconciles page membership,
//! and bumps the widgets form and signature writes repaint.

use std::rc::Rc;

use epdf_annotation_core::{Msg, PageRef, update};
use epdf_kernel::Controller;
use serde_json::Value;

use crate::controller::AnnotationCapability;
use crate::model::{AnnotationAction, AnnotationState};
use crate::repository::annotation_key_of;
use crate::value::{get, str_of};

/// `widgetKey(w)`: a widget's model key, or none when the engine could not
/// give it an annotation address.
fn widget_key(w: &Value) -> Option<String> {
    get(w, "ref").filter(|r| !r.is_null()).and_then(annotation_key_of)
}

fn widget_keys(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array).into_iter().flatten().filter_map(widget_key).collect()
}

fn page_refs(list: Option<&Value>) -> Vec<PageRef> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| serde_json::from_value(p.clone()).ok())
        .collect()
}

/// `subscribeDocumentEvents(ctx)`: subscribe first, then hydrate, so every
/// event from the snapshot's cursor onward is observed.
pub(crate) fn subscribe_document_events(
    cx: &Controller<AnnotationState, AnnotationAction>,
    host: AnnotationCapability,
) {
    let Ok(doc) = cx.doc() else { return };
    let typed = cx.clone();
    let apply = move |msg: Msg| {
        let m = typed.state().model.clone();
        let (next, _) = update(&m, &msg);
        let model = next.map_or(m, Rc::new);
        typed.dispatch(AnnotationAction::SetModel { model });
    };
    let model_cx = cx.clone();
    let events_host = host.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
        let host = &events_host;
        let kind = str_of(event, "type").unwrap_or_default();
        match kind {
            "form.valueChanged" | "form.effectsApplied" => {
                apply(Msg::BumpAp { ids: widget_keys(get(event, "changedWidgets")) });
                return;
            }
            "form.fieldUpdated" => {
                let widgets = get(event, "field").and_then(|f| get(f, "widgets"));
                apply(Msg::BumpAp { ids: widget_keys(widgets) });
                return;
            }
            "signature.completed" => {
                let widget = get(event, "signature").and_then(|s| get(s, "widget"));
                if let Some(key) = widget.filter(|w| !w.is_null()).and_then(widget_key) {
                    apply(Msg::BumpAp { ids: vec![key] });
                }
                return;
            }
            "document.versioned" | "stream.desynced" => {
                let _ = host.refresh();
                return;
            }
            "redaction.applied" => {
                let mut affected: Vec<u32> = Vec::new();
                for r in get(event, "results").and_then(Value::as_array).into_iter().flatten() {
                    if str_of(r, "status") != Some("applied") {
                        continue;
                    }
                    let Some(page) = get(r, "page")
                        .and_then(|p| serde_json::from_value::<PageRef>(p.clone()).ok())
                    else {
                        continue;
                    };
                    if !affected.contains(&page.page_object_number) {
                        affected.push(page.page_object_number);
                    }
                }
                for pon in affected {
                    let _ = host.reload_page_pon(pon);
                }
                return;
            }
            "pages.deleted" => {
                let gone: Vec<u32> =
                    page_refs(get(event, "pages")).iter().map(|p| p.page_object_number).collect();
                let m = model_cx.state().model.clone();
                let ids: Vec<String> = m
                    .order
                    .iter()
                    .filter(|id| {
                        m.by_id.get(*id).is_some_and(|a| gone.contains(&a.page.page_object_number))
                    })
                    .cloned()
                    .collect();
                if !ids.is_empty() {
                    apply(Msg::Remove { ids });
                }
                return;
            }
            "pages.inserted" => {
                for page in page_refs(get(event, "insertedPages")) {
                    let _ = host.reload_page(page);
                }
                return;
            }
            _ => {}
        }
        let remote = event
            .get("origin")
            .is_some_and(|o| o.get("kind").and_then(Value::as_str) == Some("remote"));
        if !remote {
            return;
        }
        if matches!(
            kind,
            "annotation.created" | "annotation.updated" | "annotation.moved" | "annotation.deleted"
        ) {
            host.deliver_remote_event(event.clone());
        }
    }));
    cx.listen(unsubscribe);
    host.ensure_hydrated();
}
