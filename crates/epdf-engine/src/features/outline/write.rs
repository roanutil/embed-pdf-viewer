//! Writing the outline (`OutlineMutator.ts`,
//! `internal/writeOutlineFields.ts`).
//!
//! Each write checks everything it can before its first native write, and
//! answers with what it wrote, re-read. The fork keeps the links and
//! `/Count` right (`public/epdf_outline.h`); what goes where is decided
//! here. The native writes of one call run in one layer transaction, so a
//! refused call leaves nothing.

use pdfium::Document;
use serde_json::{Map, Value};

use super::read::read_item;
use super::tree::OutlineTree;
use super::{
    OutlineItem, OutlineMutationMeta, OutlineOrder, OutlinePlacement, OutlinePosition, READ_DEPTH,
    TOP, parent_ref,
};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::actions::destination::PdfDestination;
use crate::features::annotation::write::prepare::LinkTargetWrite;
use crate::features::changes::transaction::in_layer_transaction;
use crate::identity::OutlineItemRef;
use crate::mutation::MutationMeta;
use crate::mutation::list_position::position_index;
use crate::session::document::WriteStamp;

/// `/F` bit 1.
const ITALIC: i32 = 1;
/// `/F` bit 2.
const BOLD: i32 = 2;

/// The most a PDF text string holds in every PDF app (ISO 32000-2 Annex C),
/// in UTF-16 code units.
const MAX_TITLE_LENGTH: usize = 32_767;

/// The named verbs a target may be written as.
const STANDARD_VERBS: [&str; 4] = ["NextPage", "PrevPage", "FirstPage", "LastPage"];

/// The fields a draft or a patch names, in the file's coordinates
/// (`OutlineItemPatch<PdfDestination>`), checked as a write takes them.
/// `Some(None)` removes the target or the colour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutlineFields {
    pub title: Option<String>,
    pub target: Option<Option<LinkTargetWrite>>,
    pub color: Option<Option<String>>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub open: Option<bool>,
}

fn invalid(field: &str, message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
        .with_details(serde_json::json!({ "field": field }))
}

/// The fork refused a write the checks let through: a list it would change
/// holds an item that isn't an indirect object, or `/Root /Outlines` is a
/// direct dictionary (`outlineRefused`).
fn outline_refused(what: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::MalformedPdf,
        format!("could not {what}: the outline holds an item that isn't an indirect object"),
    )
}

impl OutlineFields {
    /// The fields of a draft or a patch as sent, refused `InvalidArg`,
    /// before anything is written, when no write can take them: a title too
    /// long, a colour that isn't `#rrggbb`, a target a write can't make
    /// (`assertWritableFields`). Keys it doesn't know are left alone, as
    /// upstream's writer leaves them.
    pub fn of(fields: &Map<String, Value>) -> Result<Self, EngineError> {
        let title = match fields.get("title") {
            None => None,
            Some(Value::String(title)) => {
                if title.encode_utf16().count() > MAX_TITLE_LENGTH {
                    return Err(invalid(
                        "title",
                        format!("a title holds at most {MAX_TITLE_LENGTH} characters"),
                    ));
                }
                Some(title.clone())
            }
            Some(_) => return Err(invalid("title", "a title is a string")),
        };
        let color = match fields.get("color") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(color)) if is_hex_color(color) => Some(Some(color.clone())),
            Some(_) => return Err(invalid("color", "expected a color as '#rrggbb'")),
        };
        let target = match fields.get("target") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(target) => Some(Some(target_of(target)?)),
        };
        Ok(Self {
            title,
            target,
            color,
            bold: flag(fields, "bold")?,
            italic: flag(fields, "italic")?,
            open: flag(fields, "open")?,
        })
    }
}

