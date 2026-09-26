//! `capability.ts`: the library store's binary sidecar, the asset engine
//! port, import, single-asset intents, the per-library mutation queue,
//! dynamic-stamp materialization and placement.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_geometry::Size;
use epdf_geometry::js::number_to_string;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Ctx, Error, is_undefined, undefined};
use epdf_plugin_annotation::binary::{resolve_binary_source, sniff_binary_metadata};
use epdf_plugin_annotation::value::{get, str_of};
use epdf_plugin_annotation::{ANNOTATION, StampToolInput};
use epdf_plugin_form::scripting::{
    FormScriptingControllerOptions, create_form_scripting_controller,
};
use epdf_plugin_form::{FormCommitResult, FormCommitStatus};
use serde_json::{Value, json};

use crate::promise::{adopt, await_engine, finally};
use crate::types::{
    AddAssetInput, ArmAssetOptions, AssetEngine, AssetEngineRef, ImportLibraryOptions, StampAction,
    StampAsset, StampAssetFormat, StampAssetKind, StampAssetPreview, StampConfig, StampLibrary,
    StampLibraryStorage, StampState,
};

const DEFAULT_PREVIEW_WIDTH: f64 = 256.0;
const LIBRARY_PIECEINFO_APP: &str = "EMBD_StampLibrary";
const STAMP_PIECEINFO_APP: &str = "EMBD_Stamp";
const STAMP_SCHEMA_VERSION: u32 = 1;

// ── session ids ───────────────────────────────────────────────────────────────

thread_local! {
    /// The TypeScript's module-level `seq`. The kernel and its plugins live
    /// on one thread, so a thread is the realm a module would be.
    static SEQ: Cell<u64> = const { Cell::new(0) };
}

/// Put the id counter back to zero, as a fresh import of `capability.ts`
/// would. For test harnesses running one case after another.
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
fn uid(prefix: &str) -> String {
    let n = SEQ.with(|seq| {
        let n = seq.get();
        seq.set(n + 1);
        n
    });
    format!("{prefix}-{}-{}", base36(rt::now().max(0.0) as u64), base36(n))
}

// ── PieceInfo ─────────────────────────────────────────────────────────────────

fn typed_entry<'a>(entries: &'a Value, key: &str, kind: &str) -> Option<&'a Value> {
    let entry = get(entries, key)?;
    (str_of(entry, "type") == Some(kind)).then_some(entry)
}

/// `entryString`: a non-empty `string` entry.
fn entry_string(entries: &Value, key: &str) -> Option<String> {
    let value = str_of(typed_entry(entries, key, "string")?, "value")?;
    (!value.is_empty()).then(|| value.to_string())
}

/// `entryName`: a non-empty `name` entry.
fn entry_name(entries: &Value, key: &str) -> Option<String> {
    let value = str_of(typed_entry(entries, key, "name")?, "value")?;
    (!value.is_empty()).then(|| value.to_string())
}

/// `entryStringArray`: a `string-array` entry, copied.
fn entry_string_array(entries: &Value, key: &str) -> Option<Vec<String>> {
    let entry = typed_entry(entries, key, "string-array")?;
    Some(
        get(entry, "value")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    )
}

fn kind_to_pdf_name(kind: StampAssetKind) -> &'static str {
    match kind {
        StampAssetKind::Signature => "Signature",
        StampAssetKind::Initials => "Initials",
        StampAssetKind::Stamp => "Stamp",
    }
}

fn kind_from_pdf_name(name: Option<&str>) -> Option<StampAssetKind> {
    match name?.to_lowercase().as_str() {
        "stamp" => Some(StampAssetKind::Stamp),
        "signature" => Some(StampAssetKind::Signature),
        "initials" => Some(StampAssetKind::Initials),
        _ => None,
    }
}

fn strings_or_null(values: Option<&Vec<String>>) -> Value {
    values.map_or(Value::Null, |v| json!(v))
}

fn metadata_patch(asset: &StampAsset) -> Value {
    json!({
        "Version": STAMP_SCHEMA_VERSION,
        "Id": asset.id,
        "Name": asset.name,
        "Kind": { "name": kind_to_pdf_name(asset.kind) },
        "Subject": asset.subject.as_ref().map_or(Value::Null, |s| json!(s)),
        "Categories": strings_or_null(asset.categories.as_ref()),
    })
}

fn library_metadata_patch(id: &str, name: &str, categories: Option<&Vec<String>>) -> Value {
    json!({
        "Version": STAMP_SCHEMA_VERSION,
        "Id": id,
        "Name": name,
        "Kind": { "name": "StampLibrary" },
        "Categories": strings_or_null(categories),
    })
}

/// `snapshot?.entries ?? {}`.
fn entries_of(snapshot: &Value) -> Value {
    match get(snapshot, "entries") {
        Some(entries) if !entries.is_null() => entries.clone(),
        _ => json!({}),
    }
}

// ── engine answers ────────────────────────────────────────────────────────────

/// A byte answer: `None` when the engine answered no bytes.
fn bytes_of(value: &Value) -> Option<Vec<u8>> {
    Bytes::from_json(value).map(|b| b.0)
}

