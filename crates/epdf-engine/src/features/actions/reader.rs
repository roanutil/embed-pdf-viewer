//! Walking a detached action model.
//!
//! G6. A transcription of the walker in
//! `services/src/features/actions/ActionModelReader.ts`. The only native-model
//! walker there is: document, page, field and annotation reads all come
//! through here.
//!
//! # The payload rule
//!
//! A node whose payload cannot be materialized degrades to [`ActionNode::
//! Unknown`] with its original `/S` kept on `subtype`, and the tree gains the
//! `payload-dropped` warning. That is what keeps impossible states
//! unrepresentable while leaving malformed documents readable: a `goto`
//! without a destination and a `uri` without a URI cannot be constructed, so
//! no consumer has to check.

use pdfium::{ActionModel, ActionNodeId};
use serde::{Deserialize, Serialize};

use super::destination::{PdfDestination, read_destination};
use super::submit_flags::{SubmitFormFlags, decode_submit_form_flags};
use super::vocabulary::{
    ActionReadBudgetTracker, ActionType, ActionWarning, ActionWarningFlags, is_valid_action_node_id,
};
use crate::error::{EngineError, EngineErrorCode};

/// One `/T` or `/Fields` entry.
///
/// Deliberately unscoped: a dictionary reference carries no page, so resolving
/// a name to widgets or an object number to an annotation is the interpreter's
/// job and never the extractor's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ActionTargetRef {
    Name { name: String },
    ObjectNumber { object_number: u32 },
}

/// SubmitForm's extracted intent.
///
/// Atomic on purpose: either every required component resolved or the node
/// carries no payload at all. A partial state is unrepresentable, and an
/// unreadable `/F` degrades the whole node instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFormPayload {
    /// The resolved `/F` URL.
    pub url: String,
    /// `None` means `/Fields` was ABSENT, so include and exclude are ignored
    /// and every field but the no-export ones is submitted. An empty list
    /// means present and empty, which submits nothing in include mode and
    /// everything eligible in exclude mode. Presence and emptiness are
    /// different states and the wire keeps them apart.
    pub fields: Option<Vec<ActionTargetRef>>,
    pub flags: SubmitFormFlags,
    /// `/CharSet`, extracted but not applied by this stack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_set: Option<String>,
}

/// One node of a normalized action tree.
///
/// Discriminated on the interpreter that would execute it. Every executable
/// arm carries its whole payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// `rename_all` renames the VARIANTS and `rename_all_fields` the fields
// inside them. Without the second one `is_map` reaches the wire as `is_map`
// where the TypeScript writes `isMap`, which the corpus comparison catches
// and a hand-written test would not.
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ActionNodeKind<D = PdfDestination> {
    Javascript {
        script: String,
    },
    /// `D` is the destination's space: [`PdfDestination`] as the reader
    /// makes it, a page-space destination at the wire
    /// (`crate::page_space::destinations`).
    Goto {
        destination: D,
    },
    Uri {
        uri: String,
        is_map: bool,
    },
    Named {
        name: String,
    },
    Hide {
        targets: Vec<ActionTargetRef>,
        hide: bool,
    },
    ResetForm {
        /// `None` means `/Fields` absent, which resets every field and makes
        /// `exclude` meaningless.
        fields: Option<Vec<ActionTargetRef>>,
        exclude: bool,
    },
    /// Reported, never executed.
    GotoRemote {
        file_path: String,
    },
    GotoEmbedded {
        file_path: String,
    },
    Launch {
        file_path: String,
    },
    /// ISO allows `/Rendition` to carry `/JS`. Preserved, not collected.
    Rendition {
        #[serde(skip_serializing_if = "Option::is_none")]
        script: Option<String>,
    },
    /// Executable only with a payload. An absent one means an older runtime
    /// extracted it, and the node stays recognized and inert.
    SubmitForm {
        #[serde(skip_serializing_if = "Option::is_none")]
        payload: Option<SubmitFormPayload>,
    },
    Thread,
    Sound,
    Movie,
    ImportData,
    SetOcgState,
    Transition,
    #[serde(rename = "goto-3d-view")]
    Goto3dView,
    Unknown,
}

