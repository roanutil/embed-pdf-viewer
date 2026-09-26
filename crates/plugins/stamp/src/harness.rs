//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/stamp-fixtures.ts`:
//!
//! - every case starts the id counter at zero, as the generator's fresh
//!   import of the stamp source per case does;
//! - `console.error` and `console.warn` from this plugin log as callbacks of
//!   those names (`StampHost::console_error` and `console_warn`);
//! - `Math.random` answers `((n % 62) + 0.5) / 62` for its nth call of a case
//!   (`StampHost::random`);
//! - `config.assetEngine` names an engine built over the kernel's:
//!   `'instance'`, `'factory'`, `'async-factory'`, `{throws}` or `{rejects}`,
//!   each factory call logging `assetEngine`;
//! - `stamp-observers` routes the v2 scripting observers to the actions
//!   plugin's doors: alerts through its UI adapter, diagnostics and errors
//!   through its script hooks.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use epdf_geometry::Point;
use epdf_kernel::engine::Engine;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, Unsubscribe, define, is_undefined, undefined};
use epdf_plugin_actions::{ACTIONS, ActionUiAdapter};
use epdf_plugin_annotation::StampPlacement;
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::{StampCapability, create_stamp_controller};
use crate::model::{StampAction, StampState};
use crate::persistence::{
    MemoryStampStore, PersistOptions, StampLibraryStore, StoredLibrary, memory_stamp_store,
    persist_stamp_libraries, restore_stamp_libraries,
};
use crate::services::asset_engine::reset_id_sequence;
use crate::{initial_stamp_state, stamp_plugin, stamp_reducer};

/// The adapter.
pub struct Stamp;

/// The composed api's member order (`composeApi` over the controller's
/// slices).
pub const MEMBERS: &[&str] = &[
    "listLibraries",
    "getLibrary",
    "listAssets",
    "getAsset",
    "getAssetPreview",
    "renderAssetPreview",
    "readAssetBytes",
    "exportLibrary",
    "canPlace",
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
    "placeAsset",
    "placeAssetOnPages",
    "disarm",
    "getArmedAsset",
    "onLibraryChanged",
    "onLibraryCreated",
    "onLibraryUpdated",
    "onLibraryDeleted",
    "onAssetCreated",
    "onAssetUpdated",
    "onAssetDeleted",
    "onArmChanged",
];

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null() && !is_undefined(v))
}

/// An id argument as the property key JavaScript would look up.
fn key(args: &[Value], index: usize) -> String {
    match args.get(index) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => epdf_geometry::js::number_to_string(n.as_f64().unwrap_or(0.0)),
        _ => "undefined".to_string(),
    }
}

fn string(value: Option<&Value>) -> Option<String> {
    present(value).and_then(Value::as_str).map(str::to_string)
}

/// A member that is present, even as `null`, read as a string.
fn defined_string(value: Option<&Value>) -> Option<String> {
    value.filter(|v| !is_undefined(v)).and_then(Value::as_str).map(str::to_string)
}

fn decoded<T: serde::de::DeserializeOwned>(value: Option<&Value>) -> Result<Option<T>, Error> {
    present(value).map(from_value).transpose()
}

fn out<T: Serialize>(value: Option<T>) -> Outcome {
    Outcome::Returned(value.map_or(Value::Null, |v| to_value(&v)))
}

fn bytes(value: Option<Vec<u8>>) -> Value {
    value.map_or(Value::Null, |b| epdf_kernel::bytes::Bytes(b).to_json())
}

fn preview_json(preview: &StampAssetPreview) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(bytes) = &preview.bytes {
        out.insert("bytes".into(), epdf_kernel::bytes::Bytes(bytes.clone()).to_json());
    }
    if let Some(mime_type) = &preview.mime_type {
        out.insert("mimeType".into(), json!(mime_type));
    }
    Value::Object(out)
}

fn preview(value: Option<StampAssetPreview>) -> Value {
    value.as_ref().map_or(Value::Null, preview_json)
}

