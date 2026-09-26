//! `model.ts`: the view-manager slice, panes keyed by id, their order, the
//! focus and the id counter. Pure: every transition is here.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::contract::PaneId;
use crate::record::JsRecord;

/// A pane: one tab strip and one active document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: PaneId,
    /// Documents in this pane, in tab order.
    pub document_ids: Vec<String>,
    /// The document currently shown in this pane.
    pub active_document_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewManagerState {
    /// Panes by id, in JavaScript property order.
    pub panes: JsRecord<Pane>,
    /// Display order of the panes.
    pub order: Vec<PaneId>,
    pub focused_pane_id: Option<PaneId>,
    /// Monotonic counter for stable pane ids.
    pub seq: u64,
}

/// Index arguments are JavaScript numbers: any `f64`, NaN included, which
/// canonical JSON spells `{"$number": "NaN"}`.
pub(crate) mod js_number {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::{Value, json};

    fn to_json(n: f64) -> Value {
        if n.is_nan() {
            json!({ "$number": "NaN" })
        } else if n.is_infinite() {
            json!({ "$number": if n > 0.0 { "Infinity" } else { "-Infinity" } })
        } else {
            json!(n)
        }
    }

    fn from_json(value: &Value) -> Option<f64> {
        value.as_f64().or_else(|| match value.get("$number")?.as_str()? {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        })
    }

