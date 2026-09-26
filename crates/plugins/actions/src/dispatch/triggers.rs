//! `dispatch/triggers.ts`: the config gates, annotation identity, the
//! page-lifecycle tree cache (the fan-out's read amplifier) and ISO Table
//! 197's page-step order.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{ActionSource, ActionTrigger, PageEventKind};
use crate::services::{get, same_number, str_of, truthy};

/// `sameRef(left, right)`: two annotation refs name the same annotation.
pub fn same_ref(left: &Value, right: &Value) -> bool {
    let page_of = |v: &Value| get(v, "page").and_then(|p| get(p, "pageObjectNumber")).cloned();
    match (str_of(left, "kind"), str_of(right, "kind")) {
        ("objectNumber", "objectNumber") => {
            same_number(get(left, "annotObjectNumber"), get(right, "annotObjectNumber"))
        }
        ("nm", "nm") => {
            same_number(page_of(left).as_ref(), page_of(right).as_ref())
                && get(left, "nm") == get(right, "nm")
        }
        ("index", "index") => {
            same_number(page_of(left).as_ref(), page_of(right).as_ref())
                && same_number(get(left, "index"), get(right, "index"))
        }
        _ => false,
    }
}

/// An annotation bearing page-lifecycle trees (PO/PC/PV/PI).
#[derive(Clone)]
pub(crate) struct LifecycleAnnot {
    pub(crate) annotation_ref: Value,
    pub(crate) actions: Value,
}

const LIFECYCLE_KEYS: [&str; 4] = ["pageOpen", "pageClose", "pageVisible", "pageInvisible"];

impl ActionsCapability {
    /// `triggerEnabled(trigger)`: `activate` is never gated.
    pub(crate) fn trigger_enabled(&self, trigger: &ActionTrigger) -> bool {
        let gates = self.0.config.triggers.as_ref();
        match trigger {
            ActionTrigger::Activate { .. } => true,
            ActionTrigger::Annotation { .. } => gates.and_then(|g| g.annotation) != Some(false),
            ActionTrigger::Page { .. } => gates.and_then(|g| g.page) != Some(false),
            ActionTrigger::Document { .. } => gates.and_then(|g| g.document) != Some(false),
        }
    }

    /// The cache is invalidated wholesale on any `annotation.*` document
    /// event and on `stream.desynced` (gap events never arrive).
    pub(crate) fn subscribe_lifecycle_cache(&self) {
        let Ok(doc) = self.doc() else { return };
        let weak = self.weak();
        let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
            let kind = str_of(event, "type");
            if (kind.starts_with("annotation.") || kind == "stream.desynced")
                && let Some(this) = ActionsCapability::upgrade(&weak)
            {
                this.0.lifecycle_cache.borrow_mut().clear();
            }
        }));
        self.0.cx.listen(unsubscribe);
    }

    /// `lifecycleTreesFor(pon)`: the page's annotations that bear a
    /// page-lifecycle tree, read once per page until invalidated.
    pub(crate) async fn lifecycle_trees_for(self, pon: u32) -> Result<Vec<LifecycleAnnot>, Error> {
        let hit =
            self.0.lifecycle_cache.borrow().iter().find(|(p, _)| *p == pon).map(|(_, v)| v.clone());
        if let Some(hit) = hit {
            return Ok(hit);
        }
        let doc = self.doc()?;
        doc.page_index(pon)?;
        let listed = doc.call("page.annotations.list", Some(pon), Vec::new()).await?;
        let bearing: Vec<LifecycleAnnot> = get(&listed, "annotations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| {
                let actions = get(a, "actions");
                truthy(actions)
                    && LIFECYCLE_KEYS.iter().any(|key| {
                        truthy(actions.and_then(|x| get(x, key)).and_then(|t| get(t, "root")))
                    })
            })
            .map(|a| LifecycleAnnot {
                annotation_ref: get(&a, "ref").cloned().unwrap_or(Value::Null),
                actions: get(&a, "actions").cloned().unwrap_or(Value::Null),
            })
            .collect();
        let mut cache = self.0.lifecycle_cache.borrow_mut();
        cache.retain(|(p, _)| *p != pon);
        cache.push((pon, bearing.clone()));
        Ok(bearing)
    }
}

/// `planPageSteps(event, pon, pageActions, lifecycle)`: ISO order, PO after
/// the page O and PC before the page C. Rootless trees are skipped.
pub(crate) fn plan_page_steps(
    event: PageEventKind,
    pon: u32,
    page_actions: Option<&Value>,
    lifecycle: &[LifecycleAnnot],
) -> Vec<(ActionSource, Value)> {
    let page = PageRef::new(pon);
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
