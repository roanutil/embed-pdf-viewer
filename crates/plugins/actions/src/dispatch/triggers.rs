//! `dispatch/triggers.ts`: the `triggers` setting, the per-page cache of
//! annotations bearing page-lifecycle trees (the fan-out's read amplifier)
//! and ISO 32000-2 Table 197's page-step order.

use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{ActionSource, ActionTrigger, PageEventKind};
use crate::services::{get, str_of, truthy};

/// An annotation that carries at least one page-lifecycle tree (/PO, /PC,
/// /PV, /PI), as read.
#[derive(Clone)]
pub(crate) struct LifecycleAnnot {
    pub(crate) annotation_ref: Value,
    pub(crate) actions: Value,
}

const LIFECYCLE_KEYS: [&str; 4] = ["pageOpen", "pageClose", "pageVisible", "pageInvisible"];

/// Per page, the pending or settled read of its lifecycle-bearing
/// annotations, in insertion order (`Map<PageObjectNumber, Promise>`).
#[derive(Default)]
pub(crate) struct LifecycleReads(Vec<(u32, Promise<Vec<LifecycleAnnot>>)>);

impl LifecycleReads {
    fn get(&self, pon: u32) -> Option<Promise<Vec<LifecycleAnnot>>> {
        self.0.iter().find(|(p, _)| *p == pon).map(|(_, read)| read.clone())
    }

    fn set(&mut self, pon: u32, read: Promise<Vec<LifecycleAnnot>>) {
        match self.0.iter_mut().find(|(p, _)| *p == pon) {
            Some(entry) => entry.1 = read,
            None => self.0.push((pon, read)),
        }
    }

    fn delete(&mut self, pon: u32) {
        self.0.retain(|(p, _)| *p != pon);
    }
}

/// The pages whose annotations an event changes: those pages, every page
/// (`All`: a redaction, a skipped stretch of the stream, new document
/// bytes), or none.
enum AnnotationPages {
    None,
    All,
    Pages(Vec<u32>),
}

fn page_number(page: &Value) -> Option<u32> {
    PageRef::from_json(page).map(|p| p.page_object_number)
}

fn annotation_pages_of(event: &Value) -> AnnotationPages {
    match str_of(event, "type") {
        "annotations.created"
        | "annotations.updated"
        | "annotations.deleted"
        | "annotations.reordered"
        | "annotations.flattened"
        | "forms.widgetsReordered" => {
            AnnotationPages::Pages(get(event, "page").and_then(page_number).into_iter().collect())
        }
        // A form write can add, move or remove widgets, which carry page
        // actions too.
        "forms.created"
        | "forms.deleted"
        | "forms.widgetAdded"
        | "forms.widgetRemoved"
        | "forms.widgetDeleted"
        | "forms.widgetRestored"
        | "forms.widgetUpdated"
        | "forms.restored"
        | "forms.repaired" => AnnotationPages::All,
        "pages.flattened" => AnnotationPages::Pages(
            get(event, "pages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(page_number)
                .collect(),
        ),
        "redaction.applied" | "stream.desynced" | "document.versioned" => AnnotationPages::All,
        _ => AnnotationPages::None,
    }
}

impl ActionsCapability {
    /// `triggerEnabled(trigger)`: `activate` is never gated.
    pub(crate) fn trigger_enabled(&self, trigger: &ActionTrigger) -> bool {
        let triggers = self.settings_now().triggers;
        match trigger {
            ActionTrigger::Activate { .. } => true,
            ActionTrigger::Annotation { .. } => triggers.annotation,
            ActionTrigger::Page { .. } => triggers.page,
            ActionTrigger::Document { .. } => triggers.document,
        }
    }

    /// `lifecycleAnnotationsOf(page)`. Reads start only inside the queued
    /// operation that needs them, so a trigger sees every change made by the
    /// operations queued before it. Holding the promise lets concurrent
    /// readers share one read; dropping it on invalidation means a read in
    /// flight when its page changed is never reused.
    pub(crate) fn lifecycle_annotations_of(&self, page: PageRef) -> Promise<Vec<LifecycleAnnot>> {
        let key = page.page_object_number;
        if let Some(cached) = self.0.lifecycle_reads.borrow().get(key) {
            return cached;
        }
        let read = Promise::spawn(self.clone().read_page_annotations(page, None)).map(
            |annotations: Vec<Value>| {
                Ok(annotations
                    .into_iter()
                    .filter(|annotation| {
                        let actions = get(annotation, "actions");
                        truthy(actions)
                            && LIFECYCLE_KEYS.iter().any(|key| {
                                truthy(
                                    actions.and_then(|a| get(a, key)).and_then(|t| get(t, "root")),
                                )
                            })
                    })
                    .map(|annotation| LifecycleAnnot {
                        annotation_ref: get(&annotation, "ref").cloned().unwrap_or(Value::Null),
                        actions: get(&annotation, "actions").cloned().unwrap_or(Value::Null),
                    })
                    .collect())
            },
        );
        self.0.lifecycle_reads.borrow_mut().set(key, read.clone());
        // A failed read is not kept: the next trigger for the page reads again.
        let (weak, own) = (self.weak(), read.clone());
        let observed = read.clone();
        epdf_kernel::rt::spawn(async move {
            if observed.reaction().await.is_err()
                && let Some(this) = ActionsCapability::upgrade(&weak)
            {
                let current =
                    this.0.lifecycle_reads.borrow().get(key).is_some_and(|r| r.same(&own));
                if current {
                    this.0.lifecycle_reads.borrow_mut().delete(key);
                }
            }
        });
        read
    }

    /// `invalidate(event)`: forget the pages a confirmed document event
    /// changed.
    pub(crate) fn invalidate_lifecycle_reads(&self, event: &Value) {
        let mut reads = self.0.lifecycle_reads.borrow_mut();
        match annotation_pages_of(event) {
            AnnotationPages::All => reads.0.clear(),
            AnnotationPages::Pages(pages) => {
                for pon in pages {
                    reads.delete(pon);
                }
            }
            AnnotationPages::None => {}
        }
    }
}

/// `planPageSteps(event, page, pageActions, lifecycle)`: ISO Table 197
/// order, /PO after the page /O and /PC before the page /C. Rootless trees
/// are skipped.
pub(crate) fn plan_page_steps(
    event: PageEventKind,
    page: PageRef,
    page_actions: Option<&Value>,
    lifecycle: &[LifecycleAnnot],
) -> Vec<(ActionSource, Value)> {
    let annot_steps = |key: &str| -> Vec<(ActionSource, Value)> {
        lifecycle
            .iter()
            .filter(|a| truthy(get(&a.actions, key).and_then(|t| get(t, "root"))))
            .map(|a| {
                (
                    ActionSource::Annotation { annotation: a.annotation_ref.clone(), page },
                    get(&a.actions, key).cloned().unwrap_or(Value::Null),
                )
            })
            .collect()
    };
    let page_tree = |key: &str| -> Vec<(ActionSource, Value)> {
        match page_actions.and_then(|p| get(p, key)) {
            Some(tree) if truthy(get(tree, "root")) => {
                vec![(ActionSource::Page { page }, tree.clone())]
            }
            _ => Vec::new(),
        }
    };
    match event {
        PageEventKind::Open => [page_tree("open"), annot_steps("pageOpen")].concat(),
        PageEventKind::Close => [annot_steps("pageClose"), page_tree("close")].concat(),
        PageEventKind::Visible => annot_steps("pageVisible"),
        PageEventKind::Invisible => annot_steps("pageInvisible"),
    }
}
