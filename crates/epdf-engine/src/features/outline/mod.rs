//! The document outline (`/Root /Outlines`, the bookmarks).
//!
//! Ports `services/src/features/outline/` (`OutlineReader.ts`,
//! `OutlineMutator.ts`, `internal/readOutlineItem.ts`,
//! `internal/readOutlineTree.ts`, `internal/writeOutlineFields.ts`) and the
//! types of `core/src/dto/DocumentOutline.ts` and
//! `core/src/mutation/OutlineMutationResults.ts`, upstream 67ccb76dc.
//!
//! Items are named by object number ([`OutlineItemRef`]) and placed by
//! neighbour. The read is `outline.read`; the writes are the `document.apply`
//! ops `outline.create`, `outline.update`, `outline.move` and
//! `outline.delete` ([`crate::features::changes::outline`]). Everything here
//! is in the file's coordinates; [`crate::page_space::outline`] converts the
//! destinations at the boundary.

pub mod read;
pub mod tree;
pub mod write;

use serde::{Deserialize, Serialize};

use crate::features::actions::reader::ActionTree;
use crate::features::annotation::read::link::PdfLinkTarget;
use crate::identity::OutlineItemRef;
use crate::mutation::MutationMeta;
use crate::mutation::list_position::ListPosition;

/// The top level, as a parent: the outline's own dictionary.
pub const TOP: u32 = pdfium::OUTLINE_TOP;

/// How many items a read of the outline takes (`OUTLINE_READ_LIMITS.items`).
/// Past it, the read stops and says `incomplete`.
pub const READ_ITEMS: usize = 50_000;

/// How deep a read of the outline goes (`OUTLINE_READ_LIMITS.depth`).
pub const READ_DEPTH: usize = 128;

/// Where outline items go among one parent's children (`OutlinePosition`).
pub type OutlinePosition = ListPosition<OutlineItemRef>;

/// One item of the outline (`OutlineItem<PdfDestination>`), in the file's
/// coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineItem {
    #[serde(rename = "ref")]
    pub reference: OutlineItemRef,
    /// The item it sits under; `None` at the top level.
    pub parent: Option<OutlineItemRef>,
    /// `/Title`, as written.
    pub title: String,
    /// Where it goes, read the way a link's target is; `None` when it goes
    /// nowhere.
    pub target: Option<PdfLinkTarget>,
    /// Its `/A` as a tree; `None` without `/A`.
    pub action: Option<ActionTree>,
    /// Whether its children show when the document opens; always false for
    /// a leaf.
    pub open: bool,
    /// `/C` as `#rrggbb`, lowercase; `None` when the file sets none.
    pub color: Option<String>,
    /// `/F` bit 2.
    pub bold: bool,
    /// `/F` bit 1.
    pub italic: bool,
}

/// What `outline.read` answers (`DocumentOutline`): every item, depth first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentOutline {
    pub items: Vec<OutlineItem>,
    /// Part of the outline could not be read: a loop, an item that isn't an
    /// indirect object (with the items under it), or past the read limits.
    pub incomplete: bool,
}

/// One parent's children as a write left them, in order (`OutlineOrder`).
/// `parent: None` is the top level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineOrder {
    pub parent: Option<OutlineItemRef>,
    pub children: Vec<OutlineItemRef>,
}

/// The meta of every outline write: the items it touched
/// (`OutlineMutationMeta`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineMutationMeta {
    #[serde(flatten)]
    pub meta: MutationMeta,
    pub changed: Vec<OutlineItemRef>,
}

/// Where an item goes, or was: under `parent` ([`TOP`] for the top level),
/// right after `after` (0: first) (`OutlinePlacement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlinePlacement {
    pub item: u32,
    pub parent: u32,
    pub after: u32,
}

/// The ref of a parent number: `None` for [`TOP`].
pub fn parent_ref(parent: u32) -> Option<OutlineItemRef> {
    (parent != TOP).then(|| OutlineItemRef::new(parent))
}
