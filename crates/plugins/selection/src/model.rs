//! `model.ts`: the selection's session state (the range, its derived
//! page-space segments and two facts) and its pure transitions. The
//! controller applies them with `update_state`.
//!
//! Each transition takes the state's `Rc` and returns it unchanged when
//! nothing changes, as the TypeScript returns `state`: the slice lease
//! publishes by identity.

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_geometry::Quad;
use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::geometry::SelectionSegment;

pub type PageObjectNumber = u32;

/// A glyph address: a page and a character index within it. `i64`:
/// `select_all` parks its open end at `Number.MAX_SAFE_INTEGER`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphPosition {
    pub page: PageRef,
    pub glyph: i64,
}

/// Anchor is where the selection began, focus the current end. Inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionRange {
    pub anchor: GlyphPosition,
    pub focus: GlyphPosition,
}

/// Page object number to that page's segments, in page space: a
/// `Record<PageObjectNumber, _>`, whose integer keys JavaScript enumerates
/// in ascending order, as a `BTreeMap` does. A page's list is shared, so a
/// page whose segments did not change keeps the same list.
pub type SegmentsByPage = BTreeMap<PageObjectNumber, Rc<Vec<SelectionSegment>>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionState {
    pub selection: Option<SelectionRange>,
    /// Per-line segments derived from the range, per page, in page space.
    #[serde(deserialize_with = "integer_keys")]
    pub segments: SegmentsByPage,
    /// A consumer owns the selection visual (a markup tool draws its own preview).
    pub highlight_hidden: bool,
    /// The user is still selecting: a gesture is in flight.
    pub selecting: bool,
}

/// A `Record<number, _>` read back: integer keys from their string form.
fn integer_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<SegmentsByPage, D::Error> {
    let raw = BTreeMap::<String, Rc<Vec<SelectionSegment>>>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| k.parse().map(|k| (k, v)).map_err(serde::de::Error::custom))
        .collect()
}

/// `initialSelectionState()`.
pub fn initial_selection_state() -> SelectionState {
    SelectionState::default()
}

fn same_position(left: &GlyphPosition, right: &GlyphPosition) -> bool {
    left.glyph == right.glyph && left.page.page_object_number == right.page.page_object_number
}

fn same_range(left: Option<&SelectionRange>, right: Option<&SelectionRange>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(l), Some(r)) => {
            same_position(&l.anchor, &r.anchor) && same_position(&l.focus, &r.focus)
        }
        _ => false,
    }
}

/// Corner-wise `===` (NaN never equal, `-0` equal to `0`).
fn same_quad(left: &Quad, right: &Quad) -> bool {
    epdf_geometry::quad_equals(*left, *right)
}

fn same_segments(left: &Rc<Vec<SelectionSegment>>, right: &[SelectionSegment]) -> bool {
    std::ptr::eq(left.as_slice(), right)
        || (left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(l, r)| l.advance == r.advance && same_quad(&l.quad, &r.quad)))
}

/// `setSelection(state, selection, segments)`: replace the range and its
/// segments. A page whose segments are unchanged (the same advances and
/// quads) keeps its previous list, and a recompute that changes nothing
/// returns the same state.
pub fn set_selection(
    state: &Rc<SelectionState>,
    selection: SelectionRange,
    segments: SegmentsByPage,
) -> Rc<SelectionState> {
    let mut segments_changed = segments.len() != state.segments.len();
    let mut merged = SegmentsByPage::new();
    for (pon, next) in segments {
        match state.segments.get(&pon) {
            Some(previous) if same_segments(previous, &next) => {
                merged.insert(pon, previous.clone());
            }
            _ => {
                merged.insert(pon, next);
                segments_changed = true;
            }
        }
    }
    let range_changed = !same_range(state.selection.as_ref(), Some(&selection));
    if !range_changed && !segments_changed {
        return state.clone();
    }
    Rc::new(SelectionState {
        selection: if range_changed { Some(selection) } else { state.selection },
        segments: if segments_changed { merged } else { state.segments.clone() },
        ..(**state).clone()
    })
}

/// `clearSelection(state)`.
pub fn clear_selection(state: &Rc<SelectionState>) -> Rc<SelectionState> {
    if state.selection.is_none() && state.segments.is_empty() {
        return state.clone();
    }
    Rc::new(SelectionState {
        selection: None,
        segments: SegmentsByPage::new(),
        ..(**state).clone()
    })
}

/// `setHighlightHidden(state, hidden)`.
pub fn set_highlight_hidden(state: &Rc<SelectionState>, hidden: bool) -> Rc<SelectionState> {
    if state.highlight_hidden == hidden {
        return state.clone();
    }
    Rc::new(SelectionState { highlight_hidden: hidden, ..(**state).clone() })
}

/// `setSelecting(state, selecting)`.
pub fn set_selecting(state: &Rc<SelectionState>, selecting: bool) -> Rc<SelectionState> {
    if state.selecting == selecting {
        return state.clone();
    }
    Rc::new(SelectionState { selecting, ..(**state).clone() })
}

fn applied(result: &Value) -> bool {
    result.get("status").and_then(Value::as_str) == Some("applied")
}

/// `contentChangedPagesOf(event)`: the pages whose content (and so
/// character space) a confirmed event changed: applied redactions,
/// flattened pages, and the page an annotation flatten painted into. `None`
/// for every other event.
pub fn content_changed_pages_of(event: &Value) -> Option<Vec<PageRef>> {
    let results = || event.get("results").and_then(Value::as_array).into_iter().flatten();
    match event.get("type").and_then(Value::as_str) {
        Some("annotations.flattened") => {
            if results().any(applied) {
                event.get("page").and_then(PageRef::from_json).map(|page| vec![page])
            } else {
                None
            }
        }
        Some("redaction.applied" | "pages.flattened") => {
            let pages: Vec<PageRef> = results()
                .filter(|r| applied(r))
                .filter_map(|r| r.get("page").and_then(PageRef::from_json))
                .collect();
            (!pages.is_empty()).then_some(pages)
        }
        _ => None,
    }
}
