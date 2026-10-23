//! `model.ts`: the history as a value, and the pure steps that change it.
//!
//! An entry is one step of the history: one change this session staged, or
//! the changes of one typing session (its pauses). Each change is a part. A
//! part is named by the change the next step undoes: the change itself at
//! first, then the undo that undid it, then the redo that undid that undo,
//! and so on. An undo and a redo are the same thing to the engine: the undo
//! of a change.
//!
//! Entries are numbered in the order they were recorded (`seq`). The undo
//! stack holds them oldest first and the redo stack newest first, so the next
//! step is always the last of its stack, and an entry put back after a
//! refused step finds its place by its number.
//!
//! Each transition takes the history's `Rc` and returns it unchanged when the
//! TypeScript returns `history` itself: the slice lease publishes by identity,
//! so an unchanged history is no state change.

use std::collections::HashMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `ChangeLabel`: an i18n key and its values, as the change was called.
pub type ChangeLabel = Value;

/// `PredictedOp`: an op a view can predict.
pub type PredictedOp = Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPart {
    /// The order it was recorded in.
    pub seq: u64,
    /// The change it began as, which is what a refusal of that change names. Never changes.
    pub id: String,
    /// The change the next step undoes: the change itself, or its latest undo or redo.
    pub op_id: String,
    /// What the change shows: what a redo of it shows until the engine answers.
    pub forward: Vec<PredictedOp>,
    /// What undoing it shows until the engine answers.
    pub backward: Vec<PredictedOp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// The order it was recorded in.
    pub seq: u64,
    pub label: ChangeLabel,
    /// Consecutive changes with this key join it (typing); `None` for every other change.
    pub merge: Option<String>,
    /// Its changes, oldest first.
    pub parts: Vec<HistoryPart>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    /// What undo undoes, oldest first: the last is next.
    pub undo: Vec<HistoryEntry>,
    /// What redo redoes, newest first: the last is next.
    pub redo: Vec<HistoryEntry>,
    /// The merge key the newest entry still takes changes under: set when a
    /// change with a key is recorded, `None` again once anything else happens.
    pub merging: Option<String>,
    pub next_seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Undo,
    Redo,
}

/// A change this session staged, as the history records it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedChange {
    pub op_id: String,
    pub label: ChangeLabel,
    pub merge: Option<String>,
    pub shows: Vec<PredictedOp>,
    pub undo: Vec<PredictedOp>,
}

/// `emptyHistory()`.
pub fn empty_history() -> History {
    History { undo: Vec::new(), redo: Vec::new(), merging: None, next_seq: 0 }
}

/// `stacksOf(direction)`: the stack a step takes its entry from, and the one
/// it puts it on.
fn stacks_of(
    history: &mut History,
    direction: Direction,
) -> (&mut Vec<HistoryEntry>, &mut Vec<HistoryEntry>) {
    match direction {
        Direction::Undo => (&mut history.undo, &mut history.redo),
        Direction::Redo => (&mut history.redo, &mut history.undo),
    }
}

fn stack(history: &History, direction: Direction) -> &Vec<HistoryEntry> {
    match direction {
        Direction::Undo => &history.undo,
        Direction::Redo => &history.redo,
    }
}

/// `nextOf(history, direction)`: the entry the next step of `direction` takes.
pub fn next_of(history: &History, direction: Direction) -> Option<&HistoryEntry> {
    stack(history, direction).last()
}

/// How many of the newest entries `slice(-Math.max(1, limit))` keeps out of
/// `len`. `Math.max(1, NaN)` is `NaN`, and `slice(NaN)` keeps everything.
fn kept(len: usize, limit: f64) -> usize {
    let limit = if limit.is_nan() { return len } else { limit.max(1.0) };
    // `slice(-n)` truncates `n` toward zero; past the length it keeps all.
    let n = limit.trunc();
    if n >= len as f64 { len } else { n as usize }
}

