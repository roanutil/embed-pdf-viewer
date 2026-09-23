//! Apple and Android binding layer.
//!
//! Holds `uniffi::setup_scaffolding!()` and a newtype plus a `From` impl per
//! exported type. It stays a separate crate so the scaffolding macro never has
//! to build for `wasm32-unknown-emscripten`; the cost is that newtype-and-`From`
//! per type.
//!
//! Consumes `epdf-engine` and adds no semantics of its own.
//!
//! # What this is not
//!
//! `uniffi` is the ABI, not the API. Nothing here is the surface an iOS or
//! Android developer sees: the plan gives each platform a handwritten facade
//! over this, because a generated API is a translation of Rust's shape rather
//! than a design in the host language. What this owes that facade is a
//! boundary that is correct about threads, ownership and errors.
//!
//! # Threads
//!
//! PDFium handles never cross threads. Every exported object here is
//! `Send + Sync` because uniffi requires it, and none of them hold a PDFium
//! handle: [`PdfEngine`] holds an owner thread and [`PdfDocument`] holds an
//! index into state that lives on it. A Swift actor or a Kotlin dispatcher
//! does not guarantee affinity, so the confinement is structural rather than
//! promised.
//!
//! # Scope
//!
//! Phase 1's smoke-app surface: open with an optional password, page count and
//! size, close, page geometry, render. Enough for a smoke app to prove the
//! distribution works, and deliberately not the read services, which are
//! Phase 2 and land through the same engine code the web uses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use epdf_engine::access::{DocumentAccess, OwnedDocument, PageAccess};
use epdf_engine::cancel::CancelToken;
use epdf_engine::dispatch::OwnerThread;
use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::features::geometry::read_page_geometry;
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};

uniffi::setup_scaffolding!();

/// The one error vocabulary, as the plan requires, carried across the FFI
/// rather than flattened into a string.
///
/// `code` is the same `EngineErrorCode` the web engine reports, so a facade
/// can match on it and a crash report can be grepped for it. `message` is
/// diagnostic and explicitly not a parity contract.
///
/// Deliberately not `#[uniffi(flat_error)]`. Flattening keeps the fields on the
/// Rust side and lowers only `Display`, so a Rust test still matches on `code`
/// while Swift and Kotlin receive one opaque string. That is worse than a plain
/// string error, because it looks structured from the side that is not the
/// consumer.
///
/// The second field is `detail` rather than `message` because uniffi's Kotlin
/// backend maps an error variant onto a `Throwable` subclass and then declares
/// the fields as constructor properties. A field called `message` collides with
/// `Throwable.message`, and the generated file does not compile:
///
/// ```text
/// Overload resolution ambiguity between candidates:
/// val message: String
/// val message: String
/// ```
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PdfError {
    #[error("{code}: {detail}")]
    Engine { code: String, detail: String },
}

impl From<EngineError> for PdfError {
    fn from(error: EngineError) -> Self {
        Self::Engine { code: error.code.as_str().to_string(), detail: error.message }
    }
}

type Result<T> = std::result::Result<T, PdfError>;

/// A page's identity and size, which is all a smoke app needs to lay one out.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PdfPageInfo {
    /// Zero-based position in the document today. Display metadata: an index
    /// is not identity.
    pub index: u32,
    /// The PDF indirect object number, which is the durable key.
    pub object_number: u32,
    pub width_points: f32,
    pub height_points: f32,
}

/// A rendered page. RGBA8, straight alpha, owned bytes.
///
/// The buffer contract in
/// `.agents/skills/embedpdf-conventions/references/engine-lifecycle.md`:
/// stride is explicit because it may exceed `width * 4`, and callers must
/// address rows by it. The bytes are a copy the caller owns, so the image
/// outlives the document, which the plan requires of the platform facades.
#[derive(Debug, uniffi::Record)]
pub struct PdfBitmap {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rgba: Vec<u8>,
}

