//! Reading the outline (`OutlineReader.ts`, `internal/readOutlineItem.ts`).
//!
//! Dictionary reads only: no page is loaded. A read never fails for a bad
//! outline: what it can't read (a loop, an item that isn't an indirect
//! object, a broken action, past the read limits) it leaves out or marks,
//! and says `incomplete`.

use std::collections::HashSet;

use pdfium::{Document, OutlineItem as Bookmark};

use super::{DocumentOutline, OutlineItem, READ_DEPTH, READ_ITEMS, parent_ref};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::read_destination;
use crate::features::actions::reader::{ActionTree, read_action_model};
use crate::features::actions::vocabulary::{ActionReadBudget, ActionReadBudgetTracker};
use crate::features::annotation::read::link::{PdfLinkTarget, link_target_from_action_tree};
use crate::identity::OutlineItemRef;

/// What one read of the outline may spend on its items' actions
/// (`OUTLINE_ACTION_READ_BUDGET`): one action model per item at most, and
/// room for a URI or a script on each. Past it, an item's action reads as
/// missing and the outline as incomplete.
pub const OUTLINE_ACTION_READ_BUDGET: ActionReadBudget = ActionReadBudget {
    max_models: 50_000,
    max_nodes: 200_000,
    max_script_code_units: 16_000_000,
    max_target_entries: 65_536,
    max_payload_code_units: 16_000_000,
};

/// `/F` bit 1.
const ITALIC: i32 = 1;
/// `/F` bit 2.
const BOLD: i32 = 2;

/// The document's outline, every item depth first (`OutlineReader.read`).
pub fn read_outline(
    document: &Document,
    cancel: &CancelToken,
) -> Result<DocumentOutline, EngineError> {
    let mut read = Read {
        budget: ActionReadBudgetTracker::new(OUTLINE_ACTION_READ_BUDGET),
        items: Vec::new(),
        seen: HashSet::new(),
        incomplete: false,
        cancel,
    };
    read.visit(document.outline_first(), super::TOP, 0)?;
    Ok(DocumentOutline { items: read.items, incomplete: read.incomplete })
}

struct Read<'a> {
    budget: ActionReadBudgetTracker,
    items: Vec<OutlineItem>,
    seen: HashSet<u32>,
    incomplete: bool,
    cancel: &'a CancelToken,
}

impl Read<'_> {
    /// Depth first; `false` once the read has to stop.
    fn visit(
        &mut self,
        first: Option<Bookmark<'_>>,
        parent: u32,
        depth: usize,
    ) -> Result<bool, EngineError> {
        let mut at = first;
        while let Some(handle) = at {
            at = handle.next_sibling();
            self.cancel.check()?;
            let object_number = handle.object_number();
            if object_number == 0 {
                // Not an indirect object: it and the items under it.
                self.incomplete = true;
                continue;
            }
            if self.seen.contains(&object_number) {
                // A loop ends the list.
                self.incomplete = true;
                break;
            }
            if self.items.len() >= READ_ITEMS {
                self.incomplete = true;
                return Ok(false);
            }
            self.seen.insert(object_number);
            let (item, complete) =
                read_outline_item(&handle, object_number, parent, &mut self.budget)?;
            self.items.push(item);
            if !complete {
                self.incomplete = true;
            }
            let Some(child) = handle.first_child() else { continue };
            if depth + 1 >= READ_DEPTH {
                self.incomplete = true;
                continue;
            }
            if !self.visit(Some(child), object_number, depth + 1)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The item `object_number`, under `parent` ([`super::TOP`] for the top
/// level), as it reads now (`OutlineReader.readItem`): `NotFound` when the
/// outline has no such item.
pub fn read_item(
    document: &Document,
    object_number: u32,
    parent: u32,
) -> Result<OutlineItem, EngineError> {
    let handle = document.outline_item(object_number).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no outline item {object_number}"),
        )
        .with_details(serde_json::json!({ "field": "ref" }))
    })?;
    let mut budget = ActionReadBudgetTracker::new(OUTLINE_ACTION_READ_BUDGET);
    Ok(read_outline_item(&handle, object_number, parent, &mut budget)?.0)
}

/// The item `handle` reads, and whether all of it could be read: its action
/// may not (`readOutlineItem`).
fn read_outline_item(
    handle: &Bookmark<'_>,
    object_number: u32,
    parent: u32,
    budget: &mut ActionReadBudgetTracker,
) -> Result<(OutlineItem, bool), EngineError> {
    let title = handle
        .title()
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the title of outline item {object_number} could not be read: {e}"),
            )
        })?
        .unwrap_or_default();
    let has_children = handle.first_child().is_some();
    let style = handle.style();
    let (action, target, complete) = read_target(handle, budget);
    let item = OutlineItem {
        reference: OutlineItemRef::new(object_number),
        parent: parent_ref(parent),
        title,
        target,
        action,
        open: has_children && handle.count() > 0,
        color: handle.color().map(color_of),
        bold: style & BOLD != 0,
        italic: style & ITALIC != 0,
    };
    Ok((item, complete))
}

/// Its target, read the way a link's is: the `/A` tree's navigation view,
/// else the `/Dest`; an action that can't be read makes it `unsupported`.
fn read_target(
    handle: &Bookmark<'_>,
    budget: &mut ActionReadBudgetTracker,
) -> (Option<ActionTree>, Option<PdfLinkTarget>, bool) {
    if handle.has_action() {
        let tree = handle.action_model().map(|model| read_action_model(&model, budget));
        return match tree {
            Some(Ok(tree)) => {
                let target = link_target_from_action_tree(&tree);
                let complete = !tree.incomplete;
                (Some(tree), Some(target), complete)
            }
            // A broken action, or the read's budget spent: this target is
            // dead.
            None | Some(Err(_)) => (None, Some(PdfLinkTarget::Unsupported), false),
        };
    }
    // No /A: the /Dest, a named one resolved; a page that is gone reads as
    // none.
    let Some(destination) = handle.destination() else {
        return (None, None, true);
    };
    let target = read_destination(&destination)
        .map_or(PdfLinkTarget::Unsupported, |destination| PdfLinkTarget::Goto { destination });
    (None, Some(target), true)
}

/// `/C` as `#rrggbb` (`colorOf`), each component rounded to 0 to 255.
fn color_of(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(|channel| (f64::from(channel) * 255.0).round().clamp(0.0, 255.0) as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}
