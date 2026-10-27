//! `contract.ts`: the public action vocabulary (triggers, contexts,
//! results, events, policy, the settings, the UI adapter and submit handler
//! ports) and the two pure mappings, `eventOf` and `triggerOriginOf`. The
//! host lens lives in [`crate::host_contract`]; both are one runtime object,
//! [`crate::ActionsCapability`].

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_acrojs::{
    IdentityOverrides, ScriptBudget, ScriptDiagnostic, ScriptExecutionError, ScriptSandboxFactory,
};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
/// `OutlineItemRef`: `{objectNumber}`, as the engine wrote it.
pub type OutlineItemRef = Value;
/// `PdfActionType`: the node's `type` (`goto`, `uri`, `javascript`, ...).
pub type PdfActionType = String;
/// `FormSnapshot`, as the engine or the form plugin wrote it.
pub type FormSnapshot = Value;

/// `PageRef` as JSON, for serde derives. The kernel's `PageRef` now carries
/// serde impls of its own; this module stays for callers that name it.
pub mod page_ref_json {
    use super::*;

    pub fn serialize<S: Serializer>(page: &PageRef, s: S) -> Result<S::Ok, S::Error> {
        page.to_json().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PageRef, D::Error> {
        let value = Value::deserialize(d)?;
        PageRef::from_json(&value).ok_or_else(|| serde::de::Error::custom("not a PageRef"))
    }

    /// An optional member: absent when `None`.
    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(page: &Option<PageRef>, s: S) -> Result<S::Ok, S::Error> {
            page.map(PageRef::to_json).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PageRef>, D::Error> {
            let value = Option::<Value>::deserialize(d)?;
            match value {
                None | Some(Value::Null) => Ok(None),
                Some(value) => PageRef::from_json(&value)
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("not a PageRef")),
            }
        }
    }

    /// A nullable member: `null` when `None`.
    pub mod nullable {
        pub use super::option::{deserialize, serialize};
    }

    /// A list of refs.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer>(pages: &[PageRef], s: S) -> Result<S::Ok, S::Error> {
            pages.iter().map(|p| p.to_json()).collect::<Vec<_>>().serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PageRef>, D::Error> {
            let values = Vec::<Value>::deserialize(d)?;
            values
                .iter()
                .map(|v| {
                    PageRef::from_json(v).ok_or_else(|| serde::de::Error::custom("not a PageRef"))
                })
                .collect()
        }
    }
}

// ── the when/who axes ─────────────────────────────────────────────────────────

/// Why a dispatch happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
        #[serde(with = "page_ref_json")]
        page: PageRef,
    },
    Link {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        annotation: Option<AnnotationRef>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "page_ref_json::option")]
        page: Option<PageRef>,
    },
    /// An outline item the user followed: its /A, run as a link's is.
    Outline {
        item: OutlineItemRef,
    },
    /// A non-widget annotation's own /AA event.
    Annotation {
        annotation: AnnotationRef,
        #[serde(with = "page_ref_json")]
        page: PageRef,
    },
    /// A page /AA tree inside a page-trigger fan-out.
    Page {
        #[serde(with = "page_ref_json")]
        page: PageRef,
    },
    /// The document-open sequence.
    Document,
    Api,
}

/// The document lifecycle vocabulary: `open` plus the five catalog /AA verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum PageEventKind {
    Open,
    Close,
    Visible,
    Invisible,
}

/// The four field-level /AA scripts (ISO Table 198: K F V C).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum PdfFieldEventKind {
    Keystroke,
    Format,
    Validate,
    Calculate,
}

impl PdfFieldEventKind {
    /// The `PdfFieldActions` key.
    pub fn as_str(self) -> &'static str {
        match self {
            PdfFieldEventKind::Keystroke => "keystroke",
            PdfFieldEventKind::Format => "format",
            PdfFieldEventKind::Validate => "validate",
            PdfFieldEventKind::Calculate => "calculate",
        }
    }
}

/// Trigger provenance: which event fired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum ActionTriggerEvent {
    Activate,
    Annotation { name: PdfAnnotationEventKind },
    Page { name: PageEventKind },
    Document { name: DocumentTriggerEvent },
}