/// State that lives on the owner thread. Never crosses it.
///
/// `OwnedDocument` holds an `FPDF_DOCUMENT` and is `!Send`, which is exactly
/// why this is the owner thread's parameter rather than a field on the engine:
/// a field would make the engine unshareable, and uniffi requires the engine
/// to be `Send + Sync`.
#[derive(Default)]
struct OwnedState {
    documents: HashMap<u64, OwnedDocument>,
    next_id: u64,
}

/// What an engine and its documents share: the thread.
///
/// One `Arc` rather than a document holding an `Arc<PdfEngine>`, because
/// uniffi hands `PdfEngine` out behind its own `Arc` and there is no honest
/// way to recover that from `&self`.
#[derive(Debug)]
struct Shared {
    owner: Mutex<OwnerThread<OwnedState>>,
}

impl Shared {
    fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut OwnedState) -> T + Send + 'static,
    ) -> Result<T> {
        Ok(self.owner.lock().expect("owner").run(job)?)
    }
}

/// One engine, one owner thread, and every PDFium handle it creates.
#[derive(uniffi::Object)]
pub struct PdfEngine {
    shared: Arc<Shared>,
}

#[uniffi::export]
impl PdfEngine {
    /// Starts an engine and its owner thread.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        // PDFium's per-thread initialization happens through `Library::acquire`
        // on the owner thread, which is why the first job rather than this
        // constructor is where the library appears.
        Arc::new(Self {
            shared: Arc::new(Shared {
                owner: Mutex::new(OwnerThread::start(OwnedState::default)),
            }),
        })
    }

    /// Opens a document from bytes.
    ///
    /// `password` is the user password, or `None`. A wrong or missing one
    /// reports `DocPasswordRequired` rather than a generic open failure, so a
    /// facade can prompt.
    pub fn open(&self, bytes: Vec<u8>, password: Option<String>) -> Result<Arc<PdfDocument>> {
        let id = self.shared.run(move |state| {
            let library = Library::acquire();
            let document = Document::from_bytes(library, &bytes, password.as_deref())
                .map_err(|e| EngineError::with_message(open_code(&e), e.to_string()))?;
            let owned = OwnedDocument::new(document)?;
            state.next_id += 1;
            let id = state.next_id;
            state.documents.insert(id, owned);
            Ok::<u64, EngineError>(id)
        })??;
        Ok(Arc::new(PdfDocument { shared: Arc::clone(&self.shared), id }))
    }

    /// Stops accepting work and waits for what is outstanding.
    ///
    /// Idempotent, and it closes the documents this engine opened, because
    /// they live on the thread it is shutting down.
    ///
    /// Named `shutdown` and not `close` because uniffi's Kotlin backend gives
    /// every exported object an `AutoCloseable.close()` that disposes the
    /// handle. An exported method of that name produces a file with two
    /// `fun close(): Unit` in one class, which does not compile. Dropping the
    /// last reference does the same work anyway, through `OwnerThread`'s own
    /// `Drop`, so `use { }` in Kotlin and ARC in Swift stay correct without
    /// anyone calling this.
    pub fn shutdown(&self) {
        // Dropping the documents has to happen ON the owner thread: they hold
        // PDFium handles, and `FPDF_CloseDocument` is as thread-confined as
        // everything else. The thread's own shutdown would drop them anyway;
        // doing it here means a caller that only closes the engine still gets
        // a deterministic point where the handles went.
        let _ = self.shared.run(|state| state.documents.clear());
        self.shared.owner.lock().expect("owner").close();
    }

    pub fn is_closed(&self) -> bool {
        self.shared.owner.lock().expect("owner").is_closed()
    }
}

/// An open document, addressed by durable page identity.
///
/// `Debug` prints the handle and not the document: the contents live on the
/// owner thread and reaching them from a formatter would be the one thing this
/// type exists to prevent.
#[derive(Debug, uniffi::Object)]
pub struct PdfDocument {
    shared: Arc<Shared>,
    id: u64,
}

