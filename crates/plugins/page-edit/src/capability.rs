//! `capability.ts`: forwards structural edits to the document's engine
//! handle, addressed by PON. The handle and the page registry are both on
//! the plugin context; this is the thin layer that turns the relative
//! rotate gesture into the engine's absolute wire and resolves placements.

use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::{DocumentHandle, EngineCall};
use epdf_kernel::{Ctx, Error, undefined};
use serde_json::{Value, json};

use crate::types::{AddBlankOptions, InsertOptions, PageObjectNumber, PagePlacement, PdfSize};

/// Authorization for structural page edits: PDF bit 11 (assemble). The
/// engine enforces the same capability; `can_edit` is the UI's mirror.
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

fn page_object_number(page: &Value) -> Option<f64> {
    page.get("pageObjectNumber").and_then(Value::as_f64)
}

fn size_of(page: &Value) -> Option<PdfSize> {
    serde_json::from_value(page.get("size")?.clone()).ok()
}

/// A registry page, resolved from a placement.
struct Anchor {
    index: f64,
    size: Option<PdfSize>,
}

#[derive(Clone)]
pub struct PageEditCapability {
    ctx: Ctx<Value, Value>,
}

impl PageEditCapability {
    pub(crate) fn new(ctx: Ctx<Value, Value>) -> Self {
        PageEditCapability { ctx }
    }

    fn require_doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        self.ctx.doc().ok_or_else(|| Error::new("[page-edit] no document bound"))
    }

    /// The registry's pages, `ctx.document()?.pages ?? []`.
    fn pages(&self) -> Vec<Value> {
        self.ctx.document().and_then(|meta| meta.pages.as_array().cloned()).unwrap_or_default()
    }

    fn find(&self, pon: PageObjectNumber) -> Option<Value> {
        self.pages().into_iter().find(|p| page_object_number(p) == Some(f64::from(pon)))
    }

    /// Current absolute rotation of a page from the registry; 0 if unknown.
    fn rotation_of(&self, pon: PageObjectNumber) -> f64 {
        self.find(pon).and_then(|p| p.get("rotation").and_then(Value::as_f64)).unwrap_or(0.0)
    }

    /// Resolve a placement to the engine's index wire, from the registry at
    /// call time, with the anchor page for PON placements.
    fn resolve_placement(
        &self,
        placement: Option<PagePlacement>,
    ) -> Result<(Option<f64>, Option<Anchor>), Error> {
        let (pon, after) = match placement {
            None => return Ok((None, None)),
            Some(PagePlacement::Index(index)) => return Ok((Some(index), None)),
            Some(PagePlacement::After(pon)) => (pon, true),
            Some(PagePlacement::Before(pon)) => (pon, false),
        };
        let page = self
            .find(pon)
            .ok_or_else(|| Error::new(format!("[page-edit] placement page not found: {pon}")))?;
        let index = page.get("index").and_then(Value::as_f64).unwrap_or(f64::NAN);
        let anchor = Anchor { index, size: size_of(&page) };
        Ok((Some(if after { anchor.index + 1.0 } else { anchor.index }), Some(anchor)))
    }

    /// Default blank-page size: the insertion point's predecessor, else the
    /// last page, else (an empty registry) US Letter. A predecessor index
    /// that names no page is the TypeScript's `pages[i].size` on
    /// `undefined`.
    fn neighbour_size(&self, dest_index: Option<f64>) -> Result<Option<PdfSize>, Error> {
        let pages = self.pages();
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

    /// Whether this session may make structural page edits. Wildcard-aware,
    /// as the engine's own enforcement is.
    pub fn can_edit(&self) -> bool {
        self.ctx.doc().is_some_and(|doc| doc.security().allows(ASSEMBLE_CAPABILITY))
    }

    /// Rotate one page by a relative turn, wrapped into [0, 360).
    pub fn rotate_by(&self, pon: PageObjectNumber, delta: f64) -> Result<EngineCall<Value>, Error> {
        let next = (((self.rotation_of(pon) + delta) % 360.0) + 360.0) % 360.0;
        Ok(self.require_doc()?.call("pages.rotate", None, vec![json!([pon]), js_number(next)]))
    }

    /// Set the absolute rotation of pages to one shared value.
    pub fn set_rotation(
        &self,
        pons: &[PageObjectNumber],
        rotation: f64,
    ) -> Result<EngineCall<Value>, Error> {
        Ok(self.require_doc()?.call("pages.rotate", None, vec![json!(pons), js_number(rotation)]))
    }

    /// Reorder pages as a contiguous block starting at `dest_index`.
    pub fn move_pages(
        &self,
        pons: &[PageObjectNumber],
        dest_index: f64,
    ) -> Result<EngineCall<Value>, Error> {
        Ok(self.require_doc()?.call("pages.move", None, vec![json!(pons), js_number(dest_index)]))
    }

    /// Delete pages by PON.
    pub fn delete(&self, pons: &[PageObjectNumber]) -> Result<EngineCall<Value>, Error> {
        Ok(self.require_doc()?.call("pages.delete", None, vec![json!(pons)]))
    }

    /// Create blank pages. PON placements match the anchor's size; the
    /// rest match the neighbour the new page will follow.
    pub fn add_blank(&self, opts: AddBlankOptions) -> Result<EngineCall<Value>, Error> {
        let doc = self.require_doc()?;
        let (dest_index, anchor) = self.resolve_placement(opts.placement)?;
        let size = match opts.size.or_else(|| anchor.and_then(|a| a.size)) {
            Some(size) => Some(size),
            None => self.neighbour_size(dest_index)?,
        };
        let mut spec = serde_json::Map::new();
        if let Some(size) = size {
            spec.insert("size".into(), json!(size));
        }
        if let Some(count) = opts.count {
            spec.insert("count".into(), js_number(count));
        }
        let dest = dest_index.map_or_else(undefined, js_number);
        Ok(doc.call("pages.insertBlank", None, vec![Value::Object(spec), dest]))
    }

    /// Copy every page of a standalone PDF in at the placement.
    pub fn insert(&self, bytes: &Bytes, opts: InsertOptions) -> Result<EngineCall<Value>, Error> {
        let doc = self.require_doc()?;
        let (dest_index, _) = self.resolve_placement(opts.placement)?;
        let dest = dest_index.map_or_else(undefined, js_number);
        Ok(doc.call("pages.insert", None, vec![bytes.to_json(), dest]))
    }
}
