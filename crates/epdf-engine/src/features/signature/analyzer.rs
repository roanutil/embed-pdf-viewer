//! `SignatureAnalyzer.ts`: which revisions to compare, the fork's exact
//! object diff for each adjacent pair, each revision's structure, and the
//! pure rule engine's verdict (`analysis/`). Every revision is opened as its
//! own prefix document from the target's loaded bytes; `working-copy` first
//! snapshots the session the way a save would. Never refuses: it reports.
//!
//! The TypeScript's file-backed working copy (`EPDFDoc_OpenBaseOverlayFromPath`
//! over a scratch delta on disk) is a server path and is not ported: a layer
//! session composes its delta from memory.

use std::collections::HashSet;

use pdfium::{DerivedDocument, Document, ObjectDiff};

use crate::error::{EngineError, EngineErrorCode};
use crate::session::document::DocumentSession;

use super::analysis::types::{
    AnalysisBasis, AnalysisMode, AnalysisSince, AnalysisUntil, AnalyzeBasis, AnalyzeDetail,
    AnalyzeInput, AnalyzeSince, AnalyzeUntil, AssessmentMethod, ChangeAnalysis, ChangeFinding,
    ChangeValues, CurrentAssessment, FindingVerdict, LaterRevisions, ObjectChange,
    ObjectChangeKind, ObjectChangeType, ObjectReadStatus, ObjectReferrer, RestrictionSource,
    RevisionAnalysis, RevisionHealth, RevisionStructure, Sides, StepInput, StepRestrictions,
    StepVerdict,
};
use super::analysis::{
    EdgeResolver, assessment_of, combine, evaluate_step, parse_pdf_value, restrictions_for,
    same_effective_value,
};
use super::protection::{SIGNATURE_POLICY_VERSION, derive_protection};
use super::reader::{
    read_revisions, read_signatures_from_model, read_snapshot, read_structure, version,
    with_signature_model,
};
use super::types::{ModificationLevel, PdfRevision, SignatureSnapshot};

// Mirrors public/epdf_signature.h.
const DIFF_OLD: i32 = 0;
const DIFF_NEW: i32 = 1;

fn change_by_code(code: i32) -> ObjectChangeType {
    match code {
        0 => ObjectChangeType::Added,
        2 => ObjectChangeType::Freed,
        _ => ObjectChangeType::Modified,
    }
}

fn kind_by_code(code: i32) -> ObjectChangeKind {
    match code {
        0 => ObjectChangeKind::Dictionary,
        1 => ObjectChangeKind::Stream,
        2 => ObjectChangeKind::Array,
        4 => ObjectChangeKind::Xref,
        5 => ObjectChangeKind::Objstm,
        6 => ObjectChangeKind::Trailer,
        _ => ObjectChangeKind::Scalar,
    }
}

fn read_by_code(code: i32) -> ObjectReadStatus {
    match code {
        0 => ObjectReadStatus::Ok,
        2 => ObjectReadStatus::Absent,
        _ => ObjectReadStatus::Failed,
    }
}

fn malformed(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::MalformedPdf, message)
}

/// `anchorSet`.
fn anchor_set(s: &RevisionStructure, changed: &HashSet<u32>) -> HashSet<u32> {
    let mut out: HashSet<u32> = [0, s.root, s.pages_root].into_iter().collect();
    out.extend(s.pages.iter().copied());
    out.extend(changed.iter().copied());
    if s.acro_form != 0 {
        out.insert(s.acro_form);
    }
    for f in &s.fields {
        out.insert(f.object_number);
        out.extend(f.widgets.iter().copied());
    }
    out
}

/// A layer session's working copy borrows the layer it composes over.
enum LayerCopy<'a> {
    Loaded,
    Derived(DerivedDocument<'a>),
}

