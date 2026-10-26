//! `controller.ts`: the outline controller. The outline is a kernel mirror,
//! read when something asks for it (`ensure_loaded()`, what the state view
//! does), and every read shows its `view()`: the engine's outline with this
//! session's pending changes on top. Each write is one change on the
//! document's queue, so it shows at once, is one step of the history, and
//! undoing it shows the outline as it was. Expanded and selected items are
//! session state, the plugin's slice.
//!
//! Every optional plugin is looked up when it is needed (`ctx.tryGet`): the
//! stage (where a new item goes, the reading position, a page target),
//! actions (an item's `/A`), link (every other target), selection (a new
//! item's title) and i18n (the default title). None is required.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::changes::{PendingChange, StagedChange};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::mirror::{Fold, Load, Loaded, Mirror, MirrorChange, MirrorSpec, ResourceStatus};
use epdf_kernel::plugin::{ConnectFn, Controller};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe, undefined};
use epdf_plugin_actions::{ACTIONS, ActionContext, ActionOrigin, ActionSource, ActionTriggerEvent};
use epdf_plugin_i18n::{I18N, TranslateOptions};
use epdf_plugin_link::{LINK, LinkActivation, LinkResolution, resolve_link_target};
use epdf_plugin_selection::SELECTION;
use epdf_plugin_stage::{PageDestination, PageTarget, RevealOptions, STAGE, StageCapability};
use serde_json::{Map, Value, json};

use crate::contract::*;
use crate::model::*;
use crate::sync::{fold_outline, number_text, predict_outline, undo_of};

pub(crate) const CAPABILITY: &str = "outline";
const MODIFY: &str = "doc.outline.modify";

/// The most a PDF text string holds in every PDF app (ISO 32000-2 Annex C),
/// in UTF-16 code units.
const MAX_TITLE_LENGTH: usize = 32_767;

/// `factOf(event)`: the fact a confirmed outline event reports.
fn fact_of(event: &Value) -> Option<(&'static str, Vec<Value>)> {
    let refs = |value: Option<&Value>| value.and_then(Value::as_array).cloned().unwrap_or_default();
    let item_ref = |event: &Value| {
        vec![event.get("item").and_then(|i| i.get("ref")).cloned().unwrap_or_else(undefined)]
    };
    Some(match event.get("type").and_then(Value::as_str)? {
        "outline.created" => ("created", item_ref(event)),
        "outline.updated" => ("updated", item_ref(event)),
        "outline.moved" => ("moved", refs(event.get("meta").and_then(|m| m.get("changed")))),
        "outline.deleted" => ("deleted", refs(event.get("deleted"))),
        "outline.restored" => (
            "restored",
            refs(event.get("items"))
                .iter()
                .map(|item| item.get("ref").cloned().unwrap_or_else(undefined))
                .collect(),
        ),
        _ => return None,
    })
}

/// `TypeError: Cannot read properties of undefined (reading '<member>')`.
fn type_error(member_name: &str) -> Error {
    Error::named(
        "TypeError",
        format!("Cannot read properties of undefined (reading '{member_name}')"),
    )
}

/// `text.replace(/\s+/g, ' ').trim()`, with JavaScript's white space.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if epdf_kernel::mirror::is_js_white_space(c) || c == '\u{feff}' {
            in_space = true;
        } else {
            if in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = false;
            out.push(c);
        }
    }
    out
}

/// `text.slice(0, max)` in UTF-16 code units. A cut through a surrogate
/// pair drops the half the TypeScript would keep as a lone surrogate.
fn utf16_prefix(text: String, max: usize) -> String {
    if text.encode_utf16().count() <= max {
        return text;
    }
    let units: Vec<u16> = text.encode_utf16().take(max).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\u{fffd}').to_string()
}

