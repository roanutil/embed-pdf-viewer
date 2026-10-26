//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the
//! function table. It mirrors
//! `tooling/plugin-conformance/src/suites/stamp-current-fixtures.ts` and
//! `stamp-mutation-fixtures.ts`:
//!
//! - `stamp-current` is `stampPlugin(registered)` over a typed local port of
//!   the kernel's engine. Each case starts the module's id counter at zero,
//!   as the generator's private copy of the controller module does
//!   (`reset_id_sequence`).
//! - `Math.random` answers `((n % 62) + 0.5) / 62` for its nth call of a
//!   case; `console.error` and `console.warn` lines starting `[stamp]` log as
//!   callbacks of those names.
//! - `config.assetEngine` names the local port (`'instance'`), a function
//!   that makes it (`'factory'`, `'async-factory'`), one that throws or
//!   rejects, or none (the kernel's own engine). A factory call logs
//!   `assetEngine`.
//! - `stamp-current-persistence` and `stamp-mutation-persistence` drive the
//!   persistence helpers over a memory store.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::engine::{DocumentHandle, Engine, EngineCall};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::types::PluginScope;
use epdf_kernel::{
    AnyPlugin, AnyToken, Error, Token, Unsubscribe, define, is_undefined, undefined,
};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::StampCapability;
use crate::model::*;
use crate::persistence::{
    MemoryStampStore, PersistOptions, StampLibraryStore, StoredLibrary, memory_stamp_store,
    persist_stamp_libraries, restore_stamp_libraries,
};
use crate::read::catalog::preview_json;
use crate::services::asset_engine::reset_id_sequence;
use crate::services::settings::register;
use crate::write::libraries::bytes_json;

/// The adapter.
pub struct Stamp;

/// The capability's member order: `composeApi` over the settings, the
/// catalog, the library, asset and placement writes, then the events.
pub const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "listLibraries",
    "getLibrary",
    "listAssets",
    "getAsset",
    "getAssetPreview",
    "renderAssetPreview",
    "readAssetBytes",
    "exportLibrary",
    "canPlace",
    "canCreateFromAnnotations",
    "canImport",
    "createLibrary",
    "updateLibrary",
    "importLibrary",
    "deleteLibrary",
    "createAsset",
    "createAssetFromAnnotations",
    "updateAsset",
    "deleteAsset",
    "moveAsset",
    "duplicateAsset",
    "armAsset",
    "disarm",
    "getArmedAsset",
    "placeAsset",
    "placeAssetOnPages",
    "onLibraryChanged",
    "onLibraryCreated",
    "onLibraryUpdated",
    "onLibraryDeleted",
    "onAssetCreated",
    "onAssetUpdated",
    "onAssetDeleted",
    "onArmChanged",
];

// ── the host ──────────────────────────────────────────────────────────────────

thread_local! {
    /// Whether a case has the stamp host installed (`release` in
    /// `stamp-current-fixtures.ts`).
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// The installed host, put back as it was when the case's kernel (which
/// holds the plugin, and with it this guard) is dropped: `restoreStampHost`
/// on `onCaseEnd`.
struct HostGuard {
    sink: Option<crate::console::ConsoleSink>,
    random: Option<crate::console::RandomSource>,
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        crate::console::set_sink(self.sink.take());
        crate::console::set_random(self.random.take());
        INSTALLED.with(|i| i.set(false));
    }
}

/// `installStampHost(env)`: this case's `Math.random` and `[stamp]`
/// console lines. Other lines go to the console that was there. Once per
/// case, as the generator refuses a second install.
fn install_host(h: &Harness) -> Result<HostGuard, Error> {
    if INSTALLED.with(|i| i.replace(true)) {
        return Err(Error::new("[stamp-current] install once per case"));
    }
    let calls = Rc::new(Cell::new(0u64));
    let random = crate::console::set_random(Some(Rc::new(move || {
        let n = calls.get();
        calls.set(n + 1);
        ((n % 62) as f64 + 0.5) / 62.0
    })));
    let log = h.clone();
    let previous: Rc<RefCell<Option<crate::console::ConsoleSink>>> = Rc::default();
    let fallback = previous.clone();
    let sink = crate::console::set_sink(Some(Rc::new(move |level, args| {
        let stamped =
            args.first().and_then(Value::as_str).is_some_and(|s| s.starts_with("[stamp]"));
        if !stamped {
            if let Some(previous) = fallback.borrow().clone() {
                previous(level, args);
            }
            return;
        }
        match level {
            "error" => log.log("console.error", args),
            _ => log.log("console.warn", args),
        }
    })));
    *previous.borrow_mut() = sink.clone();
    Ok(HostGuard { sink, random })
}