/// `openWorkingCopy` for a layer session (and `freshLayerOverBase`). Every
/// session is a layer (`OpenKind`), so upstream's plain-session branch has
/// nothing to open.
fn open_layer_working_copy<'a>(session: &'a DocumentSession) -> Result<LayerCopy<'a>, EngineError> {
    if !session.has_unsaved_edits() {
        return Ok(LayerCopy::Loaded);
    }
    let doc = session.document()?.document();
    let (delta, changed) = doc.save_layer_delta_ex().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::DocOpenFailed,
            format!("failed to save layer delta: {e}"),
        )
    })?;
    if !changed {
        return Ok(LayerCopy::Loaded);
    }
    let derived = match delta {
        None => doc
            .open_fresh_layer_over_base(session.password())
            .ok_or_else(|| malformed("the working copy could not be opened over its base"))?,
        Some(delta) => doc
            .open_base_overlay(&delta)
            .ok_or_else(|| malformed("the working copy does not compose with its base"))?,
    };
    Ok(LayerCopy::Derived(derived))
}

/// Run `body` over the session's working copy: `None` when it is the
/// loaded document itself.
fn with_working_copy<T>(
    session: &DocumentSession,
    body: impl FnOnce(Option<&Document>) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    match open_layer_working_copy(session)? {
        LayerCopy::Loaded => body(None),
        LayerCopy::Derived(doc) => body(Some(&doc)),
    }
}

/// `SignatureAnalyzer.readWorkingCopySnapshot`.
pub fn read_working_copy_snapshot(
    session: &DocumentSession,
) -> Result<SignatureSnapshot, EngineError> {
    with_working_copy(session, |copy| match copy {
        None => read_snapshot(session.document()?.document()),
        Some(target) => with_signature_model(target, |model| {
            let chain_valid = model.is_revision_chain_valid();
            let signatures = read_signatures_from_model(model)?;
            let revisions =
                if chain_valid { read_revisions(target, &signatures)? } else { Vec::new() };
            let protection = derive_protection(&signatures);
            Ok(SignatureSnapshot { chain_valid, revisions, signatures, protection })
        }),
    })
}

/// `SignatureAnalyzer.analyze`.
pub fn analyze(
    session: &DocumentSession,
    input: &AnalyzeInput,
) -> Result<ChangeAnalysis, EngineError> {
    let loaded = session.document()?.document();
    let snapshot = read_snapshot(loaded)?;
    let version = version(session)?;
    let mode = if input.exploratory_level.is_some() {
        AnalysisMode::Exploratory
    } else {
        AnalysisMode::Authoritative
    };
    let detail = input.detail.unwrap_or_default();
    let edits_version = session.mutation_sequence();

    let (since_revision, since_signature) = match input.since {
        AnalyzeSince::Signature { signature_index } => {
            let sig = snapshot.signatures.get(signature_index as usize).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("no signature {signature_index}"),
                )
            })?;
            let Some(revision_index) = sig.revision_index.filter(|_| sig.signed) else {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "signature {signature_index} seals no whole revision; nothing to analyze from"
                    ),
                ));
            };
            (revision_index, Some(sig.index))
        }
        AnalyzeSince::Revision { revision_index } => (revision_index, None),
    };
    if !snapshot.chain_valid {
        let finding = ChangeFinding {
            rule: "revision-chain".into(),
            verdict: FindingVerdict::Incomplete,
            object_number: 0,
            edge: None,
            detail: Some(
                "the cross-reference chain is broken or was rebuilt: no revision can be told from another".into(),
            ),
        };
        return Ok(ChangeAnalysis {
            mode,
            policy_version: SIGNATURE_POLICY_VERSION,
            basis: AnalysisBasis { version, edits_version, source: AnalyzeBasis::Persisted },
            since: AnalysisSince {
                revision_index: since_revision,
                signature_index: since_signature,
            },
            until: AnalysisUntil { revision_index: since_revision },
            restrictions: Vec::new(),
            current: CurrentAssessment {
                assessment: super::analysis::types::Assessment {
                    verdict: StepVerdict::Indeterminate,
                    complete: false,
                    primary: Some(finding.clone()),
                    findings: vec![finding],
                },
                method: AssessmentMethod::NetState,
            },
            later: LaterRevisions { revision_count: 0, undone_object_numbers: Vec::new() },
            verdict: StepVerdict::Indeterminate,
            steps: Vec::new(),
        });
    }
    if since_revision as usize >= snapshot.revisions.len() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("no revision {since_revision}"),
        ));
    }

    let wants_working_copy = input.until == Some(AnalyzeUntil::Basis(AnalyzeBasis::WorkingCopy));
    let judge = |target: &Document, revisions: Vec<PdfRevision>, source: AnalyzeBasis| {
        analyze_target(
            target,
            revisions,
            input,
            detail,
            since_revision,
            since_signature,
            Header { mode, version: version.clone(), edits_version, source },
        )
    };
    if !wants_working_copy {
        return judge(loaded, snapshot.revisions, AnalyzeBasis::Persisted);
    }
    with_working_copy(session, |copy| match copy {
        None => judge(loaded, snapshot.revisions.clone(), AnalyzeBasis::Persisted),
        Some(target) => {
            let signatures = with_signature_model(target, read_signatures_from_model)?;
            let revisions = read_revisions(target, &signatures)?;
            if revisions.is_empty() {
                return Err(malformed("the working copy has no valid revision chain"));
            }
            judge(target, revisions, AnalyzeBasis::WorkingCopy)
        }
    })
}

