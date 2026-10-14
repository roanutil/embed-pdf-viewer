//! `packages/engine/core/src/signature/analysis/types.ts`, in the wire's
//! spelling. The TypeScript's comments explain each record.

use serde::{Deserialize, Serialize};

use crate::features::forms::snapshot::FormFieldFamily;
use crate::features::signature::types::{
    BaseVersionInfo, DocumentFieldLock, FieldLockSpec, ModificationLevel, SignatureDto,
};

use super::pdf_value::PdfValue;

/// `ObjectReferrer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReferrer {
    pub parent: u32,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectChangeType {
    Added,
    Modified,
    Freed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectReadStatus {
    Ok,
    Failed,
    Absent,
}

/// `RevisionHealth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionHealth {
    pub sparse_xref: bool,
    pub bare_references: u32,
    pub referrers_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectChangeKind {
    Dictionary,
    Stream,
    Array,
    Scalar,
    Xref,
    Objstm,
    Trailer,
}

/// The TypeScript's `{ old: T; new: T }` pairs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sides<T> {
    pub old: T,
    pub new: T,
}

/// `Side`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

pub const SIDES: [Side; 2] = [Side::Old, Side::New];

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Old => "old",
            Side::New => "new",
        }
    }
}

impl<T> Sides<T> {
    pub fn get(&self, side: Side) -> &T {
        match side {
            Side::Old => &self.old,
            Side::New => &self.new,
        }
    }
}

/// `ObjectChange['value']`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeValues {
    pub old: Option<PdfValue>,
    pub new: Option<PdfValue>,
    pub truncated: bool,
}

/// `ObjectChange`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectChange {
    pub object_number: u32,
    pub change: ObjectChangeType,
    pub kind: ObjectChangeKind,
    pub present: Sides<bool>,
    pub generation: Sides<Option<i32>>,
    pub value: ChangeValues,
    pub raw: Sides<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<Sides<ObjectReadStatus>>,
    pub stream_data_changed: bool,
    pub usage: Sides<Vec<ObjectReferrer>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub usage_incomplete: bool,
}

/// `RevisionField`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionField {
    pub object_number: u32,
    pub name: String,
    pub family: FormFieldFamily,
    pub widgets: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
}

/// `RevisionStructure`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionStructure {
    pub root: u32,
    pub acro_form: u32,
    pub pages_root: u32,
    pub pages: Vec<u32>,
    pub fields: Vec<RevisionField>,
    pub signatures: Vec<SignatureDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingVerdict {
    Permitted,
    Forbidden,
    Incomplete,
}

/// `ChangeFinding`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeFinding {
    pub rule: String,
    pub verdict: FindingVerdict,
    pub object_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `StepVerdict`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepVerdict {
    Unchanged,
    Permitted,
    Forbidden,
    Indeterminate,
}

/// `RevisionAnalysis`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionAnalysis {
    pub older: u32,
    pub newer: u32,
    pub level_in_force: ModificationLevel,
    pub locks: Vec<DocumentFieldLock>,
    pub verdict: StepVerdict,
    pub findings: Vec<ChangeFinding>,
    pub changes: Vec<ObjectChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestrictionSource {
    Docmdp,
    Fieldmdp,
    Lock,
}

/// `RestrictionAnchor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestrictionAnchor {
    pub signature_index: u32,
    pub revision_index: u32,
    pub source: RestrictionSource,
    pub own: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<FieldLockSpec>,
}

/// `Assessment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub verdict: StepVerdict,
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<ChangeFinding>,
    pub findings: Vec<ChangeFinding>,
}

/// `AnalyzeInput['since']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, rename_all = "camelCase")]
pub enum AnalyzeSince {
    #[serde(rename_all = "camelCase")]
    Signature { signature_index: u32 },
    #[serde(rename_all = "camelCase")]
    Revision { revision_index: u32 },
}

/// `AnalyzeInput['until']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnalyzeUntil {
    Basis(AnalyzeBasis),
    #[serde(rename_all = "camelCase")]
    Revision {
        revision_index: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalyzeBasis {
    Persisted,
    WorkingCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalyzeDetail {
    #[default]
    Summary,
    Full,
}

/// `AnalyzeInput`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeInput {
    pub since: AnalyzeSince,
    #[serde(default)]
    pub until: Option<AnalyzeUntil>,
    #[serde(default)]
    pub exploratory_level: Option<ModificationLevel>,
    #[serde(default)]
    pub detail: Option<AnalyzeDetail>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnalysisMode {
    Authoritative,
    Exploratory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssessmentMethod {
    #[serde(rename = "net-state")]
    NetState,
    #[serde(rename = "net-state+replay")]
    NetStateReplay,
}

/// `ChangeAnalysis['basis']`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBasis {
    pub version: BaseVersionInfo,
    pub edits_version: u64,
    pub source: AnalyzeBasis,
}

/// `ChangeAnalysis['since']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisSince {
    pub revision_index: u32,
    pub signature_index: Option<u32>,
}

/// `ChangeAnalysis['until']`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisUntil {
    pub revision_index: u32,
}

/// `ChangeAnalysis['current']`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentAssessment {
    #[serde(flatten)]
    pub assessment: Assessment,
    pub method: AssessmentMethod,
}

/// `ChangeAnalysis['later']`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaterRevisions {
    pub revision_count: u32,
    pub undone_object_numbers: Vec<u32>,
}

/// `ChangeAnalysis`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAnalysis {
    pub mode: AnalysisMode,
    pub policy_version: u32,
    pub basis: AnalysisBasis,
    pub since: AnalysisSince,
    pub until: AnalysisUntil,
    pub restrictions: Vec<RestrictionAnchor>,
    pub current: CurrentAssessment,
    pub later: LaterRevisions,
    pub verdict: StepVerdict,
    pub steps: Vec<RevisionAnalysis>,
}

/// The restrictions a step is judged under (`StepInput['restrictions']`).
#[derive(Debug, Clone, PartialEq)]
pub struct StepRestrictions {
    pub level: ModificationLevel,
    pub locks: Vec<DocumentFieldLock>,
}

/// `StepInput`.
#[derive(Debug, Clone)]
pub struct StepInput {
    pub older: u32,
    pub newer: u32,
    pub changes: Vec<ObjectChange>,
    pub before: RevisionStructure,
    pub after: RevisionStructure,
    pub health: Option<Sides<RevisionHealth>>,
    pub restrictions: Option<StepRestrictions>,
    pub level_override: Option<ModificationLevel>,
}
