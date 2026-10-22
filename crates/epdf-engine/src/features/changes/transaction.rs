//! A write as one transaction, and a change as one write
//! (`WorkerHost.handleDocumentApply`, `applyOne`, `recorded`,
//! `handleDocumentApplyChanges` and `resultOf`).
//!
//! [`in_transaction`] is the reusable part: it opens the session's write
//! transaction under an `opId`, commits it when the write succeeds and takes
//! back everything the write journaled when it fails, so a refused write
//! leaves the document as it was. Task 8.30 runs every write kind through
//! it and adds upstream's commit checks (the layer ceiling, the artifact's
//! `lastObjectNumber`).

use super::applier::{
    AppliedChange, ChangeAuthority, ChangeContext, ChangeServices, ChangeSpace, apply, undo,
};
use super::item::{ChangeResult, pages_of};
use super::ledger::KeptRecord;
use super::op::Change;
use super::record::ChangeRecord;
use crate::cancel::CancelToken;
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};
use crate::mutation::MutationMeta;
use crate::session::document::DocumentSession;

/// Run `write` in a layer transaction: this engine's own when none is open
/// (Task 8.30 opens one around every write), aborted when `write` fails, so
/// a refused write leaves nothing. A session that is not a layer opens none,
/// and keeps what a refused write wrote.
pub(crate) fn in_layer_transaction<T>(
    document: &pdfium::Document,
    write: impl FnOnce() -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    layer_transaction(document, write).map_err(|refused| refused.error)
}

/// A write [`layer_transaction`] refused, and whether the abort took back
/// what it wrote.
pub(crate) struct Refused {
    pub error: EngineError,
    pub aborted: bool,
}

/// [`in_layer_transaction`], saying whether a refused write was taken back:
/// a write that was not still changed the document, and its sequence has to
/// move.
pub(crate) fn layer_transaction<T>(
    document: &pdfium::Document,
    write: impl FnOnce() -> Result<T, EngineError>,
) -> Result<T, Refused> {
    let own = !document.layer_in_transaction() && document.layer_begin_transaction();
    match write() {
        Ok(done) => {
            if own && !document.layer_commit_transaction() {
                return Err(Refused {
                    error: EngineError::with_message(
                        EngineErrorCode::Unknown,
                        "EPDFLayer_CommitTransaction refused",
                    ),
                    aborted: false,
                });
            }
            Ok(done)
        }
        Err(error) => {
            let aborted = own && document.layer_abort_transaction();
            Err(Refused { error, aborted })
        }
    }
}

/// Run `write` in a transaction named `op_id`: commit it when it succeeds,
/// take back what it journaled when it fails, and answer its answer.
pub fn in_transaction<T>(
    session: &mut DocumentSession,
    op_id: &str,
    clock: &dyn Clock,
    write: impl FnOnce(&mut DocumentSession) -> Result<T, EngineError>,
) -> Result<T, EngineError> {
    session.begin_transaction(op_id)?;
    match write(session) {
        Ok(answer) => {
            session.commit_transaction()?;
            Ok(answer)
        }
        Err(refused) => {
            roll_back(session, clock)?;
            Err(refused)
        }
    }
}

/// Take back everything the open transaction journaled, last first, then
/// close it (`abortTransaction`). Each step meets the document exactly as
/// the write left it, so none is left alone. A step that fails leaves the
/// document holding part of the write, which is reported rather than the
/// write's own refusal.
fn roll_back(session: &mut DocumentSession, clock: &dyn Clock) -> Result<(), EngineError> {
    let journal = session.take_journal();
    let authority = ChangeAuthority::rollback();
    let cancel = CancelToken::never();
    let ChangeServices { clock, ids, fonts } = ChangeServices::of_clock(clock);
    let mut ctx = ChangeContext {
        session,
        authority: &authority,
        cancel: &cancel,
        clock,
        ids,
        fonts,
        own_fill: Default::default(),
    };
    let mut failed = None;
    for step in journal.iter().rev() {
        if let Err(e) = super::applier::run_step(&mut ctx, step) {
            failed.get_or_insert(e);
        }
    }
    session.abort_transaction();
    // What the steps removed may be a drawing the index names.
    if let Ok(drawings) = session.drawing_index() {
        drawings.borrow_mut().forget();
    }
    match failed {
        None => Ok(()),
        Some(e) => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("a refused write could not be taken back: {e}"),
        )),
    }
}