    pub fn serialize<S: Serializer>(n: &f64, s: S) -> Result<S::Ok, S::Error> {
        to_json(*n).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        from_json(&Value::deserialize(d)?).ok_or_else(|| D::Error::custom("expected a number"))
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(n: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
            n.map(to_json).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
            match Value::deserialize(d)? {
                Value::Null => Ok(None),
                value => {
                    from_json(&value).map(Some).ok_or_else(|| D::Error::custom("expected a number"))
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ViewManagerAction {
    #[serde(rename = "CREATE_PANE")]
    CreatePane { pane: Pane },
    #[serde(rename = "REMOVE_PANE")]
    RemovePane { id: PaneId },
    #[serde(rename = "REORDER_PANES")]
    ReorderPanes { order: Vec<PaneId> },
    #[serde(rename = "SET_FOCUSED")]
    SetFocused { id: Option<PaneId> },
    #[serde(rename = "SET_ACTIVE_DOC", rename_all = "camelCase")]
    SetActiveDoc { pane_id: PaneId, document_id: Option<String> },
    #[serde(rename = "ADD_DOC", rename_all = "camelCase")]
    AddDoc {
        pane_id: PaneId,
        document_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "js_number::option")]
        index: Option<f64>,
    },
    #[serde(rename = "REMOVE_DOC", rename_all = "camelCase")]
    RemoveDoc { pane_id: PaneId, document_id: String },
    #[serde(rename = "MOVE_DOC_WITHIN", rename_all = "camelCase")]
    MoveDocWithin {
        pane_id: PaneId,
        document_id: String,
        #[serde(with = "js_number")]
        to_index: f64,
    },
    #[serde(rename = "MOVE_DOC_BETWEEN", rename_all = "camelCase")]
    MoveDocBetween {
        from_pane_id: PaneId,
        to_pane_id: PaneId,
        document_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "js_number::option")]
        to_index: Option<f64>,
    },
    /// Reconcile panes against the open-document set (driven from `connect`).
    #[serde(rename = "RECONCILE", rename_all = "camelCase")]
    Reconcile { open: Vec<String>, prefer_pane_id: Option<PaneId> },
}

/// `initialViewManagerState()`.
pub fn initial_view_manager_state() -> ViewManagerState {
    ViewManagerState::default()
}

// ── small pure helpers ────────────────────────────────────────────────────────

/// `Math.min(a, b)`: NaN if either is.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `Math.max(a, b)`: NaN if either is.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// Where `list.slice(0, at)` ends for an `at` already clamped into
/// `[0, len]` (or NaN): `ToIntegerOrInfinity`, so NaN is 0 and a fraction
/// truncates.
pub(crate) fn slice_end(at: f64, len: usize) -> usize {
    if at.is_nan() { 0 } else { (at.trunc().max(0.0) as usize).min(len) }
}

/// `[...list.slice(0, at), item, ...list.slice(at)]`, with `index == null`
/// appending and anything else clamped into the list.
pub(crate) fn insert_at(list: &[String], item: &str, index: Option<f64>) -> Vec<String> {
    let at = match index {
        None => list.len() as f64,
        Some(index) => js_max(0.0, js_min(index, list.len() as f64)),
    };
    let at = slice_end(at, list.len());
    let mut out = Vec::with_capacity(list.len() + 1);
    out.extend_from_slice(&list[..at]);
    out.push(item.to_string());
    out.extend_from_slice(&list[at..]);
    out
}

/// Drop a document and, if it was active, choose a neighbour as active.
fn drop_document(pane: &Pane, document_id: &str) -> Pane {
    let Some(old_index) = pane.document_ids.iter().position(|d| d == document_id) else {
        return pane.clone();
    };
    let document_ids: Vec<String> =
        pane.document_ids.iter().filter(|d| *d != document_id).cloned().collect();
    let active_document_id = if pane.active_document_id.as_deref() == Some(document_id) {
        // `documentIds[Math.min(oldIndex, length - 1)] ?? null`
        document_ids.len().checked_sub(1).map(|last| document_ids[old_index.min(last)].clone())
    } else {
        pane.active_document_id.clone()
    };
    Pane { document_ids, active_document_id, ..pane.clone() }
}

/// Ensure the active document is one of the pane's documents. The empty
/// id is falsy, so it is replaced too.
fn with_valid_active(pane: Pane) -> Pane {
    if let Some(active) = pane.active_document_id.as_deref()
        && !active.is_empty()
        && pane.document_ids.iter().any(|d| d == active)
    {
        return pane;
    }
    Pane { active_document_id: pane.document_ids.first().cloned(), ..pane }
}

fn set_pane(state: &ViewManagerState, pane: Pane) -> ViewManagerState {
    let mut panes = state.panes.clone();
    panes.insert(pane.id.clone(), pane);
    ViewManagerState { panes, ..state.clone() }
}

fn truthy(id: &Option<String>) -> Option<&String> {
    id.as_ref().filter(|id| !id.is_empty())
}

// ── reducer ───────────────────────────────────────────────────────────────────

/// `None` is `return state`.
pub fn view_manager_reducer(
    state: &ViewManagerState,
    action: &ViewManagerAction,
) -> Option<ViewManagerState> {
    match action {
        ViewManagerAction::CreatePane { pane } => {
            let mut panes = state.panes.clone();
            panes.insert(pane.id.clone(), pane.clone());
            let mut order = state.order.clone();
            order.push(pane.id.clone());
            Some(ViewManagerState {
                panes,
                order,
                focused_pane_id: Some(pane.id.clone()),
                seq: state.seq + 1,
            })
        }
        ViewManagerAction::RemovePane { id } => {
            if !state.panes.contains_key(id) {
                return None;
            }
            let mut panes = state.panes.clone();
            panes.remove(id);
            let order: Vec<String> = state.order.iter().filter(|o| *o != id).cloned().collect();
            let focused_pane_id = if state.focused_pane_id.as_ref() == Some(id) {
                order.last().cloned()
            } else {
                state.focused_pane_id.clone()
            };
            Some(ViewManagerState { panes, order, focused_pane_id, seq: state.seq })
        }
        ViewManagerAction::ReorderPanes { order } => {
            Some(ViewManagerState { order: order.clone(), ..state.clone() })
        }
        ViewManagerAction::SetFocused { id } => {
            Some(ViewManagerState { focused_pane_id: id.clone(), ..state.clone() })
        }
        ViewManagerAction::SetActiveDoc { pane_id, document_id } => {
            let pane = state.panes.get(pane_id)?;
            if let Some(document_id) = document_id
                && !pane.document_ids.contains(document_id)
            {
                return None;
            }
            Some(set_pane(state, Pane { active_document_id: document_id.clone(), ..pane.clone() }))
        }
        ViewManagerAction::AddDoc { pane_id, document_id, index } => {
            let pane = state.panes.get(pane_id)?;
            if pane.document_ids.contains(document_id) {
                return None;
            }
            // Panes partition the open documents: a document already shown
            // in another pane moves, it is never duplicated.
            let holder = state.panes.values().find(|p| p.document_ids.contains(document_id));
            let base = match holder {
                Some(holder) => set_pane(state, drop_document(holder, document_id)),
                None => state.clone(),
            };
            let target = base.panes.get(pane_id)?.clone();
            let document_ids = insert_at(&target.document_ids, document_id, *index);
            let active_document_id =
                target.active_document_id.clone().or_else(|| Some(document_id.clone()));
            Some(set_pane(&base, Pane { document_ids, active_document_id, ..target }))
        }
        ViewManagerAction::RemoveDoc { pane_id, document_id } => {
            let pane = state.panes.get(pane_id)?;
            Some(set_pane(state, drop_document(pane, document_id)))
        }
        ViewManagerAction::MoveDocWithin { pane_id, document_id, to_index } => {
            let pane = state.panes.get(pane_id)?;
            if !pane.document_ids.contains(document_id) {
                return None;
            }
            let without: Vec<String> =
                pane.document_ids.iter().filter(|d| *d != document_id).cloned().collect();
            let document_ids = insert_at(&without, document_id, Some(*to_index));
            Some(set_pane(state, Pane { document_ids, ..pane.clone() }))
        }
        ViewManagerAction::MoveDocBetween { from_pane_id, to_pane_id, document_id, to_index } => {
            let from = state.panes.get(from_pane_id)?;
            let to = state.panes.get(to_pane_id)?;
            if !from.document_ids.contains(document_id) {
                return None;
            }
            if from_pane_id == to_pane_id {
                return view_manager_reducer(
                    state,
                    &ViewManagerAction::MoveDocWithin {
                        pane_id: to_pane_id.clone(),
                        document_id: document_id.clone(),
                        to_index: to_index.unwrap_or(to.document_ids.len() as f64),
                    },
                );
            }
            let next_from = drop_document(from, document_id);
            let document_ids = insert_at(&to.document_ids, document_id, *to_index);
            let next_to =
                Pane { document_ids, active_document_id: Some(document_id.clone()), ..to.clone() };
            let mut panes = state.panes.clone();
            panes.insert(next_from.id.clone(), next_from);
            panes.insert(next_to.id.clone(), next_to);
            Some(ViewManagerState {
                panes,
                focused_pane_id: Some(to_pane_id.clone()),
                ..state.clone()
            })
        }
        ViewManagerAction::Reconcile { open, prefer_pane_id } => {
            Some(reconcile(state, open, prefer_pane_id))
        }
    }
}

/// Make the panes consistent with the open documents: drop closed ones from
/// every pane, then assign unassigned open documents (in open order) to the
/// preferred pane, else the focused one, else the first, creating a default
/// pane when there is none.
fn reconcile(
    state: &ViewManagerState,
    open: &[String],
    prefer_pane_id: &Option<String>,
) -> ViewManagerState {
    let open_set: HashSet<&String> = open.iter().collect();

    // 1. prune closed documents
    let mut panes = JsRecord::new();
    for id in &state.order {
        // An order entry with no pane is the TypeScript's TypeError; the
        // reducer only ever sees consistent states.
        let Some(pane) = state.panes.get(id) else { continue };
        let pruned = with_valid_active(Pane {
            document_ids: pane
                .document_ids
                .iter()
                .filter(|d| open_set.contains(d))
                .cloned()
                .collect(),
            ..pane.clone()
        });
        panes.insert(id.clone(), pruned);
    }

    // 2. collect unassigned open documents (preserve open order)
    let mut assigned: HashSet<&String> = HashSet::new();
    for id in &state.order {
        if let Some(pane) = panes.get(id) {
            assigned.extend(pane.document_ids.iter());
        }
    }
    let unassigned: Vec<String> = open.iter().filter(|d| !assigned.contains(d)).cloned().collect();

    let mut order = state.order.clone();
    let mut focused_pane_id = state.focused_pane_id.clone();
    let mut seq = state.seq;

    if !unassigned.is_empty() {
        let mut target_id = truthy(prefer_pane_id)
            .filter(|id| panes.contains_key(id))
            .or_else(|| truthy(&focused_pane_id).filter(|id| panes.contains_key(id)))
            .or_else(|| order.first().filter(|id| !id.is_empty()))
            .cloned();

        let target_id = match target_id.take() {
            Some(id) => id,
            None => {
                seq += 1;
                let id = format!("pane-{seq}");
                panes.insert(
                    id.clone(),
                    Pane { id: id.clone(), document_ids: Vec::new(), active_document_id: None },
                );
                order.push(id.clone());
                if focused_pane_id.is_none() {
                    focused_pane_id = Some(id.clone());
                }
                id
            }
        };

        if let Some(target) = panes.get(&target_id).cloned() {
            let mut document_ids = target.document_ids.clone();
            document_ids.extend(unassigned);
            panes.insert(target_id, with_valid_active(Pane { document_ids, ..target }));
        }
    }

    ViewManagerState { panes, order, focused_pane_id, seq }
}