/// `recorded(history, change, limit)`: a new change. It joins the newest
/// entry when typing goes on under the same key, else it is a new entry on
/// top, and the oldest go past `limit`. Anything that could be redone is
/// forgotten.
pub fn recorded(history: &Rc<History>, change: &RecordedChange, limit: f64) -> Rc<History> {
    let part = HistoryPart {
        seq: history.next_seq,
        id: change.op_id.clone(),
        op_id: change.op_id.clone(),
        forward: change.shows.clone(),
        backward: change.undo.clone(),
    };
    if let Some(top) = history.undo.last()
        && change.merge.is_some()
        && history.merging == change.merge
    {
        let mut undo = history.undo.clone();
        let mut top = top.clone();
        top.parts.push(part);
        *undo.last_mut().unwrap() = top;
        return Rc::new(History {
            undo,
            redo: Vec::new(),
            merging: change.merge.clone(),
            next_seq: history.next_seq + 1,
        });
    }
    let entry = HistoryEntry {
        seq: part.seq,
        label: change.label.clone(),
        merge: change.merge.clone(),
        parts: vec![part],
    };
    let mut undo = history.undo.clone();
    undo.push(entry);
    let keep = kept(undo.len(), limit);
    let undo = undo.split_off(undo.len() - keep);
    Rc::new(History {
        undo,
        redo: Vec::new(),
        merging: change.merge.clone(),
        next_seq: history.next_seq + 1,
    })
}

/// `stepped(history, direction, seq, opIds)`: a step was staged. The entry
/// `seq` leaves the top of its stack for the top of the other, each part now
/// named by the change that staged it (`op_ids`, by part id).
pub fn stepped(
    history: &Rc<History>,
    direction: Direction,
    seq: u64,
    op_ids: &HashMap<String, String>,
) -> Rc<History> {
    let Some(at) = stack(history, direction).iter().position(|entry| entry.seq == seq) else {
        return history.clone();
    };
    let mut next = (**history).clone();
    let (from, to) = stacks_of(&mut next, direction);
    let mut moved = from.remove(at);
    for part in &mut moved.parts {
        if let Some(op_id) = op_ids.get(&part.id) {
            part.op_id = op_id.clone();
        }
    }
    to.push(moved);
    next.merging = None;
    Rc::new(next)
}

/// `withoutParts(entries, drop)`: every entry without the parts `drop` picks,
/// and without the entries left with none. `None` when nothing was dropped.
fn without_parts(
    entries: &[HistoryEntry],
    drop: impl Fn(&HistoryPart) -> bool,
) -> Option<Vec<HistoryEntry>> {
    let mut changed = false;
    let mut kept = Vec::with_capacity(entries.len());
    for entry in entries {
        let parts: Vec<HistoryPart> = entry.parts.iter().filter(|p| !drop(p)).cloned().collect();
        if parts.len() == entry.parts.len() {
            kept.push(entry.clone());
            continue;
        }
        changed = true;
        if !parts.is_empty() {
            kept.push(HistoryEntry { parts, ..entry.clone() });
        }
    }
    changed.then_some(kept)
}

/// `forgotten(history, id)`: the change `id` never happened (the engine
/// refused it), or did nothing that can be undone. Its part goes, from
/// whichever stack holds it.
pub fn forgotten(history: &Rc<History>, id: &str) -> Rc<History> {
    let undo = without_parts(&history.undo, |part| part.id == id);
    let redo = without_parts(&history.redo, |part| part.id == id);
    if undo.is_none() && redo.is_none() {
        return history.clone();
    }
    Rc::new(History {
        undo: undo.unwrap_or_else(|| history.undo.clone()),
        redo: redo.unwrap_or_else(|| history.redo.clone()),
        merging: history.merging.clone(),
        next_seq: history.next_seq,
    })
}

/// `stepRefused(history, direction, entry, part)`: the engine refused one
/// change of a step, so that part didn't move. It goes back where it was, as
/// `part` was before the step, into its entry there (or a new one with the
/// entry's number, in its place), wherever a later step had put it since.
/// Its other parts stay where the step put them.
pub fn step_refused(
    history: &Rc<History>,
    direction: Direction,
    entry: &HistoryEntry,
    part: &HistoryPart,
) -> Rc<History> {
    let left = forgotten(history, &part.id);
    let mut next = (*left).clone();
    let (back, _) = stacks_of(&mut next, direction);
    match back.iter().position(|candidate| candidate.seq == entry.seq) {
        Some(at) => {
            back[at].parts.push(part.clone());
            // `Array.prototype.sort` is stable, as `sort_by_key` is.
            back[at].parts.sort_by_key(|p| p.seq);
        }
        None => {
            // The undo stack is oldest first, the redo stack newest first.
            let index = back
                .iter()
                .filter(|candidate| match direction {
                    Direction::Undo => candidate.seq < entry.seq,
                    Direction::Redo => candidate.seq > entry.seq,
                })
                .count();
            back.insert(
                index,
                HistoryEntry {
                    seq: entry.seq,
                    label: entry.label.clone(),
                    merge: entry.merge.clone(),
                    parts: vec![part.clone()],
                },
            );
        }
    }
    next.merging = None;
    Rc::new(next)
}

