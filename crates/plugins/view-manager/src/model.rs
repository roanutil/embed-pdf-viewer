//! `model.ts`: the view-manager's state, panes keyed by id, their display
//! order, the focused pane and the counter that makes pane ids unique.
//! Every function below is a pure transition; the controller applies them
//! with `ctx.state.update` ([`Controller::update_state`]).
//!
//! Panes partition the open documents: a document is in at most one pane,
//! so adding it to another pane moves it.
//!
//! The state keeps the TypeScript's reference identity: `panes`, each pane
//! and `order` are `Rc`s, a transition that `return state`s hands back the
//! same `Rc`, and one that rebuilds `panes` leaves the panes it did not
//! touch shared. The controller's events and memos compare them with
//! `Rc::ptr_eq`, as the TypeScript compares with `===`.
//!
//! [`Controller::update_state`]: epdf_kernel::Controller::update_state

use std::collections::HashSet;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::contract::PaneId;
use crate::record::JsRecord;

/// A pane: one tab strip and the document it currently shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: PaneId,
    /// Documents in this pane, in tab order.
    pub document_ids: Vec<String>,
    /// The document currently shown in this pane.
    pub active_document_id: Option<String>,
}

/// The panes by id, in JavaScript property order.
pub type Panes = JsRecord<Rc<Pane>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewManagerState {
    pub panes: Rc<Panes>,
    /// Display order of the panes.
    pub order: Rc<Vec<PaneId>>,
    pub focused_pane_id: Option<PaneId>,
    /// Monotonic counter for unique pane ids.
    pub seq: u64,
}

/// `initialViewManagerState()`.
pub fn initial_view_manager_state() -> ViewManagerState {
    ViewManagerState::default()
}

/// `nextPaneId(state)`: the id the next created pane gets.
pub fn next_pane_id(state: &ViewManagerState) -> PaneId {
    format!("pane-{}", state.seq + 1)
}

/// `paneOfDocument(state, documentId)`: the first pane in display order
/// that holds the document. An `order` entry with no pane is the
/// TypeScript's `TypeError`; only a hand-built state has one, and it holds
/// nothing here.
pub fn pane_of_document(state: &ViewManagerState, document_id: &str) -> Option<PaneId> {
    state
        .order
        .iter()
        .find(|id| {
            state.panes.get(id).is_some_and(|p| p.document_ids.iter().any(|d| d == document_id))
        })
        .cloned()
}

// ── helpers ──────────────────────────────────────────────────────────────────

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

/// `insertAt(list, item, index)`: `[...list.slice(0, at), item,
/// ...list.slice(at)]`, with `index == null` appending and anything else
/// clamped into the list.
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

/// `withoutDocument(pane, documentId)`: drop a document; if it was active,
/// its neighbour becomes active. The same pane when it does not hold it.
fn without_document(pane: &Rc<Pane>, document_id: &str) -> Rc<Pane> {
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
    Rc::new(Pane { id: pane.id.clone(), document_ids, active_document_id })
}

/// `withValidActive(pane)`: keep `activeDocumentId` pointing at a document
/// the pane holds. The empty id is falsy, so it is replaced too.
fn with_valid_active(pane: Pane) -> Pane {
    if let Some(active) = pane.active_document_id.as_deref()
        && !active.is_empty()
        && pane.document_ids.iter().any(|d| d == active)
    {
        return pane;
    }
    Pane { active_document_id: pane.document_ids.first().cloned(), ..pane }
}

/// `withPane(state, pane)`: `panes: { ...state.panes, [pane.id]: pane }`.
fn with_pane(state: &ViewManagerState, pane: Rc<Pane>) -> ViewManagerState {
    let mut panes = (*state.panes).clone();
    panes.insert(pane.id.clone(), pane);
    ViewManagerState { panes: Rc::new(panes), ..state.clone() }
}

/// A JavaScript truthy id: present and not empty.
fn truthy(id: Option<&str>) -> Option<&str> {
    id.filter(|id| !id.is_empty())
}

// ── transitions ──────────────────────────────────────────────────────────────

/// `createPane(state, index)`: an empty pane with the next id at `index`
/// (the end when left out), focused.
pub fn create_pane(state: &Rc<ViewManagerState>, index: Option<f64>) -> Rc<ViewManagerState> {
    let id = next_pane_id(state);
    let mut panes = (*state.panes).clone();
    panes.insert(
        id.clone(),
        Rc::new(Pane { id: id.clone(), document_ids: Vec::new(), active_document_id: None }),
    );
    Rc::new(ViewManagerState {
        panes: Rc::new(panes),
        order: Rc::new(insert_at(&state.order, &id, index)),
        focused_pane_id: Some(id),
        seq: state.seq + 1,
    })
}