fn flag(fields: &Map<String, Value>, name: &str) -> Result<Option<bool>, EngineError> {
    match fields.get(name) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid(name, format!("{name} is true or false"))),
    }
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A writable target: a `goto`, a `uri`, or a standard named verb.
fn target_of(target: &Value) -> Result<LinkTargetWrite, EngineError> {
    let kind = target.get("kind").and_then(Value::as_str);
    let refuse = || {
        let kind = kind.map_or_else(|| "undefined".to_owned(), str::to_owned);
        invalid("target", format!("a '{kind}' target can't be written"))
    };
    match kind {
        Some("goto") => {
            let destination: PdfDestination =
                serde_json::from_value(target.get("destination").cloned().unwrap_or_default())
                    .map_err(|e| invalid("target", format!("a goto target's destination: {e}")))?;
            Ok(LinkTargetWrite::Goto(destination))
        }
        Some("uri") => match target.get("uri") {
            Some(Value::String(uri)) => Ok(LinkTargetWrite::Uri(uri.clone())),
            _ => Err(invalid("target", "a uri target's uri is a string")),
        },
        Some("named") => match target.get("name").and_then(Value::as_str) {
            Some(name) if STANDARD_VERBS.contains(&name) => {
                Ok(LinkTargetWrite::Named(name.to_owned()))
            }
            _ => Err(refuse()),
        },
        _ => Err(refuse()),
    }
}

/// Writes the fields `fields` names on the item `object_number`; the others
/// stay as they are (`writeOutlineFields`). A target is always written as
/// `/A`, as a link's is. The title of a draft is the create's, not this.
pub fn write_outline_fields(
    document: &Document,
    object_number: u32,
    fields: &OutlineFields,
) -> Result<(), EngineError> {
    let check = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("the {what} of outline item {object_number} could not be written"),
            ))
        }
    };
    if let Some(title) = &fields.title {
        check(document.set_outline_title(object_number, title), "title")?;
    }
    if let Some(target) = &fields.target {
        let action = target.as_ref().map(|target| target_action(document, target)).transpose()?;
        check(document.set_outline_action(object_number, action), "target")?;
    }
    if let Some(color) = &fields.color {
        let rgb = color.as_deref().map(|color| {
            let value = u32::from_str_radix(&color[1..], 16).unwrap_or(0);
            [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff].map(|c| c as f32 / 255.0)
        });
        check(document.set_outline_color(object_number, rgb), "colour")?;
    }
    if fields.bold.is_some() || fields.italic.is_some() {
        // Other /F bits, which a later PDF version may define, stay.
        let mut flags = document.outline_item(object_number).map_or(0, |item| item.style());
        if let Some(bold) = fields.bold {
            flags = if bold { flags | BOLD } else { flags & !BOLD };
        }
        if let Some(italic) = fields.italic {
            flags = if italic { flags | ITALIC } else { flags & !ITALIC };
        }
        check(document.set_outline_style(object_number, flags), "style")?;
    }
    if let Some(open) = fields.open {
        check(document.set_outline_open(object_number, open), "open state")?;
    }
    Ok(())
}

/// A new indirect action of the document for a writable target
/// (`createLinkTargetAction`).
fn target_action(
    document: &Document,
    target: &LinkTargetWrite,
) -> Result<pdfium::Action, EngineError> {
    let (kind, action) = match target {
        LinkTargetWrite::Uri(uri) => ("uri", document.create_uri_action(uri)),
        LinkTargetWrite::Named(name) => ("named", document.create_named_action(name)),
        LinkTargetWrite::Goto(destination) => (
            "goto",
            document.create_goto_action(crate::features::actions::write::build_destination(
                document,
                destination,
            )?),
        ),
    };
    action.map_err(|_| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("failed to create '{kind}' action"),
        )
    })
}

/// What `outline.create` made: the item, and its parent's children with it
/// (`OutlineCreateResult`).
#[derive(Debug, Clone)]
pub struct Created {
    pub item: OutlineItem,
    pub order: OutlineOrder,
    pub meta: OutlineMutationMeta,
}

