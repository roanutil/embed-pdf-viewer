//! `scripting/environment.ts`: the session-level half of the JavaScript
//! switch (sandbox factory, identity, clock, timezone, seed and budget).
//! Policy (`enabled`) decides whether one exists; realms are minted from it.
//!
//! One environment mints any number of realms: the owning document's own
//! host, and detached realms for documents that are not the viewer document
//! (a stamp asset). Every realm gets a fresh sandbox from the factory, so
//! the globals are isolated. Identity closes over the owning document: who
//! the user is, is a session fact, never read from a detached handle.

use std::rc::{Rc, Weak};

use epdf_acrojs::{
    DEFAULT_SCRIPT_BUDGET, ScriptBudget, ScriptDocumentInput, ScriptEnvironment, ScriptHost,
    ScriptHostOptions, ScriptSandboxFactory, java_script_program_from_action_tree,
    resolve_script_identity, seed_from,
};
use epdf_kernel::Controller;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{ActionsConfig, JavaScriptConfig};
use crate::controller::Inner;
use crate::host_contract::ScriptRealmTarget;
use crate::model::{ActionsAction, ActionsState};
use crate::services::get;

/// `ScriptRealmFactory`: mints realms under one environment.
#[derive(Clone)]
pub(crate) struct ScriptRealmFactory {
    js: JavaScriptConfig,
    sandbox_factory: ScriptSandboxFactory,
    identity_doc: Rc<dyn DocumentHandle>,
    host_utc_offset_minutes: Option<Rc<dyn Fn(f64) -> f64>>,
    /// The embedder's budget (or the default): the per-run budget every
    /// minted realm enforces and the transaction aggregate consumers apply.
    pub(crate) budget: ScriptBudget,
}

impl ScriptRealmFactory {
    /// `js?.enabled && ctx.doc ? createScriptRealmFactory(js, ctx.doc) : null`.
    /// The TypeScript's default sandbox is a lazily imported QuickJS; Rust has
    /// none, so a host that supplies no factory runs with scripting disabled.
    pub(crate) fn for_config(
        cx: &Controller<ActionsState, ActionsAction>,
        config: &ActionsConfig,
    ) -> Option<Self> {
        let js = config.javascript.as_ref().filter(|js| js.enabled)?;
        let sandbox_factory = js.sandbox_factory.clone()?;
        let identity_doc = cx.doc().ok()?;
        Some(ScriptRealmFactory {
            js: js.clone(),
            sandbox_factory,
            identity_doc,
            host_utc_offset_minutes: config.host_utc_offset_minutes.clone(),
            budget: js.budget.clone().unwrap_or(DEFAULT_SCRIPT_BUDGET),
        })
    }

    /// `realmFor(target)`: a fresh script host over a fresh sandbox.
    pub(crate) fn realm_for(&self, target: ScriptRealmTarget) -> ScriptHost {
        let document = {
            let (file_name, doc, meta) =
                (self.js.file_name.clone(), target.doc.clone(), target.document.clone());
            Rc::new(move || {
                let meta = meta();
                ScriptDocumentInput {
                    id: doc.id(),
                    file_name: match &file_name {
                        Some(f) => f(),
                        None => meta
                            .as_ref()
                            .and_then(|m| m.name.clone())
                            .unwrap_or_else(|| "document.pdf".into()),
                    },
                    page_count: meta.map_or(0.0, |m| f64::from(m.page_count)),
                    page_number: 0.0,
                }
            })
        };
        let identity = {
            let (doc, overrides) = (self.identity_doc.clone(), self.js.identity.clone());
            Rc::new(move || resolve_script_identity(doc.as_ref(), overrides.as_ref()))
        };
        let environment = {
            let (now, offset, seed) = (
                self.js.now.clone(),
                self.js.utc_offset_minutes.clone(),
                self.js.random_seed.clone(),
            );
            let (host_offset, doc) = (self.host_utc_offset_minutes.clone(), target.doc.clone());
            Rc::new(move |sequence: f64| {
                let now_ms = now.as_ref().map_or_else(rt::now, |f| f());
                ScriptEnvironment {
                    now_ms,
                    utc_offset_minutes: match &offset {
                        Some(f) => f(),
                        None => host_offset.as_ref().map_or(0.0, |f| f(now_ms)),
                    },
                    random_seed: match &seed {
                        Some(f) => f(),
                        None => seed_from(&doc.id(), sequence),
                    },
                }
            })
        };
        ScriptHost::new(ScriptHostOptions {
            sandbox_factory: self.sandbox_factory.clone(),
            document,
            identity,
            environment,
            boot_sources: target.boot_sources,
            budget: Some(self.budget.clone()),
        })
    }

    /// This document's own realm (`scripting/realm.ts`): the viewer document,
    /// its metadata, and the catalog's name-tree programs.
    pub(crate) fn own_realm(
        &self,
        cx: &Controller<ActionsState, ActionsAction>,
        weak: Weak<Inner>,
    ) -> ScriptHost {
        let meta_cx = cx.clone();
        self.realm_for(ScriptRealmTarget {
            doc: self.identity_doc.clone(),
            document: Rc::new(move || meta_cx.document()),
            boot_sources: Rc::new(move || {
                let Some(this) = ActionsCapability::upgrade(&weak) else {
                    return Ok(Promise::resolved(Vec::new()));
                };
                Ok(Promise::spawn(async move {
                    let snapshot = this.read_document_actions().await?;
                    let scripts = snapshot
                        .as_ref()
                        .and_then(|s| get(s, "nameTreeScripts"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    scripts
                        .iter()
                        .map(|entry| {
                            java_script_program_from_action_tree(
                                get(entry, "action").unwrap_or(&Value::Null),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                }))
            }),
        })
    }
}