fn json_promise<T: Clone + 'static>(
    promise: Promise<T>,
    map: impl Fn(&T) -> Value + 'static,
) -> Outcome {
    let (json, resolver) = Promise::pending();
    promise
        .on_settle(move |result| resolver.settle(result.as_ref().map(&map).map_err(Clone::clone)));
    Outcome::Promise(json)
}

fn import_options(value: Option<&Value>, h: &Harness) -> Result<ImportLibraryOptions, Error> {
    let Some(opts) = present(value) else { return Ok(ImportLibraryOptions::default()) };
    let asset_name = present(opts.get("assetName")).map(|spec| {
        let callback = h.callback(spec);
        Rc::new(move |index: f64| match callback.call(vec![json!(index)]) {
            Answer::Returns(value) => value.as_str().map(str::to_string),
            _ => None,
        }) as Rc<dyn Fn(f64) -> Option<String>>
    });
    Ok(ImportLibraryOptions {
        name: string(opts.get("name")),
        categories: decoded(opts.get("categories"))?,
        kind: decoded(opts.get("kind"))?,
        library_kind: string(opts.get("libraryKind")),
        asset_name,
    })
}

fn mark_source(value: &Value) -> Result<MarkSource, Error> {
    let member = |k: &str| value.get(k);
    Ok(match member("kind").and_then(Value::as_str) {
        Some("ink") => MarkSource::Ink {
            strokes: member("strokes")
                .and_then(Value::as_array)
                .map(|strokes| {
                    strokes
                        .iter()
                        .map(|s| {
                            s.as_array()
                                .map(|points| {
                                    points
                                        .iter()
                                        .map(|p| {
                                            let n = |k: &str| {
                                                p.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN)
                                            };
                                            (n("x"), n("y"))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default(),
            color: string(member("color")),
            stroke_width: present(member("strokeWidth")).and_then(Value::as_f64),
        },
        Some("text") => MarkSource::Text {
            text: string(member("text")).unwrap_or_default(),
            font_family: string(member("fontFamily")).unwrap_or_default(),
            color: string(member("color")),
            font_size: present(member("fontSize")).and_then(Value::as_f64),
        },
        Some("image") => {
            MarkSource::Image { source: member("source").cloned().unwrap_or_else(undefined) }
        }
        Some("pdf") => MarkSource::Pdf {
            source: member("source").cloned().unwrap_or_else(undefined),
            page_index: present(member("pageIndex")).and_then(Value::as_f64),
        },
        other => return Err(Error::new(format!("[stamp harness] unknown mark kind {other:?}"))),
    })
}

fn add_asset_input(value: Option<&Value>) -> Result<AddAssetInput, Error> {
    let input = value.cloned().unwrap_or(Value::Null);
    let member = |k: &str| input.get(k).filter(|v| !is_undefined(v));
    Ok(AddAssetInput {
        library_id: defined_string(member("libraryId")),
        name: defined_string(member("name")),
        label: defined_string(member("label")),
        kind: decoded(member("kind"))?,
        subject: string(member("subject")),
        categories: decoded(member("categories"))?,
        source: member("source").cloned(),
        mark: member("mark").map(mark_source).transpose()?,
        preview: member("preview").cloned(),
        size: decoded(member("size"))?,
    })
}

/// `config.assetEngine`, over the kernel's engine.
fn asset_engine(spec: &Value, engine: Rc<dyn Engine>, h: &Harness) -> AssetEngine {
    if spec.as_str() == Some("instance") {
        return AssetEngine::Instance(engine);
    }
    let (spec, h) = (spec.clone(), h.clone());
    AssetEngine::Factory(Rc::new(move || {
        h.log("assetEngine", Vec::new());
        match spec.as_str() {
            Some("factory") => Ok(AssetEngineRef::Ready(engine.clone())),
            Some("async-factory") => Ok(AssetEngineRef::Pending(Promise::resolved(engine.clone()))),
            _ => match (spec.get("throws"), spec.get("rejects")) {
                (Some(error), _) => Err(Error::from_spec(error)),
                (None, Some(error)) => {
                    Ok(AssetEngineRef::Pending(Promise::rejected(Error::from_spec(error))))
                }
                _ => Err(Error::new("[stamp harness] unknown assetEngine spec")),
            },
        }
    }))
}

fn host(h: &Harness) -> StampHost {
    let (error_log, warn_log) = (h.clone(), h.clone());
    let calls = Rc::new(Cell::new(0u64));
    StampHost {
        console_error: Some(Rc::new(move |message: &str, error: &Error| {
            error_log.log("console.error", vec![json!(message), error.to_canonical()]);
        })),
        console_warn: Some(Rc::new(move |message: &str, error: Option<&Error>| {
            let mut args = vec![json!(message)];
            if let Some(error) = error {
                args.push(error.to_canonical());
            }
            warn_log.log("console.warn", args);
        })),
        random: Some(Rc::new(move || {
            let n = calls.get();
            calls.set(n + 1);
            ((n % 62) as f64 + 0.5) / 62.0
        })),
    }
}

fn config_of(spec: &Value, engine: Rc<dyn Engine>, h: &Harness) -> StampConfig {
    StampConfig {
        asset_engine: present(spec.get("assetEngine")).map(|s| asset_engine(s, engine, h)),
        preview_width: spec.get("previewWidth").and_then(Value::as_f64),
        dynamic: spec.get("dynamic").and_then(Value::as_bool),
        host: host(h),
    }
}

fn unsubscribe_ref(h: &Harness, un: Unsubscribe) -> Outcome {
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        un.call();
        Outcome::undefined()
    })))
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
    unsubscribe_ref(h, off)
}

// ── stamp-observers ───────────────────────────────────────────────────────────

fn scripted(spec: &Value, h: &Harness) -> Callback {
    h.callback(spec)
}

fn observers_plugin(config: &Value, h: &Harness) -> AnyPlugin {
    let token: epdf_kernel::Token<()> =
        epdf_kernel::Token::from_any(h.token("stamp-observers", None));
    let (spec, h) = (config.clone(), h.clone());
    define::<Value, Value>("stamp-observers")
        .document_scoped()
        .requires(ACTIONS.any())
        .capability(&token, |_| Ok(()))
        .effects(move |fx| {
            let actions = fx.ctx.get(&ACTIONS)?;
            if let Some(on_ui) = present(spec.get("onUiEffect")) {
                let callback = scripted(on_ui, &h);
                let adapter = ActionUiAdapter {
                    open_uri: Rc::new(|_, _| Ok(())),
                    print: Rc::new(|_| Ok(())),
                    alert: Some(Rc::new(move |message: &str, opts| {
                        let mut effect = json!({ "kind": "alert", "message": message });
                        if let Value::Object(extra) = to_value(opts) {
                            for (k, v) in extra {
                                effect[k] = v;
                            }
                        }
                        match callback.call(vec![effect]) {
                            Answer::Throws(error) => Err(error),
                            _ => Ok(()),
                        }
                    })),
                    goto_page: None,
                };
                let dispose = actions.set_ui_adapter(Some(Rc::new(adapter)));
                fx.cleanup_fn(move || dispose());
            }
            if let Some(on_diagnostic) = present(spec.get("onDiagnostic")) {
                let callback = scripted(on_diagnostic, &h);
                let off =
                    actions.on_script_diagnostic(move |d| match callback.call(vec![to_value(d)]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    });
                fx.cleanup_fn(move || off.call());
            }
            if let Some(on_error) = present(spec.get("onError")) {
                let callback = scripted(on_error, &h);
                let off =
                    actions.on_script_error(move |e| match callback.call(vec![to_value(e)]) {
                        Answer::Throws(error) => Err(error),
                        _ => Ok(()),
                    });
                fx.cleanup_fn(move || off.call());
            }
            Ok(())
        })
        .build()
}

// ── stamp-persistence ─────────────────────────────────────────────────────────

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
        match self.failure("list") {
            Some(error) => Promise::rejected(error),
            None => self.memory.list(),
        }
    }
    fn put(&self, id: &str, bytes: Vec<u8>) -> Promise<()> {
        match self.failure("put") {
            Some(error) => Promise::rejected(error),
            None => self.memory.put(id, bytes),
        }
    }
    fn delete(&self, id: &str) -> Promise<()> {
        match self.failure("delete") {
            Some(error) => Promise::rejected(error),
            None => self.memory.delete(id),
        }
    }
}

/// The `stamp-persistence` driver's capability.
#[derive(Clone)]
pub struct PersistenceDriver {
    ctx: epdf_kernel::Ctx<Value, Value>,
    memory: MemoryStampStore,
    store: Rc<dyn StampLibraryStore>,
    stop: Rc<std::cell::RefCell<Option<Rc<dyn Fn()>>>>,
    warn: Rc<dyn Fn(&str, &Error)>,
}

fn persistence_plugin(config: &Value, h: &Harness) -> AnyPlugin {
    let token: epdf_kernel::Token<PersistenceDriver> =
        epdf_kernel::Token::from_any(h.token("stamp-persistence", None));
    let (fail, h) = (config.get("fail").cloned().unwrap_or(Value::Null), h.clone());
    define::<Value, Value>("stamp-persistence")
        .requires(STAMP.any())
        .capability(&token, move |ctx| {
            let memory = memory_stamp_store();
            let log = h.clone();
            Ok(PersistenceDriver {
                ctx,
                memory: memory.clone(),
                store: Rc::new(FailingStore { memory, fail: fail.clone() }),
                stop: Rc::default(),
                warn: Rc::new(move |message: &str, error: &Error| {
                    log.log("console.warn", vec![json!(message), error.to_canonical()]);
                }),
            })
        })
        .build()
}

fn call_persistence(
    driver: &PersistenceDriver,
    method: &str,
    args: &[Value],
) -> Result<Outcome, Error> {
    let stamp = || driver.ctx.get(&STAMP);
    Ok(match method {
        "seed" => {
            let puts: Vec<Promise<()>> = args
                .first()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|row| {
                    let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
                    let bytes = row
                        .get("bytes")
                        .and_then(epdf_kernel::bytes::Bytes::from_json)
                        .map(|b| b.0)
                        .unwrap_or_default();
                    driver.memory.put(id, bytes)
                })
                .collect();
            let all = crate::promise::all(puts);
            Outcome::promise_unit(Promise::spawn(async move {
                all.await?;
                Ok(())
            }))
        }
        "rows" => json_promise(driver.memory.list(), |rows| {
            Value::Array(
                rows.iter()
                    .map(|r| json!({ "id": r.id, "bytes": epdf_kernel::bytes::Bytes(r.bytes.clone()).to_json() }))
                    .collect(),
            )
        }),
        "restore" => {
            let warn = driver.warn.clone();
            Outcome::promise(restore_stamp_libraries(&stamp()?, driver.store.clone(), Some(warn)))
        }
        "persist" => {
            let opts = args.first().filter(|v| !is_undefined(v) && !v.is_null());
            let options = PersistOptions {
                except: opts
                    .and_then(|o| o.get("except"))
                    .and_then(Value::as_array)
                    .map(|e| e.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default(),
                debounce_ms: opts.and_then(|o| o.get("debounceMs")).and_then(Value::as_f64),
            };
            let stop = persist_stamp_libraries(
                &stamp()?,
                driver.store.clone(),
                options,
                Some(driver.warn.clone()),
            );
            *driver.stop.borrow_mut() = Some(stop);
            Outcome::undefined()
        }
        "stop" => {
            let stop = driver.stop.borrow().clone();
            if let Some(stop) = stop {
                stop();
            }
            Outcome::undefined()
        }
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

// ── the adapter ───────────────────────────────────────────────────────────────

fn page_list(value: Option<&Value>) -> Option<Vec<PageRef>> {
    value
        .and_then(Value::as_array)
        .map(|pages| pages.iter().filter_map(PageRef::from_json).collect())
}

fn call_stamp(
    cap: &StampCapability,
    method: &str,
    args: &[Value],
    h: &Harness,
) -> Result<Outcome, Error> {
    let arg = |i: usize| args.get(i).filter(|v| !is_undefined(v));
    Ok(match method {
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
                    library_id: string(f.get("libraryId")),
                    kind: decoded(f.get("kind"))?,
                    category: string(f.get("category")),
                }),
                None => None,
            };
            Outcome::Returned(to_value(&cap.list_assets(filter.as_ref())))
        }
        "getAsset" => out(cap.get_asset(&key(args, 0))),
        "getAssetPreview" => Outcome::Returned(preview(cap.get_asset_preview(&key(args, 0)))),
        "renderAssetPreview" => {
            let width =
                arg(1).and_then(|o| o.get("width")).and_then(Value::as_f64).unwrap_or(f64::NAN);
            json_promise(cap.render_asset_preview(&key(args, 0), width), |p| {
                p.as_ref().map_or(Value::Null, preview_json)
            })
        }
        "readAssetBytes" => Outcome::Returned(bytes(cap.read_asset_bytes(&key(args, 0)))),
        "exportLibrary" => {
            json_promise(cap.export_library(&key(args, 0)), |b| bytes(Some(b.clone())))
        }
        "canPlace" => {
            Outcome::Returned(json!(cap.can_place(arg(0).map(|_| key(args, 0)).as_deref())))
        }
        "canImport" => Outcome::Returned(json!(cap.can_import())),
        "createLibrary" => {
            let opts = present(arg(1));
            let options = CreateLibraryOptions {
                id: opts.and_then(|o| string(o.get("id"))),
                kind: opts.and_then(|o| string(o.get("kind"))),
                categories: decoded(opts.and_then(|o| o.get("categories")))?,
            };
            Outcome::promise(cap.create_library(&key(args, 0), options))
        }
        "updateLibrary" => {
            let patch = present(arg(1));
            let patch = LibraryPatch {
                name: patch.and_then(|p| string(p.get("name"))),
                categories: decoded(patch.and_then(|p| p.get("categories")))?,
            };
            Outcome::promise_unit(cap.update_library(&key(args, 0), patch))
        }
        "importLibrary" => {
            let opts = import_options(arg(1), h)?;
            Outcome::promise(cap.import_library(arg(0).cloned().unwrap_or_else(undefined), opts))
        }
        "deleteLibrary" => Outcome::promise_unit(cap.delete_library(&key(args, 0))),
        "createAsset" => Outcome::promise(cap.create_asset(add_asset_input(arg(0))?)),
        "createAssetFromAnnotations" => {
            let page =
                arg(1).and_then(PageRef::from_json).ok_or_else(|| Error::new("not a PageRef"))?;
            let refs = arg(2).and_then(Value::as_array).cloned().unwrap_or_default();
            Outcome::promise(cap.create_asset_from_annotations(
                &key(args, 0),
                page,
                refs,
                add_asset_input(arg(3))?,
            ))
        }
        "updateAsset" => {
            let patch = present(arg(1));
            let subject = match patch.and_then(|p| p.get("subject")).filter(|v| !is_undefined(v)) {
                None => None,
                Some(Value::Null) => Some(None),
                Some(s) => Some(s.as_str().map(str::to_string)),
            };
            let patch = AssetPatch {
                label: patch.and_then(|p| string(p.get("label"))),
                subject,
                categories: decoded(patch.and_then(|p| p.get("categories")))?,
            };
            Outcome::promise_unit(cap.update_asset(&key(args, 0), patch))
        }
        "deleteAsset" => Outcome::promise_unit(cap.delete_asset(&key(args, 0))),
        "moveAsset" => {
            let to = arg(1)
                .and_then(|t| t.get("libraryId"))
                .and_then(Value::as_str)
                .unwrap_or("undefined");
            Outcome::promise(cap.move_asset(&key(args, 0), to))
        }
        "duplicateAsset" => {
            let label = present(arg(1)).and_then(|o| string(o.get("label")));
            Outcome::promise(cap.duplicate_asset(&key(args, 0), label))
        }
        "armAsset" => {
            let target_width =
                present(arg(2)).and_then(|o| o.get("targetWidth")).and_then(Value::as_f64);
            Outcome::promise_unit(cap.arm_asset(&key(args, 0), &key(args, 1), target_width))
        }
        "placeAsset" => {
            let placement: StampPlacement = from_value(arg(2).unwrap_or(&Value::Null))?;
            Outcome::promise(cap.place_asset(&key(args, 0), &key(args, 1), placement))
        }
        "placeAssetOnPages" => {
            let pages = match arg(2) {
                Some(Value::String(all)) if all == "all" => None,
                other => Some(page_list(other).unwrap_or_default()),
            };
            let placement = arg(3).cloned().unwrap_or(Value::Null);
            let at: Point = from_value(placement.get("at").unwrap_or(&Value::Null))?;
            Outcome::promise(cap.place_asset_on_pages(
                &key(args, 0),
                &key(args, 1),
                pages,
                at,
                placement.get("targetWidth").and_then(Value::as_f64),
                placement.get("rotation").and_then(Value::as_f64),
            ))
        }
        "disarm" => match cap.disarm(&key(args, 0)) {
            Ok(()) => Outcome::undefined(),
            Err(error) => Outcome::Threw(error),
        },
        "getArmedAsset" => out(cap.get_armed_asset(&key(args, 0))),
        "onLibraryChanged" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_library_changed(f))
        }
        "onLibraryCreated" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_library_created(f))
        }
        "onLibraryUpdated" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_library_updated(f))
        }
        "onLibraryDeleted" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_library_deleted(f))
        }
        "onAssetCreated" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_asset_created(f))
        }
        "onAssetUpdated" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_asset_updated(f))
        }
        "onAssetDeleted" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_asset_deleted(f))
        }
        "onArmChanged" => {
            let cap = cap.clone();
            hook(h, arg(0), move |f| cap.on_arm_changed(f))
        }
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    })
}