/// What `outline.update` left: the item (`OutlineUpdateResult`).
#[derive(Debug, Clone)]
pub struct Updated {
    pub item: OutlineItem,
    pub meta: OutlineMutationMeta,
}

/// What `outline.move` changed: the children of every parent it changed
/// (`OutlineMoveResult`).
#[derive(Debug, Clone)]
pub struct Moved {
    pub orders: Vec<OutlineOrder>,
    pub meta: OutlineMutationMeta,
}

/// What an undo of a delete brought back (`OutlineRestoreResult`).
#[derive(Debug, Clone)]
pub struct Restored {
    pub items: Vec<OutlineItem>,
    pub order: OutlineOrder,
    pub meta: OutlineMutationMeta,
}

/// The outline writes of one document in one write (`OutlineMutator`).
pub struct OutlineMutator<'a> {
    document: &'a Document,
    stamp: WriteStamp,
}

impl<'a> OutlineMutator<'a> {
    pub fn new(document: &'a Document, stamp: WriteStamp) -> Self {
        Self { document, stamp }
    }

    /// The outline's shape as it is now.
    pub fn tree(&self) -> OutlineTree {
        OutlineTree::of(self.document)
    }

    /// Where a create goes, checked before anything is written: the parent's
    /// number and the child it follows (0: first). `NotFound` for a parent
    /// or a neighbour the outline doesn't have.
    pub fn plan_create(
        &self,
        parent: Option<OutlineItemRef>,
        position: &OutlinePosition,
    ) -> Result<(u32, u32), EngineError> {
        let tree = self.tree();
        let parent = parent_number_of(&tree, parent)?;
        let children = refs(tree.children_of(parent));
        let at = position_index(&children, position, |r| r.object_number())?;
        let after = if at == 0 { 0 } else { children[at - 1].object_number() };
        Ok((parent, after))
    }

    /// `create`, once [`Self::plan_create`] placed it: the item titled
    /// `title`, at `object_number` (0: the next free one), with the other
    /// fields of the draft.
    pub fn create(
        &self,
        (parent, after): (u32, u32),
        title: &str,
        fields: &OutlineFields,
        object_number: u32,
    ) -> Result<Created, EngineError> {
        let created = in_layer_transaction(self.document, || {
            let created = self.document.create_outline_item(object_number, parent, after, title);
            if created == 0 {
                return Err(outline_refused("create the item"));
            }
            let rest = OutlineFields { title: None, ..fields.clone() };
            write_outline_fields(self.document, created, &rest)?;
            Ok(created)
        })?;
        let item = read_item(self.document, created, parent)?;
        Ok(Created {
            order: order_of(&self.tree(), parent),
            meta: self.meta(vec![item.reference]),
            item,
        })
    }

    /// `update`: the fields the patch names; `NotFound` for an item the
    /// outline doesn't have.
    pub fn update(
        &self,
        reference: OutlineItemRef,
        patch: &OutlineFields,
    ) -> Result<Updated, EngineError> {
        let parent = require_item(&self.tree(), reference, "ref")?;
        in_layer_transaction(self.document, || {
            write_outline_fields(self.document, reference.object_number(), patch)
        })?;
        Ok(Updated {
            item: read_item(self.document, reference.object_number(), parent)?,
            meta: self.meta(vec![reference]),
        })
    }

