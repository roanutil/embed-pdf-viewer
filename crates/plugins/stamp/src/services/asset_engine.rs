//! `services/asset-engine.ts`: the asset engine port and the
//! canonical-library services every write needs: opening library bytes as a
//! document on a local engine, import-time previews, and the import-support
//! fact learned from the first import.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::bytes::{Bytes, release_host};
use epdf_kernel::engine::{DocumentHandle, Engine};
use epdf_kernel::error::code;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, is_undefined, undefined};
use serde_json::{Value, json};

use crate::contract::{LocalEngine, MadeAssetEngine, StampAssetEngine, StampAssetPreview};
use crate::controller::StampCapability;
use crate::services::errors::stamp_error;
use crate::services::settings::{EngineHold, EngineTable};

thread_local! {
    /// The TypeScript's module-level `idCounter`. A kernel and its plugins
    /// live on one thread, so a thread is the realm a module would be.
    static ID_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// Put the id counter back to zero, as a fresh import of
/// `services/asset-engine.ts` would. For test harnesses running one case
/// after another.
#[doc(hidden)]
pub fn reset_id_sequence() {
    ID_COUNTER.with(|n| n.set(0));
}

/// `n.toString(36)` for a whole, non-negative number.
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

/// `uid(prefix)`: session-unique ids (scratch documents, library ids a file
/// does not carry), `${prefix}-${Date.now().toString(36)}-${counter}`.
pub fn uid(prefix: &str) -> String {
    let n = ID_COUNTER.with(|counter| {
        let n = counter.get();
        counter.set(n + 1);
        n
    });
    format!("{prefix}-{}-{}", base36(rt::now().max(0.0) as u64), base36(n))
}

/// `TypeError: Cannot read properties of null (reading 'member')`.
pub(crate) fn read_off(value: &Value, member: &str) -> Error {
    let what = if value.is_null() { "null" } else { "undefined" };
    Error::named("TypeError", format!("Cannot read properties of {what} (reading '{member}')"))
}

fn nullish(value: &Value) -> bool {
    value.is_null() || is_undefined(value)
}

/// A byte answer: `None` for one that isn't bytes (the TypeScript's
/// `undefined`).
pub(crate) fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    Bytes::from_json(value).map(|b| b.0)
}

/// The engine the asset engine setting resolved to.
#[derive(Clone)]
pub(crate) enum ResolvedEngine {
    /// The kernel's engine (`ctx.engine`), local or not: `Engine::is_local`
    /// is the TypeScript's `isLocalEngine` brand check.
    Kernel(Option<Rc<dyn Engine>>),
    Ready(LocalEngine),
    Pending(Promise<LocalEngine>),
}

pub(crate) struct Resolved {
    /// The setting it was resolved from, by its stored handle.
    from: Option<u64>,
    engine: ResolvedEngine,
    /// Keeps `from`'s engine in the table while this is the resolved one,
    /// as `resolved.from` keeps the object alive in the TypeScript.
    _hold: Option<EngineHold>,
}

/// The asset engine service's state.
#[derive(Default)]
pub struct AssetEngineState {
    /// Set once an import finds the asset engine is not local (cloud).
    import_supported: Cell<Option<bool>>,
    resolved: RefCell<Option<Resolved>>,
    /// Engines this plugin made through a configured function: destroyed
    /// with it.
    owned: RefCell<Vec<MadeAssetEngine>>,
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

/// `imageToPreview(image)`: import-time preview render to bytes. The asset
/// engine is local by definition, so the image source is always inline
/// bytes. Once they are copied out, a host-held render goes back to its
/// host (`release_host`); a render whose bytes did not resolve is kept.
pub(crate) fn image_to_preview(image: &Value) -> Result<StampAssetPreview, Error> {
    if nullish(image) {
        return Err(read_off(image, "source"));
    }
    let source = image.get("source").cloned().unwrap_or_else(undefined);
    if nullish(&source) {
        return Err(read_off(&source, "kind"));
    }
    if source.get("kind").and_then(Value::as_str) != Some("bytes") {
        return Err(stamp_error(
            code::UNSUPPORTED,
            "the asset engine returned a URL-sourced render; asset engines must be local",
        ));
    }
    let bytes = source.get("bytes").filter(|v| !is_undefined(v)).map(|raw| match bytes_of(raw) {
        Some(bytes) => Bytes(bytes).to_json(),
        None => raw.clone(),
    });
    if source.get("bytes").and_then(bytes_of).is_some() {
        release_host(image);
    }
    Ok(StampAssetPreview {
        bytes,
        mime_type: image.get("contentType").filter(|v| !is_undefined(v)).cloned(),
    })
}

/// A call on a document of the asset engine.
pub(crate) fn call(doc: &Rc<dyn DocumentHandle>, path: &str, args: Vec<Value>) -> Promise<Value> {
    doc.call(path, None, args)
}

/// A call on one of its pages.
pub(crate) fn page_call(
    doc: &Rc<dyn DocumentHandle>,
    path: &str,
    page: u32,
    args: Vec<Value>,
) -> Promise<Value> {
    doc.call(path, Some(page), args)
}

/// `try { body } finally { await doc.close() }`: a failed close replaces the
/// body's outcome.
pub(crate) async fn close<T>(
    doc: &Rc<dyn DocumentHandle>,
    body: Result<T, Error>,
) -> Result<T, Error> {
    call(doc, "close", Vec::new()).await?;
    body
}

impl StampCapability {
    /// `assetEngine()`: the asset engine port, read from the setting. A
    /// configured function is called (and memoized) on the first import; a
    /// configured engine is used as it is; none falls back to the kernel's
    /// engine. A change of the setting takes effect at the next library
    /// operation.
    fn asset_engine(&self) -> Result<ResolvedEngine, Error> {
        let stored = self.settings_json();
        let setting = stored.get("assetEngine").cloned().unwrap_or(Value::Null);
        let from = EngineTable::handle_of(&setting);
        let state = &self.0.asset_engine;
        let stale = match &*state.resolved.borrow() {
            Some(resolved) => resolved.from != from,
            // `undefined !== configured`: the first call always resolves.
            None => true,
        };
        if stale {
            let configured = self.get_settings().asset_engine;
            let engine = match &configured {
                None => ResolvedEngine::Kernel(self.0.cx.engine()),
                Some(StampAssetEngine::Engine(engine)) => ResolvedEngine::Ready(engine.clone()),
                Some(StampAssetEngine::Factory(factory)) => {
                    let made = factory()?;
                    state.owned.borrow_mut().push(made.clone());
                    match made {
                        MadeAssetEngine::Ready(engine) => ResolvedEngine::Ready(engine),
                        MadeAssetEngine::Pending(promise) => ResolvedEngine::Pending(promise),
                    }
                }
            };
            let _hold = from.map(|n| self.0.table.hold(n));
            *state.resolved.borrow_mut() = Some(Resolved { from, engine, _hold });
            state.import_supported.set(None);
        }
        Ok(state
            .resolved
            .borrow()
            .as_ref()
            .map(|r| r.engine.clone())
            .unwrap_or(ResolvedEngine::Kernel(None)))
    }

