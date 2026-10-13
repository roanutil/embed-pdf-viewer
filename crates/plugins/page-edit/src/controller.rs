//! `controller.ts`: stateless, it turns the engine handle's page service
//! into a ref-addressed edit capability. The relative-to-absolute rotation
//! and the placement-to-index resolution live here, once.
//!
//! Turn costs follow the TypeScript. `ctx.doc` is the guarded handle, whose
//! promises are plain ones (awaiting one costs a turn); the raw engine, the
//! source document it opens and another document's handle answer with
//! `AbortablePromise`s, which `await` adopts as thenables (three turns).

use std::future::Future;
use std::rc::Rc;

use epdf_geometry::js::number_to_string;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::{Controller, Error, undefined};
use serde_json::{Value, json};

use crate::contract::{InsertBlankOptions, InsertFromBytesOptions, PagePlacement, PdfSize};

const CAPABILITY: &str = "page-edit";
/// PDF bit 11 (assemble: insert, rotate, delete pages). The engine enforces it too.
const ASSEMBLE_CAPABILITY: &str = "doc.pages.assemble";
/// US Letter: the blank-page size when the registry is empty.
const LETTER: PdfSize = PdfSize { width: 612.0, height: 792.0 };

/// A JavaScript number as canonical JSON: the non-finite ones spelled out.
pub fn js_number(n: f64) -> Value {
    if n.is_nan() {
        json!({ "$number": "NaN" })
    } else if n.is_infinite() {
        json!({ "$number": if n > 0.0 { "Infinity" } else { "-Infinity" } })
    } else {
        json!(n)
    }
}

/// `Math.min(a, b)`: NaN if either is.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `Math.max(a, b)`: NaN if either is.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `Date.now().toString(36)`.
fn base36(ms: f64) -> String {
    let mut n = ms.max(0.0).floor() as u64;
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(std::char::from_digit((n % 36) as u32, 36).expect("digit"));
        n /= 36;
    }
    digits.iter().rev().collect()
}

fn refs_json(pages: &[PageRef]) -> Value {
    Value::Array(pages.iter().map(|p| p.to_json()).collect())
}

fn size_of(page: &Value) -> Option<PdfSize> {
    serde_json::from_value(page.get("size")?.clone()).ok()
}

/// `await raw` on an `AbortablePromise`: the resolve-thenable job, the
/// adopted promise's reaction, then the await's own.
async fn await_adopted<T: Clone + 'static>(raw: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    let result = raw.await;
    rt::tick().await;
    result
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the reaction that settles the function's own promise.
async fn returned<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.await
}

/// A registry page, resolved from a placement: its size is what a blank
/// page inserted beside it defaults to.
#[derive(Clone)]
struct Anchor {
    size: Option<PdfSize>,
}

struct Inner {
    cx: Controller<Value, Value>,
    /// Read-modify-write verbs serialize per document.
    queue: SerialQueue,
}

/// `PageEditCapability` (and `PageEditHostCapability`, which equals it).
#[derive(Clone)]
pub struct PageEditCapability(Rc<Inner>);

