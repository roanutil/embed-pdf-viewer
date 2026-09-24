//! The action vocabulary: type codes, warnings and read budgets.
//!
//! G6. The pure half of `ActionModelReader.ts`, which the TypeScript itself
//! calls "the shared vocabulary consumers use so action-shaped surfaces cannot
//! drift". Page actions, annotation actions, field actions and the document
//! catalog all speak it, so a code mapped differently in one of them is a
//! divergence in four places at once.

use serde::{Deserialize, Serialize};

/// What a node asks a viewer to do.
///
/// Discriminated on the interpreter that would execute it rather than on the
/// `/S` name, so an unrecognized name is [`ActionType::Unknown`] with the raw
/// name kept alongside rather than being dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionType {
    Unknown,
    Goto,
    GotoRemote,
    GotoEmbedded,
    Launch,
    Thread,
    Uri,
    Sound,
    Movie,
    Hide,
    Named,
    SubmitForm,
    ResetForm,
    ImportData,
    Javascript,
    SetOcgState,
    Rendition,
    Transition,
    #[serde(rename = "goto-3d-view")]
    Goto3dView,
}

impl ActionType {
    /// Decode one `EPDF_ACTION_TYPE_*` code.
    ///
    /// A code this build does not know becomes `Unknown` rather than an
    /// error: a newer PDFium adding an action type must not make a document
    /// unreadable, and a consumer already has to handle `Unknown`.
    pub fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Goto,
            2 => Self::GotoRemote,
            3 => Self::GotoEmbedded,
            4 => Self::Launch,
            5 => Self::Thread,
            6 => Self::Uri,
            7 => Self::Sound,
            8 => Self::Movie,
            9 => Self::Hide,
            10 => Self::Named,
            11 => Self::SubmitForm,
            12 => Self::ResetForm,
            13 => Self::ImportData,
            14 => Self::Javascript,
            15 => Self::SetOcgState,
            16 => Self::Rendition,
            17 => Self::Transition,
            18 => Self::Goto3dView,
            _ => Self::Unknown,
        }
    }

    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Goto => "goto",
            Self::GotoRemote => "goto-remote",
            Self::GotoEmbedded => "goto-embedded",
            Self::Launch => "launch",
            Self::Thread => "thread",
            Self::Uri => "uri",
            Self::Sound => "sound",
            Self::Movie => "movie",
            Self::Hide => "hide",
            Self::Named => "named",
            Self::SubmitForm => "submit-form",
            Self::ResetForm => "reset-form",
            Self::ImportData => "import-data",
            Self::Javascript => "javascript",
            Self::SetOcgState => "set-ocg-state",
            Self::Rendition => "rendition",
            Self::Transition => "transition",
            Self::Goto3dView => "goto-3d-view",
        }
    }
}

/// Why a tree is not the whole truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionWarning {
    /// A `/Next` chain looped and the loop was cut.
    CycleDropped,
    /// A `/Next` entry was not an action.
    MalformedNext,
    /// A budget stopped the walk before the tree ended.
    Incomplete,
    /// A node's payload could not be read, so that node became `Unknown`.
    ///
    /// Detected above PDFium rather than reported by it, which is why it has
    /// no bit in [`ActionWarningFlags`].
    PayloadDropped,
}

/// The native warning bits, retained so a newer runtime's warnings survive an
/// older SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActionWarningFlags(pub u32);

impl ActionWarningFlags {
    const CYCLE_DROPPED: u32 = 0x1;
    const MALFORMED_NEXT: u32 = 0x2;
    const INCOMPLETE: u32 = 0x4;

    pub fn cycle_dropped(self) -> bool {
        self.0 & Self::CYCLE_DROPPED != 0
    }

    pub fn malformed_next(self) -> bool {
        self.0 & Self::MALFORMED_NEXT != 0
    }

    pub fn incomplete(self) -> bool {
        self.0 & Self::INCOMPLETE != 0
    }

    /// The warnings this word names, in a stable order.
    ///
    /// Bits this build does not know are kept in [`ActionWarningFlags::0`] and
    /// omitted here, which is the point of carrying the raw word: a consumer
    /// on a newer runtime can still see that something was flagged.
    pub fn to_warnings(self) -> Vec<ActionWarning> {
        let mut out = Vec::new();
        if self.cycle_dropped() {
            out.push(ActionWarning::CycleDropped);
        }
        if self.malformed_next() {
            out.push(ActionWarning::MalformedNext);
        }
        if self.incomplete() {
            out.push(ActionWarning::Incomplete);
        }
        out
    }
}

/// `EPDF_ACTION_NODE_INVALID`, which reaches a caller as either -1 or the
/// unsigned maximum depending on how the boundary widened it.
pub fn is_valid_action_node_id(raw: i64) -> bool {
    raw != -1 && (raw as u32) != 0xffff_ffff
}

/// The aggregate guard across every action model read by one job.
///
/// PDFium's own limits are per model. A document containing many models could
/// otherwise build an unbounded detached snapshot out of individually
/// reasonable ones, which is what this counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionReadBudget {
    pub max_models: u32,
    pub max_nodes: u32,
    pub max_script_code_units: u32,
    /// Hide targets and form field entries, across the job.
    pub max_target_entries: u32,
    /// Payload string code units, across the job. Reserved BEFORE allocation,
    /// so a declared length nobody could satisfy is refused rather than
    /// attempted.
    pub max_payload_code_units: u32,
}

impl Default for ActionReadBudget {
    fn default() -> Self {
        Self {
            max_models: 512,
            max_nodes: 16_384,
            max_script_code_units: 16_000_000,
            max_target_entries: 4_096,
            max_payload_code_units: 262_144,
        }
    }
}

/// A budget being spent, which reports when it runs out rather than throwing.
#[derive(Debug, Clone)]
pub struct ActionReadBudgetTracker {
    budget: ActionReadBudget,
    models: u32,
    nodes: u32,
    script_code_units: u32,
    target_entries: u32,
    payload_code_units: u32,
}

impl ActionReadBudgetTracker {
    pub fn new(budget: ActionReadBudget) -> Self {
        Self {
            budget,
            models: 0,
            nodes: 0,
            script_code_units: 0,
            target_entries: 0,
            payload_code_units: 0,
        }
    }

    /// Take one unit of a dimension, or report that it is exhausted.
    ///
    /// Saturating rather than wrapping, so an exhausted budget stays
    /// exhausted: a counter that wrapped would start granting again.
    fn take(spent: &mut u32, limit: u32, amount: u32) -> bool {
        if *spent >= limit || amount > limit - *spent {
            *spent = limit;
            return false;
        }
        *spent = spent.saturating_add(amount);
        true
    }

    pub fn take_model(&mut self) -> bool {
        Self::take(&mut self.models, self.budget.max_models, 1)
    }

    pub fn take_node(&mut self) -> bool {
        Self::take(&mut self.nodes, self.budget.max_nodes, 1)
    }

    pub fn take_script(&mut self, code_units: u32) -> bool {
        Self::take(&mut self.script_code_units, self.budget.max_script_code_units, code_units)
    }

    pub fn take_targets(&mut self, entries: u32) -> bool {
        Self::take(&mut self.target_entries, self.budget.max_target_entries, entries)
    }

    pub fn take_payload(&mut self, code_units: u32) -> bool {
        Self::take(&mut self.payload_code_units, self.budget.max_payload_code_units, code_units)
    }
}

impl Default for ActionReadBudgetTracker {
    fn default() -> Self {
        Self::new(ActionReadBudget::default())
    }
}