/// `localAssetEngine(engine)`: a typed local port over the table engine.
/// `open` is `AbortablePromise.run(async (signal) => ...)`: the inner
/// function awaits the engine's own `AbortablePromise`, checks that the
/// document is local, and the port's promise settles a reaction later.
struct LocalPort {
    engine: Rc<dyn Engine>,
}

impl Engine for LocalPort {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let opening = self.engine.open(input, options);
        let abort = opening.abort_handle();
        let run: Promise<Rc<dyn DocumentHandle>> = Promise::spawn(async move {
            let doc = opening.await?;
            if !doc.is_local() {
                return Err(Error::new("[stamp-current] asset document must be local"));
            }
            Ok(doc)
        });
        // `fn(signal).then(resolve, reject)`
        let (port, resolver) = Promise::pending();
        rt::spawn(async move { resolver.settle(run.reaction().await) });
        port.with_abort(move |reason| {
            if let Some(abort) = &abort {
                abort(reason);
            }
        })
    }

    fn destroy(&self) -> EngineCall<()> {
        self.engine.destroy()
    }
}

/// `assetEngine(input, engine, env)`.
fn asset_engine(spec: &Value, engine: Rc<dyn Engine>, h: &Harness) -> StampAssetEngine {
    let local = LocalEngine::new(Rc::new(LocalPort { engine }));
    if spec.as_str() == Some("instance") {
        return StampAssetEngine::Engine(local);
    }
    let (spec, h) = (spec.clone(), h.clone());
    StampAssetEngine::Factory(Rc::new(move || {
        h.log("assetEngine", Vec::new());
        match spec.as_str() {
            Some("factory") => Ok(MadeAssetEngine::Ready(local.clone())),
            Some("async-factory") => Ok(MadeAssetEngine::Pending(Promise::resolved(local.clone()))),
            _ => match (spec.get("throws"), spec.get("rejects")) {
                (Some(error), _) => Err(Error::from_spec(error)),
                (None, Some(error)) => {
                    Ok(MadeAssetEngine::Pending(Promise::rejected(Error::from_spec(error))))
                }
                _ => Err(Error::new("[stamp harness] unknown assetEngine input")),
            },
        }
    }))
}

/// The engine the kernel binds at `create`, as the generator's forwarding
/// engine reads it.
struct Forwarding(Rc<RefCell<Option<Rc<dyn Engine>>>>);

impl Forwarding {
    fn bound(&self) -> Rc<dyn Engine> {
        self.0.borrow().clone().expect("[stamp-current] engine used before create")
    }
}

impl Engine for Forwarding {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        self.bound().open(input, options)
    }
    fn destroy(&self) -> EngineCall<()> {
        self.bound().destroy()
    }
}

fn stamp_current(config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
    reset_id_sequence();
    let host = Rc::new(install_host(h)?);
    let bound: Rc<RefCell<Option<Rc<dyn Engine>>>> = Rc::default();
    let forwarding: Rc<dyn Engine> = Rc::new(Forwarding(bound.clone()));
    let input = |k: &str| config.get(k).filter(|v| !is_undefined(v));
    let registered = StampConfig {
        asset_engine: input("assetEngine")
            .filter(|v| !v.is_null())
            .map(|spec| Some(asset_engine(spec, forwarding, h))),
        preview_width: input("previewWidth").and_then(Value::as_f64),
        dynamic: input("dynamic").and_then(Value::as_bool),
    };
    let (table, registered) = register(&registered);
    Ok(crate::stamp_plugin_creating(
        table,
        Some(registered),
        Rc::new(move |cx, table| {
            // The plugin holds the host for as long as the case's kernel.
            let _ = &host;
            *bound.borrow_mut() = cx.engine();
            crate::create_stamp_controller(cx, table)
        }),
    ))
}

// ── persistence drivers ───────────────────────────────────────────────────────

/// The memory store, with the calls `config.fail` names rejecting.
struct FailingStore {
    memory: MemoryStampStore,
    fail: Value,
}

