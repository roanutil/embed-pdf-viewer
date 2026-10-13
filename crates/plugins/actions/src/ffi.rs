//! The actions plugin through uniffi: the engine's action trees as opaque
//! objects, the plugin's decisions as typed records, one document handle,
//! and the application ports as foreign traits (`ports`). JavaScript runs in
//! a foreign `ScriptEngine` behind a Rust sandbox (`sandbox`).
//!
//! Only the application members cross. The host members (commit sinks, the
//! submit resolver, script realms, script surfacing, the page-state report)
//! are for sibling plugins, which are Rust crates in the same kernel.

pub mod ports;
pub mod sandbox;

use std::sync::Arc;

use epdf_acrojs as acrojs;
use epdf_ffi::{KernelBound, KernelPost, PageRef, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::store::Unsubscribe;
use serde_json::Value;

use crate::ActionsCapability;
use crate::contract::{
    self as plugin, ActionDiagnostic, ActionNodeStatus, ActionOrigin, ActionPolicy,
    ActionPolicyPatch, ActionResultStatus, ActionTriggerEvent, DocumentTriggerEvent, PageEventKind,
    PdfAnnotationEventKind, PdfFieldEventKind,
};

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "actions", message, None, None)
}

fn parse(json: &str, what: &str) -> Result<Value, Error> {
    serde_json::from_str(json).map_err(|e| invalid(format!("{what} is not JSON: {e}")))
}

/// `PdfActionTree`: `{root, incomplete, warningFlags, warnings}` as the
/// engine extracted it. Echoed back exactly; never reshaped here.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfActionTree(pub(crate) Value);

#[uniffi::export]
impl PdfActionTree {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfActionTree(parse(&json, "an action tree")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }

    /// The root node's `type` (`goto`, `uri`, `javascript`, ...).
    pub fn root_type(&self) -> Option<String> {
        self.0.get("root")?.get("type")?.as_str().map(str::to_string)
    }

    pub fn is_incomplete(&self) -> bool {
        self.0.get("incomplete").and_then(Value::as_bool).unwrap_or(false)
    }
}

impl PdfActionTree {
    pub(crate) fn wrap(value: &Value) -> Arc<Self> {
        Arc::new(PdfActionTree(value.clone()))
    }
}

/// One `PdfActionNode`, as an executor receives it.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfActionNode(pub(crate) Value);

#[uniffi::export]
impl PdfActionNode {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfActionNode(parse(&json, "an action node")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }

    pub fn node_type(&self) -> String {
        self.0.get("type").and_then(Value::as_str).unwrap_or_default().to_string()
    }
}

/// An `AnnotationRef` or `FormFieldRef`, as the engine wrote it.
#[derive(Debug, PartialEq, uniffi::Object)]
pub struct PdfObjectRef(pub(crate) Value);

#[uniffi::export]
impl PdfObjectRef {
    #[uniffi::constructor]
    pub fn from_json(json: String) -> Result<Arc<Self>, ViewerError> {
        Ok(Arc::new(PdfObjectRef(parse(&json, "an object ref")?)))
    }

    pub fn json(&self) -> String {
        self.0.to_string()
    }
}

impl PdfObjectRef {
    fn wrap(value: &Value) -> Arc<Self> {
        Arc::new(PdfObjectRef(value.clone()))
    }
}

/// Who initiated a dispatch. Provenance only: policy never reads it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionSource {
    Widget { field: Arc<PdfObjectRef>, annotation: Arc<PdfObjectRef>, page: PageRef },
    Link { annotation: Option<Arc<PdfObjectRef>>, page: Option<PageRef> },
    Annotation { annotation: Arc<PdfObjectRef>, page: PageRef },
    Page { page: PageRef },
    Document,
    Api,
}

impl From<&plugin::ActionSource> for ActionSource {
    fn from(source: &plugin::ActionSource) -> Self {
        match source {
            plugin::ActionSource::Widget { field, annotation, page } => ActionSource::Widget {
                field: PdfObjectRef::wrap(field),
                annotation: PdfObjectRef::wrap(annotation),
                page: (*page).into(),
            },
            plugin::ActionSource::Link { annotation, page } => ActionSource::Link {
                annotation: annotation.as_ref().map(PdfObjectRef::wrap),
                page: page.map(PageRef::from),
            },
            plugin::ActionSource::Annotation { annotation, page } => ActionSource::Annotation {
                annotation: PdfObjectRef::wrap(annotation),
                page: (*page).into(),
            },
            plugin::ActionSource::Page { page } => ActionSource::Page { page: (*page).into() },
            plugin::ActionSource::Document => ActionSource::Document,
            plugin::ActionSource::Api => ActionSource::Api,
        }
    }
}