/// `removePane(state, id)`: a removed focused pane hands the focus to the
/// last remaining one.
pub fn remove_pane(state: &Rc<ViewManagerState>, id: &str) -> Rc<ViewManagerState> {
    if !state.panes.contains_key(id) {
        return state.clone();
    }
    let mut panes = (*state.panes).clone();
    panes.remove(id);
    let order: Vec<PaneId> = state.order.iter().filter(|other| *other != id).cloned().collect();
    let focused_pane_id = if state.focused_pane_id.as_deref() == Some(id) {
        order.last().cloned()
    } else {
        state.focused_pane_id.clone()
    };
    Rc::new(ViewManagerState {
        panes: Rc::new(panes),
        order: Rc::new(order),
        focused_pane_id,
        seq: state.seq,
    })
}

/// `movePane(state, id, toIndex)`: clamped into the order; an unknown id
/// is ignored.
pub fn move_pane(state: &Rc<ViewManagerState>, id: &str, to_index: f64) -> Rc<ViewManagerState> {
    let without: Vec<PaneId> = state.order.iter().filter(|other| *other != id).cloned().collect();
    if without.len() == state.order.len() {
        return state.clone();
    }
    let at = js_max(0.0, js_min(to_index, without.len() as f64));
    let order = insert_at(&without, id, Some(at));
    Rc::new(ViewManagerState { order: Rc::new(order), ..(**state).clone() })
}

/// `setFocusedPane(state, id)`: any id, or none.
pub fn set_focused_pane(state: &Rc<ViewManagerState>, id: Option<&str>) -> Rc<ViewManagerState> {
    if state.focused_pane_id.as_deref() == id {
        return state.clone();
    }
    Rc::new(ViewManagerState { focused_pane_id: id.map(str::to_string), ..(**state).clone() })
}

/// `setActiveDocument(state, paneId, documentId)`: one of the pane's
/// documents, or none.
pub fn set_active_document(
    state: &Rc<ViewManagerState>,
    pane_id: &str,
    document_id: Option<&str>,
) -> Rc<ViewManagerState> {
    let Some(pane) = state.panes.get(pane_id) else { return state.clone() };
    if pane.active_document_id.as_deref() == document_id {
        return state.clone();
    }
    if let Some(document_id) = document_id
        && !pane.document_ids.iter().any(|d| d == document_id)
    {
        return state.clone();
    }
    let next = Pane { active_document_id: document_id.map(str::to_string), ..(**pane).clone() };
    Rc::new(with_pane(state, Rc::new(next)))
}

/// `addDocument(state, paneId, documentId, index)`: a document another pane
/// holds moves here; the first holder in display order gives it up.
pub fn add_document(
    state: &Rc<ViewManagerState>,
    pane_id: &str,
    document_id: &str,
    index: Option<f64>,
) -> Rc<ViewManagerState> {
    let Some(pane) = state.panes.get(pane_id) else { return state.clone() };
    if pane.document_ids.iter().any(|d| d == document_id) {
        return state.clone();
    }
    let holder = pane_of_document(state, document_id).filter(|id| !id.is_empty());
    let base = match holder.as_deref().and_then(|id| state.panes.get(id)) {
        Some(holder) => with_pane(state, without_document(holder, document_id)),
        None => (**state).clone(),
    };
    let Some(target) = base.panes.get(pane_id).cloned() else { return state.clone() };
    let next = Pane {
        id: target.id.clone(),
        document_ids: insert_at(&target.document_ids, document_id, index),
        active_document_id: target
            .active_document_id
            .clone()
            .or_else(|| Some(document_id.to_string())),
    };
    Rc::new(with_pane(&base, Rc::new(next)))
}

/// `removeDocument(state, paneId, documentId)`: take a document out of a
/// pane without closing it.
pub fn remove_document(
    state: &Rc<ViewManagerState>,
    pane_id: &str,
    document_id: &str,
) -> Rc<ViewManagerState> {
    let Some(pane) = state.panes.get(pane_id) else { return state.clone() };
    let next = without_document(pane, document_id);
    if Rc::ptr_eq(&next, pane) {
        return state.clone();
    }
    Rc::new(with_pane(state, next))
}

/// `moveDocumentWithin(state, paneId, documentId, toIndex)`: reorder a tab
/// inside its pane. `toIndex` goes to `insertAt`, so `None` (`undefined`
/// or `null`) appends.
pub fn move_document_within(
    state: &Rc<ViewManagerState>,
    pane_id: &str,
    document_id: &str,
    to_index: Option<f64>,
) -> Rc<ViewManagerState> {
    let Some(pane) = state.panes.get(pane_id) else { return state.clone() };
    if !pane.document_ids.iter().any(|d| d == document_id) {
        return state.clone();
    }
    let without: Vec<String> =
        pane.document_ids.iter().filter(|d| *d != document_id).cloned().collect();
    let next =
        Pane { document_ids: insert_at(&without, document_id, to_index), ..(**pane).clone() };
    Rc::new(with_pane(state, Rc::new(next)))
}