/// What every analysis of one call reports alike.
struct Header {
    mode: AnalysisMode,
    version: super::types::BaseVersionInfo,
    edits_version: u64,
    source: AnalyzeBasis,
}

/// A revision document, or the target itself when the revision is its last.
enum RevisionDoc<'a> {
    Target,
    Prefix(DerivedDocument<'a>),
}

impl RevisionDoc<'_> {
    fn doc<'b>(&'b self, target: &'b Document) -> &'b Document {
        match self {
            RevisionDoc::Target => target,
            RevisionDoc::Prefix(prefix) => prefix,
        }
    }
}

/// `openPrefix`: revision `index` of `target`'s bytes as its own document.
fn open_prefix<'a>(
    target: &'a Document,
    revisions: &[PdfRevision],
    index: u32,
) -> Result<DerivedDocument<'a>, EngineError> {
    target
        .open_revision(revisions[index as usize].end)
        .ok_or_else(|| malformed(format!("revision {index} does not open as a document")))
}

/// The rest of `analyze`, once the target and its revisions are known.
fn analyze_target(
    target: &Document,
    revisions: Vec<PdfRevision>,
    input: &AnalyzeInput,
    detail: AnalyzeDetail,
    since_revision: u32,
    since_signature: Option<u32>,
    header: Header,
) -> Result<ChangeAnalysis, EngineError> {
    let last = revisions.len() as u32 - 1;
    let mut until_revision = last;
    if let Some(AnalyzeUntil::Revision { revision_index }) = input.until {
        until_revision = revision_index;
        if until_revision < since_revision || until_revision as usize >= revisions.len() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("no revision {until_revision} at or after {since_revision}"),
            ));
        }
    }

    // The sealed revision and the judged one; nothing in between is opened
    // for the net state.
    let (before, after, changes, health) = {
        let sealed = open_prefix(target, &revisions, since_revision)?;
        let judged = if until_revision == last {
            RevisionDoc::Target
        } else {
            RevisionDoc::Prefix(open_prefix(target, &revisions, until_revision)?)
        };
        let before = read_structure(&sealed)?;
        let after = if until_revision == since_revision {
            before.clone()
        } else {
            read_structure(judged.doc(target))?
        };
        let (changes, health) = if until_revision > since_revision {
            let (changes, health) = compare(&sealed, judged.doc(target), &before, &after)?;
            (changes, Some(health))
        } else {
            (Vec::new(), None)
        };
        (before, after, changes, health)
    };

    let judged_signature = since_signature.and_then(|i| before.signatures.get(i as usize));
    let restrictions = restrictions_for(&before, judged_signature);
    let level_override: Option<ModificationLevel> = input.exploratory_level;
    let step_restrictions =
        StepRestrictions { level: restrictions.level, locks: restrictions.locks.clone() };
    let undone: Vec<u32> =
        changes.iter().filter(|c| same_effective_value(c)).map(|c| c.object_number).collect();
    let net = evaluate_step(StepInput {
        older: since_revision,
        newer: until_revision,
        changes,
        before: before.clone(),
        after,
        health,
        restrictions: Some(step_restrictions.clone()),
        level_override,
    });

    // A certification's window is also replayed revision by revision.
    let certifies =
        restrictions.anchors.iter().any(|r| r.source == RestrictionSource::Docmdp && r.own);
    let intermediate = until_revision - since_revision;
    let steps = if intermediate <= 1 {
        if intermediate == 1 { vec![net.clone()] } else { Vec::new() }
    } else if certifies || detail == AnalyzeDetail::Full {
        walk_steps(
            target,
            &revisions,
            since_revision,
            until_revision,
            before,
            &step_restrictions,
            level_override,
        )?
    } else {
        Vec::new()
    };
    let current = if certifies {
        CurrentAssessment {
            assessment: combine(&net, &steps),
            method: AssessmentMethod::NetStateReplay,
        }
    } else {
        CurrentAssessment { assessment: assessment_of(&net), method: AssessmentMethod::NetState }
    };
    let verdict = current.assessment.verdict;
    Ok(ChangeAnalysis {
        mode: header.mode,
        policy_version: SIGNATURE_POLICY_VERSION,
        basis: AnalysisBasis {
            version: header.version,
            edits_version: header.edits_version,
            source: header.source,
        },
        since: AnalysisSince { revision_index: since_revision, signature_index: since_signature },
        until: AnalysisUntil { revision_index: until_revision },
        restrictions: restrictions.anchors,
        current,
        later: LaterRevisions { revision_count: intermediate, undone_object_numbers: undone },
        verdict,
        steps,
    })
}

