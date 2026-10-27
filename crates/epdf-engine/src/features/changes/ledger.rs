//! What a session keeps of its changes
//! (`services/src/features/changes/ChangeLedger.ts`).
//!
//! Every answer under its `opId`, refusals included, so a retry gets the
//! same one; and each undoable change's record, until a final change ends
//! undo for everything before it, or the capture budget runs out (the
//! oldest go first, as `expired`).

use std::collections::VecDeque;

use super::item::ChangeResult;
use super::record::{ChangeRecord, record_bytes};
use crate::error::{EngineError, EngineErrorCode};

/// The bytes of captures a session keeps for undo; the oldest records go
/// first.
const CAPTURE_BUDGET: usize = 64 * 1024 * 1024;

/// How many answers a session keeps for retries; the oldest go first.
const OUTCOME_LIMIT: usize = 4096;

/// What one change answered, kept under its `opId` (`ChangeOutcome`).
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeOutcome {
    Applied { fingerprint: String, result: ChangeResult },
    Refused { fingerprint: String, error: EngineError },
}

impl ChangeOutcome {
    fn fingerprint(&self) -> &str {
        match self {
            Self::Applied { fingerprint, .. } | Self::Refused { fingerprint, .. } => fingerprint,
        }
    }

    /// The answer a retry under the same `opId` gets: the first result or
    /// refusal, or `IdempotencyKeyReused` when the retry sent another change.
    pub fn replay(&self, fingerprint: &str, op_id: &str) -> Result<ChangeResult, EngineError> {
        if self.fingerprint() != fingerprint {
            return Err(EngineError::with_message(
                EngineErrorCode::IdempotencyKeyReused,
                format!("opId {op_id} already answered a different change"),
            ));
        }
        match self {
            Self::Applied { result, .. } => Ok(result.clone()),
            Self::Refused { error, .. } => Err(error.clone()),
        }
    }
}

/// Why a change can't be undone (`UndoUnavailableReason`), as
/// `UndoUnavailable`'s `details.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoUnavailable {
    FinalChange,
    Expired,
}

impl UndoUnavailable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FinalChange => "final-change",
            Self::Expired => "expired",
        }
    }
}

/// The record a change left, or why it can't be undone.
#[derive(Debug, Clone, PartialEq)]
pub enum KeptRecord<'a> {
    /// `None`: the change was refused or wrote nothing.
    Record(Option<&'a ChangeRecord>),
    Unavailable(UndoUnavailable),
}

#[derive(Debug)]
struct Entry {
    op_id: String,
    outcome: ChangeOutcome,
    /// Its record while it can be undone.
    record: Option<ChangeRecord>,
    bytes: usize,
    /// Why it can't be undone any more, once it can't.
    unavailable: Option<UndoUnavailable>,
}

/// One session's ledger (`ChangeLedger`).
#[derive(Debug, Default)]
pub struct ChangeLedger {
    /// Oldest first.
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl ChangeLedger {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry(&self, op_id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.op_id == op_id)
    }

    /// The answer kept under `op_id`, if any.
    pub fn outcome(&self, op_id: &str) -> Option<&ChangeOutcome> {
        self.entry(op_id).map(|entry| &entry.outcome)
    }