/// A node, its raw subtype and its children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionNode<D = PdfDestination> {
    #[serde(flatten)]
    pub kind: ActionNodeKind<D>,
    /// The raw `/S` name, kept for unknown and future action types.
    pub subtype: String,
    /// Normalized `/Next` children, in PDF order.
    pub next: Vec<ActionNode<D>>,
}

impl<D> ActionNode<D> {
    /// The node with every destination in it converted, down its `/Next`
    /// chain (`mapActionNode`). Everything else is kept.
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<ActionNode<E>, Err> {
        let next = self
            .next
            .into_iter()
            .map(|child| child.try_map_destinations(convert))
            .collect::<Result<Vec<_>, Err>>()?;
        let kind = match self.kind {
            ActionNodeKind::Goto { destination } => {
                ActionNodeKind::Goto { destination: convert(destination)? }
            }
            ActionNodeKind::Javascript { script } => ActionNodeKind::Javascript { script },
            ActionNodeKind::Uri { uri, is_map } => ActionNodeKind::Uri { uri, is_map },
            ActionNodeKind::Named { name } => ActionNodeKind::Named { name },
            ActionNodeKind::Hide { targets, hide } => ActionNodeKind::Hide { targets, hide },
            ActionNodeKind::ResetForm { fields, exclude } => {
                ActionNodeKind::ResetForm { fields, exclude }
            }
            ActionNodeKind::GotoRemote { file_path } => ActionNodeKind::GotoRemote { file_path },
            ActionNodeKind::GotoEmbedded { file_path } => {
                ActionNodeKind::GotoEmbedded { file_path }
            }
            ActionNodeKind::Launch { file_path } => ActionNodeKind::Launch { file_path },
            ActionNodeKind::Rendition { script } => ActionNodeKind::Rendition { script },
            ActionNodeKind::SubmitForm { payload } => ActionNodeKind::SubmitForm { payload },
            ActionNodeKind::Thread => ActionNodeKind::Thread,
            ActionNodeKind::Sound => ActionNodeKind::Sound,
            ActionNodeKind::Movie => ActionNodeKind::Movie,
            ActionNodeKind::ImportData => ActionNodeKind::ImportData,
            ActionNodeKind::SetOcgState => ActionNodeKind::SetOcgState,
            ActionNodeKind::Transition => ActionNodeKind::Transition,
            ActionNodeKind::Goto3dView => ActionNodeKind::Goto3dView,
            ActionNodeKind::Unknown => ActionNodeKind::Unknown,
        };
        Ok(ActionNode { kind, subtype: self.subtype, next })
    }
}

/// One extracted action root plus the reader's safety verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTree<D = PdfDestination> {
    /// `None` when the model was valid but its root exceeded a bound.
    pub root: Option<ActionNode<D>>,
    /// A consumer must never execute a tree whose `incomplete` is true.
    pub incomplete: bool,
    /// The raw native bits, so a newer runtime's warnings survive an older
    /// SDK.
    pub warning_flags: ActionWarningFlags,
    pub warnings: Vec<ActionWarning>,
}

impl<D> ActionTree<D> {
    /// The tree with every destination in it converted (`mapActionTree`).
    pub fn try_map_destinations<E, Err>(
        self,
        convert: &mut impl FnMut(D) -> Result<E, Err>,
    ) -> Result<ActionTree<E>, Err> {
        Ok(ActionTree {
            root: self.root.map(|root| root.try_map_destinations(convert)).transpose()?,
            incomplete: self.incomplete,
            warning_flags: self.warning_flags,
            warnings: self.warnings,
        })
    }
}

fn malformed(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::MalformedPdf, message)
}

fn budget_exceeded() -> EngineError {
    EngineError::with_message(
        EngineErrorCode::MalformedPdf,
        "aggregate PDF action read budget exceeded",
    )
}