/// `ActionTreeSource`'s annotation event: the /A click tree or one /AA key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnnotationTreeEvent {
    Activate(ActivateTag),
    Event(PdfAnnotationEventKind),
}

/// The literal `'activate'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivateTag {
    Activate,
}

impl AnnotationTreeEvent {
    /// The `PdfAnnotationActions` key.
    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationTreeEvent::Activate(_) => "activate",
            AnnotationTreeEvent::Event(event) => event.as_str(),
        }
    }
}

/// Where an action tree lives, for [`ActionsCapability::get_action_tree`].
/// The annotation arm defaults to the /A click tree, the page and document
/// arms to their open trees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActionTreeSource {
    Annotation {
        annotation: AnnotationRef,
        #[serde(with = "page_ref_json")]
        page: PageRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<AnnotationTreeEvent>,
    },
    Field {
        field: FormFieldRef,
        event: PdfFieldEventKind,
    },
    Page {
        #[serde(with = "page_ref_json")]
        page: PageRef,
        /// `open` or `close`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<PageEventKind>,
    },
    Document {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<DocumentTriggerEvent>,
    },
    /// An outline item's /A.
    Outline {
        item: OutlineItemRef,
    },
}

/// The document-open sequence has run (once per document).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenSequenceCompletedEvent {
    pub result: ActionTriggerResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionContext {
    pub origin: ActionOrigin,
    pub source: ActionSource,
    pub event: ActionTriggerEvent,
}

/// `Partial<ActionContext>`: what `executeNamed` overrides.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartialActionContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ActionOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ActionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<ActionTriggerEvent>,
}

/// What a feed reports. The dispatcher resolves trees and derives the
/// origin; `source` is a provenance hint that cannot change it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "camelCase")]
pub enum ActionTrigger {
    Activate {
        #[serde(rename = "ref")]
        annotation_ref: AnnotationRef,
        #[serde(with = "page_ref_json")]
        page: PageRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ActionSource>,
    },
    Annotation {
        event: PdfAnnotationEventKind,
        #[serde(rename = "ref")]
        annotation_ref: AnnotationRef,
        #[serde(with = "page_ref_json")]
        page: PageRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ActionSource>,
    },
    Page {
        event: PageEventKind,
        #[serde(with = "page_ref_json")]
        page: PageRef,
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

/// `triggerOriginOf`: the one origin mapping. A feed cannot launder a hover
/// into a user gesture.
pub fn trigger_origin_of(trigger: &ActionTrigger) -> ActionOrigin {
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum ActionResultStatus {
    Executed,
    Partial,
    Inert,
    Refused,
    /// The caller's signal fired while the tree or trigger ran: the walk
    /// stopped before its next node and nothing deferred fired.
    Cancelled,
}

/// One dispatch transaction's outcome. Document work is not rollback-atomic:
/// `partial` says an earlier write survived a later failure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDispatchResult {
    pub status: ActionResultStatus,
    pub nodes: Vec<ActionNodeResult>,
    pub diagnostics: Vec<ActionDiagnostic>,
}

// ── events ────────────────────────────────────────────────────────────────────

/// `onExecuted`: one tree ran, whatever started it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionExecutedEvent {
    pub tree: PdfActionTree,
    pub result: ActionDispatchResult,
    /// What started it.
    pub source: ActionSource,
}

/// `onDiagnosticReported`: an action was blocked, reported or couldn't run.
/// `action` and `source` are `None` (`null`) for a diagnostic about no one
/// action. The run that reported it lists the same diagnostic with its
/// message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDiagnosticReportedEvent {
    pub code: ActionDiagnosticCode,
    pub action: Option<PdfActionType>,
    pub source: Option<ActionSource>,
}

/// `onScriptDiagnosticReported`: a script was stopped from doing something,
/// or used something the viewer doesn't support.
pub type ScriptDiagnosticReportedEvent = ScriptDiagnostic;

/// `onScriptFailed`: a script threw. `source` is `None` (`null`) for a
/// script no action started (a field's K/F/V/C script, a stamp template).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptFailedEvent {
    pub error: ScriptExecutionError,
    pub source: Option<ActionSource>,
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

