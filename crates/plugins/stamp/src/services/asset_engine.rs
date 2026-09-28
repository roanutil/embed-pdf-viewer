//! `services/asset-engine.ts`: the asset engine port and the canonical
//! library services every write needs: opening library bytes as a document,
//! import-time previews, the canonical-services check, and the import
//! support fact learned from the first failed open.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::bytes::{Bytes, release_host};
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, is_undefined, undefined};
use serde_json::{Value, json};

use crate::contract::{AssetEngine, AssetEngineRef, StampAssetPreview};
use crate::controller::StampCapability;
use crate::promise::await_engine;
use crate::value::{get, str_of};

/// `DEFAULT_PREVIEW_WIDTH`.
pub const DEFAULT_PREVIEW_WIDTH: f64 = 256.0;

thread_local! {
    /// The TypeScript's module-level `seq`. A kernel and its plugins live on
    /// one thread, so a thread is the realm a module would be.
    static SEQ: Cell<u64> = const { Cell::new(0) };
}

/// Put the id counter back to zero, as a fresh import of
/// `services/asset-engine.ts` would. For test harnesses running one case
/// after another.
#[doc(hidden)]
pub fn reset_id_sequence() {
    SEQ.with(|seq| seq.set(0));
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `uid(prefix)`: `${prefix}-${Date.now().toString(36)}-${(seq++).toString(36)}`.
pub fn uid(prefix: &str) -> String {
    let n = SEQ.with(|seq| {
        let n = seq.get();
        seq.set(n + 1);
        n
    });
    format!("{prefix}-{}-{}", base36(rt::now().max(0.0) as u64), base36(n))
}

fn type_error(message: impl Into<String>) -> Error {
    Error::named("TypeError", message)
}

/// Reading `member` off a value that is `null` or `undefined`.
pub(crate) fn read_off(value: &Value, member: &str) -> Error {
    let what = if value.is_null() { "null" } else { "undefined" };
    type_error(format!("Cannot read properties of {what} (reading '{member}')"))
}

fn missing(value: Option<&Value>) -> bool {
    value.is_none_or(|v| v.is_null() || is_undefined(v))
}

/// A byte answer: `None` when the engine answered no bytes.
pub(crate) fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    Bytes::from_json(value).map(|b| b.0)
}

/// The render options every preview uses.
pub(crate) fn render_options(width: f64, include_annotations: bool) -> Value {
    json!({
        "viewport": { "kind": "width", "width": width },
        "background": "transparent",
        "includeAnnotations": include_annotations,
        "format": "png",
    })
}

/// `imageToPreview(image)`: the asset engine is local by definition, so a
/// render's source is always inline bytes. Once they are copied out, the
/// render itself goes back to its host (`release_host`): every caller
/// passes a `page.render.image` result it keeps nothing else of. When the
/// bytes did not resolve (a host with no `$hostBytes` resolver), the render
/// is kept: releasing it would drop pixels nothing has read.
pub(crate) fn image_to_preview(image: &Value) -> Result<StampAssetPreview, Error> {
    if missing(Some(image)) {
        return Err(read_off(image, "source"));
    }
    let source = get(image, "source").cloned().unwrap_or_else(undefined);
    if missing(Some(&source)) {
        return Err(read_off(&source, "kind"));
    }
    if str_of(&source, "kind") != Some("bytes") {
        return Err(Error::engine(
            "NotImplemented",
            "[stamp] asset engine returned a URL-sourced render; asset engines must be local",
        ));
    }
    let bytes = get(&source, "bytes").and_then(bytes_of);
    if bytes.is_some() {
        release_host(image);
    }
    Ok(StampAssetPreview { bytes, mime_type: str_of(image, "contentType").map(str::to_string) })
}

/// A document call.
pub(crate) fn call(doc: &Rc<dyn DocumentHandle>, path: &str, args: Vec<Value>) -> Promise<Value> {
    doc.call(path, None, args)
}

/// A page call.
pub(crate) fn page_call(
    doc: &Rc<dyn DocumentHandle>,
    path: &str,
    pon: u32,
    args: Vec<Value>,
) -> Promise<Value> {
    doc.call(path, Some(pon), args)
}