/// Read and close one detached action model.
pub fn read_action_model(
    model: &ActionModel<'_>,
    budget: &mut ActionReadBudgetTracker,
) -> Result<ActionTree, EngineError> {
    let node_count = model.node_count();
    if node_count < 0 {
        return Err(malformed(format!("invalid action node count {node_count}")));
    }
    let node_count = node_count as u32;

    if !budget.take_model() {
        return Err(budget_exceeded());
    }
    for _ in 0..node_count {
        if !budget.take_node() {
            return Err(budget_exceeded());
        }
    }

    let warning_flags = ActionWarningFlags(model.warning_flags());
    let incomplete = !model.is_complete();

    let root_raw = model.root_node_raw();
    let root_id = is_valid_action_node_id(root_raw)
        .then(|| validate_node_id(root_raw, node_count))
        .transpose()?;

    let mut state = Walk {
        model,
        budget,
        node_count,
        // The path from the root, so a `/Next` chain that loops is caught
        // rather than followed forever. PDFium reports cycles it broke through
        // `warning_flags`; this catches one it did not.
        visiting: Vec::new(),
        payload_dropped: false,
    };
    let root = root_id.map(|id| state.read_node(id)).transpose()?;
    let payload_dropped = state.payload_dropped;

    let mut warnings = warning_flags.to_warnings();
    if incomplete && !warnings.contains(&ActionWarning::Incomplete) {
        warnings.push(ActionWarning::Incomplete);
    }
    if payload_dropped {
        warnings.push(ActionWarning::PayloadDropped);
    }

    Ok(ActionTree { root, incomplete, warning_flags, warnings })
}

fn validate_node_id(raw: i64, node_count: u32) -> Result<ActionNodeId, EngineError> {
    if raw < 0 || raw >= i64::from(node_count) {
        return Err(malformed(format!(
            "action model returned node id {raw} against {node_count} nodes"
        )));
    }
    Ok(raw as ActionNodeId)
}

struct Walk<'a, 'doc> {
    model: &'a ActionModel<'doc>,
    budget: &'a mut ActionReadBudgetTracker,
    node_count: u32,
    visiting: Vec<ActionNodeId>,
    payload_dropped: bool,
}

impl Walk<'_, '_> {
    fn read_node(&mut self, node: ActionNodeId) -> Result<ActionNode, EngineError> {
        if self.visiting.contains(&node) {
            return Err(malformed(format!("action model contains a cycle at node {node}")));
        }
        self.visiting.push(node);
        let result = self.read_node_inner(node);
        self.visiting.pop();
        result
    }

    fn read_node_inner(&mut self, node: ActionNodeId) -> Result<ActionNode, EngineError> {
        let action_type = ActionType::from_code(self.model.node_type(node));
        let subtype = self.string(self.model.node_subtype(node))?.unwrap_or_default();

        let next_count = self.model.next_count(node);
        if next_count < 0 {
            return Err(malformed(format!("invalid child count {next_count} at node {node}")));
        }
        let mut next = Vec::new();
        for index in 0..next_count {
            let child = validate_node_id(self.model.next_at_raw(node, index), self.node_count)?;
            next.push(self.read_node(child)?);
        }

        let kind = self.read_kind(node, action_type)?;
        Ok(ActionNode { kind, subtype, next })
    }

    /// Mark the tree as having dropped a payload and report the node as
    /// unknown.
    fn degraded(&mut self) -> ActionNodeKind {
        self.payload_dropped = true;
        ActionNodeKind::Unknown
    }