impl FailingStore {
    fn failure(&self, call: &str) -> Option<Error> {
        self.fail.get(call).and_then(Value::as_str).map(Error::new)
    }
}

impl StampLibraryStore for FailingStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>> {
        self.failure("list").map_or_else(|| self.memory.list(), Promise::rejected)
    }
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        self.failure("put").map_or_else(|| self.memory.put(id, bytes), Promise::rejected)
    }
    fn delete(&self, id: &str) -> Promise<()> {
        self.failure("delete").map_or_else(|| self.memory.delete(id), Promise::rejected)
    }
}

/// `stamp-mutation-persistence`'s store: `put` logs, and holds library
/// `a` until the case settles `put:a`.
struct HeldStore {
    memory: MemoryStampStore,
    h: Harness,
}

impl StampLibraryStore for HeldStore {
    fn list(&self) -> Promise<Vec<StoredLibrary>> {
        self.memory.list()
    }
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        self.h.log("store.put", vec![json!(id), bytes_json(bytes.clone())]);
        let held = (id == "a").then(|| self.h.defer_unit("put:a"));
        let (memory, id) = (self.memory.clone(), id.to_string());
        Promise::spawn(async move {
            if let Some(held) = held {
                held.await?;
            }
            memory.put(&id, bytes).await
        })
    }
    fn delete(&self, id: &str) -> Promise<()> {
        self.memory.delete(id)
    }
}

/// A persistence driver's capability.
#[derive(Clone)]
pub struct PersistenceDriver {
    stamp: Rc<RefCell<Option<StampCapability>>>,
    memory: MemoryStampStore,
    store: Rc<dyn StampLibraryStore>,
    stop: Rc<RefCell<Option<Unsubscribe>>>,
    /// `stamp-mutation-persistence`: only `persist`, `rows` and `stop`.
    mutation: bool,
}

fn persistence_plugin(config: &Value, h: &Harness, mutation: bool) -> AnyPlugin {
    let token: Token<PersistenceDriver> = Token::from_any(h.token("stamp-persistence", None));
    let fail = config.get("fail").cloned().unwrap_or(Value::Null);
    let h = h.clone();
    let id = if mutation { "stamp-mutation-persistence" } else { "stamp-persistence" };
    // `definePlugin` with no state: no slice.
    define::<Value, Value>(id)
        .scope(PluginScope::Workspace)
        .initial_state(undefined)
        .requires(crate::STAMP.any())
        .create(&token, move |cx| {
            let memory = memory_stamp_store();
            let store: Rc<dyn StampLibraryStore> = if mutation {
                Rc::new(HeldStore { memory: memory.clone(), h: h.clone() })
            } else {
                Rc::new(FailingStore { memory: memory.clone(), fail: fail.clone() })
            };
            let stop: Rc<RefCell<Option<Unsubscribe>>> = Rc::default();
            let cleanup = stop.clone();
            cx.defer(Box::new(move || {
                if let Some(off) = cleanup.borrow_mut().take() {
                    off.call();
                }
                Ok(None)
            }));
            let stamp = Rc::new(RefCell::new(cx.get(&crate::STAMP).ok()));
            Ok((PersistenceDriver { stamp, memory, store, stop, mutation }, None))
        })
        .build()
}

fn rows_json(rows: &[StoredLibrary]) -> Value {
    Value::Array(
        rows.iter().map(|r| json!({ "id": r.id, "bytes": bytes_json(r.bytes.clone()) })).collect(),
    )
}