/// The refusals a change keeps under its `opId`, like a success
/// (`KEPT_REFUSALS`): the document or the caller refused it, so asking again
/// gets the same answer. Anything else applied nothing, is not kept, and may
/// run again. Upstream also keeps `PayloadTooLarge` and
/// `ObjectNumberUnavailable`, which this engine does not have yet (8.27).
pub fn is_kept_refusal(code: EngineErrorCode) -> bool {
    matches!(
        code,
        EngineErrorCode::InvalidArg
            | EngineErrorCode::NotFound
            | EngineErrorCode::Forbidden
            | EngineErrorCode::ProtectedDocument
            | EngineErrorCode::MalformedPdf
            | EngineErrorCode::LayerFull
            | EngineErrorCode::ChangeConflict
            | EngineErrorCode::UndoUnavailable
    )
}

/// A change's answer: its result, and whether it repeats an earlier answer
/// under the same `opId` (nothing ran, so nothing is published again).
#[derive(Debug)]
pub struct Answered {
    pub result: ChangeResult,
    pub replayed: bool,
}

/// One change under `op_id`, in its own transaction, kept in the session's
/// ledger (`handleDocumentApply` and `applyOne`). A change asked again under
/// its `opId` answers what it answered the first time, refusals included,
/// and a different change under it is `IdempotencyKeyReused`. An undo runs
/// the record its change left, when the same user asks and nothing ended
/// undo since.
#[allow(clippy::too_many_arguments)]
pub fn run_change(
    session: &mut DocumentSession,
    op_id: &str,
    fingerprint: String,
    change: Change,
    authority: &ChangeAuthority,
    cancel: &CancelToken,
    services: ChangeServices<'_>,
    space: ChangeSpace,
) -> Result<Answered, EngineError> {
    let clock = services.clock;
    if let Some(prior) = session.ledger().outcome(op_id) {
        return Ok(Answered { result: prior.replay(&fingerprint, op_id)?, replayed: true });
    }
    let ran = in_transaction(session, op_id, clock, |session| {
        let applied = match change {
            Change::Ops(ops) => apply(session, ops, authority, cancel, services, space)?,
            Change::UndoOf(undo_of) => {
                undo_kept(session, &undo_of, authority, cancel, services, space)?
            }
        };
        result_of(session, applied)
    });
    match ran {
        Ok((result, record)) => {
            session.ledger_mut().keep_applied(op_id, fingerprint, result.clone(), record);
            Ok(Answered { result, replayed: false })
        }
        Err(refused) => {
            if is_kept_refusal(refused.code) {
                session.ledger_mut().keep_refused(op_id, fingerprint, refused.clone());
            }
            Err(refused)
        }
    }
}

/// The undo of a change this session kept.
fn undo_kept(
    session: &mut DocumentSession,
    undo_of: &str,
    authority: &ChangeAuthority,
    cancel: &CancelToken,
    services: ChangeServices<'_>,
    space: ChangeSpace,
) -> Result<AppliedChange, EngineError> {
    let record = match session.ledger().record(undo_of) {
        None => return Err(undo_unavailable(undo_of, "expired")),
        Some(KeptRecord::Unavailable(reason)) => {
            return Err(undo_unavailable(undo_of, reason.as_str()));
        }
        // A change that was refused, or wrote nothing, leaves nothing to undo.
        Some(KeptRecord::Record(None)) => return Ok(nothing_to_undo(authority)),
        Some(KeptRecord::Record(Some(record))) => record.clone(),
    };
    // Only the user who made a change may undo it; one made by no one,
    // anyone.
    if record.user_id.is_some() && record.user_id != authority.user_id {
        return Err(EngineError::permission_denied("changes:undo", Some("target"), None));
    }
    undo(session, &record, authority, cancel, services, space)
}

fn nothing_to_undo(authority: &ChangeAuthority) -> AppliedChange {
    AppliedChange {
        items: Vec::new(),
        record: ChangeRecord { user_id: authority.user_id.clone(), steps: Vec::new() },
    }
}

fn undo_unavailable(op_id: &str, reason: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::UndoUnavailable,
        format!("change {op_id} can no longer be undone"),
    )
    .with_details(serde_json::json!({ "reason": reason }))
}

