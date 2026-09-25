//! `reducer.ts`.

use std::collections::HashSet;

use crate::types::{View, ViewManagerAction, ViewManagerState};

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
fn drop_document(view: &View, document_id: &str) -> View {
    let Some(old_index) = view.document_ids.iter().position(|d| d == document_id) else {
        return view.clone();
    };
    let document_ids: Vec<String> =
        view.document_ids.iter().filter(|d| *d != document_id).cloned().collect();
    let active_document_id = if view.active_document_id.as_deref() == Some(document_id) {
        // `documentIds[Math.min(oldIndex, length - 1)] ?? null`
        document_ids.len().checked_sub(1).map(|last| document_ids[old_index.min(last)].clone())
    } else {
        view.active_document_id.clone()
    };
    View { document_ids, active_document_id, ..view.clone() }
}

/// Ensure the active document is one of the view's documents. The empty
/// id is falsy, so it is replaced too.
fn with_valid_active(view: View) -> View {
    if let Some(active) = view.active_document_id.as_deref()
        && !active.is_empty()
        && view.document_ids.iter().any(|d| d == active)
    {
        return view;
    }
    View { active_document_id: view.document_ids.first().cloned(), ..view }
}

fn set_view(state: &ViewManagerState, view: View) -> ViewManagerState {
    let mut views = state.views.clone();
    views.insert(view.id.clone(), view);
    ViewManagerState { views, ..state.clone() }
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
        ViewManagerAction::Create { view } => {
            let mut views = state.views.clone();
            views.insert(view.id.clone(), view.clone());
            let mut order = state.order.clone();
            order.push(view.id.clone());
            Some(ViewManagerState {
                views,
                order,
                focused_view_id: Some(view.id.clone()),
                seq: state.seq + 1,
            })
        }
        ViewManagerAction::Remove { id } => {
            if !state.views.contains_key(id) {
                return None;
            }
            let mut views = state.views.clone();
            views.remove(id);
            let order: Vec<String> = state.order.iter().filter(|o| *o != id).cloned().collect();
            let focused_view_id = if state.focused_view_id.as_ref() == Some(id) {
                order.last().cloned()
            } else {
                state.focused_view_id.clone()
            };
            Some(ViewManagerState { views, order, focused_view_id, seq: state.seq })
        }
        ViewManagerAction::ReorderViews { order } => {
            Some(ViewManagerState { order: order.clone(), ..state.clone() })
        }
        ViewManagerAction::SetFocused { id } => {
            Some(ViewManagerState { focused_view_id: id.clone(), ..state.clone() })
        }
        ViewManagerAction::SetActiveDoc { view_id, document_id } => {
            let view = state.views.get(view_id)?;
            if let Some(document_id) = document_id
                && !view.document_ids.contains(document_id)
            {
                return None;
            }
            Some(set_view(state, View { active_document_id: document_id.clone(), ..view.clone() }))
        }
        ViewManagerAction::AddDoc { view_id, document_id, index } => {
            let view = state.views.get(view_id)?;
            if view.document_ids.contains(document_id) {
                return None;
            }
            let document_ids = insert_at(&view.document_ids, document_id, *index);
            let active_document_id =
                view.active_document_id.clone().or_else(|| Some(document_id.clone()));
            Some(set_view(state, View { document_ids, active_document_id, ..view.clone() }))
        }
        ViewManagerAction::RemoveDoc { view_id, document_id } => {
            let view = state.views.get(view_id)?;
            Some(set_view(state, drop_document(view, document_id)))
        }
        ViewManagerAction::MoveDocWithin { view_id, document_id, to_index } => {
            let view = state.views.get(view_id)?;
            if !view.document_ids.contains(document_id) {
                return None;
            }
            let without: Vec<String> =
                view.document_ids.iter().filter(|d| *d != document_id).cloned().collect();
            let document_ids = insert_at(&without, document_id, Some(*to_index));
            Some(set_view(state, View { document_ids, ..view.clone() }))
        }
        ViewManagerAction::MoveDocBetween { from_view_id, to_view_id, document_id, to_index } => {
            let from = state.views.get(from_view_id)?;
            let to = state.views.get(to_view_id)?;
            if !from.document_ids.contains(document_id) {
                return None;
            }
            if from_view_id == to_view_id {
                return view_manager_reducer(
                    state,
                    &ViewManagerAction::MoveDocWithin {
                        view_id: to_view_id.clone(),
                        document_id: document_id.clone(),
                        to_index: to_index.unwrap_or(to.document_ids.len() as f64),
                    },
                );
            }
            let next_from = drop_document(from, document_id);
            let document_ids = insert_at(&to.document_ids, document_id, *to_index);
            let next_to =
                View { document_ids, active_document_id: Some(document_id.clone()), ..to.clone() };
            let mut views = state.views.clone();
            views.insert(next_from.id.clone(), next_from);
            views.insert(next_to.id.clone(), next_to);
            Some(ViewManagerState {
                views,
                focused_view_id: Some(to_view_id.clone()),
                ..state.clone()
            })
        }
        ViewManagerAction::Reconcile { open, prefer_view_id } => {
            Some(reconcile(state, open, prefer_view_id))
        }
    }
}

/// Make the views consistent with the open documents: drop closed ones from
/// every pane, then assign unassigned open documents (in open order) to the
/// preferred pane, else the focused one, else the first, creating a default
/// pane when there is none.
fn reconcile(
    state: &ViewManagerState,
    open: &[String],
    prefer_view_id: &Option<String>,
) -> ViewManagerState {
    let open_set: HashSet<&String> = open.iter().collect();

    // 1. prune closed documents
    let mut views = std::collections::BTreeMap::new();
    for id in &state.order {
        // An order entry with no view is the TypeScript's TypeError; the
        // reducer only ever sees consistent states.
        let Some(view) = state.views.get(id) else { continue };
        let pruned = with_valid_active(View {
            document_ids: view
                .document_ids
                .iter()
                .filter(|d| open_set.contains(d))
                .cloned()
                .collect(),
            ..view.clone()
        });
        views.insert(id.clone(), pruned);
    }

    // 2. collect unassigned open documents (preserve open order)
    let mut assigned: HashSet<&String> = HashSet::new();
    for id in &state.order {
        if let Some(view) = views.get(id) {
            assigned.extend(view.document_ids.iter());
        }
    }
    let unassigned: Vec<String> = open.iter().filter(|d| !assigned.contains(d)).cloned().collect();

    let mut order = state.order.clone();
    let mut focused_view_id = state.focused_view_id.clone();
    let mut seq = state.seq;

    if !unassigned.is_empty() {
        let mut target_id = truthy(prefer_view_id)
            .filter(|id| views.contains_key(*id))
            .or_else(|| truthy(&focused_view_id).filter(|id| views.contains_key(*id)))
            .or_else(|| order.first().filter(|id| !id.is_empty()))
            .cloned();

        let target_id = match target_id.take() {
            Some(id) => id,
            None => {
                seq += 1;
                let id = format!("view-{seq}");
                views.insert(
                    id.clone(),
                    View { id: id.clone(), document_ids: Vec::new(), active_document_id: None },
                );
                order.push(id.clone());
                if focused_view_id.is_none() {
                    focused_view_id = Some(id.clone());
                }
                id
            }
        };

        if let Some(target) = views.get(&target_id).cloned() {
            let mut document_ids = target.document_ids.clone();
            document_ids.extend(unassigned);
            views.insert(target_id, with_valid_active(View { document_ids, ..target }));
        }
    }

    ViewManagerState { views, order, focused_view_id, seq }
}