fn call_persistence(
    driver: &PersistenceDriver,
    method: &str,
    args: &[Value],
) -> Result<Outcome, Error> {
    let stamp = || {
        driver
            .stamp
            .borrow()
            .clone()
            .ok_or_else(|| Error::new("[stamp harness] no stamp capability"))
    };
    let stop = |driver: &PersistenceDriver| {
        if let Some(off) = driver.stop.borrow_mut().take() {
            off.call();
        }
    };
    Ok(match (method, driver.mutation) {
        ("seed", false) => {
            let rows: Vec<(String, Vec<u8>)> = args
                .first()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|row| {
                    let id = row.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
                    let bytes = row.get("bytes").and_then(crate::services::asset_engine::bytes_of);
                    (id, bytes.unwrap_or_default())
                })
                .collect();
            let memory = driver.memory.clone();
            Outcome::promise_unit(Promise::spawn(async move {
                for (id, bytes) in rows {
                    memory.put(&id, bytes).await?;
                }
                Ok(())
            }))
        }
        ("rows", _) => Outcome::Promise(driver.memory.list().convert(|rows| Ok(rows_json(rows)))),
        ("restore", false) => Outcome::Promise(
            restore_stamp_libraries(&stamp()?, driver.store.clone()).convert(|ids| Ok(json!(ids))),
        ),
        ("persist", _) => {
            stop(driver);
            let except = args
                .first()
                .filter(|v| !is_undefined(v) && !v.is_null())
                .and_then(|o| o.get("except"))
                .and_then(Value::as_array)
                .map(|e| e.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let off =
                persist_stamp_libraries(&stamp()?, driver.store.clone(), PersistOptions { except });
            *driver.stop.borrow_mut() = Some(off);
            Outcome::undefined()
        }
        ("stop", _) => {
            stop(driver);
            Outcome::undefined()
        }
        (other, _) => {
            Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
        }
    })
}

// ── reading arguments ─────────────────────────────────────────────────────────

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null() && !is_undefined(v))
}

/// An id argument as the property key JavaScript would look up.
fn key(args: &[Value], index: usize) -> String {
    match args.get(index) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => epdf_kernel::js::number_to_string(n.as_f64().unwrap_or(0.0)),
        _ => "undefined".to_string(),
    }
}

/// An optional document id argument: `undefined` takes the default.
fn optional_key(args: &[Value], index: usize) -> Option<String> {
    args.get(index).filter(|v| !is_undefined(v)).map(|_| key(args, index))
}

fn string(value: Option<&Value>) -> Option<String> {
    present(value).and_then(Value::as_str).map(str::to_string)
}

fn decoded<T: serde::de::DeserializeOwned>(value: Option<&Value>) -> Result<Option<T>, Error> {
    present(value).map(from_value).transpose()
}

fn member<'a>(object: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    present(object).and_then(|o| o.get(key)).filter(|v| !is_undefined(v))
}

fn signal_of(_options: Option<&Value>) -> Option<epdf_kernel::signal::AbortSignal> {
    None
}

fn page_target(value: &Value) -> PageTarget {
    match value.as_f64() {
        Some(index) => PageTarget::Index(index),
        None => match PageRef::from_json(value) {
            Some(page) => PageTarget::Ref(page),
            None => PageTarget::Index(f64::NAN),
        },
    }
}

fn mark_source(value: &Value) -> Result<MarkSource, Error> {
    let m = |k: &str| value.get(k).filter(|v| !is_undefined(v));
    Ok(match m("kind").and_then(Value::as_str) {
        Some("ink") => MarkSource::Ink {
            strokes: m("strokes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|stroke| {
                    stroke
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|p| {
                            let n = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                            (n("x"), n("y"))
                        })
                        .collect()
                })
                .collect(),
            color: string(m("color")),
            stroke_width: present(m("strokeWidth")).and_then(Value::as_f64),
        },
        Some("text") => MarkSource::Text {
            text: string(m("text")).unwrap_or_default(),
            font_family: string(m("fontFamily")).unwrap_or_default(),
            color: string(m("color")),
            font_size: present(m("fontSize")).and_then(Value::as_f64),
        },
        Some("image") => {
            MarkSource::Image { source: m("source").cloned().unwrap_or_else(undefined) }
        }
        Some("pdf") => MarkSource::Pdf {
            source: m("source").cloned().unwrap_or_else(undefined),
            page_index: present(m("pageIndex")).and_then(Value::as_f64),
        },
        other => return Err(Error::new(format!("[stamp harness] unknown mark kind {other:?}"))),
    })
}

fn add_asset_input(value: Option<&Value>) -> Result<AddAssetInput, Error> {
    let input = value.cloned().unwrap_or(Value::Null);
    let m = |k: &str| member(Some(&input), k);
    Ok(AddAssetInput {
        library_id: m("libraryId").and_then(Value::as_str).map(str::to_string),
        name: m("name").and_then(Value::as_str).map(str::to_string),
        label: m("label").and_then(Value::as_str).map(str::to_string),
        kind: decoded(m("kind"))?,
        subject: string(m("subject")),
        categories: decoded(m("categories"))?,
        source: m("source").cloned(),
        mark: m("mark").map(mark_source).transpose()?,
        preview: m("preview").cloned(),
        size: decoded(m("size"))?,
    })
}

