//! `page-view.ts`: the document's pages as the user sees them
//! (`DocumentMeta.pages`), the engine's pages with this session's pending
//! changes replayed on top through [`crate::page_predict`], as a mirror's
//! `view()` shows its value. The stage, the thumbnails, the page selection
//! and `ctx.pageOf` all read them, so a page number in a verb is the page
//! the user sees, and a page edit shows before the engine answers.
//!
//! The engine's pages move with its layout events, this session's and other
//! sessions' alike. A change stops replaying once its last event folded: the
//! engine's pages hold it by then. A page that reads as it did is kept, and
//! pages that all read as they did are the same list (the same [`Rc`]), so
//! an answer the prediction got right changes nothing.
//!
//! Pages a pending change makes (a blank insert, a copy, a deleted page
//! coming back) are in the list before the engine has them: `pending` names
//! them, with where their look comes from until it does. The page label
//! sections move with the pages, as the engine moves them, so each page's
//! `label` is the one the engine will give it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

use crate::Error;
use crate::changes::{DocumentChanges, PendingChange};
use crate::js::same_value;
use crate::mirror::{last_event_of, showing};
use crate::page_predict::{
    PageList, pages_after, pages_after_restore, pages_relabelled, same_sections,
};
use crate::store::Report;

/// `PendingPage`: where a page the engine doesn't have yet gets its look
/// until the engine has it. A blank insert is a white page of its size
/// (`blank`); a deleted page an undo brings back has the pictures it had
/// (`own`); a copy draws as the page it copies (`copy`, with that page's
/// ref).
#[derive(Clone, Debug)]
pub enum PendingPage {
    Blank,
    Own,
    Copy { of: Value },
}

/// `sameValue` over two looks: a copy's page ref by what it holds, so an
/// object number the engine wrote as `2.0` is the page `2`.
impl PartialEq for PendingPage {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PendingPage::Blank, PendingPage::Blank) | (PendingPage::Own, PendingPage::Own) => true,
            (PendingPage::Copy { of: a }, PendingPage::Copy { of: b }) => same_value(a, b),
            _ => false,
        }
    }
}

impl Serialize for PendingPage {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match self {
            PendingPage::Blank => map.serialize_entry("look", "blank")?,
            PendingPage::Own => map.serialize_entry("look", "own")?,
            PendingPage::Copy { of } => {
                map.serialize_entry("look", "copy")?;
                map.serialize_entry("of", of)?;
            }
        }
        map.end()
    }
}

/// `ReadonlyMap<PageObjectNumber, PendingPage>`, in insertion order. It
/// prints as a `Map` does in the vectors: `{"$map": [[number, look], ...]}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PendingPages(Vec<(Value, PendingPage)>);

impl PendingPages {
    /// The look of the page with this object number, if the engine doesn't
    /// have it yet.
    pub fn get(&self, object_number: &Value) -> Option<&PendingPage> {
        self.0.iter().find(|(n, _)| same_number(n, object_number)).map(|(_, look)| look)
    }
    pub fn has(&self, object_number: &Value) -> bool {
        self.get(object_number).is_some()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &(Value, PendingPage)> {
        self.0.iter()
    }
    fn set(&mut self, object_number: Value, look: PendingPage) {
        match self.0.iter_mut().find(|(n, _)| same_number(n, &object_number)) {
            Some(entry) => entry.1 = look,
            None => self.0.push((object_number, look)),
        }
    }
}

impl Serialize for PendingPages {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        struct Entries<'a>(&'a [(Value, PendingPage)]);
        impl Serialize for Entries<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut seq = s.serialize_seq(Some(self.0.len()))?;
                for entry in self.0 {
                    seq.serialize_element(&(&entry.0, &entry.1))?;
                }
                seq.end()
            }
        }
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry("$map", &Entries(&self.0))?;
        map.end()
    }
}