impl From<ActionSource> for plugin::ActionSource {
    fn from(source: ActionSource) -> Self {
        match source {
            ActionSource::Widget { field, annotation, page } => plugin::ActionSource::Widget {
                field: field.0.clone(),
                annotation: annotation.0.clone(),
                page: page.into(),
            },
            ActionSource::Link { annotation, page } => plugin::ActionSource::Link {
                annotation: annotation.map(|a| a.0.clone()),
                page: page.map(Into::into),
            },
            ActionSource::Annotation { annotation, page } => plugin::ActionSource::Annotation {
                annotation: annotation.0.clone(),
                page: page.into(),
            },
            ActionSource::Page { page } => plugin::ActionSource::Page { page: page.into() },
            ActionSource::Document => plugin::ActionSource::Document,
            ActionSource::Api => plugin::ActionSource::Api,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionContext {
    pub origin: ActionOrigin,
    pub source: ActionSource,
    pub event: ActionTriggerEvent,
}

impl From<&plugin::ActionContext> for ActionContext {
    fn from(ctx: &plugin::ActionContext) -> Self {
        ActionContext { origin: ctx.origin, source: (&ctx.source).into(), event: ctx.event.clone() }
    }
}

impl From<ActionContext> for plugin::ActionContext {
    fn from(ctx: ActionContext) -> Self {
        plugin::ActionContext { origin: ctx.origin, source: ctx.source.into(), event: ctx.event }
    }
}

/// What `execute_named` overrides.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PartialActionContext {
    #[uniffi(default = None)]
    pub origin: Option<ActionOrigin>,
    #[uniffi(default = None)]
    pub source: Option<ActionSource>,
    #[uniffi(default = None)]
    pub event: Option<ActionTriggerEvent>,
}

impl From<PartialActionContext> for plugin::PartialActionContext {
    fn from(ctx: PartialActionContext) -> Self {
        plugin::PartialActionContext {
            origin: ctx.origin,
            source: ctx.source.map(Into::into),
            event: ctx.event,
        }
    }
}

/// What a feed reports. The dispatcher resolves the trees and derives the
/// origin; `source` can't change it.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionTrigger {
    Activate {
        annotation: Arc<PdfObjectRef>,
        page: PageRef,
        source: Option<ActionSource>,
    },
    Annotation {
        event: PdfAnnotationEventKind,
        annotation: Arc<PdfObjectRef>,
        page: PageRef,
        source: Option<ActionSource>,
    },
    Page {
        event: PageEventKind,
        page: PageRef,
    },
    Document {
        event: DocumentTriggerEvent,
    },
}

impl From<ActionTrigger> for plugin::ActionTrigger {
    fn from(trigger: ActionTrigger) -> Self {
        match trigger {
            ActionTrigger::Activate { annotation, page, source } => {
                plugin::ActionTrigger::Activate {
                    annotation_ref: annotation.0.clone(),
                    page: page.into(),
                    source: source.map(Into::into),
                }
            }
            ActionTrigger::Annotation { event, annotation, page, source } => {
                plugin::ActionTrigger::Annotation {
                    event,
                    annotation_ref: annotation.0.clone(),
                    page: page.into(),
                    source: source.map(Into::into),
                }
            }
            ActionTrigger::Page { event, page } => {
                plugin::ActionTrigger::Page { event, page: page.into() }
            }
            ActionTrigger::Document { event } => plugin::ActionTrigger::Document { event },
        }
    }
}

/// The /A click tree or one /AA key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ActionAnnotationTreeEvent {
    Activate,
    Event { kind: PdfAnnotationEventKind },
}

/// Where a tree lives, for `action_tree`. Annotation and field sources read
/// `page.annotations.list` and `forms.list` (plans 05 and 06 map them).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionTreeSource {
    Annotation {
        annotation: Arc<PdfObjectRef>,
        page: PageRef,
        event: Option<ActionAnnotationTreeEvent>,
    },
    Field {
        field: Arc<PdfObjectRef>,
        event: PdfFieldEventKind,
    },
    Page {
        page: PageRef,
        event: Option<PageEventKind>,
    },
    Document {
        event: Option<DocumentTriggerEvent>,
    },
}