impl HarnessPlugin for Stamp {
    fn factories(&self) -> Vec<&'static str> {
        vec!["stamp", "stamp-plugin", "stamp-observers", "stamp-persistence"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        if factory == "stamp-observers" {
            return Ok(observers_plugin(config, h));
        }
        if factory == "stamp-persistence" {
            return Ok(persistence_plugin(config, h));
        }
        reset_id_sequence();
        if factory == "stamp-plugin" {
            return Ok(stamp_plugin(StampConfig { host: host(h), ..StampConfig::default() }));
        }
        let (spec, h) = (config.clone(), h.clone());
        // `stamp.plugin.ts`, with the config built over the kernel's engine.
        Ok(define::<StampState, StampAction>("stamp")
            .optional(epdf_plugin_annotation::ANNOTATION.any())
            .optional(ACTIONS.any())
            .initial_state(initial_stamp_state)
            .reduce(stamp_reducer)
            .create(&STAMP, move |cx| {
                let engine = cx.engine().ok_or_else(|| Error::new("[stamp harness] no engine"))?;
                let config = config_of(&spec, engine, &h);
                Ok((create_stamp_controller(cx, config), None))
            })
            .build())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![STAMP.any().clone()]
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
        if capability.downcast_ref::<PersistenceDriver>().is_some() {
            return Some(vec!["seed", "rows", "restore", "persist", "stop"]);
        }
        capability.downcast_ref::<StampCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_stamp_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_stamp_state()))
}

/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn stamp_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: StampState = from_value(&a[0])?;
    let next = match from_value::<StampAction>(&a[1]) {
        Ok(action) => stamp_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("initialStampState", initial_stamp_state_fn), ("stampReducer", stamp_reducer_fn)]
}