impl PageEditCapability {
    /// Aborted when the document closes. The foreign handle, which can
    /// outlive the close, checks it before every call.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// `ctx.doc`: the guarded handle. Errs where the TypeScript getter throws.
    fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.0.cx.doc()
    }

    /// `engine(work)`: an `async` function that awaits the work and maps a
    /// failure, thrown or rejected, through `toPluginError`.
    fn engine<T: Clone + 'static>(
        work: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
        adopted: bool,
    ) -> Promise<T> {
        Promise::spawn(async move {
            let pending = work().map_err(|e| to_plugin_error(CAPABILITY, &e))?;
            let result = if adopted { await_adopted(pending).await } else { pending.await };
            result.map_err(|e| to_plugin_error(CAPABILITY, &e))
        })
    }

    /// `ctx.document()?.pages ?? []`.
    fn registry(&self) -> Vec<Value> {
        self.0.cx.document().and_then(|meta| meta.pages.as_array().cloned()).unwrap_or_default()
    }

    fn entry_of(&self, page: PageRef) -> Option<Value> {
        self.registry().into_iter().find(|p| {
            p.get("ref").and_then(|r| r.get("pageObjectNumber")).and_then(Value::as_f64)
                == Some(f64::from(page.page_object_number))
        })
    }

    fn require_entry(&self, page: PageRef) -> Result<Value, Error> {
        self.entry_of(page).ok_or_else(|| {
            plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("no page {}", page.page_object_number),
                None,
                None,
            )
        })
    }

    /// A placement to the engine's index wire, from the registry at call time.
    fn resolve_placement(
        &self,
        placement: Option<PagePlacement>,
    ) -> Result<(Option<f64>, Option<Anchor>), Error> {
        let (page, after) = match placement {
            None | Some(PagePlacement::End) => return Ok((None, None)),
            Some(PagePlacement::Index(index)) => return Ok((Some(index), None)),
            Some(PagePlacement::After(page)) => (page, true),
            Some(PagePlacement::Before(page)) => (page, false),
        };
        let entry = self.require_entry(page)?;
        let index = entry.get("index").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let anchor = Anchor { size: size_of(&entry) };
        Ok((Some(if after { index + 1.0 } else { index }), Some(anchor)))
    }

    /// Default blank-page size: the insertion point's predecessor, else the
    /// last page, else US Letter. A predecessor index that names no page is
    /// the TypeScript's `pages[i].size` on `undefined`.
    fn neighbour_size(&self, dest_index: Option<f64>) -> Result<Option<PdfSize>, Error> {
        let pages = self.registry();
        if pages.is_empty() {
            return Ok(Some(LETTER));
        }
        let Some(dest_index) = dest_index else {
            return Ok(pages.last().and_then(size_of));
        };
        let at = js_max(0.0, js_min(dest_index - 1.0, pages.len() as f64 - 1.0));
        if at.fract() != 0.0 || at.is_nan() {
            return Err(Error::named(
                "TypeError",
                "Cannot read properties of undefined (reading 'size')",
            ));
        }
        Ok(size_of(&pages[at as usize]))
    }

    /// `insertAt(doc, bytes, placement)`.
    fn insert_at(
        &self,
        doc: Rc<dyn DocumentHandle>,
        bytes: Value,
        placement: Option<PagePlacement>,
    ) -> Promise<Value> {
        let this = self.clone();
        Self::engine(
            move || {
                let (dest, _) = match this.resolve_placement(placement) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        // An extract's `{"$hostBytes": id}` is parked in the
                        // host until something reads it; nothing will now,
                        // so read it here to let it go. Rust-only: the
                        // TypeScript holds the bytes in memory, not a table.
                        if bytes.get("$hostBytes").is_some() {
                            let _ = Bytes::from_json(&bytes);
                        }
                        return Err(error);
                    }
                };
                let dest = dest.map_or_else(undefined, js_number);
                Ok(doc.call("pages.insert", None, vec![bytes, dest]))
            },
            false,
        )
    }

    fn enqueue<T: Clone + 'static>(
        &self,
        operation: impl FnOnce(PageEditCapability) -> Result<Promise<T>, Error> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        self.0.queue.run(move || operation(this))
    }

    fn enqueue_async<T: Clone + 'static, F: Future<Output = Result<T, Error>> + 'static>(
        &self,
        operation: impl FnOnce(PageEditCapability) -> F + 'static,
    ) -> Promise<T> {
        self.enqueue(move |this| Ok(Promise::spawn(operation(this))))
    }

    /// Whether this caller may make structural page edits: the session
    /// grants `doc.pages.assemble`. Wildcard-aware, as the engine's own
    /// enforcement is. Errs only where the TypeScript's `ctx.doc` throws.
    pub fn can_edit(&self) -> Result<bool, Error> {
        Ok(self.require_doc()?.security().allows(ASSEMBLE_CAPABILITY))
    }

    /// Rotate pages by a relative turn. Each page's own rotation is read
    /// from the registry; pages are grouped by the resulting absolute
    /// rotation (one engine call per group, in first-seen order), and the
    /// last group's answer is the result.
    pub fn rotate_by(&self, pages: Vec<PageRef>, delta: f64) -> Promise<Value> {
        self.enqueue_async(move |this| async move {
            let doc = this.require_doc()?;
            let mut groups: Vec<(f64, Vec<PageRef>)> = Vec::new();
            for page in pages {
                let rotation = this
                    .require_entry(page)?
                    .get("rotation")
                    .and_then(Value::as_f64)
                    .unwrap_or(f64::NAN);
                // Wrap to [0, 360): the double mod keeps -90 from 0 at 270.
                let next = (((rotation + delta) % 360.0) + 360.0) % 360.0;
                // A `Map` key compares by SameValueZero: NaN is one key.
                match groups.iter_mut().find(|(r, _)| *r == next || (r.is_nan() && next.is_nan())) {
                    Some(group) => group.1.push(page),
                    None => groups.push((next, vec![page])),
                }
            }
            let mut result = None;
            for (rotation, group) in groups {
                let doc = doc.clone();
                let call = move || {
                    Ok(doc.call("pages.rotate", None, vec![refs_json(&group), js_number(rotation)]))
                };
                result = Some(Self::engine(call, false).await?);
            }
            result.ok_or_else(|| {
                plugin_error(code::INVALID_INPUT, CAPABILITY, "no pages given", None, None)
            })
        })
    }

    /// Set one absolute rotation on pages.
    pub fn set_rotation(&self, pages: Vec<PageRef>, rotation: f64) -> Promise<Value> {
        self.enqueue(move |this| {
            Ok(Self::engine(
                move || {
                    Ok(this.require_doc()?.call(
                        "pages.rotate",
                        None,
                        vec![refs_json(&pages), js_number(rotation)],
                    ))
                },
                false,
            ))
        })
    }

    /// Reorder pages as a contiguous block at the placement (the end when
    /// there is none).
    pub fn move_pages(
        &self,
        pages: Vec<PageRef>,
        placement: Option<PagePlacement>,
    ) -> Promise<Value> {
        self.enqueue(move |this| {
            let doc = this.require_doc()?;
            let (dest, _) = this.resolve_placement(placement)?;
            Ok(Self::engine(
                move || {
                    let dest = dest.unwrap_or_else(|| this.registry().len() as f64);
                    Ok(doc.call("pages.move", None, vec![refs_json(&pages), js_number(dest)]))
                },
                false,
            ))
        })
    }

    /// Delete pages. The engine rejects deleting every page.
    pub fn delete(&self, pages: Vec<PageRef>) -> Promise<Value> {
        self.enqueue(move |this| {
            Ok(Self::engine(
                move || Ok(this.require_doc()?.call("pages.delete", None, vec![refs_json(&pages)])),
                false,
            ))
        })
    }

    /// Blank pages. A ref placement matches the anchor's size; everything
    /// else matches the neighbour the new page will follow.
    pub fn insert_blank(&self, options: InsertBlankOptions) -> Promise<Value> {
        self.enqueue(move |this| {
            let doc = this.require_doc()?;
            let (dest, anchor) = this.resolve_placement(options.placement)?;
            let size = match options.size.or_else(|| anchor.and_then(|a| a.size)) {
                Some(size) => Some(size),
                None => this.neighbour_size(dest)?,
            };
            Ok(Self::engine(
                move || {
                    let mut spec = serde_json::Map::new();
                    if let Some(size) = size {
                        spec.insert("size".into(), json!(size));
                    }
                    if let Some(count) = options.count {
                        spec.insert("count".into(), js_number(count));
                    }
                    let dest = dest.map_or_else(undefined, js_number);
                    Ok(doc.call("pages.insertBlank", None, vec![Value::Object(spec), dest]))
                },
                false,
            ))
        })
    }

    /// Pages of another PDF. A subset by index opens the bytes beside the
    /// document, extracts the chosen pages, inserts them and closes the
    /// source again, whatever happened.
    pub fn insert_from_bytes(
        &self,
        bytes: Bytes,
        options: InsertFromBytesOptions,
    ) -> Promise<Value> {
        self.enqueue_async(move |this| async move {
            let doc = this.require_doc()?;
            let Some(indexes) = options.page_indexes else {
                return returned(this.insert_at(doc, bytes.to_json(), options.placement)).await;
            };
            let engine = this.0.cx.engine().ok_or_else(|| Error::new("[page-edit] no engine"))?;
            let input = json!({
                "kind": "bytes",
                "id": format!("page-edit-source-{}", base36(rt::now())),
                "bytes": bytes.to_json(),
            });
            let source =
                Self::engine(move || Ok(engine.open(input, Some(json!({ "scope": ["*"] })))), true)
                    .await?;
            let inserted = async {
                let list = source.clone();
                let layout =
                    Self::engine(move || Ok(list.call("pages.list", None, Vec::new())), true)
                        .await?;
                let listed =
                    layout.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
                let mut refs = Vec::new();
                for index in indexes {
                    let page = (index >= 0.0 && index.fract() == 0.0)
                        .then(|| listed.get(index as usize))
                        .flatten()
                        .ok_or_else(|| {
                            plugin_error(
                                code::NOT_FOUND,
                                CAPABILITY,
                                format!("the PDF has no page {}", number_to_string(index)),
                                None,
                                None,
                            )
                        })?;
                    refs.push(page.get("ref").cloned().unwrap_or(Value::Null));
                }
                let extract = source.clone();
                let subset = Self::engine(
                    move || Ok(extract.call("pages.extract", None, vec![Value::Array(refs)])),
                    true,
                )
                .await?;
                this.insert_at(doc, subset, options.placement).await
            }
            .await;
            // `finally { await source.close() }`: a close failure wins.
            await_adopted(source.call("close", None, Vec::new())).await?;
            inserted
        })
    }

    /// Pages of another open document.
    pub fn insert_from_document(
        &self,
        document_id: String,
        pages: Vec<PageRef>,
        placement: Option<PagePlacement>,
    ) -> Promise<Value> {
        self.enqueue_async(move |this| async move {
            let doc = this.require_doc()?;
            let other = this.0.cx.document_handle(Some(&document_id)).ok_or_else(|| {
                plugin_error(
                    code::NOT_FOUND,
                    CAPABILITY,
                    format!("document '{document_id}' is not open"),
                    None,
                    None,
                )
            })?;
            let bytes = Self::engine(
                move || Ok(other.call("pages.extract", None, vec![refs_json(&pages)])),
                true,
            )
            .await?;
            returned(this.insert_at(doc, bytes, placement)).await
        })
    }

    /// Copy pages within the document; by default right after the last of them.
    pub fn duplicate(
        &self,
        pages: Vec<PageRef>,
        placement: Option<PagePlacement>,
    ) -> Promise<Value> {
        self.enqueue_async(move |this| async move {
            let doc = this.require_doc()?;
            let Some(last) = pages.last().copied() else {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    CAPABILITY,
                    "no pages given",
                    None,
                    None,
                ));
            };
            let source = doc.clone();
            let bytes = Self::engine(
                move || Ok(source.call("pages.extract", None, vec![refs_json(&pages)])),
                false,
            )
            .await?;
            let placement = placement.or(Some(PagePlacement::After(last)));
            returned(this.insert_at(doc, bytes, placement)).await
        })
    }

    /// A new PDF from a page subset. Not serialized with the edits.
    pub fn extract(&self, pages: Vec<PageRef>) -> Promise<Bytes> {
        let this = self.clone();
        let extracted = Self::engine(
            move || Ok(this.require_doc()?.call("pages.extract", None, vec![refs_json(&pages)])),
            false,
        );
        // Read as bytes the moment it settles, so the conversion costs no turn.
        let (promise, resolver) = Promise::pending();
        extracted.on_settle(move |result| {
            resolver.settle(result.clone().and_then(|value| {
                Bytes::from_json(&value)
                    .ok_or_else(|| Error::new("[page-edit] extract answered no bytes"))
            }))
        });
        promise
    }
}

/// `createPageEditController(ctx)`: the api; the plugin has no `connect`.
pub fn create_page_edit_controller(cx: Controller<Value, Value>) -> PageEditCapability {
    PageEditCapability(Rc::new(Inner { cx, queue: SerialQueue::new() }))
}