fn same_number(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

thread_local! {
    static NO_PENDING: Rc<PendingPages> = Rc::default();
}

/// `NO_PENDING_PAGES`: one shared empty map, so a view with no pending
/// pages keeps the same one.
pub fn no_pending_pages() -> Rc<PendingPages> {
    NO_PENDING.with(Rc::clone)
}

/// `PageView`: the pages as the user sees them, their label sections, and
/// the pages the engine doesn't have yet.
#[derive(Clone, Debug)]
pub struct View {
    pub pages: Rc<Value>,
    pub labels: Rc<Value>,
    pub pending: Rc<PendingPages>,
}

/// `layoutFromEvent(event)`: the engine's pages a document event carries,
/// or `None` for an event that doesn't change them (annotations, metadata).
pub fn layout_from_event(event: &Value) -> Option<&Value> {
    match event.get("type").and_then(Value::as_str)? {
        "pages.reordered" | "pages.rotated" | "pages.deleted" | "pages.inserted"
        | "pages.restored" => event.get("layout"),
        _ => None,
    }
}

/// `predictPages(list, op)`: one op of a pending change over the pages, the
/// page ops by the engine's rules.
pub fn predict_pages(list: &Rc<PageList>, op: &Value) -> Result<Rc<PageList>, Error> {
    match op.get("type").and_then(Value::as_str) {
        Some(
            "pages.rotate" | "pages.reorder" | "pages.delete" | "pages.insertBlank" | "pages.copy",
        ) => pages_after(list, op),
        Some("pages.restore") => pages_after_restore(list, op.get("pages").unwrap_or(&Value::Null)),
        Some("pages.relabel") => pages_relabelled(list, op.get("labels").unwrap_or(&Value::Null)),
        _ => Ok(list.clone()),
    }
}

struct Inner {
    changes: DocumentChanges,
    report: Report,
    folded: RefCell<HashSet<String>>,
    listeners: RefCell<Vec<(u64, Rc<dyn Fn()>)>>,
    next_listener: std::cell::Cell<u64>,
    engine: RefCell<Rc<PageList>>,
    /// What the last replay gave, before it was matched to the view: the
    /// same list means nothing moved.
    replayed: RefCell<Rc<PageList>>,
    view: RefCell<View>,
}

/// A document's page view (`PageViewController`). Cheap to clone.
#[derive(Clone)]
pub struct PageView(Rc<Inner>);

impl PageView {
    /// `createPageView({ changes, report })`.
    pub fn new(changes: DocumentChanges, report: Report) -> Self {
        let empty = Rc::new(Value::Array(Vec::new()));
        let engine = PageList::new(empty.clone(), empty.clone());
        let view = View { pages: empty.clone(), labels: empty, pending: no_pending_pages() };
        Self(Rc::new(Inner {
            changes,
            report,
            folded: RefCell::default(),
            listeners: RefCell::default(),
            next_listener: std::cell::Cell::new(0),
            replayed: RefCell::new(engine.clone()),
            engine: RefCell::new(engine),
            view: RefCell::new(view),
        }))
    }

    /// `setEnginePages(list)`: the engine's pages and sections, the snapshot
    /// at open, then each layout event's. A member the answer left out is
    /// the `undefined` marker.
    pub fn set_engine_pages(&self, list: &Value) {
        let read = |name: &str| Rc::new(list.get(name).cloned().unwrap_or_else(crate::undefined));
        *self.0.engine.borrow_mut() = PageList::new(read("pages"), read("labels"));
    }

    /// `fold(event)`: once an event is a change's last, the change stops
    /// replaying (`createFoldedChanges`).
    pub fn fold(&self, event: &Value) {
        let Some(id) = last_event_of(event) else { return };
        let mut folded = self.0.folded.borrow_mut();
        if folded.contains(id) || self.0.changes.find(id).is_none() {
            return;
        }
        folded.retain(|each| self.0.changes.find(each).is_some());
        folded.insert(id.to_string());
    }

    fn shown(&self) -> Vec<PendingChange> {
        showing(&self.0.changes, &[], &self.0.folded.borrow())
    }

    /// `refresh()`: work the view out again, the new one when it changed
    /// since the last time, else `None`. `Err` where the TypeScript throws
    /// (an engine answer without its pages).
    pub fn refresh(&self) -> Result<Option<View>, Error> {
        let showing = self.shown();
        let engine = self.0.engine.borrow().clone();
        let next = replay(&engine, &showing, &self.0.report);
        if Rc::ptr_eq(&next, &self.0.replayed.borrow()) {
            return Ok(None);
        }
        *self.0.replayed.borrow_mut() = next.clone();
        let previous = self.0.view.borrow().clone();
        let pages = kept_where_same(&previous.pages, &next.pages)?;
        let labels = if same_sections(&previous.labels, &next.labels)? {
            previous.labels.clone()
        } else {
            next.labels.clone()
        };
        let pending = kept_if_same(&previous.pending, pending_of(&pages, &engine.pages, &showing));
        if Rc::ptr_eq(&pages, &previous.pages)
            && Rc::ptr_eq(&labels, &previous.labels)
            && Rc::ptr_eq(&pending, &previous.pending)
        {
            return Ok(None);
        }
        let view = View { pages, labels, pending };
        *self.0.view.borrow_mut() = view.clone();
        let listeners: Vec<_> = self.0.listeners.borrow().iter().map(|(_, l)| l.clone()).collect();
        for listener in listeners {
            listener();
        }
        Ok(Some(view))
    }

    /// `current()`: the view as the last `refresh` left it.
    pub fn current(&self) -> View {
        self.0.view.borrow().clone()
    }

    /// `pending(page)`: where a page's look comes from until the engine has
    /// it; `None` when it has it, or no such page.
    pub fn pending(&self, page_ref: &Value) -> Option<PendingPage> {
        let number = page_ref.get("objectNumber")?;
        self.0.view.borrow().pending.get(number).cloned()
    }

    /// `has(page)`: whether the page is one the user sees.
    pub fn has(&self, page_ref: &Value) -> bool {
        let Some(number) = page_ref.get("objectNumber") else { return false };
        let view = self.0.view.borrow();
        view.pages.as_array().is_some_and(|pages| {
            pages.iter().any(|p| {
                p.get("ref")
                    .and_then(|r| r.get("objectNumber"))
                    .is_some_and(|n| same_number(n, number))
            })
        })
    }

    /// `onChange(listener)`: the view changed. The returned function stops
    /// listening.
    pub fn on_change(&self, listener: Rc<dyn Fn()>) -> Rc<dyn Fn()> {
        let id = self.0.next_listener.get() + 1;
        self.0.next_listener.set(id);
        self.0.listeners.borrow_mut().push((id, listener));
        let weak = Rc::downgrade(&self.0);
        Rc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.listeners.borrow_mut().retain(|(each, _)| *each != id);
            }
        })
    }
}