// ── policy ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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

    /// `{ ...row, ...patch }`.
    pub fn merged(&self, patch: &PartialActionPolicyRow) -> ActionPolicyRow {
        ActionPolicyRow {
            user: patch.user.unwrap_or(self.user),
            hover: patch.hover.unwrap_or(self.hover),
            lifecycle: patch.lifecycle.unwrap_or(self.lifecycle),
        }
    }
}

/// `Partial<ActionPolicyRow>`: only the origins a patch changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PartialActionPolicyRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub user: Option<ActionPolicyDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub hover: Option<ActionPolicyDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub lifecycle: Option<ActionPolicyDecision>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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

/// `ActionPolicyPatch`: row-wise overrides. A row names only the origins it
/// changes; an absent or `null` row is left alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ActionPolicyPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub goto: Option<PartialActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub named: Option<PartialActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub hide: Option<PartialActionPolicyRow>,
    #[serde(rename = "reset-form", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub reset_form: Option<PartialActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub javascript: Option<PartialActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub uri: Option<PartialActionPolicyRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub print: Option<PartialActionPolicyRow>,
    #[serde(rename = "submit-form", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub submit_form: Option<PartialActionPolicyRow>,
}

/// A change to the `triggers` setting: only the families it names.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TriggerGates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub document: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub page: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "uniffi", uniffi(default = None))]
    pub annotation: Option<bool>,
}

/// The `triggers` setting: run the actions of the `document`, its `page`s,
/// or `annotation`s. A click on a link or button always runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ActionTriggers {
    pub document: bool,
    pub page: bool,
    pub annotation: bool,
}

/// When the document-open sequence fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
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

impl OpenSequence {
    pub fn as_str(self) -> &'static str {
        match self {
            OpenSequence::Auto => "auto",
            OpenSequence::Headless => "headless",
            OpenSequence::Off => "off",
        }
    }
}

/// `ActionsScriptIdentity`: who scripts think the user is, fields over the
/// document's user or a function that returns them.
pub type ActionsScriptIdentity = IdentityOverrides;

/// `ActionsSettings['javascript']`.
#[derive(Clone)]
pub struct JavaScriptSettings {
    /// Run the document's JavaScript. Off by default. Read when a document
    /// opens: its script realm exists from then on, or never.
    pub enabled: bool,
    /// Who scripts think the user is, on top of the document's user. `None`
    /// (`null`): the document's user. Read at every run.
    pub identity: Option<ActionsScriptIdentity>,
}

/// `ActionsSettings`: the plugin's settings, registered over
/// [`actions_defaults`] and changed with `update_settings` for every
/// document. `policy`, `triggers` and `javascript.identity` apply to the
/// next action; `javascript.enabled` to documents opened after the change;
/// `open_sequence` until a document's opening actions have run.
#[derive(Clone)]
pub struct ActionsSettings {
    /// The rules, per kind of action and per origin.
    pub policy: ActionPolicy,
    pub triggers: ActionTriggers,
    pub open_sequence: OpenSequence,
    pub javascript: JavaScriptSettings,
}

/// `ACTIONS_DEFAULTS`: the settings when the app registers none.
pub fn actions_defaults() -> ActionsSettings {
    ActionsSettings {
        policy: crate::services::policy::default_policy(),
        triggers: ActionTriggers { document: true, page: true, annotation: true },
        open_sequence: OpenSequence::Auto,
        javascript: JavaScriptSettings { enabled: false, identity: None },
    }
}

/// A change to `ActionsSettings['javascript']`.
#[derive(Clone, Default)]
pub struct JavaScriptSettingsPatch {
    pub enabled: Option<bool>,
    /// `Some(None)` sets `null`; `None` leaves the identity as it is.
    pub identity: Option<Option<ActionsScriptIdentity>>,
}

/// `DeepPartial<ActionsSettings>`: what `update_settings` merges. Policy
/// rows merge origin by origin; a function identity is given whole.
#[derive(Clone, Default)]
pub struct ActionsSettingsPatch {
    pub policy: Option<ActionPolicyPatch>,
    pub triggers: Option<TriggerGates>,
    pub open_sequence: Option<OpenSequence>,
    pub javascript: Option<JavaScriptSettingsPatch>,
}