impl From<ActionTreeSource> for plugin::ActionTreeSource {
    fn from(source: ActionTreeSource) -> Self {
        match source {
            ActionTreeSource::Annotation { annotation, page, event } => {
                plugin::ActionTreeSource::Annotation {
                    annotation: annotation.0.clone(),
                    page: page.into(),
                    event: event.map(|e| match e {
                        ActionAnnotationTreeEvent::Activate => {
                            plugin::AnnotationTreeEvent::Activate(plugin::ActivateTag::Activate)
                        }
                        ActionAnnotationTreeEvent::Event { kind } => {
                            plugin::AnnotationTreeEvent::Event(kind)
                        }
                    }),
                }
            }
            ActionTreeSource::Field { field, event } => {
                plugin::ActionTreeSource::Field { field: field.0.clone(), event }
            }
            ActionTreeSource::Page { page, event } => {
                plugin::ActionTreeSource::Page { page: page.into(), event }
            }
            ActionTreeSource::Document { event } => plugin::ActionTreeSource::Document { event },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ActionNodeResult {
    /// Child indexes from the root: `[]` is the root, `[0]` its first `/Next`.
    pub path: Vec<u32>,
    pub node_type: String,
    pub status: ActionNodeStatus,
    pub detail: Option<String>,
}

impl From<&plugin::ActionNodeResult> for ActionNodeResult {
    fn from(node: &plugin::ActionNodeResult) -> Self {
        ActionNodeResult {
            path: node.path.iter().map(|i| u32::try_from(*i).unwrap_or(u32::MAX)).collect(),
            node_type: node.node_type.clone(),
            status: node.status,
            detail: node.detail.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionDispatchResult {
    pub status: ActionResultStatus,
    pub nodes: Vec<ActionNodeResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

impl From<&plugin::ActionDispatchResult> for ActionDispatchResult {
    fn from(result: &plugin::ActionDispatchResult) -> Self {
        ActionDispatchResult {
            status: result.status,
            nodes: result.nodes.iter().map(Into::into).collect(),
            diagnostics: result.diagnostics.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionStepResult {
    pub source: ActionSource,
    pub tree: Arc<PdfActionTree>,
    pub result: ActionDispatchResult,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionTriggerResult {
    pub status: ActionResultStatus,
    pub steps: Vec<ActionStepResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

impl From<&plugin::ActionTriggerResult> for ActionTriggerResult {
    fn from(result: &plugin::ActionTriggerResult) -> Self {
        ActionTriggerResult {
            status: result.status,
            steps: result
                .steps
                .iter()
                .map(|step| ActionStepResult {
                    source: (&step.source).into(),
                    tree: PdfActionTree::wrap(&step.tree),
                    result: (&step.result).into(),
                })
                .collect(),
            diagnostics: result.diagnostics.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ActionDispatchEvent {
    pub context: ActionContext,
    pub tree: Arc<PdfActionTree>,
    pub result: ActionDispatchResult,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OpenSequenceCompleted {
    pub result: ActionTriggerResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptDiagnosticCode {
    BlockedNetwork,
    UnsupportedApi,
    InvalidFieldValue,
    ScriptError,
    UiEffectSuppressed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptDiagnostic {
    pub code: ScriptDiagnosticCode,
    pub message: String,
}

impl From<&acrojs::ScriptDiagnostic> for ScriptDiagnostic {
    fn from(diagnostic: &acrojs::ScriptDiagnostic) -> Self {
        use acrojs::ScriptDiagnosticCode as C;
        ScriptDiagnostic {
            code: match diagnostic.code {
                C::BlockedNetwork => ScriptDiagnosticCode::BlockedNetwork,
                C::UnsupportedApi => ScriptDiagnosticCode::UnsupportedApi,
                C::InvalidFieldValue => ScriptDiagnosticCode::InvalidFieldValue,
                C::ScriptError => ScriptDiagnosticCode::ScriptError,
                C::UiEffectSuppressed => ScriptDiagnosticCode::UiEffectSuppressed,
            },
            message: diagnostic.message.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScriptErrorKind {
    Exception,
    Budget,
    InvalidOutput,
}

impl From<acrojs::ScriptExecutionErrorKind> for ScriptErrorKind {
    fn from(kind: acrojs::ScriptExecutionErrorKind) -> Self {
        match kind {
            acrojs::ScriptExecutionErrorKind::Exception => ScriptErrorKind::Exception,
            acrojs::ScriptExecutionErrorKind::Budget => ScriptErrorKind::Budget,
            acrojs::ScriptExecutionErrorKind::InvalidOutput => ScriptErrorKind::InvalidOutput,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ScriptExecutionError {
    pub kind: ScriptErrorKind,
    pub message: String,
    pub stack: Option<String>,
}

impl From<&acrojs::ScriptExecutionError> for ScriptExecutionError {
    fn from(error: &acrojs::ScriptExecutionError) -> Self {
        ScriptExecutionError {
            kind: error.kind.into(),
            message: error.message.clone(),
            stack: error.stack.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ActionsEvent {
    /// Every executed tree: dispatch-driven, verb-driven and lifecycle-driven.
    Executed {
        event: ActionDispatchEvent,
    },
    Diagnostic {
        event: ActionDiagnostic,
    },
    ScriptDiagnostic {
        event: ScriptDiagnostic,
    },
    ScriptError {
        event: ScriptExecutionError,
    },
    OpenSequenceCompleted {
        event: OpenSequenceCompleted,
    },
}

#[uniffi::export(with_foreign)]
pub trait ActionsEventListener: Send + Sync {
    fn on_event(&self, event: ActionsEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

/// The capability, if its document is still open.
fn open(cap: &ActionsCapability) -> Result<&ActionsCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "actions",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// `null` and `undefined` both mean "no tree here".
fn tree_or_none(value: Value) -> Option<Arc<PdfActionTree>> {
    (!value.is_null() && !epdf_kernel::is_undefined(&value)).then(|| Arc::new(PdfActionTree(value)))
}

#[derive(uniffi::Object)]
pub struct ActionsHandle {
    pub(crate) cap: KernelBound<ActionsCapability>,
    pub(crate) post: KernelPost,
}

impl ActionsHandle {
    /// On the kernel thread.
    pub fn new(cap: ActionsCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(ActionsHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl ActionsHandle {
    /// The live policy.
    pub async fn policy(&self) -> Result<ActionPolicy, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_policy())).await
    }

    /// Row-wise: a row names only the origins it changes.
    pub async fn update_policy(&self, patch: ActionPolicyPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.update_policy(&patch);
                Ok(())
            })
            .await
    }

    /// True when the config enables JavaScript and supplies an engine.
    pub async fn is_scripting_enabled(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_scripting_enabled())).await
    }

    /// Run one tree as one transaction on the document's serial queue.
    pub async fn execute(
        &self,
        tree: Arc<PdfActionTree>,
        context: ActionContext,
    ) -> Result<ActionDispatchResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.execute(tree.0.clone(), context.into())),
                |result: plugin::ActionDispatchResult| ActionDispatchResult::from(&result),
            )
            .await
    }

    /// Whether the root would be allowed (or handed to the adapter) now.
    pub async fn can_execute(
        &self,
        tree: Arc<PdfActionTree>,
        context: ActionContext,
    ) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_execute(&tree.0, &context.into()))).await
    }

    /// A Named verb (`NextPage`, `Print`, ...) as a one-node tree.
    pub async fn execute_named(
        &self,
        name: String,
        context: Option<PartialActionContext>,
    ) -> Result<ActionDispatchResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.execute_named(&name, context.map(Into::into))),
                |result: plugin::ActionDispatchResult| ActionDispatchResult::from(&result),
            )
            .await
    }

    /// `None` when the source has no tree for that event.
    pub async fn action_tree(
        &self,
        source: ActionTreeSource,
    ) -> Result<Option<Arc<PdfActionTree>>, ViewerError> {
        self.cap
            .call_promise(move |c| Ok(open(c)?.get_action_tree(source.into())), tree_or_none)
            .await
    }

    /// Resolve the trigger's trees and run each; a failed step never skips
    /// its siblings.
    pub async fn dispatch(
        &self,
        trigger: ActionTrigger,
    ) -> Result<ActionTriggerResult, ViewerError> {
        self.cap
            .call_promise(
                move |c| Ok(open(c)?.dispatch(trigger.into())),
                |result: plugin::ActionTriggerResult| ActionTriggerResult::from(&result),
            )
            .await
    }

    pub async fn can_dispatch(&self, trigger: ActionTrigger) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_dispatch(&trigger.into()))).await
    }

    /// The will-close trigger: call before closing the document.
    pub async fn prepare_close(&self) -> Result<ActionTriggerResult, ViewerError> {
        self.cap
            .call_promise(
                |c| Ok(open(c)?.prepare_close()),
                |result: plugin::ActionTriggerResult| ActionTriggerResult::from(&result),
            )
            .await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn ActionsEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let hooks: Vec<Unsubscribe> = vec![
                        {
                            let l = listener.clone();
                            c.on_executed(move |e| {
                                l.on_event(ActionsEvent::Executed {
                                    event: ActionDispatchEvent {
                                        context: (&e.ctx).into(),
                                        tree: PdfActionTree::wrap(&e.tree),
                                        result: (&e.result).into(),
                                    },
                                });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_diagnostic(move |e| {
                                l.on_event(ActionsEvent::Diagnostic { event: e.clone() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_script_diagnostic(move |e| {
                                l.on_event(ActionsEvent::ScriptDiagnostic { event: e.into() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_script_error(move |e| {
                                l.on_event(ActionsEvent::ScriptError { event: e.into() });
                                Ok(())
                            })
                        },
                        {
                            let l = listener.clone();
                            c.on_open_sequence_completed(move |e| {
                                l.on_event(ActionsEvent::OpenSequenceCompleted {
                                    event: OpenSequenceCompleted { result: (&e.result).into() },
                                });
                                Ok(())
                            })
                        },
                    ];
                    Unsubscribe::new(move || hooks.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