fn import_options(value: Option<&Value>, h: &Harness) -> Result<ImportLibraryOptions, Error> {
    let asset_name = member(value, "assetName").map(|spec| {
        let callback = h.callback(spec);
        Rc::new(move |index: f64| match callback.call(vec![json!(index)]) {
            Answer::Throws(error) => Err(error),
            Answer::Returns(value) => Ok(value.as_str().map(str::to_string)),
            _ => Ok(None),
        }) as Rc<dyn Fn(f64) -> Result<Option<String>, Error>>
    });
    Ok(ImportLibraryOptions {
        name: string(member(value, "name")),
        categories: decoded(member(value, "categories"))?,
        kind: decoded(member(value, "kind"))?,
        library_kind: string(member(value, "libraryKind")),
        asset_name,
        signal: signal_of(value),
    })
}

/// A promise whose value prints as `map` does, settling in its turn.
fn json_promise<T: Clone + 'static>(
    promise: Promise<T>,
    map: impl Fn(&T) -> Value + 'static,
) -> Outcome {
    Outcome::Promise(promise.convert(move |value| Ok(map(value))))
}

fn wrapped<T: Clone + Serialize + 'static>(promise: Promise<T>, member: &'static str) -> Outcome {
    json_promise(promise, move |value| json!({ member: to_value(value) }))
}

fn out<T: Serialize>(value: Option<T>) -> Outcome {
    Outcome::Returned(value.map_or(Value::Null, |v| to_value(&v)))
}

/// An event hook: subscribe a scripted callback, return its unsubscribe.
fn hook<E: Serialize + 'static>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| match callback.call(vec![to_value(event)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn document_options(value: Option<&Value>) -> StampDocumentOptions {
    StampDocumentOptions {
        document_id: member(value, "documentId")
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)),
        signal: signal_of(value),
    }
}

fn operation(value: Option<&Value>) -> OperationOptions {
    OperationOptions { signal: signal_of(value) }
}

