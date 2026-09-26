//! `model.ts`: the range, its derived segments, and two facts.

use std::collections::BTreeMap;

use epdf_kernel::geometry::PageRef;
use epdf_plugin_interaction::page_ref_json;
use serde::{Deserialize, Serialize};

use crate::geometry::SelectionSegment;

pub type PageObjectNumber = u32;

/// A page plus a flat character index within it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlyphPointer {
    #[serde(with = "page_ref_json")]
    pub page: PageRef,
    pub glyph: i64,
}

/// Anchor is where the selection began, focus the current end. Inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionRange {
    pub anchor: GlyphPointer,
    pub focus: GlyphPointer,
}

/// The slice. `segments` and `loaded` are `Record<number, _>` keyed by page
/// object number: integer keys, which JavaScript enumerates in ascending
/// order, as a `BTreeMap` does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionState {
    pub selection: Option<SelectionRange>,
    pub segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>>,
    /// Pages whose text geometry has loaded.
    pub loaded: BTreeMap<PageObjectNumber, bool>,
    /// A consumer owns the selection visual.
    pub highlight_hidden: bool,
    /// A selection gesture is in flight (pointer-down to pointer-up).
    pub gesture_active: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SelectionAction {
    PageLoaded {
        #[serde(with = "page_ref_json")]
        page: PageRef,
    },
    Set {
        selection: SelectionRange,
        #[serde(deserialize_with = "integer_keys")]
        segments: BTreeMap<PageObjectNumber, Vec<SelectionSegment>>,
    },
    Clear,
    SetHighlightHidden {
        hidden: bool,
    },
    SetGestureActive {
        active: bool,
    },
}

/// A `Record<number, _>` read back inside a tagged enum, where serde buffers
/// the map and no longer parses integer keys from their string form.
fn integer_keys<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<PageObjectNumber, Vec<SelectionSegment>>, D::Error> {
    let raw = BTreeMap::<String, Vec<SelectionSegment>>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| k.parse().map(|k| (k, v)).map_err(serde::de::Error::custom))
        .collect()
}

/// `initialSelectionState()`.
pub fn initial_selection_state() -> SelectionState {
    SelectionState::default()
}

/// `reduceSelection`. Pure; `None` is the TypeScript `return state`.
pub fn reduce_selection(
    state: &SelectionState,
    action: &SelectionAction,
) -> Option<SelectionState> {
    match action {
        SelectionAction::PageLoaded { page } => {
            let pon = page.page_object_number;
            if state.loaded.get(&pon).copied().unwrap_or(false) {
                return None;
            }
            let mut loaded = state.loaded.clone();
            loaded.insert(pon, true);
            Some(SelectionState { loaded, ..state.clone() })
        }
        SelectionAction::Set { selection, segments } => Some(SelectionState {
            selection: Some(*selection),
            segments: segments.clone(),
            ..state.clone()
        }),
        SelectionAction::Clear => {
            if state.selection.is_none() && state.segments.is_empty() {
                return None;
            }
            Some(SelectionState { selection: None, segments: BTreeMap::new(), ..state.clone() })
        }
        SelectionAction::SetHighlightHidden { hidden } => (state.highlight_hidden != *hidden)
            .then(|| SelectionState { highlight_hidden: *hidden, ..state.clone() }),
        SelectionAction::SetGestureActive { active } => (state.gesture_active != *active)
            .then(|| SelectionState { gesture_active: *active, ..state.clone() }),
    }
}