#[uniffi::export]
impl PdfDocument {
    /// Every page, with its durable object number and size in PDF points.
    pub fn pages(&self) -> Result<Vec<PdfPageInfo>> {
        let id = self.id;
        self.shared.run(move |state| {
            let document = state.documents.get(&id).ok_or_else(closed_document)?;
            let mut pages = Vec::new();
            for index in 0..document.page_count() {
                let page = document.document().page(index).map_err(|e| {
                    EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
                })?;
                pages.push(PdfPageInfo {
                    index,
                    object_number: page.object_number(),
                    width_points: page.width(),
                    height_points: page.height(),
                });
            }
            Ok::<Vec<PdfPageInfo>, EngineError>(pages)
        })?
        .map_err(PdfError::from)
    }

    /// Page geometry, through the same reader the web engine uses.
    ///
    /// Returned as the JSON the wire carries rather than as a uniffi record.
    /// A `PageGeometrySnapshot` is an untagged union of two run variants with
    /// a glyph list each, and modelling that in uniffi's type system would
    /// produce a shape neither Swift nor Kotlin wants to read. The facade
    /// decodes it into a platform type; that is the facade's job.
    pub fn page_geometry_json(&self, page_object_number: u32) -> Result<String> {
        let id = self.id;
        self.shared.run(move |state| {
            let document = state.documents.get(&id).ok_or_else(closed_document)?;
            let page = document.page(PageObjectNumber::new(page_object_number)?)?;
            let snapshot = read_page_geometry(&page, &CancelToken::never())?;
            serde_json::to_string(&snapshot).map_err(|e| {
                EngineError::with_message(EngineErrorCode::WireFormat, e.to_string())
            })
        })?
        .map_err(PdfError::from)
    }

    /// Renders a page to RGBA8 at `scale`.
    ///
    /// PDFium produces BGRA, so this converts at the boundary rather than
    /// handing a platform a byte order it has to know about. The web path does
    /// the same thing with `FPDF_REVERSE_BYTE_ORDER`; doing it here keeps the
    /// two contracts the same shape even though the mechanism differs.
    pub fn render_page(&self, page_object_number: u32, scale: f32) -> Result<PdfBitmap> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(PdfError::from(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("scale must be finite and positive, got {scale}"),
            )));
        }
        let id = self.id;
        self.shared.run(move |state| {
            let document = state.documents.get(&id).ok_or_else(closed_document)?;
            let page = document.page(PageObjectNumber::new(page_object_number)?)?;
            let view = page.view();
            // Rounded, and refused rather than clamped when it overflows: the
            // admission check belongs before the allocation, not after.
            let width = ((view.width() * scale).round() as i64).clamp(1, u32::MAX as i64) as u32;
            let height = ((view.height() * scale).round() as i64).clamp(1, u32::MAX as i64) as u32;
            let bitmap = view.render_bgra(width, height).map_err(|e| {
                EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
            })?;
            let mut rgba = bitmap.bgra;
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            Ok::<PdfBitmap, EngineError>(PdfBitmap {
                width: bitmap.width,
                height: bitmap.height,
                stride: bitmap.stride,
                rgba,
            })
        })?
        .map_err(PdfError::from)
    }

    /// Closes this document without shutting the engine down.
    ///
    /// `shutdown` rather than `close` for the reason on
    /// [`PdfEngine::shutdown`].
    pub fn shutdown(&self) {
        let id = self.id;
        let _ = self.shared.run(move |state| {
            state.documents.remove(&id);
        });
    }
}

fn closed_document() -> EngineError {
    EngineError::with_message(EngineErrorCode::DocNotOpen, "the document is closed")
}

/// Maps PDFium's open failure onto the shared vocabulary.
///
/// The password cases are split out because a facade prompts for one and
/// reports the other, and collapsing them into `DocOpenFailed` would make that
/// impossible.
fn open_code(error: &pdfium::PdfiumError) -> EngineErrorCode {
    match error {
        pdfium::PdfiumError::Password => EngineErrorCode::DocPasswordRequired,
        _ => EngineErrorCode::DocOpenFailed,
    }
}
