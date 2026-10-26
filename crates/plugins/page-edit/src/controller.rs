//! `controller.ts`: page edits that take refs or indexes, and the page
//! selection. Turning a page relative to its own rotation, and fixing a
//! position's page, live here once instead of in every app's click handler.
//!
//! A verb refuses before anything starts (permission, pages, position), then
//! stages its edit as one change on the document's queue (`ctx.changes`):
//! one step of the history, sent after everything staged before it. The page
//! list shows it once the engine answers; nothing predicts pages yet.
//!
//! Every verb is a TypeScript `async` function, so each is a
//! [`Promise::spawn`] here: the body runs at the call up to its first await,
//! and a refusal before that is a rejected promise, never a throw.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_geometry::js::number_to_string;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::changes::{PendingChange, StagedChange};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, DOCUMENTS, Error, Promise, rt};
use serde_json::{Value, json};

use crate::contract::{
    InsertBlankOptions, InsertFromBytesOptions, OperationOptions, PageArg, PageEditInsertResult,
    PagePosition, PagePositionOptions, PdfSize, ResolvedPosition,
};
use crate::model::{PageEditState, selected};

const CAPABILITY: &str = "page-edit";
/// Inserting, rotating and deleting pages (PDF permission bit 4 or 11). The engine enforces it too.
const ASSEMBLE: &str = "doc.pages.assemble";
/// Copying pages out of a document is a partial download. The engine enforces it too.
const DOWNLOAD: &str = "doc.download";
/// The size of a blank page when the document has no page to match: Letter, in points.
const LETTER_SIZE: PdfSize = PdfSize { width: 612.0, height: 792.0 };

/// What a change is called in history and refusals.
#[derive(Clone, Copy)]
enum PageVerb {
    Rotate,
    Reorder,
    Delete,
    InsertBlank,
    Insert,
    Duplicate,
}

impl PageVerb {
    fn key(self) -> &'static str {
        match self {
            PageVerb::Rotate => "pages.rotate",
            PageVerb::Reorder => "pages.reorder",
            PageVerb::Delete => "pages.delete",
            PageVerb::InsertBlank => "pages.insertBlank",
            PageVerb::Insert => "pages.insert",
            PageVerb::Duplicate => "pages.duplicate",
        }
    }
}

