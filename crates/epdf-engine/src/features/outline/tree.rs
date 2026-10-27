//! The outline's shape (`internal/readOutlineTree.ts`).

use std::collections::HashMap;

use pdfium::{Document, OutlineItem as Bookmark};

use super::{READ_DEPTH, TOP};

/// The outline's shape as the document links it now, by object number:
/// each item's parent ([`TOP`] for a top-level item) and each parent's
/// children in order (`OutlineTree`). Only what a write needs: no titles, no
/// targets. An item that isn't an indirect object is left out with the
/// items under it, a loop ends its list, and items deeper than a read goes
/// aren't in it, as every read of the outline does.
#[derive(Debug, Clone, Default)]
pub struct OutlineTree {
    children_by_parent: HashMap<u32, Vec<u32>>,
    parent_by_item: HashMap<u32, u32>,
}

impl OutlineTree {
    pub fn of(document: &Document) -> Self {
        let mut tree = Self::default();
        tree.visit(document.outline_first(), TOP, 0);
        tree
    }

    fn visit(&mut self, first: Option<Bookmark<'_>>, parent: u32, depth: usize) {
        if depth >= READ_DEPTH {
            return;
        }
        let mut children = Vec::new();
        let mut at = first;
        while let Some(handle) = at {
            at = handle.next_sibling();
            let item = handle.object_number();
            if item == 0 {
                continue;
            }
            if self.parent_by_item.contains_key(&item) {
                break;
            }
            self.parent_by_item.insert(item, parent);
            children.push(item);
            self.visit(handle.first_child(), item, depth + 1);
        }
        self.children_by_parent.insert(parent, children);
    }

    pub fn has(&self, item: u32) -> bool {
        self.parent_by_item.contains_key(&item)
    }

    /// The item's parent ([`TOP`] at the top level); `None` when it isn't
    /// in the outline.
    pub fn parent_of(&self, item: u32) -> Option<u32> {
        self.parent_by_item.get(&item).copied()
    }

    /// A parent's children in order: none for a leaf, or an item the
    /// outline doesn't have.
    pub fn children_of(&self, parent: u32) -> &[u32] {
        self.children_by_parent.get(&parent).map_or(&[], Vec::as_slice)
    }

    /// The item and the items under it, depth first: the item first.
    pub fn subtree_of(&self, item: u32) -> Vec<u32> {
        let mut items = vec![item];
        for child in self.children_of(item) {
            items.extend(self.subtree_of(*child));
        }
        items
    }

    /// How deep the item sits: 0 at the top level; `None` when it isn't in
    /// the outline.
    pub fn level_of(&self, item: u32) -> Option<usize> {
        if !self.has(item) {
            return None;
        }
        let mut level = 0;
        let mut current = self.parent_of(item);
        while let Some(parent) = current.filter(|parent| *parent != TOP) {
            level += 1;
            current = self.parent_of(parent);
        }
        Some(level)
    }

    /// Whether `node` is `item` or one of the items under it. The top level
    /// is under nothing.
    pub fn is_at_or_under(&self, node: u32, item: u32) -> bool {
        let mut current = Some(node);
        while let Some(at) = current.filter(|at| *at != TOP) {
            if at == item {
                return true;
            }
            current = self.parent_of(at);
        }
        false
    }

    /// Every item, depth first: what an order of the whole outline is.
    pub fn items(&self) -> Vec<u32> {
        self.children_of(TOP).iter().flat_map(|item| self.subtree_of(*item)).collect()
    }
}
