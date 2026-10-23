//! `host-contract.ts`: what sibling plugins (stage, annotation, link, form,
//! stamp) need to register commit sinks, the submit resolver, script realms
//! and the page-state report. The same runtime object as the public lens:
//! these types are the arguments and results of the host members on
//! [`crate::ActionsCapability`].

use std::rc::Rc;

use epdf_acrojs::{
    ScriptBudget, ScriptDiagnostic, ScriptExecutionError, ScriptHost, ScriptTransaction,
    ScriptUiEffect,
};
use epdf_kernel::Error;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::types::DocumentMeta;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::{
    ActionContext, ActionDiagnostic, ActionOrigin, ActionSource, ActionSubmitRequest, FormEffect,
    FormSnapshot, ScriptPhase, SubmitIntent, page_ref_json,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageStateCause {
    User,
    Programmatic,
}

/// Stage's page-truth report, the one door into the lifecycle coordinator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageStateReport {
    #[serde(with = "page_ref_json::nullable")]
    pub current_page: Option<PageRef>,
    #[serde(with = "page_ref_json::list")]
    pub visible_pages: Vec<PageRef>,
    /// False until layout exists; unplaced reports are ignored.
    pub placed: bool,
    pub cause: PageStateCause,
}

// ── owner commit sinks ────────────────────────────────────────────────────────

/// One annotation-effect commit entry. `page` is absent for a bare Hide
/// object-number target; the sink resolves it from its model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotCommitEntry {
    pub annot_object_number: f64,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "page_ref_json::option")]
    pub page: Option<PageRef>,
    pub patch: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotCommitStatus {
    Applied,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotCommitResultEntry {
    pub annot_object_number: f64,
    pub status: AnnotCommitStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnnotCommitResult {
    pub results: Vec<AnnotCommitResultEntry>,
}

/// Invoked from inside a serialized dispatch; a sink never enqueues and never
/// acquires the script host.
pub type AnnotCommitSink =
    Rc<dyn Fn(Vec<AnnotCommitEntry>) -> Result<Promise<AnnotCommitResult>, Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormCommitStatus {
    Applied,
    Failed,
}

/// What the form plugin's commit did: `applied`, the effects are one change
/// on its way (shown at once; the engine's refusal comes through the change
/// queue); `failed`, none of them is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormCommitSinkResult {
    pub status: FormCommitStatus,
    /// Why the change couldn't be made, when it couldn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The form plugin's document commit: a script run's form effects, written
/// as one change.
pub type FormCommitSink =
    Rc<dyn Fn(Vec<FormEffect>) -> Result<Promise<FormCommitSinkResult>, Error>>;

/// The form as the user sees it: the form plugin's fields and widgets with
/// this session's pending fills, or `None` before it has read the form.
/// Scripts and dispatch read it instead of the engine.
pub type FormReader = Rc<dyn Fn() -> Option<Rc<FormSnapshot>>>;

// ── script results ────────────────────────────────────────────────────────────

/// Which realm produced a script result. `document` is this document's own
/// realm; `detached` a realm minted for a document that is not displayed,
/// which has no document surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptRealmKind {
    #[default]
    Document,
    Detached,
}

/// What a script transaction surfaced besides document effects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSurfaceResult {
    pub ui_effects: Vec<ScriptUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
    pub origin: ActionOrigin,
    pub phase: ScriptPhase,
    /// Absent reads as `document`: only `detached` changes the routing.
    #[serde(default)]
    pub realm: ScriptRealmKind,
    /// What started the script, when an action did: `onScriptFailed` and
    /// diagnostics carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ActionSource>,
}

/// A UI effect tagged with the phase that produced it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhasedUiEffect {
    #[serde(flatten)]
    pub effect: ScriptUiEffect,
    pub phase: ScriptPhase,
}

/// A K/V/C/F transaction's result as the form controller reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCommitSurface {
    pub ui_effects: Vec<PhasedUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
}

/// `surfaceScriptCommit`'s context.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptCommitContext {
    pub origin: ActionOrigin,
    pub realm: ScriptRealmKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ActionSource>,
}

// ── script realms ─────────────────────────────────────────────────────────────

/// `ScriptDocument`: what a script realm reads of the document it runs for,
/// `Pick<DocumentMeta, 'name' | 'pageCount' | 'pages'>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptDocument {
    pub name: Option<String>,
    pub page_count: u32,
    pub pages: Rc<Value>,
}

impl From<DocumentMeta> for ScriptDocument {
    fn from(meta: DocumentMeta) -> Self {
        ScriptDocument { name: meta.name, page_count: meta.page_count, pages: meta.pages }
    }
}

/// What a realm is minted for. `doc` is the document the scripts operate on;
/// `document()` the metadata scripts observe; `boot_sources()` the
/// name-tree programs, fetched lazily once per realm build (`Err` is a
/// synchronous throw).
#[derive(Clone)]
pub struct ScriptRealmTarget {
    pub doc: Rc<dyn DocumentHandle>,
    pub document: Rc<dyn Fn() -> Option<ScriptDocument>>,
    pub boot_sources: Rc<dyn Fn() -> Result<Promise<Vec<String>>, Error>>,
}

/// The realm transaction port plus the budget consumers apply as their
/// transaction aggregate.
#[derive(Clone)]
pub struct ScriptRealmPort {
    pub(crate) host: ScriptHost,
    pub budget: ScriptBudget,
}

impl ScriptRealmPort {
    /// The body must prefetch, run, commit through the sinks and reconcile
    /// before its promise settles. `Err` is a synchronous throw.
    pub fn transaction<T: Clone + 'static>(
        &self,
        body: impl FnOnce(ScriptTransaction) -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        self.host.transaction(body)
    }
}

/// A detached realm: the caller owns its lifetime.
#[derive(Clone)]
pub struct DetachedScriptRealm {
    pub port: ScriptRealmPort,
}

impl DetachedScriptRealm {
    pub fn transaction<T: Clone + 'static>(
        &self,
        body: impl FnOnce(ScriptTransaction) -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        self.port.transaction(body)
    }

    pub fn budget(&self) -> &ScriptBudget {
        &self.port.budget
    }

    pub fn dispose(&self) {
        self.port.host.dispose();
    }
}

/// The form plugin's dataset resolver. `diagnose` is the per-entry
/// observability channel.
pub type SubmitResolver = Rc<
    dyn Fn(
        &SubmitIntent,
        &ActionContext,
        Rc<dyn Fn(ActionDiagnostic)>,
    ) -> Result<Promise<ActionSubmitRequest>, Error>,
>;