/// `doc.close()` in a `finally`: its failure replaces the body's outcome.
pub(crate) async fn close<T>(
    doc: &Rc<dyn DocumentHandle>,
    body: Result<T, Error>,
) -> Result<T, Error> {
    await_engine(call(doc, "close", Vec::new())).await?;
    body
}

impl StampCapability {
    pub(crate) fn preview_width(&self) -> f64 {
        self.0.config.preview_width.unwrap_or(DEFAULT_PREVIEW_WIDTH)
    }

    /// The asset engine: a configured factory is called (and memoized) on
    /// the first open, an instance is used as is, and nothing configured is
    /// the kernel's engine.
    fn asset_engine(&self) -> Result<AssetEngineRef, Error> {
        if let Some(engine) = self.0.asset_engine_ref.borrow().clone() {
            return Ok(engine);
        }
        let engine = match &self.0.config.asset_engine {
            None => AssetEngineRef::Ready(
                self.cx()
                    .engine()
                    .ok_or_else(|| Error::new("[kernel] used after the kernel was dropped"))?,
            ),
            Some(AssetEngine::Factory(factory)) => factory()?,
            Some(AssetEngine::Instance(engine)) => AssetEngineRef::Ready(engine.clone()),
        };
        *self.0.asset_engine_ref.borrow_mut() = Some(engine.clone());
        Ok(engine)
    }

    /// `openAssetDocument(bytes)`. Callers await it through `rt::returned`,
    /// the turn an `async` function's caller spends.
    pub(crate) async fn open_asset_document(
        &self,
        bytes: Vec<u8>,
    ) -> Result<Rc<dyn DocumentHandle>, Error> {
        let opened = async {
            let engine = match self.asset_engine()? {
                AssetEngineRef::Ready(engine) => {
                    rt::tick().await;
                    engine
                }
                AssetEngineRef::Pending(promise) => promise.await?,
            };
            let input = json!({ "kind": "bytes", "id": uid("stamp-import"), "bytes": Bytes(bytes).to_json() });
            await_engine(engine.open(input, Some(json!({ "scope": ["*"] })))).await
        }
        .await;
        opened.map_err(|error| {
            // A cloud kernel engine rejects 'bytes' with InvalidArg: turn the
            // generic contract error into the configuration fix.
            if self.0.config.asset_engine.is_none()
                && error.name == "EngineError"
                && error.code() == Some("InvalidArg")
            {
                self.0.import_supported.set(Some(false));
                Error::engine(
                    "NotImplemented",
                    "[stamp] importing a library PDF needs an engine that can open local bytes, and this viewer's engine cannot (cloud). Pass stampPlugin({ assetEngine: () => import('@embedpdf/engine').then((m) => m.createLocalEngine()) }) — it loads lazily, on first import.",
                )
            } else {
                error
            }
        })
    }

    /// `renderThumbnail(handle)`: the small picker image of one library page.
    pub(crate) async fn render_thumbnail(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        pon: u32,
    ) -> Result<StampAssetPreview, Error> {
        let render = page_call(
            doc,
            "page.render.image",
            pon,
            vec![render_options(self.preview_width(), true)],
        );
        image_to_preview(&await_engine(render).await?)
    }

    /// `requireCanonicalServices(doc)`.
    pub(crate) fn require_canonical_services(doc: &Rc<dyn DocumentHandle>) -> Result<(), Error> {
        if !doc.has("pieceInfo") {
            return Err(Error::engine(
                "NotImplemented",
                "[stamp] canonical PDF libraries need an asset engine with pieceInfo",
            ));
        }
        if !doc.has("pages.setName") || !doc.has("pages.removeName") {
            return Err(Error::engine(
                "NotImplemented",
                "[stamp] canonical PDF libraries need an asset engine with named pages (pages.setName)",
            ));
        }
        Ok(())
    }

    /// `canImport()`: optimistic until an import proves the engine cannot
    /// open local bytes.
    pub fn can_import(&self) -> bool {
        self.0.import_supported.get() != Some(false)
    }
}