/// A change's answer, and its record when it can be undone (`resultOf`):
/// undoable exactly when its record has a step, every item's meta saying
/// so, and the change's meta naming every page its items touched.
fn result_of(
    session: &mut DocumentSession,
    applied: AppliedChange,
) -> Result<(ChangeResult, Option<ChangeRecord>), EngineError> {
    let undoable = !applied.record.steps.is_empty();
    if undoable {
        session.mark_undoable()?;
    }
    let mut items = applied.items;
    for item in &mut items {
        item.meta_mut().undoable = undoable;
    }
    let meta = MutationMeta::local(pages_of(&items), &session.write_stamp()?);
    let record = undoable.then_some(applied.record);
    Ok((ChangeResult { items, meta }, record))
}

/// One change of a server request (`ServerChange`).
#[derive(Debug)]
pub struct ServerChange {
    pub op_id: String,
    pub change: Change,
    pub authority: ChangeAuthority,
    /// For an undo of a change outside the request: the record that change
    /// left (`Some(None)`: it left none). `None`: the request sent none.
    pub record: Option<Option<ChangeRecord>>,
}

/// What one change of a server request answered (`ServerChangeOutcome`).
#[derive(Debug)]
pub enum ServerChangeOutcome {
    Applied { op_id: String, result: ChangeResult, record: Option<ChangeRecord> },
    Refused { op_id: String, error: EngineError },
}

impl ServerChangeOutcome {
    /// The outcome as the wire spells it.
    pub fn to_wire(&self) -> Result<serde_json::Value, EngineError> {
        let to = |value: serde_json::Result<serde_json::Value>| {
            value.map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::WireFormat,
                    format!("an outcome could not be serialized: {e}"),
                )
            })
        };
        Ok(match self {
            Self::Applied { op_id, result, record } => serde_json::json!({
                "opId": op_id,
                "status": "applied",
                "result": to(serde_json::to_value(result))?,
                "record": to(serde_json::to_value(record))?,
            }),
            Self::Refused { op_id, error } => serde_json::json!({
                "opId": op_id,
                "status": "refused",
                "error": error.to_wire(),
            }),
        })
    }
}

/// A server request's changes as one job (`handleDocumentApplyChanges`):
/// each in its own transaction, committed or taken back on its own, its
/// record handed back for the server to keep. An undo of an earlier change
/// of the same request runs the record this job kept for it. A refusal that
/// is not kept fails the whole job; the changes before it stay.
pub fn run_server_changes(
    session: &mut DocumentSession,
    changes: Vec<ServerChange>,
    cancel: &CancelToken,
    services: ChangeServices<'_>,
) -> Result<Vec<ServerChangeOutcome>, EngineError> {
    let clock = services.clock;
    let mut records: Vec<(String, Option<ChangeRecord>)> = Vec::new();
    let mut outcomes = Vec::with_capacity(changes.len());
    for entry in changes {
        let ServerChange { op_id, change, authority, record } = entry;
        let ran = in_transaction(session, &op_id, clock, |session| {
            let applied = match change {
                Change::Ops(ops) => {
                    apply(session, ops, &authority, cancel, services, ChangeSpace::Page)?
                }
                Change::UndoOf(undo_of) => {
                    let kept = match records.iter().find(|(id, _)| *id == undo_of) {
                        Some((_, kept)) => Some(kept.clone()),
                        None => record,
                    };
                    match kept {
                        None => return Err(undo_unavailable(&undo_of, "expired")),
                        Some(None) => nothing_to_undo(&authority),
                        Some(Some(kept)) => {
                            undo(session, &kept, &authority, cancel, services, ChangeSpace::Page)?
                        }
                    }
                }
            };
            result_of(session, applied)
        });
        match ran {
            Ok((result, record)) => {
                records.push((op_id.clone(), record.clone()));
                outcomes.push(ServerChangeOutcome::Applied { op_id, result, record });
            }
            Err(error) if is_kept_refusal(error.code) => {
                records.push((op_id.clone(), None));
                outcomes.push(ServerChangeOutcome::Refused { op_id, error });
            }
            Err(error) => return Err(error),
        }
    }
    Ok(outcomes)
}