    /// `move`: the items go together, in the order given, under `parent` at
    /// `position` among its children; the rules of a reorder hold.
    pub fn move_items(
        &self,
        refs_moving: &[OutlineItemRef],
        parent: Option<OutlineItemRef>,
        position: &OutlinePosition,
    ) -> Result<Moved, EngineError> {
        let tree = self.tree();
        let items = refs_moving
            .iter()
            .map(|reference| {
                require_item(&tree, *reference, "refs")?;
                Ok(reference.object_number())
            })
            .collect::<Result<Vec<u32>, EngineError>>()?;
        let mut moving = items.clone();
        moving.sort_unstable();
        moving.dedup();
        if moving.len() != items.len() {
            return Err(invalid("refs", "an item is named twice"));
        }
        for item in &items {
            if items.iter().any(|other| other != item && tree.is_at_or_under(*item, *other)) {
                return Err(invalid(
                    "refs",
                    format!("item {item} is under another item that moves"),
                ));
            }
        }
        let parent = parent_number_of(&tree, parent)?;
        if items.iter().any(|item| tree.is_at_or_under(parent, *item)) {
            return Err(invalid("parent", "an item would go under itself"));
        }
        let staying: Vec<u32> = tree
            .children_of(parent)
            .iter()
            .copied()
            .filter(|child| !moving.contains(child))
            .collect();
        if let Some(anchor) = position.anchor()
            && moving.contains(&anchor.object_number())
        {
            return Err(invalid(
                "position",
                format!("the neighbour {} is one of the items that move", anchor.object_number()),
            ));
        }
        let staying_refs = refs(&staying);
        let at = position_index(&staying_refs, position, |r| r.object_number())?;
        let mut after = if at == 0 { 0 } else { staying[at - 1] };
        let placements: Vec<OutlinePlacement> = items
            .iter()
            .map(|item| {
                let placement = OutlinePlacement { item: *item, parent, after };
                after = *item;
                placement
            })
            .collect();
        self.place(&placements, &tree)
    }

    /// Puts items where `placements` say, in order: each right after its
    /// `after` while that is a child of its parent, else first. What an undo
    /// of a move does; the caller has checked that each item and parent is
    /// there.
    pub fn place(
        &self,
        placements: &[OutlinePlacement],
        before: &OutlineTree,
    ) -> Result<Moved, EngineError> {
        let mut parents: Vec<u32> = Vec::new();
        let mut add = |parent: u32| {
            if !parents.contains(&parent) {
                parents.push(parent);
            }
        };
        for placement in placements {
            add(before.parent_of(placement.item).unwrap_or(TOP));
            add(placement.parent);
        }
        in_layer_transaction(self.document, || {
            for &OutlinePlacement { item, parent, after } in placements {
                let now = self.tree();
                let anchor =
                    if after != 0 && now.parent_of(after) == Some(parent) { after } else { 0 };
                if !self.document.move_outline_item(item, parent, anchor) {
                    return Err(outline_refused(&format!("move item {item}")));
                }
            }
            Ok(())
        })?;
        let after = self.tree();
        Ok(Moved {
            orders: parents.iter().map(|parent| order_of(&after, *parent)).collect(),
            meta: self.meta(placements.iter().map(|p| OutlineItemRef::new(p.item)).collect()),
        })
    }

    /// Where `items` are now, in document order: an anchor always comes
    /// before what follows it (`placementsOf`).
    pub fn placements_of(&self, items: &[u32], tree: &OutlineTree) -> Vec<OutlinePlacement> {
        tree.items()
            .into_iter()
            .filter(|item| items.contains(item))
            .map(|item| {
                let parent = tree.parent_of(item).unwrap_or(TOP);
                let siblings = tree.children_of(parent);
                let index = siblings.iter().position(|s| *s == item).unwrap_or(0);
                OutlinePlacement {
                    item,
                    parent,
                    after: if index > 0 { siblings[index - 1] } else { 0 },
                }
            })
            .collect()
    }

    /// `delete`: the item goes, with the items under it.
    pub fn delete(&self, reference: OutlineItemRef) -> Result<OutlineMutationMeta, EngineError> {
        let tree = self.tree();
        require_item(&tree, reference, "ref")?;
        let going = tree.subtree_of(reference.object_number());
        in_layer_transaction(self.document, || {
            if self.document.delete_outline_item(reference.object_number()) {
                Ok(())
            } else {
                Err(outline_refused(&format!("delete item {}", reference.object_number())))
            }
        })?;
        Ok(self.meta(refs(&going)))
    }

