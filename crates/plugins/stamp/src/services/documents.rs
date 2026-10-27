//! `services/documents.ts`: the document a placing verb acts on (the one
//! its options name, else the active one) and the page arguments resolved
//! in it.

use epdf_kernel::error::code;
use epdf_kernel::js::number_to_string;
use epdf_kernel::{DOCUMENTS, Documents, Error};
use serde_json::Value;

use crate::contract::PageTarget;
use crate::controller::StampCapability;
use crate::services::errors::stamp_error;

impl StampCapability {
    /// `ctx.get(DocumentsToken)`.
    pub(crate) fn documents(&self) -> Result<Documents, Error> {
        self.0.cx.get(&DOCUMENTS)
    }

    /// `documentIdOf(documentId)`: the document's id, or `None` with no
    /// document open.
    pub(crate) fn document_id_of(&self, document_id: Option<&str>) -> Option<String> {
        match document_id {
            Some(id) => Some(id.to_string()),
            None => self.documents().ok()?.get_active_id(),
        }
    }

    /// `targetOf(options)`: the document a verb acts on. `not-ready` with no
    /// document open.
    pub(crate) fn target_of(&self, document_id: Option<&str>) -> Result<String, Error> {
        self.document_id_of(document_id)
            .ok_or_else(|| stamp_error(code::NOT_READY, "no document is open"))
    }

    /// `pageOf(documentId, page)`: a page argument of the document, as a ref
    /// or an index. `not-found` for a page that isn't there.
    pub(crate) fn page_of(&self, document_id: &str, page: PageTarget) -> Result<Value, Error> {
        let documents = self.documents()?;
        let found = match page {
            PageTarget::Ref(page) => documents.get_page(page, Some(document_id)),
            PageTarget::Index(index) if index >= 0.0 && index.fract() == 0.0 => {
                documents.get_page_at(index as i64, Some(document_id))
            }
            PageTarget::Index(_) => None,
        };
        found.ok_or_else(|| {
            let named = match page {
                PageTarget::Index(index) => number_to_string(index),
                PageTarget::Ref(page) => page.page_object_number.to_string(),
            };
            stamp_error(code::NOT_FOUND, format!("no page {named} in document '{document_id}'"))
        })
    }
}