/// `unavailable(history, seq)`: the engine can no longer undo the entry
/// `seq` (a final change or a new version came after it, or it is too old).
/// It goes, and so does everything recorded before it. Always a new history,
/// as the TypeScript's.
pub fn unavailable(history: &Rc<History>, seq: u64) -> Rc<History> {
    let newer = |entries: &[HistoryEntry]| {
        entries.iter().filter(|entry| entry.seq > seq).cloned().collect::<Vec<_>>()
    };
    Rc::new(History {
        undo: newer(&history.undo),
        redo: newer(&history.redo),
        merging: None,
        next_seq: history.next_seq,
    })
}

/// `cleared(history)`: nothing can be undone or redone.
pub fn cleared(history: &Rc<History>) -> Rc<History> {
    if history.undo.is_empty() && history.redo.is_empty() && history.merging.is_none() {
        return history.clone();
    }
    Rc::new(History {
        undo: Vec::new(),
        redo: Vec::new(),
        merging: None,
        next_seq: history.next_seq,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn change(op_id: &str, key: &str, merge: Option<&str>) -> RecordedChange {
        RecordedChange {
            op_id: op_id.into(),
            label: json!({ "key": key }),
            merge: merge.map(str::to_string),
            shows: vec![json!(op_id)],
            undo: vec![json!(format!("undo {op_id}"))],
        }
    }

    fn keys(entries: &[HistoryEntry]) -> Vec<String> {
        entries.iter().map(|e| e.label["key"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn recording_merges_under_one_key_and_trims_to_the_limit() {
        let mut h = Rc::new(empty_history());
        h = recorded(&h, &change("1", "text", Some("k")), 100.0);
        h = recorded(&h, &change("2", "text", Some("k")), 100.0);
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0].parts.len(), 2);
        h = recorded(&h, &change("3", "move", None), 2.0);
        h = recorded(&h, &change("4", "move2", None), 2.0);
        assert_eq!(keys(&h.undo), ["move", "move2"]);
        assert_eq!(h.next_seq, 4);
        // `Math.max(1, 0)` keeps one; NaN keeps everything.
        assert_eq!(recorded(&h, &change("5", "x", None), 0.0).undo.len(), 1);
        assert_eq!(recorded(&h, &change("5", "x", None), f64::NAN).undo.len(), 3);
        assert_eq!(recorded(&h, &change("5", "x", None), 2.9).undo.len(), 2);
    }

    #[test]
    fn unchanged_transitions_keep_the_identity() {
        let h = Rc::new(empty_history());
        assert!(Rc::ptr_eq(&cleared(&h), &h));
        assert!(Rc::ptr_eq(&forgotten(&h, "nope"), &h));
        assert!(Rc::ptr_eq(&stepped(&h, Direction::Undo, 0, &HashMap::new()), &h));
        // `unavailable` always makes a new value, as the TypeScript does.
        assert!(!Rc::ptr_eq(&unavailable(&h, 0), &h));
    }

    #[test]
    fn a_step_moves_the_entry_and_renames_its_parts() {
        let h = recorded(&Rc::new(empty_history()), &change("1", "a", None), 100.0);
        let h = stepped(&h, Direction::Undo, 0, &HashMap::from([("1".into(), "u1".into())]));
        assert!(h.undo.is_empty());
        assert_eq!(h.redo[0].parts[0].op_id, "u1");
        assert_eq!(h.redo[0].parts[0].id, "1");
        assert_eq!(next_of(&h, Direction::Redo).unwrap().seq, 0);
    }

    #[test]
    fn a_refused_part_goes_back_in_its_place() {
        let mut h = Rc::new(empty_history());
        for (id, key) in [("1", "a"), ("2", "b"), ("3", "c")] {
            h = recorded(&h, &change(id, key, None), 100.0);
        }
        let entry = h.undo[1].clone();
        let h = stepped(&h, Direction::Undo, 1, &HashMap::from([("2".into(), "u2".into())]));
        let h = step_refused(&h, Direction::Undo, &entry, &entry.parts[0]);
        assert_eq!(keys(&h.undo), ["a", "b", "c"]);
        assert!(h.redo.is_empty());
        assert_eq!(h.undo[1].parts[0].op_id, "2");
    }

    #[test]
    fn an_unavailable_entry_goes_with_everything_before() {
        let mut h = Rc::new(empty_history());
        for (id, key) in [("1", "a"), ("2", "b"), ("3", "c")] {
            h = recorded(&h, &change(id, key, None), 100.0);
        }
        assert_eq!(keys(&unavailable(&h, 1).undo), ["c"]);
    }
}