/// A JavaScript number as canonical JSON: an integer as one, the
/// non-finite spelled out.
pub(crate) fn js_number(n: f64) -> Value {
    if n.is_nan() {
        json!({ "$number": "NaN" })
    } else if n.is_infinite() {
        json!({ "$number": if n > 0.0 { "Infinity" } else { "-Infinity" } })
    } else if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// `pages[index]` for a JavaScript number: only a whole, non-negative,
/// in-range index names an entry (`-0` is `0`).
fn at_index(pages: &[Value], index: f64) -> Option<&Value> {
    if index >= 0.0 && index.fract() == 0.0 && index < pages.len() as f64 {
        pages.get(index as usize)
    } else {
        None
    }
}

/// `Date.now().toString(36)`.
fn base36(ms: f64) -> String {
    let mut n = ms.max(0.0).floor() as u64;
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(std::char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    digits.iter().rev().collect()
}

fn refs_json(pages: &[PageRef]) -> Value {
    Value::Array(pages.iter().map(|page| page.to_json()).collect())
}

/// `TypeError: Cannot read properties of <what> (reading '<member>')`.
fn type_error_reading(what: &str, member: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{member}')"))
}

fn no_pages() -> Error {
    plugin_error(code::INVALID_INPUT, CAPABILITY, "no pages given", None, None)
}

fn not_found(message: String) -> Error {
    plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `then` reaction that settles the function's own promise. A
/// `then` reaction, not an `await`, so an `AbortablePromise` costs no more.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

/// `mapErrors(work)`'s body, `return await work()`: an engine call outside
/// the guarded `ctx.doc` (another document, a scratch document), its failure
/// mapped through `toPluginError`. A plain `.await` costs what JavaScript's
/// does: three turns for the `AbortablePromise` the raw engine and its
/// handles answer, one for the plain promise `ctx.cancellable` returns with
/// a signal (`rt.rs`, `IntoFuture for Promise`). A caller's `await
/// mapErrors(...)` is the turn after that, so call sites wrap it in
/// [`rt::returned`].
async fn map_errors<T: Clone + 'static>(work: Promise<T>) -> Result<T, Error> {
    work.await.map_err(|error| to_plugin_error(CAPABILITY, &error))
}

/// The memo behind `getSelected`: the answer for one registry and one state.
struct SelectedMemo {
    pages: Rc<Value>,
    state: Rc<PageEditState>,
    selected: Rc<Vec<PageRef>>,
}

struct Inner {
    cx: Controller<PageEditState, Value>,
    selected: RefCell<Option<SelectedMemo>>,
}

/// `PageEditCapability`.
#[derive(Clone)]
pub struct PageEditCapability(Rc<Inner>);

impl PageEditCapability {
    /// Aborted when the document closes. The foreign handle, which can
    /// outlive the close, checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &AbortSignal {
        self.0.cx.lifetime()
    }

    // ── pages ──

    /// `ctx.document()?.pages ?? []`: the confirmed page list. A verb's
    /// promise settles after the engine's page fact replaced it.
    pub(crate) fn registry(&self) -> Rc<Value> {
        self.0
            .cx
            .document()
            .map(|meta| meta.pages)
            .unwrap_or_else(|| Rc::new(Value::Array(Vec::new())))
    }

    /// `ctx.getPage(page)`: the page's layout, or `None`.
    fn find_page(&self, page: PageArg) -> Option<Value> {
        let registry = self.registry();
        let pages = registry.as_array()?;
        match page {
            PageArg::Ref(page) => pages.iter().find(|layout| page.names(layout)).cloned(),
            PageArg::ForeignRef(_) => None,
            PageArg::Index(index) => at_index(pages, index).cloned(),
        }
    }

    /// `ctx.pageOf(page)`: the page's layout, or `not-found`.
    fn page_of(&self, page: PageArg) -> Result<Value, Error> {
        self.find_page(page).ok_or_else(|| not_found(page.not_found_message()))
    }

    /// `ctx.pageOf(page).ref`.
    fn ref_of(&self, page: PageArg) -> Result<PageRef, Error> {
        let layout = self.page_of(page)?;
        layout
            .get("ref")
            .and_then(PageRef::from_json)
            .ok_or_else(|| not_found(page.not_found_message()))
    }

    /// `refsOf(pages)`: a verb's pages as refs, checked when it is called. An
    /// index names the page at that position now, the page the user clicked.
    fn refs_of(&self, pages: &[PageArg]) -> Result<Vec<PageRef>, Error> {
        if pages.is_empty() {
            return Err(no_pages());
        }
        pages.iter().map(|page| self.ref_of(*page)).collect()
    }

    /// `positionOf(position)`: its page checked when the verb is called, like
    /// the verb's own pages. Default `End`.
    fn position_of(&self, position: Option<PagePosition>) -> Result<ResolvedPosition, Error> {
        Ok(match position {
            None | Some(PagePosition::End) => ResolvedPosition::End,
            Some(PagePosition::Start) => ResolvedPosition::Start,
            Some(PagePosition::After(page)) => ResolvedPosition::After(self.ref_of(page)?),
            Some(PagePosition::Before(page)) => ResolvedPosition::Before(self.ref_of(page)?),
        })
    }

    /// `sizeAt(position)`: the page the position names, else the page the new
    /// ones go next to (the first or the last), else Letter. `None` is the
    /// TypeScript's `undefined`, a layout without a `size`.
    fn size_at(&self, position: ResolvedPosition) -> Result<Option<Value>, Error> {
        let page = match position {
            ResolvedPosition::After(page) | ResolvedPosition::Before(page) => {
                return Ok(self.page_of(PageArg::Ref(page))?.get("size").cloned());
            }
            ResolvedPosition::Start | ResolvedPosition::End => position,
        };
        let registry = self.registry();
        let pages = registry.as_array().map(Vec::as_slice).unwrap_or_default();
        let neighbour = if page == ResolvedPosition::Start { pages.first() } else { pages.last() };
        Ok(match neighbour {
            None => Some(json!(LETTER_SIZE)),
            Some(layout) => layout.get("size").cloned(),
        })
    }

    // ── the selection ──

    /// The selected pages, in the order they were selected; pages the
    /// document no longer has drop out. The same registry and selection
    /// answer the same list.
    pub fn get_selected(&self) -> Rc<Vec<PageRef>> {
        let pages = self.registry();
        let state = self.0.cx.state();
        let mut memo = self.0.selected.borrow_mut();
        if let Some(memo) = memo.as_ref()
            && Rc::ptr_eq(&memo.pages, &pages)
            && Rc::ptr_eq(&memo.state, &state)
        {
            return memo.selected.clone();
        }
        let selected = Rc::new(if state.selected.is_empty() {
            Vec::new()
        } else {
            let present: HashSet<PageRef> = pages
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|layout| layout.get("ref").and_then(PageRef::from_json))
                .collect();
            state.selected.iter().copied().filter(|page| present.contains(page)).collect()
        });
        *memo = Some(SelectedMemo { pages, state, selected: selected.clone() });
        selected
    }

    /// Make these pages the selection; `[]` clears it. Pages the document
    /// doesn't have are left out.
    pub fn select(&self, pages: &[PageArg]) {
        let refs: Vec<PageRef> = pages
            .iter()
            .filter_map(|page| self.find_page(*page)?.get("ref").and_then(PageRef::from_json))
            .collect();
        self.0.cx.update_state(|state| selected(state, &refs));
    }

    // ── writes ──

    /// Whether rotating, reordering, deleting and inserting pages is allowed
    /// (`doc.pages.assemble`).
    pub fn can_edit(&self) -> bool {
        self.0.cx.allows(ASSEMBLE)
    }

    /// Whether copying pages out is allowed (`doc.download`): `extract()`,
    /// `duplicate()` and `insert_from_document()` need it.
    pub fn can_extract(&self) -> bool {
        self.0.cx.allows(DOWNLOAD)
    }

    /// `refuse(permission, operation, options)`: refuse before anything
    /// starts, without the right or once the caller cancelled.
    fn refuse(
        &self,
        permission: &str,
        operation: &str,
        signal: Option<&AbortSignal>,
    ) -> Result<(), Error> {
        self.0.cx.assert_allowed(permission, operation)?;
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

    /// `stageChange(verb, count, ops, undo)`: one user action, one change,
    /// one step of the history. Its label counts the pages it acts on, when
    /// the verb knows how many.
    fn stage_change(
        &self,
        verb: PageVerb,
        count: Option<f64>,
        ops: Vec<Value>,
        undo: Option<Vec<Value>>,
    ) -> Result<PendingChange, Error> {
        let label = match count {
            None => json!({ "key": verb.key() }),
            Some(count) => json!({ "key": verb.key(), "count": js_number(count) }),
        };
        let mut change = StagedChange::ops(label, ops);
        if let Some(undo) = undo {
            change.undo = undo;
        }
        self.0.cx.changes()?.stage(change)
    }

    /// `answer(change, options)`: the engine's answer to a staged change, its
    /// first item.
    async fn answer(
        change: PendingChange,
        signal: Option<AbortSignal>,
    ) -> Result<Option<Value>, Error> {
        let result = cancellable(CAPABILITY, signal, change.result())
            .await
            .map_err(|error| to_plugin_error(CAPABILITY, &error))?;
        // `result.items[0]`, inside the `try`: an answer without `items` is
        // the TypeError reading `0`, mapped like an engine failure.
        match result.get("items") {
            Some(Value::Array(items)) => Ok(items.first().cloned()),
            Some(Value::Object(items)) => Ok(items.get("0").cloned()),
            Some(Value::Null) => Err(type_error_reading("null", "0")),
            Some(_) => Ok(None),
            None => Err(type_error_reading("undefined", "0")),
        }
        .map_err(|error| to_plugin_error(CAPABILITY, &error))
    }

    /// `inserted(change, options)`: what an insert's answer names, the new
    /// pages. An answer with no item throws the `in` TypeError, unmapped.
    fn inserted(
        change: PendingChange,
        signal: Option<AbortSignal>,
    ) -> Promise<PageEditInsertResult> {
        Promise::spawn(async move {
            let item = rt::returned(Self::answer(change, signal)).await?;
            // `'insertedPages' in item`: any object answers, an array too;
            // `undefined` or a primitive throws.
            let pages = match item {
                Some(Value::Object(item)) => match item.get("insertedPages") {
                    Some(Value::Array(pages)) => pages.clone(),
                    _ => Vec::new(),
                },
                Some(Value::Array(_)) => Vec::new(),
                other => {
                    let shown = match other {
                        Some(Value::String(text)) => text,
                        Some(value) => value.to_string(),
                        None => "undefined".to_string(),
                    };
                    return Err(Error::named(
                        "TypeError",
                        format!(
                            "Cannot use 'in' operator to search for 'insertedPages' in {shown}"
                        ),
                    ));
                }
            };
            Ok(PageEditInsertResult { pages })
        })
    }

    /// `rotationOf(ref)`: a page's rotation once this session's changes on
    /// their way apply, the last rotation one of them gives it, else the
    /// page's own. What a turn starts from, so two quick quarter turns are a
    /// half turn. A rotation that is not a number reads as `NaN`, as
    /// `undefined + delta` does.
    fn rotation_of(&self, page: PageRef) -> Result<f64, Error> {
        let rotation_value = |value: Option<&Value>| value.and_then(Value::as_f64);
        let mut rotation = rotation_value(self.page_of(PageArg::Ref(page))?.get("rotation"));
        for change in self.0.cx.changes()?.pending() {
            for op in change.shows() {
                if op.get("type").and_then(Value::as_str) != Some("pages.rotate") {
                    continue;
                }
                let names = op.get("pages").and_then(Value::as_array).is_some_and(|pages| {
                    pages.iter().any(|p| {
                        p.get("objectNumber").and_then(Value::as_f64)
                            == Some(f64::from(page.page_object_number))
                    })
                });
                if names {
                    rotation = rotation_value(op.get("rotation"));
                }
            }
        }
        Ok(rotation.unwrap_or(f64::NAN))
    }

    /// `rotate(refs, rotationAfter)`: rotate pages as one change, one op per
    /// rotation they end up with. Undoing it is each page turned back, so a
    /// turn right after an undo starts from where the undo leaves the page.
    /// The groups are keyed the way a `Map` keys numbers (`NaN` is one key,
    /// `-0` is `0`), in first-seen order.
    fn rotate(
        &self,
        refs: &[PageRef],
        rotation_after: impl Fn(&Self, PageRef) -> Result<f64, Error>,
    ) -> Result<PendingChange, Error> {
        fn add(groups: &mut Vec<(f64, Vec<PageRef>)>, rotation: f64, page: PageRef) {
            let same = |key: f64| key == rotation || (key.is_nan() && rotation.is_nan());
            match groups.iter_mut().find(|(key, _)| same(*key)) {
                Some((_, pages)) => pages.push(page),
                None => groups.push((rotation, vec![page])),
            }
        }
        let mut after = Vec::new();
        let mut before = Vec::new();
        for page in refs {
            add(&mut before, self.rotation_of(*page)?, *page);
            add(&mut after, rotation_after(self, *page)?, *page);
        }
        let ops_of = |groups: Vec<(f64, Vec<PageRef>)>| -> Vec<Value> {
            groups
                .into_iter()
                .map(|(rotation, pages)| {
                    json!({ "type": "pages.rotate", "pages": refs_json(&pages), "rotation": js_number(rotation) })
                })
                .collect()
        };
        self.stage_change(
            PageVerb::Rotate,
            Some(refs.len() as f64),
            ops_of(after),
            Some(ops_of(before)),
        )
    }

    /// `settled(options)`: the pages as they show, every change on its way
    /// sent and answered first, so a copy holds the edits made before it.
    async fn settled(&self, signal: Option<AbortSignal>) -> Result<(), Error> {
        let changes = self.0.cx.changes()?;
        changes.send_holds();
        cancellable(CAPABILITY, signal, changes.when_settled()).await
    }

    /// `extractShown(refs, options)`: copy pages of this document out, as
    /// they show.
    async fn extract_shown(
        &self,
        refs: Vec<PageRef>,
        signal: Option<AbortSignal>,
    ) -> Result<Value, Error> {
        rt::returned(self.settled(signal.clone())).await?;
        let doc = self.0.cx.doc()?;
        adopt(cancellable(
            CAPABILITY,
            signal,
            doc.call("pages.extract", None, vec![refs_json(&refs)]),
        ))
        .await
    }

    /// `insertBytes(verb, count, bytes, position, options)`: stage the insert
    /// of a PDF's pages; resolves the new pages.
    fn insert_bytes(
        &self,
        verb: PageVerb,
        count: Option<f64>,
        source: Value,
        position: ResolvedPosition,
        signal: Option<AbortSignal>,
    ) -> Promise<PageEditInsertResult> {
        let op =
            json!({ "type": "pages.insert", "source": source, "position": position.to_json() });
        match self.stage_change(verb, count, vec![op], None) {
            Ok(change) => Self::inserted(change, signal),
            Err(error) => {
                // An extract's `{"$hostBytes": n}` is parked in the host until
                // something reads it; nothing will now, so read it here to let
                // it go. Rust-only: the TypeScript holds the bytes in memory.
                if source.get("$hostBytes").is_some() {
                    let _ = Bytes::from_json(&source);
                }
                Promise::rejected(error)
            }
        }
    }

    /// `pagesOfBytes(bytes, pageIndexes, signal)`: some pages of a PDF. Open
    /// the bytes beside the document and extract them, and close the source
    /// again whatever happened.
    async fn pages_of_bytes(
        &self,
        bytes: &Bytes,
        page_indexes: &[f64],
        signal: Option<AbortSignal>,
    ) -> Result<Value, Error> {
        let engine = self.0.cx.engine().ok_or_else(|| Error::new("[page-edit] no engine"))?;
        let input = json!({
            "kind": "bytes",
            "id": format!("page-edit-source-{}", base36(rt::now())),
            "bytes": bytes.to_json(),
        });
        let opened = engine.open(input, Some(json!({ "scope": ["*"] })));
        let source = rt::returned(map_errors(opened)).await?;
        let copied = async {
            let listed = source.call("pages.list", None, Vec::new());
            let layout =
                rt::returned(map_errors(cancellable(CAPABILITY, signal.clone(), listed))).await?;
            let pages = layout.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut refs = Vec::with_capacity(page_indexes.len());
            for index in page_indexes {
                let page = at_index(&pages, *index).ok_or_else(|| {
                    not_found(format!("the PDF has no page {}", number_to_string(*index)))
                })?;
                refs.push(page.get("ref").cloned().unwrap_or(Value::Null));
            }
            let extracted = source.call("pages.extract", None, vec![Value::Array(refs)]);
            rt::returned(map_errors(cancellable(CAPABILITY, signal, extracted))).await
        }
        .await;
        // `finally { await source.close() }`: a close failure wins, unmapped.
        source.call("close", None, Vec::new()).await?;
        copied
    }

    // ── the verbs ──

    /// Turn each page by a quarter or half turn from its rotation, the one it
    /// has once the edits on their way apply: two quick quarter turns are a
    /// half turn. `delta` is any JavaScript number; the wrap is done in
    /// `f64`, as the TypeScript does it.
    pub fn rotate_by(
        &self,
        pages: Vec<PageArg>,
        delta: f64,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.rotateBy", options.signal.as_ref())?;
            let refs = this.refs_of(&pages)?;
            // The double modulo keeps -90 from 0 at 270.
            let change = this.rotate(&refs, |this, page| {
                Ok((((this.rotation_of(page)? + delta) % 360.0) + 360.0) % 360.0)
            })?;
            rt::returned(Self::answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// Give every page the same rotation.
    pub fn set_rotation(
        &self,
        pages: Vec<PageArg>,
        rotation: f64,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.setRotation", options.signal.as_ref())?;
            let refs = this.refs_of(&pages)?;
            let change = this.rotate(&refs, |_, _| Ok(rotation))?;
            rt::returned(Self::answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// Move pages to `position`; several pages keep their order and land
    /// together.
    pub fn reorder(
        &self,
        pages: Vec<PageArg>,
        position: PagePosition,
        options: OperationOptions,
    ) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.reorder", options.signal.as_ref())?;
            let refs = this.refs_of(&pages)?;
            let position = this.position_of(Some(position))?;
            let op = json!({ "type": "pages.reorder", "pages": refs_json(&refs), "position": position.to_json() });
            let change =
                this.stage_change(PageVerb::Reorder, Some(refs.len() as f64), vec![op], None)?;
            rt::returned(Self::answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// Delete pages. Deleting every page is refused (`invalid-input`): a
    /// document keeps one.
    pub fn delete(&self, pages: Vec<PageArg>, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.delete", options.signal.as_ref())?;
            let refs = this.refs_of(&pages)?;
            let deleting: HashSet<PageRef> = refs.iter().copied().collect();
            let registry = this.registry();
            let every = registry.as_array().into_iter().flatten().all(|layout| {
                layout
                    .get("ref")
                    .and_then(PageRef::from_json)
                    .is_some_and(|r| deleting.contains(&r))
            });
            if every {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    CAPABILITY,
                    "a document keeps at least one page",
                    None,
                    None,
                ));
            }
            let op = json!({ "type": "pages.delete", "pages": refs_json(&refs) });
            let change =
                this.stage_change(PageVerb::Delete, Some(refs.len() as f64), vec![op], None)?;
            rt::returned(Self::answer(change, options.signal)).await?;
            Ok(())
        })
    }

    /// Insert blank pages, one unless `count` says more.
    pub fn insert_blank(&self, options: InsertBlankOptions) -> Promise<PageEditInsertResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.insertBlank", options.signal.as_ref())?;
            let position = this.position_of(options.position)?;
            let count = options.count.unwrap_or(1.0);
            let size = match options.size {
                Some(size) => Some(json!(size)),
                None => this.size_at(position)?,
            };
            let mut spec = serde_json::Map::new();
            if let Some(size) = size {
                spec.insert("size".into(), size);
            }
            spec.insert("count".into(), js_number(count));
            let op = json!({ "type": "pages.insertBlank", "spec": spec, "position": position.to_json() });
            let change = this.stage_change(PageVerb::InsertBlank, Some(count), vec![op], None)?;
            adopt(Self::inserted(change, options.signal)).await
        })
    }

    /// Insert the pages of another PDF, or only the ones at `page_indexes`.
    pub fn insert_from_bytes(
        &self,
        bytes: Bytes,
        options: InsertFromBytesOptions,
    ) -> Promise<PageEditInsertResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.insertFromBytes", options.signal.as_ref())?;
            let position = this.position_of(options.position)?;
            let signal = options.signal;
            let (source, count) = match options.page_indexes {
                Some(indexes) if indexes.is_empty() => return Err(no_pages()),
                Some(indexes) => {
                    let pages =
                        rt::returned(this.pages_of_bytes(&bytes, &indexes, signal.clone())).await?;
                    (pages, Some(indexes.len() as f64))
                }
                None => (bytes.to_json(), None),
            };
            adopt(this.insert_bytes(PageVerb::Insert, count, source, position, signal)).await
        })
    }

    /// Insert copies of pages from another document open in the viewer.
    /// Needs `doc.download` on that document and `doc.pages.assemble` on this
    /// one. The pages are the other document's, so its page list resolves
    /// them.
    pub fn insert_from_document(
        &self,
        document_id: String,
        pages: Vec<PageArg>,
        options: PagePositionOptions,
    ) -> Promise<PageEditInsertResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.insertFromDocument", options.signal.as_ref())?;
            if pages.is_empty() {
                return Err(no_pages());
            }
            let other: Rc<dyn DocumentHandle> = this
                .0
                .cx
                .document_handle(Some(&document_id))
                .ok_or_else(|| not_found(format!("document '{document_id}' is not open")))?;
            let other_pages = this.0.cx.get(&DOCUMENTS)?.list_pages(Some(&document_id));
            let mut refs = Vec::with_capacity(pages.len());
            for page in &pages {
                let found = match *page {
                    PageArg::Index(index) => at_index(&other_pages, index),
                    PageArg::Ref(page) => other_pages.iter().find(|layout| page.names(layout)),
                    PageArg::ForeignRef(_) => None,
                };
                let found = found
                    .and_then(|layout| layout.get("ref").and_then(PageRef::from_json))
                    .ok_or_else(|| {
                        not_found(format!("document '{document_id}' has no such page"))
                    })?;
                refs.push(found);
            }
            let position = this.position_of(options.position)?;
            // The other document's session decides whether its pages may be
            // copied out: the engine refuses `doc.download` there with the
            // permission named.
            let extracted = other.call("pages.extract", None, vec![refs_json(&refs)]);
            let bytes = rt::returned(map_errors(cancellable(
                CAPABILITY,
                options.signal.clone(),
                extracted,
            )))
            .await?;
            adopt(this.insert_bytes(
                PageVerb::Insert,
                Some(refs.len() as f64),
                bytes,
                position,
                options.signal,
            ))
            .await
        })
    }

    /// Copy pages as they show, with the edits on their way, right after the
    /// last of them unless `position` says otherwise.
    pub fn duplicate(
        &self,
        pages: Vec<PageArg>,
        options: PagePositionOptions,
    ) -> Promise<PageEditInsertResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(ASSEMBLE, "pageEdit.duplicate", options.signal.as_ref())?;
            this.0.cx.assert_allowed(DOWNLOAD, "pageEdit.duplicate")?;
            let refs = this.refs_of(&pages)?;
            let position = match options.position {
                Some(position) => this.position_of(Some(position))?,
                None => ResolvedPosition::After(*refs.last().ok_or_else(no_pages)?),
            };
            let count = refs.len() as f64;
            let bytes = rt::returned(this.extract_shown(refs, options.signal.clone())).await?;
            adopt(this.insert_bytes(
                PageVerb::Duplicate,
                Some(count),
                bytes,
                position,
                options.signal,
            ))
            .await
        })
    }

    /// Copy pages into a new PDF, as they show with the edits on their way,
    /// and resolve its bytes. This document doesn't change.
    pub fn extract(&self, pages: Vec<PageArg>, options: OperationOptions) -> Promise<Bytes> {
        let this = self.clone();
        Promise::spawn(async move {
            this.refuse(DOWNLOAD, "pageEdit.extract", options.signal.as_ref())?;
            let refs = this.refs_of(&pages)?;
            let value = adopt(Promise::spawn({
                let this = this.clone();
                async move { this.extract_shown(refs, options.signal).await }
            }))
            .await?;
            Bytes::from_json(&value)
                .ok_or_else(|| Error::new("[page-edit] extract answered no bytes"))
        })
    }
}

/// `createPageEditController(ctx)`: the api; the plugin has no `connect`.
pub fn create_page_edit_controller(cx: Controller<PageEditState, Value>) -> PageEditCapability {
    PageEditCapability(Rc::new(Inner { cx, selected: RefCell::new(None) }))
}