fn call_stamp(
    cap: &StampCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let arg = |i: usize| args.get(i).filter(|v| !is_undefined(v));
    Ok(match method {
        // The asset engine prints as its handle in the store,
        // `{"$assetEngine": n}`, where the TypeScript prints the engine.
        "getSettings" => Outcome::Returned((*cap.settings_json()).clone()),
        "updateSettings" => {
            let changes = arg(0);
            let asset_engine = match member(changes, "assetEngine") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(other) => {
                    return Err(Error::new(format!(
                        "[stamp harness] an asset engine a vector can name is null, got {other}"
                    )));
                }
            };
            cap.update_settings(&StampConfig {
                asset_engine,
                preview_width: member(changes, "previewWidth").and_then(Value::as_f64),
                dynamic: member(changes, "dynamic").and_then(Value::as_bool),
            });
            Outcome::undefined()
        }
        "onSettingsChanged" => {
            let callback = h.callback(arg(0).unwrap_or(&Value::Null));
            let off = cap.on_settings_changed_json(move |settings, changed| {
                match callback.call(vec![json!({ "settings": settings, "changed": changed })]) {
                    Answer::Throws(error) => Err(error),
                    _ => Ok(()),
                }
            });
            Outcome::Returned(h.register_fn(Rc::new(move |_| {
                off.call();
                Outcome::undefined()
            })))
        }
        "resetSettings" => {
            cap.reset_settings();
            Outcome::undefined()
        }
        "listLibraries" => {
            let filter = present(arg(0)).map(|f| StampLibraryFilter {
                kind: match f.get("kind") {
                    Some(Value::String(kind)) => Some(vec![kind.clone()]),
                    Some(Value::Array(kinds)) => {
                        Some(kinds.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    }
                    _ => None,
                },
            });
            Outcome::Returned(to_value(&cap.list_libraries(filter.as_ref())))
        }
        "getLibrary" => out(cap.get_library(&key(args, 0))),
        "listAssets" => {
            let filter = match present(arg(0)) {
                Some(f) => Some(StampAssetFilter {
                    library_id: member(Some(f), "libraryId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    kind: decoded(member(Some(f), "kind"))?,
                    category: member(Some(f), "category")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }),
                None => None,
            };
            Outcome::Returned(to_value(&cap.list_assets(filter.as_ref())))
        }
        "getAsset" => out(cap.get_asset(&key(args, 0))),
        "getAssetPreview" => {
            Outcome::Returned(preview_json(cap.get_asset_preview(&key(args, 0)).as_ref()))
        }
        "renderAssetPreview" => {
            let width = member(arg(1), "width").and_then(Value::as_f64).unwrap_or(f64::NAN);
            json_promise(cap.render_asset_preview(&key(args, 0), width, operation(arg(1))), |p| {
                preview_json(p.as_ref())
            })
        }
        "readAssetBytes" => {
            Outcome::Returned(cap.read_asset_bytes(&key(args, 0)).map_or(Value::Null, bytes_json))
        }
        "exportLibrary" => {
            json_promise(cap.export_library(&key(args, 0), operation(arg(1))), |b| {
                bytes_json(b.clone())
            })
        }
        "canPlace" => Outcome::Returned(json!(cap.can_place(optional_key(args, 0).as_deref()))),
        "canCreateFromAnnotations" => Outcome::Returned(json!(
            cap.can_create_from_annotations(optional_key(args, 0).as_deref())
        )),
        "canImport" => Outcome::Returned(json!(cap.can_import())),
        "createLibrary" => {
            let o = arg(1);
            let options = CreateLibraryOptions {
                id: member(o, "id").and_then(Value::as_str).map(str::to_string),
                kind: string(member(o, "kind")),
                categories: decoded(member(o, "categories"))?,
                signal: signal_of(o),
            };
            wrapped(cap.create_library(&key(args, 0), options), "library")
        }
        "updateLibrary" => {
            let p = arg(1);
            let patch = StampLibraryPatch {
                name: member(p, "name").and_then(Value::as_str).map(str::to_string),
                categories: decoded(member(p, "categories"))?,
            };
            wrapped(cap.update_library(&key(args, 0), patch, operation(arg(2))), "library")
        }
        "importLibrary" => {
            let options = import_options(arg(1), h)?;
            wrapped(
                cap.import_library(arg(0).cloned().unwrap_or_else(undefined), options),
                "library",
            )
        }
        "deleteLibrary" => {
            Outcome::promise_unit(cap.delete_library(&key(args, 0), operation(arg(1))))
        }
        "createAsset" => {
            wrapped(cap.create_asset(add_asset_input(arg(0))?, operation(arg(1))), "asset")
        }
        "createAssetFromAnnotations" => {
            let page = page_target(arg(0).unwrap_or(&Value::Null));
            let refs = arg(1).and_then(Value::as_array).cloned().unwrap_or_default();
            wrapped(
                cap.create_asset_from_annotations(
                    page,
                    refs,
                    add_asset_input(arg(2))?,
                    document_options(arg(3)),
                ),
                "asset",
            )
        }
        "updateAsset" => {
            let p = arg(1);
            let subject = match member(p, "subject") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(s) => Some(s.as_str().map(str::to_string)),
            };
            let patch = StampAssetPatch {
                label: member(p, "label").and_then(Value::as_str).map(str::to_string),
                subject,
                categories: decoded(member(p, "categories"))?,
            };
            wrapped(cap.update_asset(&key(args, 0), patch, operation(arg(2))), "asset")
        }
        "deleteAsset" => Outcome::promise_unit(cap.delete_asset(&key(args, 0), operation(arg(1)))),
        "moveAsset" => {
            let to = member(arg(1), "libraryId").and_then(Value::as_str).unwrap_or("undefined");
            wrapped(cap.move_asset(&key(args, 0), to, operation(arg(2))), "asset")
        }
        "duplicateAsset" => {
            let label = member(arg(1), "label").and_then(Value::as_str).map(str::to_string);
            wrapped(cap.duplicate_asset(&key(args, 0), label, operation(arg(1))), "asset")
        }
        "armAsset" => {
            let o = arg(1);
            let options = StampArmOptions {
                document_id: document_options(o).document_id,
                target_width: member(o, "targetWidth").and_then(Value::as_f64),
                signal: signal_of(o),
            };
            Outcome::promise_unit(cap.arm_asset(&key(args, 0), options))
        }
        "disarm" => {
            cap.disarm(optional_key(args, 0).as_deref());
            Outcome::undefined()
        }
        "getArmedAsset" => out(cap.get_armed_asset(optional_key(args, 0).as_deref())),
        "placeAsset" => Outcome::Promise(cap.place_asset(
            &key(args, 0),
            arg(1).cloned().unwrap_or_else(undefined),
            document_options(arg(2)),
        )),
        "placeAssetOnPages" => {
            let pages = match arg(1) {
                Some(Value::String(all)) if all == "all" => PageSelection::All,
                other => PageSelection::Pages(
                    other
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(page_target)
                        .collect(),
                ),
            };
            json_promise(
                cap.place_asset_on_pages(
                    &key(args, 0),
                    pages,
                    arg(2).cloned().unwrap_or_else(undefined),
                    document_options(arg(3)),
                ),
                to_value,
            )
        }
        "onLibraryChanged" => hook(h, arg(0), |f| cap.on_library_changed(f)),
        "onLibraryCreated" => hook(h, arg(0), |f| cap.on_library_created(f)),
        "onLibraryUpdated" => hook(h, arg(0), |f| cap.on_library_updated(f)),
        "onLibraryDeleted" => hook(h, arg(0), |f| cap.on_library_deleted(f)),
        "onAssetCreated" => hook(h, arg(0), |f| cap.on_asset_created(f)),
        "onAssetUpdated" => hook(h, arg(0), |f| cap.on_asset_updated(f)),
        "onAssetDeleted" => hook(h, arg(0), |f| cap.on_asset_deleted(f)),
        "onArmChanged" => hook(h, arg(0), |f| cap.on_arm_changed(f)),
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Stamp {
    fn factories(&self) -> Vec<&'static str> {
        vec!["stamp-current", "stamp-current-persistence", "stamp-mutation-persistence"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        Ok(match factory {
            "stamp-current-persistence" => persistence_plugin(config, h, false),
            "stamp-mutation-persistence" => persistence_plugin(config, h, true),
            _ => stamp_current(config, h)?,
        })
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![crate::STAMP.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        if let Some(driver) = capability.downcast_ref::<PersistenceDriver>() {
            return Some(call_persistence(driver, method, &args).unwrap_or_else(Outcome::Threw));
        }
        let cap = capability.downcast_ref::<StampCapability>()?;
        Some(call_stamp(cap, method, &args, h).unwrap_or_else(Outcome::Threw))
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        if let Some(driver) = capability.downcast_ref::<PersistenceDriver>() {
            return Some(if driver.mutation {
                vec!["persist", "rows", "stop"]
            } else {
                vec!["seed", "rows", "restore", "persist", "stop"]
            });
        }
        capability.downcast_ref::<StampCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn state_arg(a: &[Value], index: usize) -> Result<Rc<StampState>, Error> {
    Ok(Rc::new(from_value(a.get(index).unwrap_or(&Value::Null))?))
}

fn initial_stamp_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_stamp_state()))
}

fn arm_asset_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*arm_asset(&state_arg(a, 0)?, &key(a, 1), &key(a, 2))))
}