fn type_error(message: impl Into<String>) -> Error {
    Error::named("TypeError", message)
}

/// Reading `member` off a value that is `null` or `undefined`.
fn read_off(value: &Value, member: &str) -> Error {
    let what = if value.is_null() { "null" } else { "undefined" };
    type_error(format!("Cannot read properties of {what} (reading '{member}')"))
}

fn missing(value: Option<&Value>) -> bool {
    value.is_none_or(|v| v.is_null() || is_undefined(v))
}

/// A JavaScript value in a template literal.
fn js_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".to_string(),
        _ => "undefined".to_string(),
    }
}

/// `a === b` for two members, `undefined` when absent.
fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.filter(|v| !is_undefined(v)), b.filter(|v| !is_undefined(v))) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(x), Some(y)) => !x.is_object() && !x.is_array() && x == y,
        _ => false,
    }
}

fn pon_of(layout: &Value) -> u32 {
    get(layout, "pageObjectNumber").and_then(Value::as_u64).unwrap_or(0) as u32
}

fn size_of(layout: &Value) -> Size {
    let size = get(layout, "size").cloned().unwrap_or(Value::Null);
    let dim = |key| get(&size, key).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Size { width: dim("width"), height: dim("height") }
}

fn render_options(width: f64, include_annotations: bool) -> Value {
    json!({
        "viewport": { "kind": "width", "width": width },
        "background": "transparent",
        "includeAnnotations": include_annotations,
        "format": "png",
    })
}

/// `imageToPreview(image)`: the asset engine is local by definition, so a
/// render's source is always inline bytes.
fn image_to_preview(image: &Value) -> Result<StampAssetPreview, Error> {
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
    Ok(StampAssetPreview {
        bytes: get(&source, "bytes").and_then(bytes_of),
        mime_type: str_of(image, "contentType").map(str::to_string),
    })
}

fn call(doc: &Rc<dyn DocumentHandle>, path: &str, args: Vec<Value>) -> Promise<Value> {
    doc.call(path, None, args)
}

fn page_call(
    doc: &Rc<dyn DocumentHandle>,
    path: &str,
    pon: u32,
    args: Vec<Value>,
) -> Promise<Value> {
    doc.call(path, Some(pon), args)
}

// ── the capability ────────────────────────────────────────────────────────────

/// The binary sidecar of one asset: its placement bytes and cached preview.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetBinary {
    pub bytes: Option<Vec<u8>>,
    pub preview: Option<StampAssetPreview>,
}

struct Inner {
    ctx: Ctx<StampState, StampAction>,
    config: StampConfig,
    /// Asset bytes and cached previews, keyed by asset id. The reducer
    /// never sees these.
    binaries: RefCell<HashMap<String, AssetBinary>>,
    /// The durable source of truth for a canonical library. `None` is a
    /// save that answered no bytes.
    library_binaries: RefCell<HashMap<String, Option<Vec<u8>>>>,
    /// Whole-PDF rewrites are serialized per library.
    library_mutation_tails: RefCell<HashMap<String, Promise<()>>>,
    asset_engine_ref: RefCell<Option<AssetEngineRef>>,
}

/// `StampCapability`.
#[derive(Clone)]
pub struct StampCapability(Rc<Inner>);

/// `createStampCapability(ctx, config)`.
pub fn create_stamp_capability(
    ctx: Ctx<StampState, StampAction>,
    config: StampConfig,
) -> StampCapability {
    let capability = StampCapability(Rc::new(Inner {
        ctx: ctx.clone(),
        config,
        binaries: RefCell::default(),
        library_binaries: RefCell::default(),
        library_mutation_tails: RefCell::default(),
        asset_engine_ref: RefCell::new(None),
    }));
    let inner = capability.0.clone();
    ctx.cleanup_fn(move || {
        inner.binaries.borrow_mut().clear();
        inner.library_binaries.borrow_mut().clear();
        inner.library_mutation_tails.borrow_mut().clear();
        let owned = match inner.config.asset_engine {
            Some(AssetEngine::Factory(_)) => inner.asset_engine_ref.borrow().clone(),
            _ => None,
        };
        *inner.asset_engine_ref.borrow_mut() = None;
        if let Some(owned) = owned {
            // `void Promise.resolve(owned).then((e) => e.destroy()).catch(() => {})`
            rt::spawn(async move {
                let engine = match owned {
                    AssetEngineRef::Ready(engine) => {
                        rt::tick().await;
                        engine
                    }
                    AssetEngineRef::Pending(promise) => match promise.await {
                        Ok(engine) => engine,
                        Err(_) => return,
                    },
                };
                engine.destroy().ignore();
            });
        }
    });
    capability
}

impl StampCapability {
    fn ctx(&self) -> &Ctx<StampState, StampAction> {
        &self.0.ctx
    }

    /// `ctx.getState()`, read for `member`: after the kernel is destroyed
    /// the slice is gone and the TypeScript reads a member of `undefined`.
    fn read(&self, member: &str) -> Result<Rc<StampState>, Error> {
        self.ctx().try_state().ok_or_else(|| read_off(&undefined(), member))
    }