    /// The items a capture brought back: the item first, then the items
    /// under it, and its parent's children (`restored`).
    pub fn restored(&self, object_number: u32) -> Result<Restored, EngineError> {
        let tree = self.tree();
        let parent = tree
            .parent_of(object_number)
            .ok_or_else(|| outline_refused(&format!("restore item {object_number}")))?;
        let items = self.subtree(object_number, &tree)?;
        Ok(Restored {
            order: order_of(&tree, parent),
            meta: self.meta(items.iter().map(|item| item.reference).collect()),
            items,
        })
    }

    /// The item as it reads now; `None` when the outline has no such item.
    pub fn find(
        &self,
        reference: OutlineItemRef,
        tree: &OutlineTree,
    ) -> Result<Option<OutlineItem>, EngineError> {
        tree.parent_of(reference.object_number())
            .map(|parent| read_item(self.document, reference.object_number(), parent))
            .transpose()
    }

    /// The item and the items under it, depth first, as they read now.
    pub fn subtree(
        &self,
        object_number: u32,
        tree: &OutlineTree,
    ) -> Result<Vec<OutlineItem>, EngineError> {
        tree.subtree_of(object_number)
            .into_iter()
            .map(|item| read_item(self.document, item, tree.parent_of(item).unwrap_or(TOP)))
            .collect()
    }

    /// An update's answer for an item written some other way: a revert put
    /// it back.
    pub fn updated(&self, reference: OutlineItemRef) -> Result<Updated, EngineError> {
        let item = self
            .find(reference, &self.tree())?
            .ok_or_else(|| outline_refused(&format!("read item {}", reference.object_number())))?;
        Ok(Updated { item, meta: self.meta(vec![reference]) })
    }

    /// Counts the items above `reference` again from the tree, as every
    /// outline write does: a revert puts back the item's own dictionary, its
    /// `/Count` with it, and nothing above it. Setting the open state it
    /// reads changes nothing else (`countAgain`).
    pub fn count_again(&self, reference: OutlineItemRef) -> Result<(), EngineError> {
        let Some(item) = self.find(reference, &self.tree())? else {
            return Ok(());
        };
        let fields = OutlineFields { open: Some(item.open), ..OutlineFields::default() };
        write_outline_fields(self.document, reference.object_number(), &fields)
    }

    fn meta(&self, changed: Vec<OutlineItemRef>) -> OutlineMutationMeta {
        OutlineMutationMeta { meta: MutationMeta::local(Vec::new(), &self.stamp), changed }
    }
}

/// The number a parent ref names: [`TOP`] for `None`; `NotFound` when it
/// isn't an item; `InvalidArg` for an item at the deepest level a read goes
/// to, whose children no read shows (`parentNumberOf`).
fn parent_number_of(
    tree: &OutlineTree,
    parent: Option<OutlineItemRef>,
) -> Result<u32, EngineError> {
    let Some(parent) = parent else { return Ok(TOP) };
    require_item(tree, parent, "parent")?;
    if tree.level_of(parent.object_number()).unwrap_or(0) >= READ_DEPTH - 1 {
        return Err(invalid(
            "parent",
            format!("an item under {} would be deeper than reads go", parent.object_number()),
        ));
    }
    Ok(parent.object_number())
}

/// The item's parent; `NotFound` when the outline has no such item
/// (`requireItem`).
pub fn require_item(
    tree: &OutlineTree,
    reference: OutlineItemRef,
    field: &str,
) -> Result<u32, EngineError> {
    tree.parent_of(reference.object_number()).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no outline item {}", reference.object_number()),
        )
        .with_details(serde_json::json!({ "field": field }))
    })
}

/// One parent's children, as refs (`orderOf`).
pub fn order_of(tree: &OutlineTree, parent: u32) -> OutlineOrder {
    OutlineOrder { parent: parent_ref(parent), children: refs(tree.children_of(parent)) }
}

fn refs(items: &[u32]) -> Vec<OutlineItemRef> {
    items.iter().map(|item| OutlineItemRef::new(*item)).collect()
}