/// `replay(engine, showing, predictPages, report)`: the changes in staging
/// order; a prediction that throws is reported, and the engine's pages show.
fn replay(engine: &Rc<PageList>, changes: &[PendingChange], report: &Report) -> Rc<PageList> {
    let mut ordered: Vec<&PendingChange> = changes.iter().collect();
    ordered.sort_by_key(|c| c.seq());
    let mut next = engine.clone();
    for change in ordered {
        for op in change.shows() {
            match predict_pages(&next, &op) {
                Ok(list) => next = list,
                Err(error) => {
                    report(&error);
                    return engine.clone();
                }
            }
        }
    }
    next
}

fn object_number(page: &Value) -> Option<&Value> {
    page.get("ref").and_then(|r| r.get("objectNumber"))
}

/// `keptWhereSame(previous, next)`: `next`, with each page that reads as
/// it does in `previous` being that page, and `previous` itself when every
/// page is and none moved.
fn kept_where_same(previous: &Rc<Value>, next: &Rc<Value>) -> Result<Rc<Value>, Error> {
    let read = |v: &Value| -> Result<Vec<Value>, Error> {
        match v {
            Value::Array(items) => Ok(items.clone()),
            _ => Err(Error::named(
                "TypeError",
                "Cannot read properties of undefined (reading 'length')",
            )),
        }
    };
    let next_pages = read(next)?;
    let before = read(previous)?;
    let mut same = before.len() == next_pages.len();
    let mut pages = Vec::with_capacity(next_pages.len());
    for (index, page) in next_pages.into_iter().enumerate() {
        // `new Map(previous.map(...))`: the last page with a number wins.
        let was = object_number(&page).and_then(|number| {
            before.iter().rposition(|p| object_number(p).is_some_and(|n| same_number(n, number)))
        });
        match was {
            Some(at) if same_value(&before[at], &page) => {
                if at != index {
                    same = false;
                }
                pages.push(before[at].clone());
            }
            _ => {
                same = false;
                pages.push(page);
            }
        }
    }
    Ok(if same { previous.clone() } else { Rc::new(Value::Array(pages)) })
}