/// `listRows`: the view and the expanded keys it was read from.
type RowsMemo = (Rc<Value>, Rc<Map<String, Value>>, Rc<Vec<OutlineRow>>);
/// `getSelected`: the view and the selection it was read from.
type SelectedMemo = (Rc<Value>, Rc<Vec<Value>>, Rc<Vec<Value>>);
/// `spots`: the list and the page revision it was read from.
type SpotsMemo = (Rc<Vec<Value>>, u64, Rc<Vec<OutlineSpot>>);

/// The memos behind the reads (`memo`, `memoByKey`): each holds the inputs
/// it was computed from and answers the same `Rc` while they are the same
/// `Rc`s (`Rc::ptr_eq`).
#[derive(Default)]
struct Memos {
    list: Option<(Rc<Value>, Rc<Vec<Value>>)>,
    rows: Option<RowsMemo>,
    selected: Option<SelectedMemo>,
    spots: Option<SpotsMemo>,
    /// `memoByKey(..., { maxEntries: 256 })`: per parent key, for one view.
    children: VecDeque<(String, Rc<Value>, Rc<Vec<Value>>)>,
}

/// How many parents' children `get_children` keeps.
const CHILDREN_ENTRIES: usize = 256;

pub(crate) struct Core {
    cx: Controller<OutlineState, Value>,
    outline: Mirror,
    changed: EventHookSource<OutlineChangedEvent>,
    resynced: EventHookSource<OutlineResyncedEvent>,
    memos: RefCell<Memos>,
}

/// `OutlineCapability`: the api `createOutlineController` returns.
#[derive(Clone)]
pub struct OutlineCapability(pub(crate) Rc<Core>);

impl Core {
    fn view(&self) -> Rc<Value> {
        self.outline.view()
    }

    fn state(&self) -> Rc<OutlineState> {
        self.cx.state()
    }

    fn stage(&self) -> Option<StageCapability> {
        self.cx.try_get(&STAGE).ok().flatten()
    }

    fn get(&self, item_ref: &Value) -> Option<Value> {
        let view = self.view();
        item_at(&view, &outline_key_of(Some(item_ref))).cloned()
    }

    fn known(&self, item_ref: &Value) -> bool {
        self.get(item_ref).is_some()
    }

    fn list(&self) -> Rc<Vec<Value>> {
        let view = self.view();
        let mut memos = self.memos.borrow_mut();
        if let Some((tree, list)) = &memos.list
            && Rc::ptr_eq(tree, &view)
        {
            return list.clone();
        }
        let list = Rc::new(if view.is_null() { Vec::new() } else { items_of(&view) });
        memos.list = Some((view, list.clone()));
        list
    }

    /// The page's display index, `None` for a page this document lacks.
    fn page_index_of(&self, page: &Value) -> Option<f64> {
        let page = PageRef::from_json(page)?;
        self.cx.get_page(page)?.get("index").and_then(Value::as_f64)
    }

    /// The items in reading order, again when the outline or the page order
    /// changes.
    fn spots(&self) -> Rc<Vec<OutlineSpot>> {
        let list = self.list();
        let revision = self.cx.document().map_or(0, |doc| doc.revision);
        if let Some((items, at, spots)) = &self.memos.borrow().spots
            && Rc::ptr_eq(items, &list)
            && *at == revision
        {
            return spots.clone();
        }
        let spots = Rc::new(spots_of(&list, |page| self.page_index_of(page)));
        self.memos.borrow_mut().spots = Some((list, revision, spots.clone()));
        spots
    }

    fn current_destination(&self) -> Option<PageDestination> {
        self.stage()?.get_current_destination()
    }

    fn get_current_item(&self) -> Option<Value> {
        let destination = self.current_destination();
        let page_index = destination
            .as_ref()
            .and_then(|d| self.cx.get_page(d.page()))
            .and_then(|layout| layout.get("index").and_then(Value::as_f64))
            .unwrap_or(-1.0);
        let y = match &destination {
            Some(PageDestination::Xyz { y, .. }) => y.unwrap_or(0.0),
            _ => 0.0,
        };
        if page_index < 0.0 {
            return None;
        }
        let spots = self.spots();
        let spot = spot_at(&spots, page_index, y)?;
        let view = self.view();
        item_at(&view, &spot.key).and_then(|item| item.get("ref")).cloned()
    }

