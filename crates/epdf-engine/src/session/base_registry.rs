//! Shared base documents, counted by key.
//!
//! G3. Replaces
//! `services/src/document-session/lifecycle/BaseDocumentRegistry.ts`.
//!
//! A base document is a PDF parsed once and frozen; layers are opened on top
//! of it. The point of the registry is that several sessions viewing the same
//! file share one parse and one copy of the bytes, so the second viewer of a
//! 200 MB document costs a layer rather than another 200 MB.
//!
//! Keyed by a caller-supplied string. The key is the caller's assertion that
//! two requests are the same document, and the registry does not check it: a
//! key reused across genuinely different bytes hands out the wrong document,
//! which is why callers derive it from content rather than from a file name.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pdfium::{BaseDocument, FileAccess, Library, PdfiumError};

use crate::error::{EngineError, EngineErrorCode};

/// Bases held open, by key.
///
/// The registry holds a strong reference, so a base stays parsed while it is
/// registered even with no live acquisition. That is the caching this type
/// exists for; [`BaseDocumentRegistry::release`] is what gives it back.
#[derive(Debug, Default)]
pub struct BaseDocumentRegistry {
    entries: RefCell<BTreeMap<String, Entry>>,
}

#[derive(Debug)]
struct Entry {
    base: Rc<BaseDocument>,
    /// Acquisitions outstanding. The registry's own strong reference is not
    /// counted here: this is how many callers asked for it and have not given
    /// it back.
    refs: usize,
}

impl BaseDocumentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The base for `key`, parsing `bytes` only if it is not already held.
    ///
    /// `bytes` is not consulted when the key is already registered. A caller
    /// that reuses a key for different content gets the first one, silently,
    /// which is the trade the shared parse is built on.
    pub fn acquire_memory(
        &self,
        key: &str,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<Rc<BaseDocument>, EngineError> {
        if let Some(entry) = self.entries.borrow_mut().get_mut(key) {
            entry.refs += 1;
            return Ok(Rc::clone(&entry.base));
        }

        // Loaded outside the borrow: a panic inside PDFium with the map
        // borrowed would poison every later acquire.
        let base = Rc::new(
            BaseDocument::from_bytes(Library::acquire(), bytes, password)
                .map_err(|e| open_error(e, password_given(password)))?,
        );

        self.entries
            .borrow_mut()
            .insert(key.to_string(), Entry { base: Rc::clone(&base), refs: 1 });
        Ok(base)
    }

    /// The base for `key`, read from a file on demand rather than copied.
    ///
    /// `open.layerFileBase`. The difference from
    /// [`BaseDocumentRegistry::acquire_memory`] is where the bytes live: PDFium
    /// calls back into the file for the ranges it needs, so a 200 MB document
    /// costs the pages it touches rather than 200 MB.
    pub fn acquire_file(
        &self,
        key: &str,
        path: &std::path::Path,
        password: Option<&str>,
    ) -> Result<Rc<BaseDocument>, EngineError> {
        if let Some(entry) = self.entries.borrow_mut().get_mut(key) {
            entry.refs += 1;
            return Ok(Rc::clone(&entry.base));
        }

        let access = FileAccess::from_file(path).map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::DocOpenFailed,
                format!("failed to read {}: {e}", path.display()),
            )
        })?;
        let base = Rc::new(
            BaseDocument::from_file_access(Library::acquire(), access, password)
                .map_err(|e| open_error(e, password_given(password)))?,
        );
        self.entries
            .borrow_mut()
            .insert(key.to_string(), Entry { base: Rc::clone(&base), refs: 1 });
        Ok(base)
    }

    /// Give back one acquisition.
    ///
    /// The base is dropped from the registry when the last one is returned.
    /// Any layer still open keeps it alive through its own `Rc`, so this
    /// cannot release a base out from under a live layer.
    pub fn release(&self, key: &str) {
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.get_mut(key) else {
            return;
        };
        entry.refs = entry.refs.saturating_sub(1);
        if entry.refs == 0 {
            entries.remove(key);
        }
    }

    /// Outstanding acquisitions for a key, or zero if it is not registered.
    pub fn ref_count(&self, key: &str) -> usize {
        self.entries.borrow().get(key).map_or(0, |entry| entry.refs)
    }

    pub fn is_registered(&self, key: &str) -> bool {
        self.entries.borrow().contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Forget every base.
    ///
    /// Bases with a live layer stay alive through that layer's own reference,
    /// so this drops the cache rather than freeing memory somebody is using.
    pub fn release_all(&self) {
        self.entries.borrow_mut().clear();
    }
}

/// Whether a password was given, as `loadFailure` asks it (`password ?`):
/// an empty one is none.
pub(crate) fn password_given(password: Option<&str>) -> bool {
    password.is_some_and(|p| !p.is_empty())
}