    fn preview_width(&self) -> f64 {
        self.0.config.preview_width.unwrap_or(DEFAULT_PREVIEW_WIDTH)
    }

    /// `mutateLibrary(libraryId, mutation)`.
    fn mutate_library<T: Clone + 'static>(
        &self,
        library_id: &str,
        mutation: impl FnOnce() -> Promise<T> + 'static,
    ) -> Promise<T> {
        let previous = self.0.library_mutation_tails.borrow().get(library_id).cloned();
        // `(previous ?? Promise.resolve()).then(mutation)`
        let result = Promise::spawn(async move {
            match previous {
                Some(previous) => {
                    let _ = previous.await;
                }
                None => rt::tick().await,
            }
            adopt(mutation()).await
        });
        let tail = {
            let result = result.clone();
            Promise::spawn(async move {
                let _ = result.await;
                Ok(())
            })
        };
        self.0.library_mutation_tails.borrow_mut().insert(library_id.to_string(), tail.clone());
        let inner = self.0.clone();
        let key = library_id.to_string();
        finally(result, move || {
            let current = inner.library_mutation_tails.borrow().get(&key).cloned();
            if current.is_some_and(|current| current.same(&tail)) {
                inner.library_mutation_tails.borrow_mut().remove(&key);
            }
        })
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
                self.ctx()
                    .engine()
                    .ok_or_else(|| Error::new("[kernel] used after the kernel was dropped"))?,
            ),
            Some(AssetEngine::Factory(factory)) => factory()?,
            Some(AssetEngine::Instance(engine)) => AssetEngineRef::Ready(engine.clone()),
        };
        *self.0.asset_engine_ref.borrow_mut() = Some(engine.clone());
        Ok(engine)
    }

    /// `openAssetDocument(bytes, identity?)`. Callers await it through
    /// `rt::returned`, the turn an `async` function's caller spends.
    async fn open_asset_document(
        &self,
        bytes: Vec<u8>,
        identity: Option<Value>,
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
            let mut options = json!({ "scope": ["*"] });
            if let Some(identity) = identity {
                options["identity"] = identity;
            }
            await_engine(engine.open(input, Some(options))).await
        }
        .await;
        opened.map_err(|error| {
            // A cloud kernel engine rejects 'bytes' with InvalidArg: turn the
            // generic contract error into the configuration fix.
            if self.0.config.asset_engine.is_none()
                && error.name == "EngineError"
                && error.code() == Some("InvalidArg")
            {
                Error::engine(
                    "NotImplemented",
                    "[stamp] importing a library PDF needs an engine that can open local bytes, and this viewer's engine cannot (cloud). Pass stampPlugin({ assetEngine: () => import('@embedpdf/engine').then((m) => m.createLocalEngine()) }) — it loads lazily, on first import.",
                )
            } else {
                error
            }
        })
    }

    /// `doc.close()` in a `finally`: its failure replaces the body's outcome.
    async fn close<T>(doc: &Rc<dyn DocumentHandle>, body: Result<T, Error>) -> Result<T, Error> {
        await_engine(call(doc, "close", Vec::new())).await?;
        body
    }

    fn allocate_id(preferred: Option<String>, prefix: &str, taken: &mut HashSet<String>) -> String {
        let id = match preferred {
            Some(id) if !taken.contains(&id) => id,
            _ => loop {
                let id = uid(prefix);
                if !taken.contains(&id) {
                    break id;
                }
            },
        };
        taken.insert(id.clone());
        id
    }

    fn create_loose_library(&self, name: &str) -> String {
        let id = uid("stamp-lib");
        self.ctx().dispatch(StampAction::LibraryAdded {
            library: StampLibrary {
                id: id.clone(),
                name: name.to_string(),
                storage: StampLibraryStorage::Loose,
                categories: None,
                asset_ids: Vec::new(),
            },
        });
        id
    }

    // ── import ────────────────────────────────────────────────────────────────

    /// `importLibraryPdf(source, opts?)`: every page becomes one vector
    /// asset. Resolves to the new library id.
    pub fn import_library_pdf(&self, source: Value, opts: ImportLibraryOptions) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            let resolved = resolve_binary_source(&source).await?;
            let meta = sniff_binary_metadata(&resolved.bytes);
            if meta.map(|m| m.mime_type) != Some("application/pdf") {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] importLibraryPdf needs PDF bytes (use addAsset for a raster image)",
                ));
            }
            let doc = rt::returned(this.open_asset_document(resolved.bytes, None)).await?;
            let body = this.import_pages(&doc, &opts).await;
            let (library, canonical, assets) = Self::close(&doc, body).await?;

            this.0.library_binaries.borrow_mut().insert(library.id.clone(), canonical);
            let id = library.id.clone();
            this.ctx().dispatch(StampAction::LibraryAdded { library });
            for (asset, binary) in assets {
                this.0.binaries.borrow_mut().insert(asset.id.clone(), binary);
                this.ctx().dispatch(StampAction::AssetAdded { asset });
            }
            Ok(id)
        })
    }

    /// The `try` block of `importLibraryPdf`.
    async fn import_pages(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        opts: &ImportLibraryOptions,
    ) -> Result<(StampLibrary, Option<Vec<u8>>, Vec<(StampAsset, AssetBinary)>), Error> {
        if !doc.has("pieceInfo") {
            return Err(Error::engine(
                "NotImplemented",
                "[stamp] canonical PDF libraries need an asset engine with pieceInfo",
            ));
        }
        let snapshot = await_engine(call(doc, "pages.list", Vec::new())).await?;
        if get(&snapshot, "pageCount").and_then(Value::as_f64) == Some(0.0) {
            return Err(Error::engine(
                "InvalidArg",
                "[stamp] a canonical stamp library PDF must contain at least one page",
            ));
        }
        let layouts = match get(&snapshot, "pages").and_then(Value::as_array) {
            Some(layouts) => layouts.clone(),
            None => return Err(read_off(&undefined(), "map")),
        };

        // Discover and validate every required service before the first write.
        let mut pages = Vec::new();
        for layout in layouts {
            let pon = pon_of(&layout);
            doc.page_index(pon)?;
            if !doc.has("page.pieceInfo") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] canonical PDF libraries need page pieceInfo support",
                ));
            }
            pages.push((layout, pon));
        }

        let catalog_metadata =
            await_engine(call(doc, "pieceInfo.read", vec![json!(LIBRARY_PIECEINFO_APP)])).await?;
        let mut page_metadata = Vec::new();
        for (_, pon) in &pages {
            let read =
                page_call(doc, "page.pieceInfo.read", *pon, vec![json!(STAMP_PIECEINFO_APP)]);
            page_metadata.push(await_engine(read).await?);
        }

        let state = self.read("libraries")?;
        let mut taken_library_ids: HashSet<String> = state.libraries.keys().cloned().collect();
        let mut taken_asset_ids: HashSet<String> = state.assets.keys().cloned().collect();
        let catalog_entries = entries_of(&catalog_metadata);
        let library_id = Self::allocate_id(
            entry_string(&catalog_entries, "Id"),
            "stamp-lib",
            &mut taken_library_ids,
        );
        let library_name = opts
            .name
            .clone()
            .or_else(|| entry_string(&catalog_entries, "Name"))
            .unwrap_or_else(|| "Stamps".to_string());
        let library_categories =
            opts.categories.clone().or_else(|| entry_string_array(&catalog_entries, "Categories"));

        let mut descriptors = Vec::new();
        for ((layout, pon), metadata) in pages.iter().zip(&page_metadata) {
            let entries = entries_of(metadata);
            let id = Self::allocate_id(entry_string(&entries, "Id"), "stamp", &mut taken_asset_ids);
            let kind = opts
                .kind
                .or_else(|| kind_from_pdf_name(entry_name(&entries, "Kind").as_deref()))
                .unwrap_or(StampAssetKind::Stamp);
            let index = get(layout, "index").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let name = opts
                .asset_name
                .as_ref()
                .and_then(|name| name(index))
                .or_else(|| entry_string(&entries, "Name"))
                .unwrap_or_else(|| format!("Stamp {}", number_to_string(index + 1.0)));
            descriptors.push((
                *pon,
                StampAsset {
                    id,
                    library_id: library_id.clone(),
                    kind,
                    name,
                    size: size_of(layout),
                    format: StampAssetFormat::Pdf,
                    page_object_number: Some(*pon),
                    subject: entry_string(&entries, "Subject"),
                    categories: entry_string_array(&entries, "Categories"),
                },
            ));
        }

        let patch = library_metadata_patch(&library_id, &library_name, library_categories.as_ref());
        await_engine(call(doc, "pieceInfo.update", vec![json!(LIBRARY_PIECEINFO_APP), patch]))
            .await?;

        let mut assets = Vec::new();
        for (pon, asset) in descriptors {
            let update = page_call(
                doc,
                "page.pieceInfo.update",
                pon,
                vec![json!(STAMP_PIECEINFO_APP), metadata_patch(&asset)],
            );
            await_engine(update).await?;
            // One canonical page: one derived placement PDF plus a
            // transparent preview, extracted after the metadata so the cache
            // carries the same id.
            let bytes = await_engine(call(doc, "pages.extract", vec![json!([pon])])).await?;
            let render = page_call(
                doc,
                "page.render.image",
                pon,
                vec![render_options(self.preview_width(), true)],
            );
            let image = await_engine(render).await?;
            let preview = image_to_preview(&image)?;
            assets.push((asset, AssetBinary { bytes: bytes_of(&bytes), preview: Some(preview) }));
        }

        let library = StampLibrary {
            id: library_id,
            name: library_name,
            storage: StampLibraryStorage::CanonicalPdf,
            categories: library_categories,
            asset_ids: Vec::new(),
        };
        let canonical = await_engine(call(doc, "download", Vec::new())).await?;
        Ok((library, bytes_of(&canonical), assets))
    }

    // ── single assets ─────────────────────────────────────────────────────────

    /// `addAsset(input)`: a PDF added to a canonical library is inserted as a
    /// page; loose assets keep their bytes.
    pub fn add_asset(&self, input: AddAssetInput) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            let wanted = input.library_id.clone().filter(|id| !id.is_empty());
            let target_library = match &wanted {
                Some(id) => this.read("libraries")?.libraries.get(id).cloned(),
                None => None,
            };
            if let (Some(id), None) = (&wanted, &target_library) {
                return Err(Error::engine("NotFound", format!("[stamp] unknown library '{id}'")));
            }
            let resolved = resolve_binary_source(&input.source).await?;
            let Some(meta) = sniff_binary_metadata(&resolved.bytes) else {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] asset source must be PNG, JPEG, or single-page PDF bytes",
                ));
            };
            let is_pdf = meta.mime_type == "application/pdf";

            let mut supplied_preview = None;
            if let Some(preview) =
                input.preview.as_ref().filter(|p| epdf_plugin_annotation::value::truthy(Some(p)))
            {
                let preview = resolve_binary_source(preview).await?;
                supplied_preview = Some(StampAssetPreview {
                    bytes: Some(preview.bytes),
                    mime_type: Some(preview.mime_type.unwrap_or_else(|| "image/png".to_string())),
                });
            }

            if let Some(target) =
                target_library.as_ref().filter(|l| l.storage == StampLibraryStorage::CanonicalPdf)
            {
                if !is_pdf {
                    return Err(Error::engine(
                        "InvalidArg",
                        "[stamp] raster assets cannot be appended to a canonical PDF library",
                    ));
                }
                let (mutating, target_id) = (this.clone(), target.id.clone());
                let source = resolved.bytes;
                return adopt(this.mutate_library(&target.id, move || {
                    Promise::spawn(async move {
                        mutating.append_canonical(&target_id, input, source, supplied_preview).await
                    })
                }))
                .await;
            }

            let size = input.size.or(meta.size.map(|(width, height)| Size { width, height }));
            let Some(size) = size else {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] a PDF asset needs `size` (its page size in points) — PDF bytes carry no sniffable dimensions",
                ));
            };
            let mut preview = supplied_preview;
            if preview.is_none() && !is_pdf {
                preview = Some(StampAssetPreview {
                    bytes: Some(resolved.bytes.clone()),
                    mime_type: Some(meta.mime_type.to_string()),
                });
            }
            let format = if is_pdf {
                StampAssetFormat::Pdf
            } else if meta.mime_type == "image/png" {
                StampAssetFormat::Png
            } else {
                StampAssetFormat::Jpeg
            };
            let loose = LooseAsset { input, size, format, bytes: resolved.bytes, preview };
            let Some(target) = target_library else {
                let library_id = this.create_loose_library(&loose.input.name);
                return Ok(this.commit_loose_asset(&library_id, &loose));
            };
            let (mutating, target_id) = (this.clone(), target.id.clone());
            adopt(this.mutate_library(&target.id, move || {
                Promise::spawn(async move {
                    let live = mutating.read("libraries")?.libraries.get(&target_id).cloned();
                    match live {
                        Some(live) if live.storage == StampLibraryStorage::Loose => {
                            Ok(mutating.commit_loose_asset(&live.id, &loose))
                        }
                        _ => Err(Error::engine(
                            "NotFound",
                            format!("[stamp] loose library '{target_id}' no longer exists"),
                        )),
                    }
                })
            }))
            .await
        })
    }

    /// `commitLooseAsset(libraryId)`.
    fn commit_loose_asset(&self, library_id: &str, loose: &LooseAsset) -> String {
        let asset = StampAsset {
            id: uid("stamp"),
            library_id: library_id.to_string(),
            kind: loose.input.kind.unwrap_or(StampAssetKind::Stamp),
            name: loose.input.name.clone(),
            size: loose.size,
            format: loose.format,
            page_object_number: None,
            subject: loose.input.subject.clone(),
            categories: loose.input.categories.clone(),
        };
        let binary =
            AssetBinary { bytes: Some(loose.bytes.clone()), preview: loose.preview.clone() };
        self.0.binaries.borrow_mut().insert(asset.id.clone(), binary);
        let id = asset.id.clone();
        self.ctx().dispatch(StampAction::AssetAdded { asset });
        id
    }

    /// The canonical-library mutation of `addAsset`.
    async fn append_canonical(
        &self,
        target_id: &str,
        input: AddAssetInput,
        source: Vec<u8>,
        supplied_preview: Option<StampAssetPreview>,
    ) -> Result<String, Error> {
        let live = self.read("libraries")?.libraries.get(target_id).cloned();
        let Some(live) = live.filter(|l| l.storage == StampLibraryStorage::CanonicalPdf) else {
            return Err(Error::engine(
                "NotFound",
                format!("[stamp] canonical library '{target_id}' no longer exists"),
            ));
        };
        let Some(canonical) = self.0.library_binaries.borrow().get(&live.id).cloned().flatten()
        else {
            return Err(Error::engine(
                "Unknown",
                format!("[stamp] canonical bytes are missing for library '{}'", live.id),
            ));
        };

        let asset_id = uid("stamp");
        let doc = rt::returned(self.open_asset_document(canonical, None)).await?;
        let body = async {
            let result =
                await_engine(call(&doc, "pages.insert", vec![Bytes(source).to_json()])).await?;
            let inserted = get(&result, "insertedPageObjectNumbers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if inserted.len() != 1 {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] a canonical library asset must be a single-page PDF",
                ));
            }
            let pon = inserted[0].as_u64().unwrap_or(0) as u32;
            let layout = get(&result, "layout")
                .and_then(|l| get(l, "pages"))
                .and_then(Value::as_array)
                .and_then(|pages| pages.iter().find(|p| pon_of(p) == pon).cloned());
            doc.page_index(pon)?;
            let (Some(layout), true) = (layout, doc.has("page.pieceInfo")) else {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] canonical PDF libraries need page layout and pieceInfo support",
                ));
            };
            let asset = StampAsset {
                id: asset_id,
                library_id: live.id.clone(),
                kind: input.kind.unwrap_or(StampAssetKind::Stamp),
                name: input.name.clone(),
                size: size_of(&layout),
                format: StampAssetFormat::Pdf,
                page_object_number: Some(pon),
                subject: input.subject.clone(),
                categories: input.categories.clone(),
            };
            let update = page_call(
                &doc,
                "page.pieceInfo.update",
                pon,
                vec![json!(STAMP_PIECEINFO_APP), metadata_patch(&asset)],
            );
            await_engine(update).await?;
            let bytes = await_engine(call(&doc, "pages.extract", vec![json!([pon])])).await?;
            let preview = match supplied_preview {
                Some(preview) => preview,
                None => {
                    let render = page_call(
                        &doc,
                        "page.render.image",
                        pon,
                        vec![render_options(self.preview_width(), true)],
                    );
                    image_to_preview(&await_engine(render).await?)?
                }
            };
            let canonical = await_engine(call(&doc, "download", Vec::new())).await?;
            Ok((asset, AssetBinary { bytes: bytes_of(&bytes), preview: Some(preview) }, canonical))
        }
        .await;
        let (asset, binary, canonical) = Self::close(&doc, body).await?;

        self.0.library_binaries.borrow_mut().insert(live.id.clone(), bytes_of(&canonical));
        self.0.binaries.borrow_mut().insert(asset.id.clone(), binary);
        let id = asset.id.clone();
        self.ctx().dispatch(StampAction::AssetAdded { asset });
        Ok(id)
    }

    fn drop_library(&self, id: &str) -> Result<(), Error> {
        let state = self.read("libraries")?;
        if let Some(library) = state.libraries.get(id) {
            let mut binaries = self.0.binaries.borrow_mut();
            for asset_id in &library.asset_ids {
                binaries.remove(asset_id);
            }
        }
        self.0.library_binaries.borrow_mut().remove(id);
        self.ctx().dispatch(StampAction::LibraryRemoved { library_id: id.to_string() });
        Ok(())
    }

    /// `removeAsset(id)`: a canonical page is deleted from the library PDF
    /// before the descriptor goes.
    pub fn remove_asset(&self, id: &str) -> Promise<()> {
        let this = self.clone();
        let id = id.to_string();
        Promise::spawn(async move {
            let Some(initial) = this.read("assets")?.assets.get(&id).cloned() else {
                return Ok(());
            };
            let mutating = this.clone();
            adopt(this.mutate_library(&initial.library_id, move || {
                Promise::spawn(async move { mutating.remove_now(&id).await })
            }))
            .await
        })
    }

    /// The mutation of `removeAsset`.
    async fn remove_now(&self, id: &str) -> Result<(), Error> {
        let Some(asset) = self.read("assets")?.assets.get(id).cloned() else {
            return Ok(());
        };
        let library = self.read("libraries")?.libraries.get(&asset.library_id).cloned();
        if let Some(library) = library.filter(|l| l.storage == StampLibraryStorage::CanonicalPdf) {
            if library.asset_ids.len() == 1 {
                return self.drop_library(&library.id);
            }
            let Some(pon) = asset.page_object_number else {
                return Err(Error::engine(
                    "Unknown",
                    format!("[stamp] canonical asset '{id}' has no page object number"),
                ));
            };
            let canonical = self.0.library_binaries.borrow().get(&library.id).cloned().flatten();
            let Some(canonical) = canonical else {
                return Err(Error::engine(
                    "Unknown",
                    format!("[stamp] canonical bytes are missing for library '{}'", library.id),
                ));
            };
            let doc = rt::returned(self.open_asset_document(canonical, None)).await?;
            let body = async {
                await_engine(call(&doc, "pages.delete", vec![json!([pon])])).await?;
                await_engine(call(&doc, "download", Vec::new())).await
            }
            .await;
            let rewritten = Self::close(&doc, body).await?;
            self.0.library_binaries.borrow_mut().insert(library.id.clone(), bytes_of(&rewritten));
        }
        self.0.binaries.borrow_mut().remove(id);
        self.ctx().dispatch(StampAction::AssetRemoved { asset_id: id.to_string() });
        Ok(())
    }

    /// `removeLibrary(id)`: the library and every asset in it, ordered with
    /// in-flight library mutations.
    pub fn remove_library(&self, id: &str) -> Promise<()> {
        let this = self.clone();
        let key = id.to_string();
        self.mutate_library(id, move || Promise::spawn(async move { this.drop_library(&key) }))
    }

    // ── placement ─────────────────────────────────────────────────────────────

    /// `surfaceScriptingResult(result)`.
    fn surface_scripting_result(&self, result: &FormCommitResult) {
        let scripting = self.0.config.scripting.as_ref();
        let observe = || -> Result<(), Error> {
            for effect in &result.ui_effects {
                if let Some(observer) = scripting.and_then(|s| s.on_ui_effect.as_ref()) {
                    observer(effect)?;
                }
            }
            for diagnostic in &result.diagnostics {
                if let Some(observer) = scripting.and_then(|s| s.on_diagnostic.as_ref()) {
                    observer(diagnostic)?;
                }
            }
            if let Some(error) = &result.error
                && let Some(observer) = scripting.and_then(|s| s.on_error.as_ref())
            {
                observer(error)?;
            }
            Ok(())
        };
        if let Err(error) = observe()
            && let Some(console_error) = &self.0.config.host.console_error
        {
            console_error("[stamp] scripting observer failed:", &error);
        }
    }

    /// `materializeForPlacement(documentId, asset, bin)`: a form-backed PDF
    /// evaluated on an isolated copy against the target document, then
    /// flattened, so neither the canonical library nor its reusable page
    /// extraction becomes target, user or time specific.
    async fn materialize_for_placement(
        &self,
        document_id: &str,
        asset: &StampAsset,
        bin: AssetBinary,
    ) -> Result<AssetBinary, Error> {
        let Some(scripting) = self.0.config.scripting.as_ref().filter(|s| s.enabled) else {
            return Ok(bin);
        };
        if asset.format != StampAssetFormat::Pdf {
            return Ok(bin);
        }

        let target_doc = self.ctx().document_handle(Some(document_id));
        let target_meta = self.ctx().core().documents.get(document_id).cloned();
        let (Some(target_doc), Some(target_meta)) = (target_doc, target_meta) else {
            return Err(Error::engine(
                "NotFound",
                format!("[stamp] target document '{document_id}' is not open"),
            ));
        };

        // A page extraction does not keep catalog-owned AcroForm and action
        // structures, so canonical assets evaluate from the whole library
        // PDF and extract only their page after the flatten.
        let canonical = self.0.library_binaries.borrow().get(&asset.library_id).cloned().flatten();
        let source = canonical.clone().or_else(|| bin.bytes.clone()).unwrap_or_default();
        let identity = target_doc.security().identity();
        let identity =
            (!missing(Some(&identity)) && identity != Value::Bool(false)).then_some(identity);
        let doc = rt::returned(self.open_asset_document(source, identity)).await?;
        let controller = RefCell::new(None);
        let body = async {
            let layout = await_engine(call(&doc, "pages.list", Vec::new())).await?;
            let pages = get(&layout, "pages").and_then(Value::as_array).cloned().unwrap_or_default();
            let selected = match asset.page_object_number {
                None => {
                    if get(&layout, "pageCount").and_then(Value::as_f64) == Some(1.0) {
                        pages.first().cloned()
                    } else {
                        None
                    }
                }
                Some(pon) => pages.iter().find(|p| pon_of(p) == pon).cloned(),
            };
            let Some(selected) = selected else {
                let message = if canonical.is_some() {
                    format!(
                        "[stamp] canonical page {} no longer exists",
                        asset.page_object_number.map_or("unknown".to_string(), |p| p.to_string())
                    )
                } else {
                    "[stamp] a loose dynamic stamp asset must contain exactly one page".to_string()
                };
                return Err(Error::engine("InvalidArg", message));
            };
            let selected_pon = get(&selected, "pageObjectNumber").cloned();
            let snapshot = await_engine(call(&doc, "forms.list", Vec::new())).await?;
            let fields = get(&snapshot, "fields").and_then(Value::as_array).cloned().unwrap_or_default();
            let has_selected_page_field = fields.iter().any(|field| {
                get(field, "widgets").and_then(Value::as_array).is_some_and(|widgets| {
                    widgets
                        .iter()
                        .any(|w| strict_eq(get(w, "pageObjectNumber"), selected_pon.as_ref()))
                })
            });
            if !has_selected_page_field {
                return Ok(bin.clone());
            }
            if !doc.has("pages.flatten") || !doc.has("pages.extract") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] dynamic PDF stamps need an asset engine with pages.flatten and pages.extract",
                ));
            }

            // The detached copy gets its own standalone realm, never the
            // viewer document's shared host.
            let scripted = create_form_scripting_controller(FormScriptingControllerOptions::Config {
                doc: doc.clone(),
                document: Rc::new(move || Some(target_meta.clone())),
                config: scripting.form.clone(),
            });
            *controller.borrow_mut() = Some(scripted.clone());
            let result = scripted.recalculate().await?;
            self.surface_scripting_result(&result);
            if result.status == FormCommitStatus::Failed {
                let message = result
                    .error
                    .as_ref()
                    .map_or("native form effect failed".to_string(), |e| e.message.clone());
                return Err(Error::engine(
                    "Unknown",
                    format!("[stamp] dynamic stamp scripting failed: {message}"),
                ));
            }

            let pon = pon_of(&selected);
            let flattened =
                await_engine(call(&doc, "pages.flatten", vec![json!([pon]), json!("display")]))
                    .await?;
            let results =
                get(&flattened, "results").and_then(Value::as_array).cloned().unwrap_or_default();
            if let Some(failed) = results.iter().find(|r| {
                matches!(str_of(r, "status"), Some("failed") | Some("skipped"))
            }) {
                return Err(Error::engine(
                    "Unknown",
                    format!(
                        "[stamp] dynamic stamp flatten failed for page {}",
                        js_string(get(failed, "pageObjectNumber"))
                    ),
                ));
            }

            let bytes = await_engine(call(&doc, "pages.extract", vec![json!([pon])])).await?;
            doc.page_index(pon)?;
            let render = page_call(
                &doc,
                "page.render.image",
                pon,
                vec![render_options(self.preview_width(), false)],
            );
            let image = await_engine(render).await?;
            Ok(AssetBinary { bytes: bytes_of(&bytes), preview: Some(image_to_preview(&image)?) })
        }
        .await;
        if let Some(controller) = controller.borrow().as_ref() {
            controller.dispose();
        }
        Self::close(&doc, body).await
    }

    /// `armAsset(documentId, assetId, opts?)`: the next click on that
    /// document's pages places the asset, through its annotation plugin.
    pub fn arm_asset(
        &self,
        document_id: &str,
        asset_id: &str,
        opts: ArmAssetOptions,
    ) -> Promise<()> {
        let this = self.clone();
        let (document_id, asset_id) = (document_id.to_string(), asset_id.to_string());
        Promise::spawn(async move {
            let asset = this.read("assets")?.assets.get(&asset_id).cloned();
            let bin = this.0.binaries.borrow().get(&asset_id).cloned();
            let (Some(asset), Some(bin)) = (asset, bin) else {
                return Err(Error::engine(
                    "NotFound",
                    format!("[stamp] unknown asset '{asset_id}'"),
                ));
            };
            let placement =
                rt::returned(this.materialize_for_placement(&document_id, &asset, bin)).await?;
            let annotation = this.ctx().for_document(&ANNOTATION, &document_id)?;
            let preview = placement.preview.map(|preview| {
                let mut payload = serde_json::Map::new();
                payload.insert(
                    "data".into(),
                    preview.bytes.map_or_else(undefined, |b| Bytes(b).to_json()),
                );
                if let Some(mime_type) = preview.mime_type {
                    payload.insert("mimeType".into(), json!(mime_type));
                }
                Value::Object(payload)
            });
            annotation
                .arm_stamp(StampToolInput {
                    source: placement.bytes.map_or_else(undefined, |b| Bytes(b).to_json()),
                    target_width: opts.target_width,
                    preview,
                    intrinsic_size: Some(asset.size),
                })
                .await
        })
    }

    /// `disarm(documentId)`.
    pub fn disarm(&self, document_id: &str) -> Result<(), Error> {
        self.ctx().for_document(&ANNOTATION, document_id)?.disarm_stamp();
        Ok(())
    }

    // ── selectors ─────────────────────────────────────────────────────────────

    /// Libraries in display order.
    pub fn libraries(&self) -> Vec<StampLibrary> {
        let state = self.ctx().state();
        state.library_order.iter().filter_map(|id| state.libraries.get(id).cloned()).collect()
    }

    pub fn library(&self, id: &str) -> Option<StampLibrary> {
        self.ctx().state().libraries.get(id).cloned()
    }

    /// Assets of one library in library order, or every asset when
    /// `library_id` is `None` or empty.
    pub fn assets(&self, library_id: Option<&str>) -> Vec<StampAsset> {
        let state = self.ctx().state();
        let listed = |library: &StampLibrary| {
            library
                .asset_ids
                .iter()
                .filter_map(|id| state.assets.get(id).cloned())
                .collect::<Vec<_>>()
        };
        match library_id.filter(|id| !id.is_empty()) {
            Some(id) => state.libraries.get(id).map(listed).unwrap_or_default(),
            None => state
                .library_order
                .iter()
                .flat_map(|id| state.libraries.get(id).map(listed).unwrap_or_default())
                .collect(),
        }
    }

    pub fn asset(&self, id: &str) -> Option<StampAsset> {
        self.ctx().state().assets.get(id).cloned()
    }

    /// The asset's paintable preview, or `None` while none is cached.
    pub fn asset_preview(&self, id: &str) -> Option<StampAssetPreview> {
        self.0.binaries.borrow().get(id).and_then(|b| b.preview.clone())
    }

    /// Placement bytes.
    pub fn asset_bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.0.binaries.borrow().get(id).and_then(|b| b.bytes.clone())
    }

    /// Canonical library PDF bytes, or `None` for a loose library or an
    /// unknown id.
    pub fn library_bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.0.library_binaries.borrow().get(id).cloned().flatten()
    }
}

/// The loose half of `addAsset`, carried into its mutation.
#[derive(Clone)]
struct LooseAsset {
    input: AddAssetInput,
    size: Size,
    format: StampAssetFormat,
    bytes: Vec<u8>,
    preview: Option<StampAssetPreview>,
}