    fn get_current_view_target(&self) -> Option<Value> {
        let destination = self.current_destination()?;
        Some(json!({ "kind": "goto", "destination": epdf_kernel::js::to_value(&destination) }))
    }

    fn update_state(&self, transition: impl FnOnce(Rc<OutlineState>) -> Rc<OutlineState>) {
        self.cx.update_state(transition);
    }

    fn reveal(&self, item_ref: &Value) {
        let view = self.view();
        if !view.is_null() {
            let ancestors = ancestors_of(&view, &outline_key_of(Some(item_ref)));
            self.update_state(|state| expanded(state, &ancestors));
        }
    }

    fn is_expanded(&self, item_ref: &Value) -> bool {
        let view = self.view();
        let key = outline_key_of(Some(item_ref));
        !view.is_null()
            && !children_of(&view, &key).is_empty()
            && self.state().expanded.get(&key) == Some(&Value::Bool(true))
    }

    // ── activation ────────────────────────────────────────────────────────────

    fn activate(&self, item_ref: &Value) -> Result<LinkActivation, Error> {
        let Some(item) = self.get(item_ref) else {
            return Err(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("the outline has no item {}", number_text(item_ref.get("objectNumber"))),
                None,
                None,
            ));
        };
        // An /A runs as a link's does: a script, a chain of actions, a GoTo.
        let actions = self.cx.try_get(&ACTIONS).ok().flatten();
        if let (true, Some(actions)) = (truthy(item.get("action")), actions) {
            let dispatch = actions.execute(
                item["action"].clone(),
                ActionContext {
                    origin: ActionOrigin::User,
                    source: ActionSource::Outline {
                        item: item.get("ref").cloned().unwrap_or_else(undefined),
                    },
                    event: ActionTriggerEvent::Activate,
                },
                None,
            );
            return Ok(LinkActivation::Dispatched { dispatch });
        }
        let Some(target) = item.get("target").filter(|t| truthy(Some(t))) else {
            return Ok(LinkActivation::None);
        };
        // The link plugin opens websites through the framework; without it
        // the caller is handed what it can't do itself.
        if let Some(link) = self.cx.try_get(&LINK).ok().flatten() {
            return Ok(link.activate(target, None));
        }
        let resolution = resolve_link_target(target, |page| {
            PageRef::from_json(page).is_some_and(|page| self.cx.get_page(page).is_some())
        });
        Ok(match resolution {
            LinkResolution::Destination { destination } => match self.stage() {
                None => LinkActivation::Destination { destination },
                Some(stage) => {
                    self.go_to_destination(&stage, &destination);
                    LinkActivation::Revealed
                }
            },
            LinkResolution::Uri { uri } => LinkActivation::Uri { uri },
            LinkResolution::Named { name } => LinkActivation::Named { name },
            LinkResolution::Reported { target } => LinkActivation::Reported { target },
        })
    }

    /// `stage.goToDestination(destination)`. A destination the stage's
    /// `PageDestination` cannot decode still reveals its page, as the
    /// TypeScript's `goToDestination` does for a kind it does not know (the
    /// link plugin's rule).
    fn go_to_destination(&self, stage: &StageCapability, destination: &Value) {
        let error = match serde_json::from_value::<PageDestination>(destination.clone()) {
            Ok(decoded) => return stage.go_to_destination(&decoded, None),
            Err(error) => error,
        };
        if let Some(page) = destination.get("page").and_then(PageRef::from_json) {
            stage.reveal(PageTarget::Ref(page), Some(RevealOptions::default()));
        }
        const KINDS: &[&str] = &["xyz", "fit", "fitH", "fitV", "fitR", "fitB", "fitBH", "fitBV"];
        let kind = destination.get("kind").and_then(Value::as_str);
        if kind.is_some_and(|kind| KINDS.contains(&kind)) {
            self.cx.report_error(&Error::new(format!(
                "[outline] the stage cannot read destination {destination}: {error}; revealed its page"
            )));
        }
    }

    // ── writes ────────────────────────────────────────────────────────────────

    /// `refuse(operation, options)`: without `doc.outline.modify`, or once
    /// the caller cancelled, refuse before anything starts.
    fn refuse(&self, operation: &str, signal: Option<&AbortSignal>) -> Result<(), Error> {
        self.cx.assert_allowed(MODIFY, operation)?;
        if signal.is_some_and(AbortSignal::aborted) {
            return Err(plugin_error(
                code::OPERATION_CANCELLED,
                CAPABILITY,
                format!("{operation} was cancelled"),
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `assertKnown(ref, operation)`: a ref the read outline doesn't have;
    /// before the first read lands the engine decides.
    fn assert_known(&self, item_ref: &Value, operation: &str) -> Result<(), Error> {
        if !self.view().is_null() && !self.known(item_ref) {
            return Err(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!(
                    "{operation}: the outline has no item {}",
                    number_text(item_ref.get("objectNumber"))
                ),
                None,
                None,
            ));
        }
        Ok(())
    }

    /// `stageChange(verb, ops, count)`: one user action; it shows at once,
    /// and undoing it shows the outline as it was.
    fn stage_change(
        &self,
        verb: &str,
        ops: Vec<Value>,
        count: Option<usize>,
    ) -> Result<PendingChange, Error> {
        let key = format!("outline.{verb}");
        let label = match count {
            None => json!({ "key": key }),
            Some(count) => json!({ "key": key, "count": count }),
        };
        let undo = undo_of(&self.view(), &ops);
        let mut change = StagedChange::ops(label, ops);
        change.undo = undo;
        self.cx.changes()?.stage(change)
    }

    /// `answer(change, options)`: the engine's answer to a staged change, its
    /// first item.
    async fn answer(
        self: Rc<Self>,
        change: PendingChange,
        signal: Option<AbortSignal>,
    ) -> Result<Value, Error> {
        let answered = async {
            let result = cancellable(CAPABILITY, signal, change.result()).await?;
            let items =
                result.get("items").filter(|i| !i.is_null() && !epdf_kernel::is_undefined(i));
            let items = items.ok_or_else(|| type_error("0"))?;
            Ok::<Value, Error>(items.get(0).cloned().unwrap_or_else(undefined))
        };
        answered.await.map_err(|error| to_plugin_error(CAPABILITY, &error))
    }

    /// `takeObjectNumber()`: a final name for the new item, from the
    /// document's pool.
    async fn take_object_number(self: Rc<Self>) -> Result<u32, Error> {
        let changes = self.cx.changes()?;
        loop {
            if let Some(number) = changes.take_object_number()? {
                return Ok(number);
            }
            changes.reserve_object_numbers(1).await?;
        }
    }

    /// `defaultTitle(options)`: the selected text on one line, else
    /// "Untitled" in the viewer's language.
    async fn default_title(self: Rc<Self>, signal: Option<AbortSignal>) -> Result<String, Error> {
        if let Some(selection) = self.cx.try_get(&SELECTION).ok().flatten()
            && selection.has_selection()
        {
            let options = epdf_plugin_selection::OperationOptions { signal: signal.clone() };
            match selection.read_text(options).await {
                Ok(text) => {
                    let text = one_line(&text);
                    if !text.is_empty() {
                        return Ok(utf16_prefix(text, MAX_TITLE_LENGTH));
                    }
                }
                // Without the right to copy text the title falls back; a
                // cancel does not.
                Err(error) => {
                    if signal.as_ref().is_some_and(AbortSignal::aborted) {
                        return Err(to_plugin_error(CAPABILITY, &error));
                    }
                }
            }
        }
        let i18n = self.cx.try_get(&I18N).ok().flatten();
        Ok(match i18n {
            Some(i18n) => i18n.t(
                "outline.untitled",
                Some(&TranslateOptions { fallback: Some("Untitled".into()), ..Default::default() }),
            ),
            None => "Untitled".into(),
        })
    }
}

/// `{ item: item.item }`: a `TypeError` for an answer with no item, as the
/// TypeScript reads it outside its `try`.
fn item_of(item: &Value) -> Result<Value, Error> {
    if item.is_null() || epdf_kernel::is_undefined(item) {
        return Err(type_error("item"));
    }
    Ok(json!({ "item": member_or_undefined(item, "item") }))
}

impl OutlineCapability {
    /// Every item, depth first, with this session's pending changes shown.
    /// Reference-stable until it changes; empty until the first read lands.
    pub fn list(&self) -> Rc<Vec<Value>> {
        self.0.list()
    }

    /// One item, or `None` for a ref the outline doesn't have.
    pub fn get(&self, item_ref: &Value) -> Option<OutlineItem> {
        self.0.get(item_ref)
    }

    /// An item's children in order; the top level for `None`. Empty for a
    /// ref the outline doesn't have.
    pub fn get_children(&self, parent: Option<&Value>) -> Rc<Vec<Value>> {
        let key = outline_key_of(parent);
        let view = self.0.view();
        let mut memos = self.0.memos.borrow_mut();
        if let Some(i) =
            memos.children.iter().position(|(k, tree, _)| *k == key && Rc::ptr_eq(tree, &view))
        {
            let entry = memos.children.remove(i).expect("the entry was found");
            let children = entry.2.clone();
            memos.children.push_back(entry);
            return children;
        }
        let children: Rc<Vec<Value>> = Rc::new(if view.is_null() {
            Vec::new()
        } else {
            children_of(&view, &key)
                .iter()
                .filter_map(|child| item_at(&view, child).cloned())
                .collect()
        });
        memos.children.retain(|(k, _, _)| *k != key);
        if memos.children.len() >= CHILDREN_ENTRIES {
            memos.children.pop_front();
        }
        memos.children.push_back((key, view, children.clone()));
        children
    }

    /// The rows a tree view shows: every item whose parents are all
    /// expanded, depth first, with its depth. Reference-stable until the
    /// outline or the expanded items change.
    pub fn list_rows(&self) -> Rc<Vec<OutlineRow>> {
        let view = self.0.view();
        let open = self.0.state().expanded.clone();
        let mut memos = self.0.memos.borrow_mut();
        if let Some((tree, expanded, rows)) = &memos.rows
            && Rc::ptr_eq(tree, &view)
            && Rc::ptr_eq(expanded, &open)
        {
            return rows.clone();
        }
        let rows = Rc::new(if view.is_null() { Vec::new() } else { rows_of(&view, &open) });
        memos.rows = Some((view, open, rows.clone()));
        rows
    }

    /// The item for where the main view is reading: the last item, in
    /// reading order, that goes to a place at or above the top of the view.
    /// `None` before the first such item, and without a stage.
    pub fn get_current_item(&self) -> Option<OutlineItemRef> {
        self.0.get_current_item()
    }

    /// Part of the outline couldn't be read (a loop in it, or past the read
    /// limits); the rest is listed.
    pub fn is_incomplete(&self) -> bool {
        self.0.view().get("incomplete").and_then(Value::as_bool).unwrap_or(false)
    }

    /// Load state: `idle` until `ensure_loaded()` asks, then `loading` →
    /// `ready`, or `forbidden` / `error`.
    pub fn get_status(&self) -> ResourceStatus {
        self.0.outline.get_status()
    }

    /// Read the outline unless it is read or being read: nothing reads it
    /// when a document opens. Resolves once it landed and `on_resynced`
    /// fired. Rejects `operation-cancelled` or `instance-closed`; after a
    /// failure it reads again.
    pub fn ensure_loaded(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.outline.ensure_loaded())
    }

    /// Read it again. Rarely needed: the event stream keeps it current.
    pub fn refresh(&self, options: OperationOptions) -> Promise<()> {
        cancellable(CAPABILITY, options.signal, self.0.outline.refresh())
    }

    /// Whether an item's children show.
    pub fn is_expanded(&self, item_ref: &Value) -> bool {
        self.0.is_expanded(item_ref)
    }

    /// Show an item's children.
    pub fn expand(&self, item_ref: &Value) {
        if self.0.known(item_ref) {
            let key = outline_key_of(Some(item_ref));
            self.0.update_state(|state| expanded(state, &[key]));
        }
    }

    /// Hide an item's children.
    pub fn collapse(&self, item_ref: &Value) {
        let key = outline_key_of(Some(item_ref));
        self.0.update_state(|state| collapsed(state, &[key]));
    }

    /// Expand a collapsed item, collapse an expanded one.
    pub fn toggle(&self, item_ref: &Value) {
        if self.0.is_expanded(item_ref) {
            self.collapse(item_ref);
        } else {
            self.expand(item_ref);
        }
    }

    /// Expand every item with children.
    pub fn expand_all(&self) {
        let view = self.0.view();
        let keys: Vec<OutlineKey> = children_map(&view)
            .map(|children| {
                children.keys().filter(|key| item_at(&view, key).is_some()).cloned().collect()
            })
            .unwrap_or_default();
        self.0.update_state(|state| expanded(state, &keys));
    }

    /// Collapse every item.
    pub fn collapse_all(&self) {
        let keys: Vec<OutlineKey> = self.0.state().expanded.keys().cloned().collect();
        self.0.update_state(|state| collapsed(state, &keys));
    }

    /// Expand every item `item_ref` sits under, so its row shows.
    pub fn reveal(&self, item_ref: &Value) {
        self.0.reveal(item_ref);
    }

    /// The selected items, in the order they were selected; refs the
    /// outline no longer has drop out.
    pub fn get_selected(&self) -> Rc<Vec<OutlineItemRef>> {
        let view = self.0.view();
        let refs = self.0.state().selected.clone();
        let mut memos = self.0.memos.borrow_mut();
        if let Some((tree, selection, out)) = &memos.selected
            && Rc::ptr_eq(tree, &view)
            && Rc::ptr_eq(selection, &refs)
        {
            return out.clone();
        }
        let out = Rc::new(if view.is_null() || refs.is_empty() {
            Vec::new()
        } else {
            refs.iter()
                .filter(|r| item_at(&view, &outline_key_of(Some(r))).is_some())
                .cloned()
                .collect()
        });
        memos.selected = Some((view, refs, out.clone()));
        out
    }

    /// Make these items the selection; an empty list clears it. Refs the
    /// outline doesn't have are left out.
    pub fn select(&self, refs: &[Value]) {
        let known: Vec<Value> = refs.iter().filter(|r| self.0.known(r)).cloned().collect();
        self.0.update_state(|state| selected(state, &known));
    }

    /// Follow an item as a click does, and return what happened at once (the
    /// user's gesture is still on the stack). An item with an `/A` action
    /// runs it through the actions plugin; else its target goes the way a
    /// link's does: a page moves the main view, a website opens through the
    /// link plugin. `not-found` for a ref the outline doesn't have.
    pub fn activate(&self, item_ref: &Value) -> Result<LinkActivation, Error> {
        self.0.activate(item_ref)
    }

    /// Whether this session may change the outline (`doc.outline.modify`):
    /// every write below.
    pub fn can_edit(&self) -> bool {
        self.0.cx.allows(MODIFY)
    }

    /// Where the main view is: the page at its top, the point at its
    /// top-left corner, the zoom left as the reader has it. What a new item
    /// goes to. `None` without a stage, or before a page is laid out.
    pub fn get_current_view_target(&self) -> Option<PdfLinkTargetWritable> {
        self.0.get_current_view_target()
    }

    /// Add an item. It shows at once and resolves `{ item }` with the
    /// engine's item. Its target defaults to
    /// [`get_current_view_target`](Self::get_current_view_target), its title
    /// to the selected text, or else "Untitled" in the viewer's language.
    /// Fires `on_changed`. Rejects `permission-denied`, `invalid-input`,
    /// `not-found` for a parent or a neighbour that isn't there,
    /// `operation-cancelled` or `instance-closed`.
    pub fn create(&self, input: OutlineItemInput, options: OutlineCreateOptions) -> Promise<Value> {
        let this = self.0.clone();
        Promise::spawn(async move {
            let signal = options.signal.clone();
            this.refuse("outline.create", signal.as_ref())?;
            // `const { parent = null, position, title, target, ... } = input`
            let parent = member(&input, "parent").cloned().unwrap_or(Value::Null);
            if truthy(Some(&parent)) {
                this.assert_known(&parent, "outline.create")?;
            }
            let chosen_target = match member(&input, "target") {
                None => this.get_current_view_target(),
                Some(target) => Some(target.clone()),
            };
            let title = match member(&input, "title").filter(|t| !t.is_null()) {
                Some(title) => title.clone(),
                None => {
                    Value::String(rt::returned(this.clone().default_title(signal.clone())).await?)
                }
            };
            let mut data = Map::new();
            data.insert("title".into(), title);
            if let Some(target) = chosen_target.filter(|t| truthy(Some(t))) {
                data.insert("target".into(), target);
            }
            for name in ["color", "bold", "italic"] {
                if let Some(value) = member(&input, name) {
                    data.insert(name.into(), value.clone());
                }
            }
            let object_number = rt::returned(this.clone().take_object_number()).await?;
            let mut op = Map::new();
            op.insert("type".into(), json!("outline.create"));
            op.insert("parent".into(), parent);
            if let Some(position) = member(&input, "position").filter(|p| truthy(Some(p))) {
                op.insert("position".into(), position.clone());
            }
            op.insert("data".into(), Value::Object(data));
            op.insert("objectNumber".into(), json!(object_number));
            let change = this.stage_change("create", vec![Value::Object(op)], None)?;
            if options.select {
                let item_ref = to_outline_item_ref(json!(object_number));
                this.reveal(&item_ref);
                this.update_state(|state| selected(state, &[item_ref]));
            }
            let item = rt::returned(this.clone().answer(change, signal)).await?;
            item_of(&item)
        })
    }

    /// Change the fields the patch names. `target: null` removes the target
    /// and `color: null` the colour; `open` holds only on an item with
    /// children. It shows at once and resolves `{ item }`. Fires
    /// `on_changed`. Rejects as `create` does, and `not-found` for an item
    /// that isn't there.
    pub fn update(
        &self,
        item_ref: OutlineItemRef,
        patch: OutlineItemPatch,
        options: OperationOptions,
    ) -> Promise<Value> {
        let this = self.0.clone();
        Promise::spawn(async move {
            this.refuse("outline.update", options.signal.as_ref())?;
            this.assert_known(&item_ref, "outline.update")?;
            let op = json!({ "type": "outline.update", "ref": item_ref, "patch": patch });
            let change = this.stage_change("update", vec![op], None)?;
            let item = rt::returned(this.clone().answer(change, options.signal)).await?;
            item_of(&item)
        })
    }

    /// Move items, with the items under them, together and in the order
    /// given, to `position` among `parent`'s children (`null`: the top
    /// level). It shows at once. Fires `on_changed`. Rejects
    /// `permission-denied`, `not-found`, `invalid-input`,
    /// `operation-cancelled` or `instance-closed`.
    pub fn move_items(
        &self,
        refs: Vec<OutlineItemRef>,
        parent: Value,
        position: OutlinePosition,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.0.clone();
        Promise::spawn(async move {
            this.refuse("outline.move", options.signal.as_ref())?;
            if refs.is_empty() {
                return Ok(());
            }
            for item_ref in &refs {
                this.assert_known(item_ref, "outline.move")?;
            }
            if truthy(Some(&parent)) {
                this.assert_known(&parent, "outline.move")?;
            }
            let op = json!({ "type": "outline.move", "refs": refs, "parent": parent, "position": position });
            let change = this.stage_change("move", vec![op], None)?;
            rt::returned(this.clone().answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// Delete items, with the items under them, as one change: one step of
    /// the history. A named item under another named item goes with it. It
    /// shows at once. Fires `on_changed`. Rejects `permission-denied`,
    /// `not-found`, `operation-cancelled` or `instance-closed`.
    pub fn delete(&self, refs: Vec<OutlineItemRef>, options: OperationOptions) -> Promise<()> {
        let this = self.0.clone();
        Promise::spawn(async move {
            this.refuse("outline.delete", options.signal.as_ref())?;
            for item_ref in &refs {
                this.assert_known(item_ref, "outline.delete")?;
            }
            let view = this.view();
            let named = if view.is_null() { refs } else { topmost_of(&view, &refs) };
            if named.is_empty() {
                return Ok(());
            }
            let count = named.len();
            let ops =
                named.into_iter().map(|r| json!({ "type": "outline.delete", "ref": r })).collect();
            let change = this.stage_change("delete", ops, Some(count))?;
            rt::returned(this.clone().answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// A confirmed change, whoever made it.
    pub fn on_changed(
        &self,
        listener: impl Fn(&OutlineChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.changed.on(listener)
    }

    /// The outline was read, first or again. Fires no per-item `on_changed`.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&OutlineResyncedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.resynced.on(listener)
    }
}

/// `createOutlineController(ctx)`: the api; there is no `connect`, since
/// nothing is read until someone asks.
pub fn create_outline_controller(
    cx: Controller<OutlineState, Value>,
) -> Result<(OutlineCapability, Option<ConnectFn>), Error> {
    let changed = cx.event_source::<OutlineChangedEvent>();
    let resynced = cx.event_source::<OutlineResyncedEvent>();

    // `load: async (doc) => ({ value: treeOf(await doc.outline.get()) })`
    let load: Load = Rc::new(|doc, _signal| {
        let call = doc.map(|doc| doc.call("outline.get", None, Vec::new()));
        Promise::spawn(async move {
            let Some(call) = call else {
                return Err(Error::new("[outline] no document"));
            };
            let outline = call.await?;
            Ok(Loaded { value: tree_of(&outline), cursor: None })
        })
    });
    let mut spec = MirrorSpec::new(
        "outline",
        Value::Null,
        load,
        Rc::new(fold_outline) as Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>>,
    );
    spec.on_demand = true;
    spec.predict = Some(Rc::new(predict_outline));

    // The `changed` hook reads the mirror's view, so it reaches the core
    // through a weak slot filled once the core exists.
    let slot: Rc<RefCell<Weak<Core>>> = Rc::new(RefCell::new(Weak::new()));
    let hook_slot = slot.clone();
    spec.changed = Some(Rc::new(move |change: MirrorChange| {
        let Some(core) = hook_slot.borrow().upgrade() else { return Ok(()) };
        let next = change.next;
        if next.is_null() {
            return Ok(());
        }
        if change.cause == "load" {
            core.update_state(|state| seeded(state, &next));
            let view = core.view();
            let shown = if view.is_null() { next.clone() } else { view };
            core.update_state(|state| pruned(state, &shown));
            let items = items_of(&next);
            let incomplete = next.get("incomplete").cloned().unwrap_or_else(undefined);
            core.resynced.emit(&OutlineResyncedEvent {
                outline: json!({ "items": items, "incomplete": incomplete }),
            });
            return Ok(());
        }
        let Some(event) = change.event.as_ref() else { return Ok(()) };
        let (Some(origin), Some((kind, items))) = (event.get("origin"), fact_of(event)) else {
            return Ok(());
        };
        core.changed.emit(&OutlineChangedEvent { kind, items, origin: origin.clone() });
        Ok(())
    }));
    let outline = cx.mirror(spec)?;

    let core =
        Rc::new(Core { cx, outline, changed, resynced, memos: RefCell::new(Memos::default()) });
    *slot.borrow_mut() = Rc::downgrade(&core);
    Ok((OutlineCapability(core), None))
}