/// `keptIfSame(previous, next)`: `next`, or `previous` when it names the
/// same pages with the same looks.
fn kept_if_same(previous: &Rc<PendingPages>, next: Rc<PendingPages>) -> Rc<PendingPages> {
    if previous.len() != next.len() {
        return next;
    }
    if next.iter().all(|(number, look)| previous.get(number) == Some(look)) {
        previous.clone()
    } else {
        next
    }
}

/// `pendingOf(pages, engine, showing)`: the pages the user sees that the
/// engine doesn't have, by the change that makes each one.
fn pending_of(pages: &Value, engine: &Value, showing: &[PendingChange]) -> Rc<PendingPages> {
    let held: Vec<&Value> =
        engine.as_array().into_iter().flatten().filter_map(object_number).collect();
    let unheld: Vec<&Value> = pages
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(object_number)
        .filter(|number| !held.iter().any(|h| same_number(h, number)))
        .collect();
    if unheld.is_empty() {
        return no_pending_pages();
    }
    let mut looks = PendingPages::default();
    for change in showing {
        for op in change.shows() {
            match op.get("type").and_then(Value::as_str) {
                Some("pages.insertBlank") => {
                    for number in
                        op.get("objectNumbers").and_then(Value::as_array).into_iter().flatten()
                    {
                        looks.set(number.clone(), PendingPage::Blank);
                    }
                }
                Some("pages.copy") => {
                    let copied = op.get("pages").and_then(Value::as_array);
                    for (at, number) in op
                        .get("objectNumbers")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        if let Some(of) = copied.and_then(|pages| pages.get(at)) {
                            looks.set(number.clone(), PendingPage::Copy { of: of.clone() });
                        }
                    }
                }
                Some("pages.restore") => {
                    for restored in op.get("pages").and_then(Value::as_array).into_iter().flatten()
                    {
                        if let Some(number) = restored.get("page").and_then(object_number) {
                            looks.set(number.clone(), PendingPage::Own);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = PendingPages::default();
    for number in unheld {
        let look = looks.get(number).cloned().unwrap_or(PendingPage::Own);
        out.set(number.clone(), look);
    }
    Rc::new(out)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde_json::json;

    use super::*;
    use crate::Promise;
    use crate::changes::StagedChange;

    fn page_ref(number: u32) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": number })
    }

    /// A page as the Rust engine answers it: every geometry number a float.
    fn engine_page(number: u32, index: usize, width: f64, height: f64) -> Value {
        let page_box = json!({ "x": 0.0, "y": 0.0, "width": width, "height": height });
        json!({
            "index": index,
            "ref": page_ref(number),
            "label": null,
            "size": { "width": width, "height": height },
            "rotation": 0,
            "userUnit": 1.0,
            "tabOrder": null,
            "boxes": { "media": page_box, "crop": page_box, "bleed": page_box,
                "trim": page_box, "art": page_box },
            "pdfCropBox": { "left": 0.0, "bottom": 0.0, "right": width, "top": height },
        })
    }

    fn view_of(changes: &DocumentChanges) -> (PageView, Rc<Cell<u32>>) {
        let view = PageView::new(changes.clone(), Rc::new(|e| panic!("{e}")));
        let fired = Rc::new(Cell::new(0));
        let count = fired.clone();
        let _ = view.on_change(Rc::new(move || count.set(count.get() + 1)));
        (view, fired)
    }

    #[test]
    fn same_value_compares_numbers_by_what_they_hold() {
        assert!(same_value(&json!(1), &json!(1.0)));
        assert!(same_value(&json!({ "a": [0, { "b": 2 }] }), &json!({ "a": [0.0, { "b": 2.0 }] })));
        assert!(!same_value(&json!(0.0), &json!(-0.0)));
        assert!(!same_value(&json!({ "a": 1 }), &json!({ "a": 1, "b": 1 })));
        assert!(!same_value(&json!([1]), &json!({ "0": 1 })));
        assert!(
            PendingPage::Copy { of: json!({ "objectNumber": 2 }) }
                == PendingPage::Copy { of: json!({ "objectNumber": 2.0 }) }
        );
    }

    #[test]
    fn an_engine_answer_in_floats_keeps_the_integer_prediction_it_matches() {
        crate::rt::reset();
        let (answer, _resolve) = Promise::pending();
        let changes = DocumentChanges::new(
            Rc::new(move |_, _| answer.clone()),
            Rc::new(|| {}),
            Rc::new(|e| panic!("{e}")),
        );
        let (view, fired) = view_of(&changes);
        let one = engine_page(1, 0, 612.0, 792.0);
        view.set_engine_pages(&json!({ "pages": [one], "labels": [] }));
        view.refresh().unwrap();
        let fired_at_open = fired.get();

        // The blank page the prediction writes has integer geometry.
        let change = changes
            .for_plugin("test")
            .stage(StagedChange::ops(
                json!({ "key": "pages.insertBlank" }),
                vec![json!({ "type": "pages.insertBlank",
                    "spec": { "size": { "width": 300, "height": 200 } },
                    "objectNumbers": [40] })],
            ))
            .unwrap();
        view.refresh().unwrap().expect("the blank page shows at once");
        let shown = view.current();
        assert_eq!(shown.pages.as_array().unwrap().len(), 2);
        assert_eq!(shown.pages[1]["size"]["width"], json!(300));
        assert!(shown.pending.has(&json!(40)));

        // The engine answers the same page, every number a float.
        let layout = json!({ "pages": [one, engine_page(40, 1, 300.0, 200.0)], "labels": [] });
        view.set_engine_pages(&layout);
        view.fold(&json!({ "type": "pages.inserted", "layout": layout,
            "origin": { "kind": "local", "tx": { "id": change.op_id(), "index": 0, "count": 1 } } }));
        let answered = view.refresh().unwrap().expect("the page is no longer pending");
        assert!(Rc::ptr_eq(&answered.pages, &shown.pages), "the same pages, kept whole");
        assert!(answered.pending.is_empty());
        assert_eq!(fired.get(), fired_at_open + 2);

        // Nothing moved since: no new view, and no listener runs.
        assert!(view.refresh().unwrap().is_none());
        assert_eq!(fired.get(), fired_at_open + 2);
    }

    #[test]
    fn engine_pages_that_read_the_same_in_floats_change_nothing() {
        let changes = DocumentChanges::new(
            Rc::new(|_, _| Promise::pending().0),
            Rc::new(|| {}),
            Rc::new(|e| panic!("{e}")),
        );
        let (view, fired) = view_of(&changes);
        // An engine that writes integers, then one that writes the same in floats.
        let integers = json!({ "pages": [{ "index": 0, "ref": page_ref(1),
            "size": { "width": 612, "height": 792 }, "rotation": 0 }], "labels": [] });
        view.set_engine_pages(&integers);
        let opened = view.refresh().unwrap().expect("the first pages");
        let before = fired.get();
        let floats = json!({ "pages": [{ "index": 0, "ref": page_ref(1),
            "size": { "width": 612.0, "height": 792.0 }, "rotation": 0 }], "labels": [] });
        view.set_engine_pages(&floats);
        assert!(view.refresh().unwrap().is_none(), "the same pages are no new view");
        assert!(Rc::ptr_eq(&view.current().pages, &opened.pages));
        assert_eq!(fired.get(), before);
    }
}
