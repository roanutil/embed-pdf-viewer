use std::collections::HashMap;
use std::rc::Rc;

use crate::OpsError;

/// Everything that must live on the PDFium thread. Held by whichever dispatch
/// implementation is compiled in, and only ever touched from that thread.
pub struct State {
    /// An `Rc` so every `pdfium::Document` in `docs` holds its own share. That
    /// is what keeps `FPDF_DestroyLibrary` from running while a document handle
    /// is still open, and it makes this struct's field order irrelevant.
    lib: Rc<pdfium::Library>,
    docs: HashMap<u64, pdfium::Document>,
    next_id: u64,
}

impl State {
    pub fn new() -> Result<Self, OpsError> {
        Ok(Self {
            lib: pdfium::Library::acquire()?,
            docs: HashMap::new(),
            next_id: 1,
        })
    }

    pub fn insert(&mut self, doc: pdfium::Document) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(id, doc);
        id
    }

    /// Clones the handle rather than lending it: `Document::from_bytes` takes
    /// `Rc<Library>` so the library outlives every document opened from it.
    pub fn library(&self) -> Rc<pdfium::Library> {
        Rc::clone(&self.lib)
    }

    pub fn get(&self, id: u64) -> Result<&pdfium::Document, OpsError> {
        self.docs.get(&id).ok_or(OpsError::Closed)
    }

    pub fn remove(&mut self, id: u64) {
        self.docs.remove(&id);
    }
}
