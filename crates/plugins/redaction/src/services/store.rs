//! `services/store.ts`: the page registry reads and the engine's redaction
//! service, with the kernel's `ctx.getPage`/`ctx.pageOf` lookups for a page
//! given as a ref or an index (the Rust kernel's take a ref only).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::number_to_string;
use epdf_kernel::{ControllerContext, Error};
use serde_json::Value;

use crate::contract::PageTarget;

pub(crate) const CAPABILITY: &str = "redaction";

/// Applying destroys content: its own granted power, narrower than
/// annotate. The engine's apply asserts all three (the local redaction
/// service and the cloud route guards alike): the redact grant itself, the
/// page-content rewrite, and the annotation consumption. `can_apply`
/// mirrors the full set.
pub const APPLY_CAPABILITIES: [&str; 3] = ["doc.redact", "doc.pages.modify", "doc.annotate.modify"];

/// Page object number to display index, for one page registry.
type IndexByPage = Rc<HashMap<u32, i64>>;

pub struct RedactionStore {
    cx: ControllerContext,
    /// `indexByPage`: rebuilt only when the page registry changes.
    index: RefCell<Option<(Rc<Value>, IndexByPage)>>,
}

impl RedactionStore {
    pub fn new(cx: ControllerContext) -> RedactionStore {
        RedactionStore { cx, index: RefCell::new(None) }
    }

    /// `ctx.document()?.pages`.
    fn layouts(&self) -> Option<Rc<Value>> {
        self.cx.document().map(|meta| meta.pages)
    }

    /// `requireService()`: the handle, when the engine has a redaction
    /// service; `unsupported` otherwise.
    pub fn require_service(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let doc = self.cx.doc()?;
        if !doc.has("redaction") {
            return Err(plugin_error(
                code::UNSUPPORTED,
                CAPABILITY,
                "this engine has no redaction service",
                None,
                None,
            ));
        }
        Ok(doc)
    }

    /// `pages()`: every page's ref, in display order.
    pub fn pages(&self) -> Vec<PageRef> {
        let Some(layouts) = self.layouts() else { return Vec::new() };
        layouts
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|layout| layout.get("ref").and_then(PageRef::from_json))
            .collect()
    }

    fn index_by_page(&self) -> IndexByPage {
        let layouts = self.layouts();
        if let (Some(layouts), Some((built, index))) = (&layouts, &*self.index.borrow())
            && Rc::ptr_eq(layouts, built)
        {
            return index.clone();
        }
        let Some(layouts) = layouts else { return Rc::default() };
        let mut index = HashMap::new();
        for layout in layouts.as_array().into_iter().flatten() {
            let page = layout.get("ref").and_then(PageRef::from_json);
            let at = layout.get("index").and_then(Value::as_f64);
            if let (Some(page), Some(at)) = (page, at) {
                // `new Map(entries)`: a later entry for a page wins.
                index.insert(page.page_object_number, at as i64);
            }
        }
        let index = Rc::new(index);
        *self.index.borrow_mut() = Some((layouts, index.clone()));
        index
    }

    /// `pageIndexOf(page)`: the display index, `-1` for a page the registry
    /// lacks.
    pub fn page_index_of(&self, page: PageRef) -> i64 {
        self.index_by_page().get(&page.page_object_number).copied().unwrap_or(-1)
    }

    /// `ctx.getPage(page)`: the page's layout, `None` for a page that isn't
    /// there. An index reads `pages[index]`.
    pub fn get_page(&self, page: PageTarget) -> Option<Value> {
        let layouts = self.layouts()?;
        let rows = layouts.as_array()?;
        match page {
            PageTarget::Ref(page) => rows.iter().find(|row| page.names(row)).cloned(),
            PageTarget::Index(index) if index >= 0.0 && index.fract() == 0.0 => {
                rows.get(index as usize).cloned()
            }
            PageTarget::Index(_) => None,
        }
    }

    /// `ctx.pageOf(page)`: the page's layout, `not-found` for a page that
    /// isn't there.
    pub fn page_of(&self, page: PageTarget) -> Result<Value, Error> {
        self.get_page(page).ok_or_else(|| {
            let message = match page {
                PageTarget::Ref(page) => {
                    format!("page {} is not in this document", page.page_object_number)
                }
                PageTarget::Index(index) => {
                    format!("there is no page {} in this document", number_to_string(index))
                }
            };
            plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
        })
    }

    /// `ctx.pageOf(page).ref`.
    pub fn page_ref_of(&self, page: PageTarget) -> Result<PageRef, Error> {
        self.ref_of_layout(&self.page_of(page)?)
    }

    /// `layout.ref`. A registry row always carries one.
    pub fn ref_of_layout(&self, layout: &Value) -> Result<PageRef, Error> {
        layout.get("ref").and_then(PageRef::from_json).ok_or_else(|| {
            plugin_error(code::NOT_FOUND, CAPABILITY, "the page has no ref", None, None)
        })
    }

    /// `canApply()`: the engine has a redaction service and every
    /// capability its apply asserts is granted.
    pub fn can_apply(&self) -> bool {
        let Ok(doc) = self.cx.doc() else { return false };
        if !doc.has("redaction") {
            return false;
        }
        let security = doc.security();
        APPLY_CAPABILITIES.iter().all(|capability| security.allows(capability))
    }
}