/// `moveDocumentBetween(state, fromPaneId, toPaneId, documentId, toIndex)`:
/// the document becomes the target's active one, and the target the
/// focused pane.
pub fn move_document_between(
    state: &Rc<ViewManagerState>,
    from_pane_id: &str,
    to_pane_id: &str,
    document_id: &str,
    to_index: Option<f64>,
) -> Rc<ViewManagerState> {
    let (Some(from), Some(to)) = (state.panes.get(from_pane_id), state.panes.get(to_pane_id))
    else {
        return state.clone();
    };
    if !from.document_ids.iter().any(|d| d == document_id) {
        return state.clone();
    }
    if from_pane_id == to_pane_id {
        let to_index = to_index.unwrap_or(to.document_ids.len() as f64);
        return move_document_within(state, to_pane_id, document_id, Some(to_index));
    }
    let next_from = without_document(from, document_id);
    let next_to = Pane {
        id: to.id.clone(),
        document_ids: insert_at(&to.document_ids, document_id, to_index),
        active_document_id: Some(document_id.to_string()),
    };
    let mut panes = (*state.panes).clone();
    panes.insert(next_from.id.clone(), next_from);
    panes.insert(next_to.id.clone(), Rc::new(next_to));
    Rc::new(ViewManagerState {
        panes: Rc::new(panes),
        focused_pane_id: Some(to_pane_id.to_string()),
        ..(**state).clone()
    })
}

/// `renameDocument(state, previousId, nextId)`: a document still opening
/// got its real id. It keeps its pane, its place in the tabs, and whether
/// it is shown.
pub fn rename_document(
    state: &Rc<ViewManagerState>,
    previous_id: &str,
    next_id: &str,
) -> Rc<ViewManagerState> {
    let Some(pane) = pane_of_document(state, previous_id)
        .filter(|id| !id.is_empty())
        .and_then(|id| state.panes.get(&id).cloned())
    else {
        return state.clone();
    };
    let rename = |id: &String| if id == previous_id { next_id.to_string() } else { id.clone() };
    let next = Pane {
        id: pane.id.clone(),
        document_ids: pane.document_ids.iter().map(rename).collect(),
        active_document_id: pane.active_document_id.as_ref().map(rename),
    };
    Rc::new(with_pane(state, Rc::new(next)))
}

/// `reconcile(state, open, preferPaneId)`: make the panes match the open
/// documents. Drop closed documents from every pane, then put each
/// unassigned open document into the preferred pane (the focused one by
/// default), creating a first pane if there is none. Always a new state.
pub fn reconcile(
    state: &Rc<ViewManagerState>,
    open: &[String],
    prefer_pane_id: Option<&str>,
) -> Rc<ViewManagerState> {
    let open_set: HashSet<&String> = open.iter().collect();

    let mut panes = Panes::new();
    for id in state.order.iter() {
        // An order entry with no pane is the TypeScript's TypeError; only a
        // hand-built state has one.
        let Some(pane) = state.panes.get(id) else { continue };
        let pruned = with_valid_active(Pane {
            id: pane.id.clone(),
            document_ids: pane
                .document_ids
                .iter()
                .filter(|d| open_set.contains(d))
                .cloned()
                .collect(),
            active_document_id: pane.active_document_id.clone(),
        });
        panes.insert(id.clone(), Rc::new(pruned));
    }

    let mut assigned: HashSet<&String> = HashSet::new();
    for id in state.order.iter() {
        if let Some(pane) = panes.get(id) {
            assigned.extend(pane.document_ids.iter());
        }
    }
    let unassigned: Vec<String> = open.iter().filter(|d| !assigned.contains(d)).cloned().collect();

    let mut order = state.order.clone();
    let mut focused_pane_id = state.focused_pane_id.clone();
    let mut seq = state.seq;

    if !unassigned.is_empty() {
        let target_id = truthy(prefer_pane_id)
            .filter(|id| panes.contains_key(id))
            .or_else(|| truthy(focused_pane_id.as_deref()).filter(|id| panes.contains_key(id)))
            .or_else(|| truthy(order.first().map(String::as_str)))
            .map(str::to_string);
        let target_id = match target_id {
            Some(id) => id,
            None => {
                seq += 1;
                let id = format!("pane-{seq}");
                panes.insert(
                    id.clone(),
                    Rc::new(Pane {
                        id: id.clone(),
                        document_ids: Vec::new(),
                        active_document_id: None,
                    }),
                );
                Rc::make_mut(&mut order).push(id.clone());
                if focused_pane_id.is_none() {
                    focused_pane_id = Some(id.clone());
                }
                id
            }
        };
        if let Some(target) = panes.get(&target_id).cloned() {
            let mut document_ids = target.document_ids.clone();
            document_ids.extend(unassigned);
            let next = with_valid_active(Pane { document_ids, ..(*target).clone() });
            panes.insert(target_id, Rc::new(next));
        }
    }

    Rc::new(ViewManagerState { panes: Rc::new(panes), order, focused_pane_id, seq })
}