fn disarm_documents_fn(a: &[Value]) -> Result<Value, Error> {
    let ids: Vec<String> = from_value(a.get(1).unwrap_or(&Value::Null))?;
    Ok(to_value(&*disarm_documents(&state_arg(a, 0)?, &ids)))
}

fn add_library_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*add_library(&state_arg(a, 0)?, &from_value(&a[1])?)))
}

fn set_library_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*set_library(&state_arg(a, 0)?, &from_value(&a[1])?)))
}

fn remove_library_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*remove_library(&state_arg(a, 0)?, &key(a, 1))))
}

fn add_asset_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*add_asset(&state_arg(a, 0)?, &from_value(&a[1])?)))
}

fn set_asset_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*set_asset(&state_arg(a, 0)?, &from_value(&a[1])?)))
}

fn remove_asset_fn(a: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&*remove_asset(&state_arg(a, 0)?, &key(a, 1))))
}

/// The pure transitions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![
        ("initialStampState", initial_stamp_state_fn),
        ("armAsset", arm_asset_fn),
        ("disarmDocuments", disarm_documents_fn),
        ("addLibrary", add_library_fn),
        ("setLibrary", set_library_fn),
        ("removeLibrary", remove_library_fn),
        ("addAsset", add_asset_fn),
        ("setAsset", set_asset_fn),
        ("removeAsset", remove_asset_fn),
    ]
}