/// PDFium's open failure in the engine's vocabulary.
///
/// Three answers, not one. A missing password means prompt, a wrong one means
/// the user got it wrong, and anything else means the bytes are not a document
/// this can open. Collapsing them makes a prompt impossible to write.
fn open_error(error: PdfiumError, password_supplied: bool) -> EngineError {
    match error {
        PdfiumError::Password if password_supplied => EngineError::with_message(
            EngineErrorCode::DocPasswordIncorrect,
            "incorrect document password",
        ),
        PdfiumError::Password => EngineError::with_message(
            EngineErrorCode::DocPasswordRequired,
            "document requires a password",
        ),
        // `baseOpenError`'s words, whatever PDFium said.
        _ => EngineError::with_message(
            EngineErrorCode::DocOpenFailed,
            "failed to open base document",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Vec<u8> {
        test_support::fixture(test_support::REPORT)
    }

    #[test]
    fn the_same_key_shares_one_parse() {
        let registry = BaseDocumentRegistry::new();
        let first = registry.acquire_memory("a", &report(), None).expect("first");
        let second = registry.acquire_memory("a", &report(), None).expect("second");

        assert!(Rc::ptr_eq(&first, &second), "the second acquire reparsed the document");
        assert_eq!(registry.ref_count("a"), 2);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn different_keys_are_different_documents() {
        let registry = BaseDocumentRegistry::new();
        let a = registry.acquire_memory("a", &report(), None).expect("a");
        let b = registry.acquire_memory("b", &report(), None).expect("b");
        assert!(!Rc::ptr_eq(&a, &b));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn a_base_is_forgotten_when_the_last_acquisition_is_returned() {
        let registry = BaseDocumentRegistry::new();
        registry.acquire_memory("a", &report(), None).expect("first");
        registry.acquire_memory("a", &report(), None).expect("second");

        registry.release("a");
        assert!(registry.is_registered("a"), "one acquisition left");
        registry.release("a");
        assert!(!registry.is_registered("a"));
        assert_eq!(registry.ref_count("a"), 0);
    }

    #[test]
    fn releasing_more_than_was_acquired_does_not_underflow() {
        let registry = BaseDocumentRegistry::new();
        registry.acquire_memory("a", &report(), None).expect("acquire");
        registry.release("a");
        registry.release("a");
        registry.release("unknown");
        assert_eq!(registry.ref_count("a"), 0);
    }

    #[test]
    fn a_released_base_stays_alive_for_a_layer_that_is_still_open() {
        // The invariant the whole type exists to protect: a layer points into
        // its base, so releasing the base out from under one would leave it
        // reading freed memory.
        let registry = BaseDocumentRegistry::new();
        let base = registry.acquire_memory("a", &report(), None).expect("acquire");
        let layer = base.open_layer().expect("open layer");
        drop(base);
        registry.release_all();

        assert!(!registry.is_registered("a"));
        assert_eq!(layer.page_count(), 14, "the layer can still read its base");
    }

    #[test]
    fn a_fresh_layer_starts_with_an_empty_overlay() {
        let registry = BaseDocumentRegistry::new();
        let base = registry.acquire_memory("a", &report(), None).expect("acquire");
        let layer = base.open_layer().expect("open layer");

        assert_eq!(BaseDocument::promoted_object_count(layer.document()), 0);
        // And it reads the base's pages, which is what makes it a view rather
        // than a new document.
        assert_eq!(layer.page_count(), 14);
    }

    #[test]
    fn two_layers_on_one_base_are_independent_documents() {
        let registry = BaseDocumentRegistry::new();
        let base = registry.acquire_memory("a", &report(), None).expect("acquire");
        let first = base.open_layer().expect("first");
        let second = base.open_layer().expect("second");

        assert_eq!(first.page_count(), second.page_count());
        assert!(Rc::ptr_eq(first.base(), second.base()), "one base, two layers");
    }

    #[test]
    fn a_file_backed_base_reads_the_same_document() {
        // open.layerFileBase. PDFium calls back into the file for the ranges
        // it needs, so the bytes are never copied into the process.
        let registry = BaseDocumentRegistry::new();
        let path = test_support::fixture_path(test_support::REPORT);
        let base = registry.acquire_file("report", &path, None).expect("acquire");

        assert_eq!(base.open_layer().expect("layer").page_count(), 14);
        assert_eq!(registry.ref_count("report"), 1);
    }

    #[test]
    fn a_file_key_shares_like_a_memory_one() {
        let registry = BaseDocumentRegistry::new();
        let path = test_support::fixture_path(test_support::REPORT);
        let first = registry.acquire_file("report", &path, None).expect("first");
        let second = registry.acquire_file("report", &path, None).expect("second");

        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(registry.ref_count("report"), 2);
    }

    #[test]
    fn a_missing_file_reports_which_path_it_could_not_read() {
        let registry = BaseDocumentRegistry::new();
        let missing = test_support::workspace_root().join("no-such-document.pdf");
        let error = registry.acquire_file("missing", &missing, None).expect_err("no file");
        assert_eq!(error.code, EngineErrorCode::DocOpenFailed);
        assert!(error.message.contains("no-such-document.pdf"), "{}", error.message);
        assert!(!registry.is_registered("missing"), "a failed acquire registered nothing");
    }

    #[test]
    fn a_password_protected_base_reports_which_kind_of_failure() {
        let path = test_support::workspace_root()
            .join("tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf");
        let bytes = std::fs::read(path).expect("read");
        let registry = BaseDocumentRegistry::new();

        assert_eq!(
            registry.acquire_memory("a", &bytes, None).expect_err("no password").code,
            EngineErrorCode::DocPasswordRequired,
        );
        assert_eq!(
            registry.acquire_memory("a", &bytes, Some("wrong")).expect_err("wrong").code,
            EngineErrorCode::DocPasswordIncorrect,
        );
        // An empty password is none (`loadFailure`'s `password ?`).
        assert_eq!(
            registry.acquire_memory("a", &bytes, Some("")).expect_err("empty").code,
            EngineErrorCode::DocPasswordRequired,
        );
        registry.acquire_memory("a", &bytes, Some("user")).expect("the right password");
        // A failed acquire leaves nothing registered.
        assert_eq!(registry.ref_count("a"), 1);
    }
}