    fn string(
        &self,
        result: Result<Option<String>, pdfium::PdfiumError>,
    ) -> Result<Option<String>, EngineError> {
        result.map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading an action payload failed: {e}"),
            )
        })
    }

    /// Charge the budget before the string is materialized, not after.
    fn payload_string(
        &mut self,
        result: Result<Option<String>, pdfium::PdfiumError>,
    ) -> Result<Option<String>, EngineError> {
        let Some(value) = self.string(result)? else {
            return Ok(None);
        };
        if !self.budget.take_payload(value.encode_utf16().count() as u32) {
            return Err(budget_exceeded());
        }
        Ok(Some(value))
    }

    fn read_kind(
        &mut self,
        node: ActionNodeId,
        action_type: ActionType,
    ) -> Result<ActionNodeKind, EngineError> {
        Ok(match action_type {
            ActionType::Javascript => match self.string(self.model.node_javascript(node))? {
                Some(script) => {
                    if !self.budget.take_script(script.encode_utf16().count() as u32) {
                        return Err(budget_exceeded());
                    }
                    ActionNodeKind::Javascript { script }
                }
                None => self.degraded(),
            },
            ActionType::Rendition => {
                // A rendition without a script is still a rendition, unlike a
                // javascript node without one.
                if self.model.node_has_javascript(node) {
                    let script = self.string(self.model.node_javascript(node))?.unwrap_or_default();
                    if !self.budget.take_script(script.encode_utf16().count() as u32) {
                        return Err(budget_exceeded());
                    }
                    ActionNodeKind::Rendition { script: Some(script) }
                } else {
                    ActionNodeKind::Rendition { script: None }
                }
            }
            ActionType::Goto => {
                match self.model.node_destination(node).as_ref().and_then(read_destination) {
                    Some(destination) => ActionNodeKind::Goto { destination },
                    None => self.degraded(),
                }
            }
            ActionType::Uri => match self.payload_string(self.model.node_uri(node))? {
                // `isMap` is not read: the fork exposes no getter for it in
                // this build, and defaulting to false matches a document that
                // omits `/IsMap`, which is nearly all of them.
                Some(uri) => ActionNodeKind::Uri { uri, is_map: false },
                None => self.degraded(),
            },
            ActionType::Named => match self.payload_string(self.model.node_name(node))? {
                Some(name) => ActionNodeKind::Named { name },
                None => self.degraded(),
            },
            ActionType::GotoRemote => match self.payload_string(self.model.node_file_path(node))? {
                Some(file_path) => ActionNodeKind::GotoRemote { file_path },
                None => self.degraded(),
            },
            ActionType::GotoEmbedded => {
                match self.payload_string(self.model.node_file_path(node))? {
                    Some(file_path) => ActionNodeKind::GotoEmbedded { file_path },
                    None => self.degraded(),
                }
            }
            ActionType::Launch => match self.payload_string(self.model.node_file_path(node))? {
                Some(file_path) => ActionNodeKind::Launch { file_path },
                None => self.degraded(),
            },
            // The target-list and form payloads need accessors this build of
            // the fork does not expose through the wrapper yet. They stay
            // recognized and inert, which is the pre-payload behaviour and
            // NOT a degraded unknown: the node type is known, only its
            // details are missing.
            ActionType::Hide => ActionNodeKind::Hide { targets: Vec::new(), hide: true },
            ActionType::ResetForm => ActionNodeKind::ResetForm { fields: None, exclude: false },
            ActionType::SubmitForm => ActionNodeKind::SubmitForm { payload: None },
            ActionType::Thread => ActionNodeKind::Thread,
            ActionType::Sound => ActionNodeKind::Sound,
            ActionType::Movie => ActionNodeKind::Movie,
            ActionType::ImportData => ActionNodeKind::ImportData,
            ActionType::SetOcgState => ActionNodeKind::SetOcgState,
            ActionType::Transition => ActionNodeKind::Transition,
            ActionType::Goto3dView => ActionNodeKind::Goto3dView,
            ActionType::Unknown => ActionNodeKind::Unknown,
        })
    }
}

/// Keep `decode_submit_form_flags` reachable from this module, so a future
/// payload read does not have to reach across.
#[allow(dead_code)]
fn decode_flags(raw: u32) -> SubmitFormFlags {
    decode_submit_form_flags(raw)
}