/// `ActionsSettingsChangedEvent`: the settings after a change, and which
/// top-level settings changed.
#[derive(Clone)]
pub struct ActionsSettingsChanged {
    pub settings: ActionsSettings,
    pub changed: Vec<String>,
}

/// `ActionsConfig.javascript`: the `javascript` settings plus the script
/// environment, which is no setting: a document's script engine is built
/// with it when the document opens.
#[derive(Clone, Default)]
pub struct JavaScriptConfig {
    /// The `javascript.enabled` setting.
    pub enabled: Option<bool>,
    /// The `javascript.identity` setting (`Some(None)` registers `null`).
    pub identity: Option<Option<ActionsScriptIdentity>>,
    /// The sandbox. The TypeScript lazily imports QuickJS when this is
    /// absent. Rust ships no JavaScript engine: a realm built without one
    /// exists (as the TypeScript's does) and fails each sandbox build with
    /// [`NO_DEFAULT_SANDBOX`].
    pub sandbox_factory: Option<ScriptSandboxFactory>,
    pub file_name: Option<Rc<dyn Fn() -> String>>,
    /// The deterministic clock. Also the submit `sentAt`, with scripting on
    /// or off. Default: `rt::now()`.
    pub now: Option<Rc<dyn Fn() -> f64>>,
    pub utc_offset_minutes: Option<Rc<dyn Fn() -> f64>>,
    pub random_seed: Option<Rc<dyn Fn() -> f64>>,
    pub budget: Option<ScriptBudget>,
    /// The most JS nodes one dispatch may run. Default 16.
    pub max_script_nodes_per_dispatch: Option<f64>,
    /// Host input: `-new Date(nowMs).getTimezoneOffset()`, the local UTC
    /// offset in minutes at an instant, used when `utc_offset_minutes` is not
    /// configured. The TypeScript reads it from the platform. Default: zero.
    pub host_utc_offset_minutes: Option<Rc<dyn Fn(f64) -> f64>>,
}

/// The message a realm's sandbox build rejects with when the host supplied
/// no sandbox factory.
pub const NO_DEFAULT_SANDBOX: &str = "[actions] no script sandbox: Rust has no default JavaScript engine; the host must supply javascript.sandbox_factory";

/// `ActionsConfig`: any of the settings, merged over the defaults, and the
/// script environment beside `javascript`'s settings.
#[derive(Clone, Default)]
pub struct ActionsConfig {
    /// Row-wise overrides merged over the default policy.
    pub policy: Option<ActionPolicyPatch>,
    pub triggers: Option<TriggerGates>,
    pub open_sequence: Option<OpenSequence>,
    pub javascript: Option<JavaScriptConfig>,
}

// ── registration surfaces ─────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ActionExecutorResult {
    Executed,
    Inert { reason: String },
    Failed { error: String },
}

/// One node of one registered type, executed in dispatch order. Executors
/// never see the tree or the capability. `Err` is a synchronous throw; the
/// promise may reject.
pub type ActionExecutor =
    Rc<dyn Fn(&PdfActionNode, &ActionContext) -> Result<Promise<ActionExecutorResult>, Error>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum ScriptPhase {
    Boot,
    User,
}

/// Origin and phase context every script-produced UI request carries.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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
    /// A link the document asked to open. The dispatcher has already
    /// passed it through [`sanitize_external_uri`](crate::sanitize_external_uri)
    /// with the default allowlist (http, https, mailto, tel), but the string
    /// is still document-supplied: treat it as untrusted.
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

/// A disposer an identity-safe registration returns. Calling it again does
/// what it did the first time, as a TypeScript closure would.
pub type Disposer = Rc<dyn Fn()>;

/// `runDocumentVerb`'s verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "camelCase")]
pub enum DocumentVerb {
    Save,
    Print,
}

/// `ActionsToken`, shared by the public and host lenses.
pub static ACTIONS: LazyLock<Token<ActionsCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "actions",
        "add actionsPlugin() from '@embedpdf/plugin-actions' to your plugins list",
    )
});
