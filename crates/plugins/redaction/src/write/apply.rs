//! `write/apply.ts`: the destructive half, `doc.redaction.apply` in refs or
//! pages scope. `apply`, `apply_pages` and `apply_all` are plain functions
//! in the TypeScript: a refusal is `Promise.reject`, and otherwise they
//! return `runApply`'s own promise.

use std::collections::HashSet;

use epdf_annotation_core::{AnnotationRef, PageRef};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::annotation_key;
use epdf_plugin_annotation::promise::{await_engine, spawn};
use serde_json::{Value, json};

use crate::contract::{RedactionAppliedEvent, RedactionApplyResult};
use crate::controller::RedactionCapability;
use crate::model::RedactionAction;
use crate::services::events::origin_api;

impl RedactionCapability {
    /// `runApply(scope)`: latch, the engine call, then the result or the
    /// rejection.
    fn run_apply(&self, scope: Value) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        spawn(async move {
            let doc = this.require_service()?;
            this.0.cx.dispatch(RedactionAction::ApplyStarted);
            match await_engine(doc.call("redaction.apply", None, vec![scope])).await {
                Ok(result) => {
                    this.0
                        .cx
                        .dispatch(RedactionAction::ApplyFinished { result: Some(result.clone()) });
                    this.0.events.applied.emit(&RedactionAppliedEvent {
                        result: result.clone(),
                        origin: origin_api(),
                    });
                    Ok(result)
                }
                Err(error) => {
                    this.0.cx.dispatch(RedactionAction::ApplyFinished { result: None });
                    Err(error)
                }
            }
        })
    }

    /// Apply specific pending marks. Irreversible.
    pub fn apply(&self, refs: &[AnnotationRef]) -> Promise<RedactionApplyResult> {
        let wanted: HashSet<String> = refs.iter().map(annotation_key).collect();
        let marks: Vec<AnnotationRef> = self
            .list_pending(None)
            .into_iter()
            .filter(|mark| wanted.contains(&annotation_key(&mark.ref_)))
            .map(|mark| mark.ref_)
            .collect();
        if marks.is_empty() {
            return Promise::rejected(plugin_error(
                code::NOT_FOUND,
                "redaction",
                "no matching pending marks",
                None,
                None,
            ));
        }
        self.run_apply(json!({ "kind": "annotations", "refs": marks }))
    }

    /// Apply the marks on some pages. Irreversible.
    pub fn apply_pages(&self, targets: &[PageRef]) -> Promise<RedactionApplyResult> {
        if targets.is_empty() {
            return Promise::rejected(plugin_error(
                code::INVALID_INPUT,
                "redaction",
                "no pages given",
                None,
                None,
            ));
        }
        self.run_apply(json!({ "kind": "pages", "pages": targets }))
    }

    /// Apply every redaction in the document: pages scope, authoritative
    /// for the whole document, including marks on pages this client never
    /// loaded. Pages without marks report `unchanged`. Irreversible.
    pub fn apply_all(&self) -> Promise<RedactionApplyResult> {
        let all = self.pages();
        if all.is_empty() {
            return Promise::rejected(plugin_error(
                code::NOT_READY,
                "redaction",
                "the document has no pages",
                None,
                None,
            ));
        }
        self.run_apply(json!({ "kind": "pages", "pages": all }))
    }
}
