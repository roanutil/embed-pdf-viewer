//! `services/store.ts`: the page registry, the calibrate twin and the
//! page-target expansion, with the kernel's `ctx.getPage`/`ctx.pageOf`
//! lookups they rest on.
//!
//! The lookups go through an index built once per page registry: a linear
//! scan per call made the per-page work at open O(pages²) (the M22 pattern
//! in `docs/research/rust-plugins-branch-review.md`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::{ControllerContext, Error};
use serde_json::Value;

use crate::contract::{PageInput, PageTarget, SCALE_PERMISSION};
use crate::model::PageSize;

pub(crate) const CAPABILITY: &str = "measurement";

/// One row of the document's page registry (`PageInfo`), shared with the
/// kernel rather than copied.
#[derive(Clone, Debug)]
pub struct PageRow {
    pages: Rc<Value>,
    index: usize,
}

impl PageRow {
    pub fn row(&self) -> &Value {
        &self.pages[self.index]
    }

    /// `layout.ref`.
    pub fn page(&self) -> PageRef {
        self.row().get("ref").and_then(PageRef::from_json).expect("an indexed row has a page ref")
    }

    /// `layout.size`.
    pub fn size(&self) -> PageSize {
        PageSize::of(self.row().get("size").unwrap_or(&Value::Null))
    }

    /// `layout.userUnit`: `None` when absent, `NaN` when not a number.
    pub fn user_unit(&self) -> Option<f64> {
        match self.row().get("userUnit") {
            None => None,
            Some(value) if epdf_kernel::is_undefined(value) => None,
            Some(value) => Some(value.as_f64().unwrap_or(f64::NAN)),
        }
    }

    /// `layout?.userUnit ?? 1`.
    pub fn user_unit_or_one(&self) -> f64 {
        match self.row().get("userUnit") {
            None | Some(Value::Null) => 1.0,
            Some(value) if epdf_kernel::is_undefined(value) => 1.0,
            Some(value) => value.as_f64().unwrap_or(f64::NAN),
        }
    }
}

type Index = Rc<HashMap<PageRef, usize>>;

/// The page registry of the bound document, indexed by page ref.
pub struct Registry {
    cx: ControllerContext,
    index: RefCell<Option<(Rc<Value>, Index)>>,
}

impl Registry {
    pub fn new(cx: ControllerContext) -> Registry {
        Registry { cx, index: RefCell::new(None) }
    }

    /// `ctx.document()?.pages`: the registry the store holds, else the one
    /// staged during bring-up.
    pub fn pages(&self) -> Option<Rc<Value>> {
        let core = self.cx.core();
        if let Some(meta) = self.cx.document_id().and_then(|id| core.documents.get(id)) {
            return Some(meta.pages.clone());
        }
        self.cx.document().map(|meta| meta.pages)
    }

    /// The pages in order, as refs (`ctx.document()?.pages ?? []`).
    pub fn refs(&self) -> Vec<PageRef> {
        let Some(pages) = self.pages() else { return Vec::new() };
        pages
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row.get("ref").and_then(PageRef::from_json))
            .collect()
    }

    fn index_of(&self, pages: &Rc<Value>) -> Index {
        if let Some((built, index)) = &*self.index.borrow()
            && Rc::ptr_eq(built, pages)
        {
            return index.clone();
        }
        let mut index = HashMap::new();
        for (i, row) in pages.as_array().into_iter().flatten().enumerate() {
            if let Some(page) = row.get("ref").and_then(PageRef::from_json) {
                index.entry(page).or_insert(i);
            }
        }
        let index = Rc::new(index);
        *self.index.borrow_mut() = Some((pages.clone(), index.clone()));
        index
    }

    /// `ctx.getPage(page)`: the page's row, or `None` for a page that isn't
    /// there. An index reads `pages[index]`.
    pub fn find(&self, page: PageInput) -> Option<PageRow> {
        let pages = self.pages()?;
        let len = pages.as_array()?.len();
        let index = match page {
            PageInput::Ref(page) => *self.index_of(&pages).get(&page)?,
            PageInput::Index(i) if i >= 0.0 && i.fract() == 0.0 && i < len as f64 => i as usize,
            PageInput::Index(_) => return None,
        };
        Some(PageRow { pages, index })
    }

    /// `ctx.pageOf(page)`: `not-found` for a page that isn't there.
    pub fn page_of(&self, page: PageInput) -> Result<PageRow, Error> {
        self.find(page).ok_or_else(|| {
            let message = match page {
                PageInput::Ref(page) => {
                    format!("page {} is not in this document", page.page_object_number)
                }
                PageInput::Index(index) => format!(
                    "there is no page {} in this document",
                    epdf_kernel::js::number_to_string(index)
                ),
            };
            plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
        })
    }

    /// `ctx.assertPageRef(ref)`.
    pub fn assert_page_ref(&self, page: PageRef) -> Result<(), Error> {
        self.page_of(PageInput::Ref(page)).map(|_| ())
    }

    /// `canCalibrate()`: may write a page scale (`doc.annotate.modify`).
    pub fn can_calibrate(&self) -> bool {
        self.cx.allows(SCALE_PERMISSION)
    }

    /// `targets(pages)`: the pages a verb changes, each resolved; a page that
    /// isn't there refuses the whole call.
    pub fn targets(&self, pages: &PageTarget) -> Result<Vec<PageRef>, Error> {
        match pages {
            PageTarget::All => Ok(self.refs()),
            PageTarget::Page(page) => Ok(vec![self.page_of(*page)?.page()]),
            PageTarget::Pages(list) => {
                list.iter().map(|page| self.page_of(*page).map(|row| row.page())).collect()
            }
        }
    }
}