    /// `openAssetDocument(bytes)`: library PDFs are sliced and tagged
    /// (`/PieceInfo`) on a local engine only.
    pub(crate) fn open_asset_document(&self, bytes: Vec<u8>) -> Promise<Rc<dyn DocumentHandle>> {
        let this = self.clone();
        Promise::spawn(async move {
            let engine = match this.asset_engine()? {
                ResolvedEngine::Pending(promise) => Some(promise.await?),
                ResolvedEngine::Ready(engine) => {
                    rt::tick().await;
                    Some(engine)
                }
                // `if (!isLocalEngine(engine))`: the kernel's engine, used as
                // it is when it carries the local brand.
                ResolvedEngine::Kernel(engine) => {
                    rt::tick().await;
                    engine.filter(|engine| engine.is_local()).map(LocalEngine::new)
                }
            };
            let Some(engine) = engine else {
                let state = &this.0.asset_engine;
                if state.import_supported.get() != Some(false) {
                    state.import_supported.set(Some(false));
                    this.0.cx.notify();
                }
                let configured = this.get_settings().asset_engine.is_some();
                return Err(stamp_error(
                    code::UNSUPPORTED,
                    if configured {
                        "the stamp plugin's assetEngine must be a local engine (createLocalEngine())"
                    } else {
                        "importing a library PDF needs a local engine, and this viewer's engine is a cloud engine. Pass stampPlugin({ assetEngine: () => import('@embedpdf/engine').then((module) => module.createLocalEngine()) }): it loads lazily, on first import."
                    },
                ));
            };
            let input = json!({ "kind": "bytes", "id": uid("stamp-import"), "bytes": Bytes(bytes).to_json() });
            // `return engine.open(...)` from an `async` function: the
            // resolve-thenable job, then the `then` reaction.
            let opened = engine.open(input, Some(json!({ "scope": ["*"] })));
            rt::tick().await;
            opened.reaction().await
        })
    }

    /// `renderThumbnail(handle)`: the small picker image of one library
    /// page.
    pub(crate) fn render_thumbnail(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        page: u32,
    ) -> Promise<StampAssetPreview> {
        let render = page_call(
            doc,
            "page.render.image",
            page,
            vec![render_options(self.get_settings().preview_width, true)],
        );
        Promise::spawn(async move { image_to_preview(&render.await?) })
    }

    /// `canImport()`: optimistic until an import finds the asset engine is
    /// not local. Reading the setting re-resolves after a change, which
    /// makes imports possible again.
    pub fn can_import(&self) -> bool {
        let state = &self.0.asset_engine;
        if let Some(resolved) = &*state.resolved.borrow() {
            let setting = self.settings_json().get("assetEngine").cloned().unwrap_or(Value::Null);
            if resolved.from != EngineTable::handle_of(&setting) {
                return true;
            }
        }
        state.import_supported.get() != Some(false)
    }
}

impl AssetEngineState {
    /// The asset engine service's cleanup: forget the resolved engine and
    /// destroy every engine a configured function made,
    /// `void Promise.resolve(engine).then((made) => made.destroy()).catch(() => {})`.
    pub(crate) fn release(&self) {
        let state = self;
        *state.resolved.borrow_mut() = None;
        let owned: Vec<MadeAssetEngine> = state.owned.borrow_mut().drain(..).collect();
        for engine in owned {
            rt::spawn(async move {
                let made = match engine {
                    MadeAssetEngine::Ready(engine) => {
                        rt::tick().await;
                        engine
                    }
                    MadeAssetEngine::Pending(promise) => match promise.reaction().await {
                        Ok(engine) => engine,
                        Err(_) => return,
                    },
                };
                made.destroy().ignore();
            });
        }
    }
}