/// `walkSteps`: every pairwise step between `since` and `until`, two prefix
/// documents open at a time.
fn walk_steps(
    target: &Document,
    revisions: &[PdfRevision],
    since: u32,
    until: u32,
    sealed_structure: RevisionStructure,
    restrictions: &StepRestrictions,
    level_override: Option<ModificationLevel>,
) -> Result<Vec<RevisionAnalysis>, EngineError> {
    let last = revisions.len() as u32 - 1;
    let mut steps = Vec::new();
    let mut older = RevisionDoc::Prefix(open_prefix(target, revisions, since)?);
    let mut older_structure = sealed_structure;
    for r in since + 1..=until {
        let newer = if r == last {
            RevisionDoc::Target
        } else {
            RevisionDoc::Prefix(open_prefix(target, revisions, r)?)
        };
        let newer_structure = read_structure(newer.doc(target))?;
        let (changes, health) =
            compare(older.doc(target), newer.doc(target), &older_structure, &newer_structure)?;
        steps.push(evaluate_step(StepInput {
            older: r - 1,
            newer: r,
            changes,
            before: older_structure,
            after: newer_structure.clone(),
            health: Some(health),
            restrictions: Some(restrictions.clone()),
            level_override,
        }));
        older_structure = newer_structure;
        older = newer;
    }
    drop(older);
    Ok(steps)
}

