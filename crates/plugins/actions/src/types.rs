//! `types.ts`: the dispatcher's vocabulary, its results, its policy and
//! config, the registration surfaces sibling plugins use, and the token.
//!
//! The TypeScript splits the capability into a public lens and a host lens
//! (`ActionsHostCapability`, imported from `/contract/host`). The split is an
//! API-visibility boundary over one runtime object, so Rust has one
//! [`crate::ActionsCapability`] and marks the host members in their docs.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_acrojs::{
    IdentityOverrides, ScriptBudget, ScriptDiagnostic, ScriptExecutionError, ScriptSandboxFactory,
    ScriptUiEffect,
};
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ActionsCapability;

/// `AnnotationRef`, as the engine wrote it.
pub type AnnotationRef = Value;
/// `FormFieldRef`, as the engine wrote it.
pub type FormFieldRef = Value;
/// `PdfActionTree`, carried as the engine extracted it and echoed back in
/// results and events exactly.
pub type PdfActionTree = Value;
/// `PdfActionNode`.
pub type PdfActionNode = Value;
/// `PdfActionTargetRef`: `{kind: 'name', name}` or `{kind: 'objectNumber', objectNumber}`.
pub type PdfActionTargetRef = Value;
/// `FormEffect`.
pub type FormEffect = Value;
/// `PageObjectNumber`. A JSON number, compared as JavaScript compares it.
pub type PageObjectNumber = f64;

// ── the when/who axes ─────────────────────────────────────────────────────────

/// Why a dispatch happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionOrigin {
    User,
    Hover,
    Lifecycle,
}

impl ActionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionOrigin::User => "user",
            ActionOrigin::Hover => "hover",
            ActionOrigin::Lifecycle => "lifecycle",
        }
    }
}

/// Who initiated the dispatch. Provenance only: policy never reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActionSource {
    Widget {
        field: FormFieldRef,
        annotation: AnnotationRef,
        pon: PageObjectNumber,
    },
    Link {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotation: Option<AnnotationRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pon: Option<PageObjectNumber>,
    },
    /// A non-widget annotation's own /AA event.
    Annotation {
        annotation: AnnotationRef,
        pon: PageObjectNumber,
    },
    /// A page /AA tree inside a page-trigger fan-out.
    Page {
        pon: PageObjectNumber,
    },
    /// The document-open sequence.
    Document,
    Api,
}

/// The document lifecycle vocabulary: `open` plus the five catalog /AA verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentTriggerEvent {
    Open,
    WillSave,
    DidSave,
    WillPrint,
    DidPrint,
    WillClose,
}

impl DocumentTriggerEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentTriggerEvent::Open => "open",
            DocumentTriggerEvent::WillSave => "will-save",
            DocumentTriggerEvent::DidSave => "did-save",
            DocumentTriggerEvent::WillPrint => "will-print",
            DocumentTriggerEvent::DidPrint => "did-print",
            DocumentTriggerEvent::WillClose => "will-close",
        }
    }
}

/// The six annotation /AA pointer and focus events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PdfAnnotationEventKind {
    CursorEnter,
    CursorExit,
    MouseDown,
    MouseUp,
    Focus,
    Blur,
}

impl PdfAnnotationEventKind {
    /// The `PdfAnnotationActions` key.
    pub fn as_str(self) -> &'static str {
        match self {
            PdfAnnotationEventKind::CursorEnter => "cursorEnter",
            PdfAnnotationEventKind::CursorExit => "cursorExit",
            PdfAnnotationEventKind::MouseDown => "mouseDown",
            PdfAnnotationEventKind::MouseUp => "mouseUp",
            PdfAnnotationEventKind::Focus => "focus",
            PdfAnnotationEventKind::Blur => "blur",
        }
    }
}

/// The page lifecycle events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PageEventKind {
    Open,
    Close,
    Visible,
    Invisible,
}

/// Trigger provenance: which event fired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum ActionTriggerEvent {
    Activate,
    Annotation { name: PdfAnnotationEventKind },
    Page { name: PageEventKind },
    Document { name: DocumentTriggerEvent },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionContext {
    pub origin: ActionOrigin,
    pub source: ActionSource,
    pub event: ActionTriggerEvent,
}