    /// The record of the change `op_id`, or why it can't be undone. `None`
    /// for a change this session has no answer for.
    pub fn record(&self, op_id: &str) -> Option<KeptRecord<'_>> {
        let entry = self.entry(op_id)?;
        Some(match entry.unavailable {
            Some(reason) => KeptRecord::Unavailable(reason),
            None => KeptRecord::Record(entry.record.as_ref()),
        })
    }

    /// Keep an applied change's answer, and its record when it can be
    /// undone.
    pub fn keep_applied(
        &mut self,
        op_id: &str,
        fingerprint: String,
        result: ChangeResult,
        record: Option<ChangeRecord>,
    ) {
        let bytes = record.as_ref().map_or(0, record_bytes);
        self.add(Entry {
            op_id: op_id.to_owned(),
            outcome: ChangeOutcome::Applied { fingerprint, result },
            record,
            bytes,
            unavailable: None,
        });
        self.bytes += bytes;
        self.trim_captures();
    }

    /// Keep a refusal: asking again answers it again.
    pub fn keep_refused(&mut self, op_id: &str, fingerprint: String, error: EngineError) {
        self.add(Entry {
            op_id: op_id.to_owned(),
            outcome: ChangeOutcome::Refused { fingerprint, error },
            record: None,
            bytes: 0,
            unavailable: None,
        });
    }

    /// A final change (redaction apply, flatten, signing, form repair) ended
    /// undo for itself and every change before it: their records go.
    pub fn end_undo(&mut self) {
        let mut freed = 0;
        for entry in &mut self.entries {
            if matches!(entry.outcome, ChangeOutcome::Applied { .. }) && entry.unavailable.is_none()
            {
                freed += drop_record(entry, UndoUnavailable::FinalChange);
            }
        }
        self.bytes -= freed;
    }

    /// Forget everything: the session closed.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn add(&mut self, entry: Entry) {
        // Upstream's `Map.set` replaces a kept opId where it stands.
        if let Some(existing) = self.entries.iter_mut().find(|e| e.op_id == entry.op_id) {
            self.bytes -= existing.bytes;
            *existing = entry;
            return;
        }
        self.entries.push_back(entry);
        while self.entries.len() > OUTCOME_LIMIT {
            if let Some(old) = self.entries.pop_front() {
                self.bytes -= old.bytes;
            }
        }
    }

    fn trim_captures(&mut self) {
        let mut freed = 0;
        for entry in &mut self.entries {
            if self.bytes - freed <= CAPTURE_BUDGET {
                break;
            }
            if entry.bytes > 0 {
                freed += drop_record(entry, UndoUnavailable::Expired);
            }
        }
        self.bytes -= freed;
    }
}

/// Drop an entry's record, answering the bytes it freed.
fn drop_record(entry: &mut Entry, reason: UndoUnavailable) -> usize {
    let freed = entry.bytes;
    entry.record = None;
    entry.bytes = 0;
    entry.unavailable = Some(reason);
    freed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutation::MutationMeta;

    fn result(op_id: &str) -> ChangeResult {
        ChangeResult {
            items: Vec::new(),
            meta: MutationMeta {
                affected_pages: Vec::new(),
                cache_delta: None,
                op_id: op_id.into(),
                undoable: true,
            },
        }
    }

    fn record() -> ChangeRecord {
        ChangeRecord { user_id: None, steps: Vec::new() }
    }

    #[test]
    fn a_retry_gets_the_first_answer_and_another_change_is_refused() {
        let mut ledger = ChangeLedger::new();
        ledger.keep_applied("op-1", "f1".into(), result("op-1"), Some(record()));
        let kept = ledger.outcome("op-1").unwrap();
        assert_eq!(kept.replay("f1", "op-1").unwrap(), result("op-1"));
        assert_eq!(
            kept.replay("f2", "op-1").unwrap_err().code,
            EngineErrorCode::IdempotencyKeyReused
        );

        let refusal = EngineError::new(EngineErrorCode::ChangeConflict);
        ledger.keep_refused("op-2", "f3".into(), refusal.clone());
        assert_eq!(ledger.outcome("op-2").unwrap().replay("f3", "op-2").unwrap_err(), refusal);
        assert_eq!(ledger.record("op-2"), Some(KeptRecord::Record(None)));
        assert_eq!(ledger.record("op-3"), None);
    }

    #[test]
    fn a_final_change_ends_undo_for_every_change_before_it() {
        let mut ledger = ChangeLedger::new();
        ledger.keep_applied("op-1", "f1".into(), result("op-1"), Some(record()));
        ledger.end_undo();
        assert_eq!(
            ledger.record("op-1"),
            Some(KeptRecord::Unavailable(UndoUnavailable::FinalChange))
        );
        // Its answer is still kept for a retry.
        assert!(ledger.outcome("op-1").is_some());
        ledger.keep_applied("op-2", "f2".into(), result("op-2"), Some(record()));
        assert!(matches!(ledger.record("op-2"), Some(KeptRecord::Record(Some(_)))));
    }

    #[test]
    fn the_oldest_answers_go_past_the_limit() {
        let mut ledger = ChangeLedger::new();
        for n in 0..=OUTCOME_LIMIT {
            let op = format!("op-{n}");
            ledger.keep_refused(&op, "f".into(), EngineError::new(EngineErrorCode::InvalidArg));
        }
        assert!(ledger.outcome("op-0").is_none());
        assert!(ledger.outcome("op-1").is_some());
        assert!(ledger.outcome(&format!("op-{OUTCOME_LIMIT}")).is_some());
    }
}