/// `compare`: the fork's exact object diff between two revision documents,
/// values in pass one, anchored edges in pass two.
fn compare(
    older: &Document,
    newer: &Document,
    before: &RevisionStructure,
    after: &RevisionStructure,
) -> Result<(Vec<ObjectChange>, Sides<RevisionHealth>), EngineError> {
    let diff = older
        .compare_revisions(newer)
        .ok_or_else(|| malformed("the revisions do not share a byte history"))?;
    let count = diff.count();
    let mut changes = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count.max(0) {
        let entry = diff.entry(i).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("failed to read diff entry {i}"),
            )
        })?;
        let (old_text, old_truncated) = diff.value(i, DIFF_OLD);
        let (new_text, new_truncated) = diff.value(i, DIFF_NEW);
        let mut truncated = old_truncated || new_truncated;
        let mut old_parsed = None;
        let mut new_parsed = None;
        // A serialisation this engine cannot read carries no evidence.
        let parsed: Result<(), String> = (|| {
            old_parsed = match &old_text {
                Some(text) if !old_truncated => Some(parse_pdf_value(text)?),
                _ => None,
            };
            new_parsed = match &new_text {
                Some(text) if !new_truncated => Some(parse_pdf_value(text)?),
                _ => None,
            };
            Ok(())
        })();
        if parsed.is_err() {
            truncated = true;
        }
        let change = change_by_code(entry.change);
        changes.push(ObjectChange {
            object_number: entry.object_number,
            change,
            kind: kind_by_code(entry.kind),
            // The trailer carries no generation numbers: presence follows the change type.
            present: Sides {
                old: change != ObjectChangeType::Added,
                new: change != ObjectChangeType::Freed,
            },
            generation: Sides {
                old: (entry.old_generation >= 0).then_some(entry.old_generation),
                new: (entry.new_generation >= 0).then_some(entry.new_generation),
            },
            value: ChangeValues { old: old_parsed, new: new_parsed, truncated },
            raw: Sides { old: old_text, new: new_text },
            read: Some(Sides {
                old: read_by_code(diff.read_status(i, DIFF_OLD)),
                new: read_by_code(diff.read_status(i, DIFF_NEW)),
            }),
            stream_data_changed: entry.stream_data_changed,
            usage: Sides { old: Vec::new(), new: Vec::new() },
            usage_incomplete: false,
        });
    }
    let health = Sides { old: read_health(&diff, DIFF_OLD)?, new: read_health(&diff, DIFF_NEW)? };
    Ok((with_anchored_usage(&diff, changes, before, after), health))
}

/// `readHealth`.
fn read_health(diff: &ObjectDiff, which: i32) -> Result<RevisionHealth, EngineError> {
    let h = diff.revision_health(which).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "failed to read revision health")
    })?;
    Ok(RevisionHealth {
        sparse_xref: h.sparse_xref,
        bare_references: h.bare_references,
        referrers_complete: h.referrers_complete,
    })
}

/// `readReferrers`.
fn read_referrers(diff: &ObjectDiff, which: i32, object_number: u32) -> Vec<ObjectReferrer> {
    diff.referrers(which, object_number)
        .into_iter()
        .map(|(parent, label)| ObjectReferrer { parent, label, via: None })
        .collect()
}

/// `withAnchoredUsage`.
fn with_anchored_usage(
    diff: &ObjectDiff,
    changes: Vec<ObjectChange>,
    before: &RevisionStructure,
    after: &RevisionStructure,
) -> Vec<ObjectChange> {
    let changed: HashSet<u32> = changes.iter().map(|c| c.object_number).collect();
    let mut old_resolver =
        EdgeResolver::new(|n| read_referrers(diff, DIFF_OLD, n), anchor_set(before, &changed));
    let mut new_resolver =
        EdgeResolver::new(|n| read_referrers(diff, DIFF_NEW, n), anchor_set(after, &changed));
    changes
        .into_iter()
        .map(|mut c| {
            let old = if c.present.old {
                old_resolver.resolve(c.object_number)
            } else {
                Some(Vec::new())
            };
            let now = if c.present.new {
                new_resolver.resolve(c.object_number)
            } else {
                Some(Vec::new())
            };
            c.usage_incomplete = old.is_none() || now.is_none();
            c.usage = Sides { old: old.unwrap_or_default(), new: now.unwrap_or_default() };
            c
        })
        .collect()
}