/// What a feed reports. The dispatcher resolves trees and derives the
/// origin; `source` is a provenance hint that cannot change it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum ActionTrigger {
    Activate {
        #[serde(rename = "ref")]
        annotation_ref: AnnotationRef,
        pon: PageObjectNumber,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ActionSource>,
    },
    Annotation {
        event: PdfAnnotationEventKind,
        #[serde(rename = "ref")]
        annotation_ref: AnnotationRef,
        pon: PageObjectNumber,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ActionSource>,
    },
    Page {
        event: PageEventKind,
        pon: PageObjectNumber,
    },
    Document {
        event: DocumentTriggerEvent,
    },
}

/// `eventOf`: trigger to provenance descriptor.
pub fn event_of(trigger: &ActionTrigger) -> ActionTriggerEvent {
    match trigger {
        ActionTrigger::Activate { .. } => ActionTriggerEvent::Activate,
        ActionTrigger::Annotation { event, .. } => ActionTriggerEvent::Annotation { name: *event },
        ActionTrigger::Page { event, .. } => ActionTriggerEvent::Page { name: *event },
        ActionTrigger::Document { event } => ActionTriggerEvent::Document { name: *event },
    }
}

/// `originOf`: the one origin mapping. A feed cannot launder a hover into a
/// user gesture.
pub fn origin_of(trigger: &ActionTrigger) -> ActionOrigin {
    match trigger {
        ActionTrigger::Activate { .. } => ActionOrigin::User,
        ActionTrigger::Annotation { event, .. } => match event {
            PdfAnnotationEventKind::CursorEnter | PdfAnnotationEventKind::CursorExit => {
                ActionOrigin::Hover
            }
            _ => ActionOrigin::User,
        },
        ActionTrigger::Page { .. } | ActionTrigger::Document { .. } => ActionOrigin::Lifecycle,
    }
}

// ── results ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionNodeStatus {
    Executed,
    Blocked,
    NoExecutor,
    Inert,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionNodeResult {
    /// Child indexes from the root: `[]` is the root, `[0]` its first `/Next`.
    pub path: Vec<usize>,
    #[serde(rename = "type")]
    pub node_type: String,
    pub status: ActionNodeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionDiagnosticCode {
    IncompleteTree,
    Blocked,
    NoExecutor,
    NoAdapter,
    NoSessionSink,
    UnresolvedTarget,
    DuplicateExecutor,
    ExecutorInert,
    ExecutorFailed,
    TriggerDisabled,
    NoCommitSink,
    TriggerFailed,
    CascadeBudget,
    OpenSequenceReplayed,
    NoSubmitSink,
    NoSubmitResolver,
    SubmitPayloadUnavailable,
    SubmitEntryUnsupported,
    ReentrantPrint,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDiagnostic {
    pub code: ActionDiagnosticCode,
    pub message: String,
}

impl ActionDiagnostic {
    pub fn new(code: ActionDiagnosticCode, message: impl Into<String>) -> Self {
        ActionDiagnostic { code, message: message.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionResultStatus {
    Executed,
    Partial,
    Inert,
    Refused,
}

/// One dispatch transaction's outcome. Document work is not rollback-atomic:
/// `partial` says an earlier write survived a later failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDispatchResult {
    pub status: ActionResultStatus,
    pub nodes: Vec<ActionNodeResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDispatchEvent {
    pub ctx: ActionContext,
    pub tree: PdfActionTree,
    pub result: ActionDispatchResult,
}

/// One tree's execution inside a trigger, with its own source and results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionStepResult {
    pub source: ActionSource,
    pub tree: PdfActionTree,
    pub result: ActionDispatchResult,
}

/// What `dispatch(trigger)` resolves to. A step failure never skips its
/// siblings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionTriggerResult {
    pub status: ActionResultStatus,
    pub steps: Vec<ActionStepResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

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
    pub current_pon: Option<PageObjectNumber>,
    pub visible_pons: Vec<PageObjectNumber>,
    /// False until layout exists; unplaced reports are ignored.
    pub placed: bool,
    pub cause: PageStateCause,
}

// ── policy ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionPolicyDecision {
    Allow,
    Adapter,
    Report,
    Block,
}

impl ActionPolicyDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionPolicyDecision::Allow => "allow",
            ActionPolicyDecision::Adapter => "adapter",
            ActionPolicyDecision::Report => "report",
            ActionPolicyDecision::Block => "block",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPolicyRow {
    pub user: ActionPolicyDecision,
    pub hover: ActionPolicyDecision,
    pub lifecycle: ActionPolicyDecision,
}

impl ActionPolicyRow {
    pub fn get(&self, origin: ActionOrigin) -> ActionPolicyDecision {
        match origin {
            ActionOrigin::User => self.user,
            ActionOrigin::Hover => self.hover,
            ActionOrigin::Lifecycle => self.lifecycle,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionPolicy {
    pub goto: ActionPolicyRow,
    pub named: ActionPolicyRow,
    pub hide: ActionPolicyRow,
    #[serde(rename = "reset-form")]
    pub reset_form: ActionPolicyRow,
    pub javascript: ActionPolicyRow,
    pub uri: ActionPolicyRow,
    /// The Named `Print` verb.
    pub print: ActionPolicyRow,
    #[serde(rename = "submit-form")]
    pub submit_form: ActionPolicyRow,
}

impl ActionPolicy {
    /// `policy[type]`: the row for a node type, by its key.
    pub fn row(&self, node_type: &str) -> Option<&ActionPolicyRow> {
        Some(match node_type {
            "goto" => &self.goto,
            "named" => &self.named,
            "hide" => &self.hide,
            "reset-form" => &self.reset_form,
            "javascript" => &self.javascript,
            "uri" => &self.uri,
            "print" => &self.print,
            "submit-form" => &self.submit_form,
            _ => return None,
        })
    }
}

/// `Partial<ActionPolicy>`: rows given here replace the defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartialActionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goto: Option<ActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<ActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide: Option<ActionPolicyRow>,
    #[serde(rename = "reset-form", default, skip_serializing_if = "Option::is_none")]
    pub reset_form: Option<ActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub javascript: Option<ActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<ActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<ActionPolicyRow>,
    #[serde(rename = "submit-form", default, skip_serializing_if = "Option::is_none")]
    pub submit_form: Option<ActionPolicyRow>,
}

/// Trigger-family gates, default all on. `activate` is never gated.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TriggerGates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<bool>,
}

/// When the document-open sequence fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenSequence {
    /// At the earliest of a UI adapter installing or the first user-origin
    /// dispatch (the default).
    Auto,
    /// At bring-up, falling back to the first page for the initial open.
    Headless,
    /// Never, but the page-lifecycle barrier still releases.
    Off,
}

/// `ActionsPluginConfig.javascript`: the one JavaScript switch.
#[derive(Clone, Default)]
pub struct JavaScriptConfig {
    pub enabled: bool,
    /// The sandbox. The TypeScript lazily imports QuickJS when this is
    /// absent; Rust has no default, and `enabled` with no factory runs as the
    /// TypeScript does with scripting disabled.
    pub sandbox_factory: Option<ScriptSandboxFactory>,
    /// Embedder identity fields layered over the engine identity.
    pub identity: Option<IdentityOverrides>,
    pub file_name: Option<Rc<dyn Fn() -> String>>,
    /// The deterministic clock. Also `clientTimeMs` for submits, with
    /// scripting on or off. Default: `rt::now()`.
    pub now: Option<Rc<dyn Fn() -> f64>>,
    pub utc_offset_minutes: Option<Rc<dyn Fn() -> f64>>,
    pub random_seed: Option<Rc<dyn Fn() -> f64>>,
    pub budget: Option<ScriptBudget>,
    /// The most JS nodes one dispatch may run. Default 16.
    pub max_script_nodes_per_dispatch: Option<f64>,
}

#[derive(Clone, Default)]
pub struct ActionsPluginConfig {
    pub policy: Option<PartialActionPolicy>,
    pub triggers: Option<TriggerGates>,
    pub open_sequence: Option<OpenSequence>,
    pub javascript: Option<JavaScriptConfig>,
    /// Host input: where a throwing `onAction`/`onDiagnostic`/
    /// `onScriptDiagnostic`/`onScriptError` observer is reported. The
    /// TypeScript writes these to `globalThis.console.error` with the same
    /// message; with no sink they are dropped.
    pub observer_error: Option<Rc<dyn Fn(&str, &Error)>>,
    /// Host input: `-new Date(nowMs).getTimezoneOffset()`, the local UTC
    /// offset in minutes at an instant, used when `javascript.utc_offset_minutes`
    /// is not configured. Default: zero (UTC).
    pub host_utc_offset_minutes: Option<Rc<dyn Fn(f64) -> f64>>,
}

// ── registration surfaces (host lens) ─────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ActionExecutorResult {
    Executed,
    Inert { reason: String },
    Failed { error: String },
}

/// One node of one registered type, executed in dispatch order. `Err` is a
/// synchronous throw; the promise may reject.
pub type ActionExecutor =
    Rc<dyn Fn(&PdfActionNode, &ActionContext) -> Result<Promise<ActionExecutorResult>, Error>>;

/// One annotation-effect commit entry. `page_object_number` is absent for
/// a bare Hide object-number target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotCommitEntry {
    pub annot_object_number: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_object_number: Option<f64>,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormEffectError {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// One entry of `FormEffectsResult.results`: the members the dispatcher reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormEffectResultEntry {
    pub index: f64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FormEffectError>,
}

/// `FormEffectsResult`, read for its per-effect results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormEffectsResult {
    pub results: Vec<FormEffectResultEntry>,
}

/// The form plugin's document commit.
pub type FormCommitSink = Rc<dyn Fn(Vec<FormEffect>) -> Result<Promise<FormEffectsResult>, Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptPhase {
    Boot,
    User,
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionUiContext {
    pub origin: ActionOrigin,
    pub phase: ScriptPhase,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UriOptions {
    pub is_map: bool,
    pub origin: ActionOrigin,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertOptions {
    pub origin: ActionOrigin,
    pub phase: ScriptPhase,
    pub icon: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// The UI port. Identity is the `Rc`: a disposer clears the slot only while
/// the same adapter is installed. `Err` is a throw, which propagates.
pub struct ActionUiAdapter {
    pub open_uri: Rc<dyn Fn(&str, &UriOptions) -> Result<(), Error>>,
    /// The Named Print verb and script `print()`; `None` when the TypeScript
    /// calls `print()` with no context.
    pub print: Rc<dyn Fn(Option<&ActionUiContext>) -> Result<(), Error>>,
    pub alert: Option<Rc<dyn Fn(&str, &AlertOptions) -> Result<(), Error>>>,
    pub goto_page: Option<Rc<dyn Fn(f64, &ActionUiContext) -> Result<(), Error>>>,
}

// ── the submit pipeline ───────────────────────────────────────────────────────

/// A normalized submit intent, from a SubmitForm node or a script
/// `doc.submitForm()`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitIntent {
    pub url: Option<String>,
    /// Table-239 targets; `None` is the whole eligible form.
    pub fields: Option<Vec<PdfActionTargetRef>>,
    pub exclude: bool,
    pub include_no_value_fields: bool,
    pub format: String,
    pub method: String,
    pub flags_raw: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_set: Option<String>,
}

/// `FormSubmissionEntry`: `value` is a string, a list (multi-select), or
/// null (name only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormSubmissionEntry {
    pub name: String,
    pub value: Value,
}

/// The resolved dataset a sink receives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSubmitRequest {
    pub url: Option<String>,
    pub method: String,
    pub format: String,
    pub flags_raw: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_set: Option<String>,
    pub entries: Vec<FormSubmissionEntry>,
    pub origin: ActionOrigin,
    pub event: ActionTriggerEvent,
}

/// `() => Promise<FormSubmissionReceipt>`: the document home, for a handler
/// to compose with. `Err` is a synchronous throw.
pub type SubmitToDocumentHome = Rc<dyn Fn() -> Result<Promise<Value>, Error>>;

pub struct SubmitHandlerContext {
    pub submit_to_document_home: Option<SubmitToDocumentHome>,
}

/// Sink 1 of the chain. `Ok(None)` is synchronous acceptance; `Ok(Some)` a
/// promise the dispatcher detaches; `Err` a synchronous throw.
pub type ActionSubmitHandler =
    Rc<dyn Fn(&ActionSubmitRequest, SubmitHandlerContext) -> Result<Option<Promise<Value>>, Error>>;

/// The form plugin's dataset resolver.
pub type SubmitResolver = Rc<
    dyn Fn(
        &SubmitIntent,
        &ActionContext,
        Rc<dyn Fn(ActionDiagnostic)>,
    ) -> Result<Promise<ActionSubmitRequest>, Error>,
>;

/// A disposer an identity-safe registration returns. Calling it again does
/// what it did the first time, as a TypeScript closure would.
pub type Disposer = Rc<dyn Fn()>;

// ── state and the token ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActionsState {
    /// Monotonic dispatch counter.
    pub seq: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ActionsAction {
    #[serde(rename = "ACTIONS_DISPATCHED")]
    ActionsDispatched,
}

/// `ActionsToken`, shared by the public and host lenses.
pub static ACTIONS: LazyLock<Token<ActionsCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "actions",
        "add actionsPlugin() from '@embedpdf/plugin-actions' to your plugins list",
    )
});
