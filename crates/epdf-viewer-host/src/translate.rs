//! Kernel engine calls as host wire requests, and back.
//!
//! The kernel speaks member chains (`crates/epdf-kernel/src/engine.rs`); the
//! host speaks wire kinds (`crates/epdf-engine/src/host.rs`). On the web the
//! two hops are `engine-bridge.ts` and `@embedpdf/engine`'s worker protocol;
//! here they are this one table.
//!
//! A page travels as `pageObjectNumber`, not as the `page: PageRef` the
//! current `worker-protocol.ts` spells: the host reads the former
//! (`host.rs` `page_field`), and
//! `tooling/engine-conformance/vectors/wire/requests.json` pins it.
//!
//! Answers are read the host's way too, which is not the TypeScript worker's:
//! the host sends a result bare where the worker tags it (`{tag, snapshot}`),
//! and wraps a mutation's result with its sequence and event. What decoding
//! produces is what the plugins read, so where the host and the
//! `engine-core` DTOs differ for a mapped kind (`PageLayout.ref`,
//! `SearchMatch.page`, a raster's pixel format) it is filled in here, a
//! `SearchRequest`'s `startPage` goes out as the object number the host
//! reads, and a render request leaves its encode options behind. A shape
//! the host did not send is an error, never a guess.

use epdf_engine::host::HostResponse;
use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use serde_json::{Map, Value};

use crate::host_table::HostTable;

pub struct WireCall {
    pub request: Value,
    pub payloads: Vec<Vec<u8>>,
    pub decode: Decode,
}

pub enum Decode {
    /// The result as the host sent it.
    Result,
    /// One member of the result, which must be there.
    Field(&'static str),
    /// A `PageListSnapshot`, each page given the `ref` `PageLayout` has.
    PageList,
    /// A `SearchSlice`, each match given the `page` `SearchMatch` has.
    SearchSlice,
    /// A mutation: `{result, mutationSequence, event}`. Answers `result`
    /// and hands back the event to publish.
    Mutation,
    /// Width, height and stride, with the pixels moved into the `HostTable`
    /// behind a `$host` id, plus what the render plugin and its ffi read:
    /// the pixel format as `PageRaster` spells it and a raw-pixel `source`
    /// carrying the byte count.
    Raster,
    Unit,
    /// A read whose answer carries annotation refs: the result, with every
    /// ref nested the kernel's way.
    Annotations,
    /// An annotation mutation: `Mutation`, with every ref in the result and
    /// the event nested the kernel's way.
    AnnotationMutation,
    /// An annotation's embedded file: the result (`name`, `mimeType`,
    /// `size`), with the payload parked in the `HostTable` as `data`.
    AttachmentContent,
    /// A page mutation: `Mutation`, with `ref` added to every layout page of
    /// the result and the event (the kernel's page registry and the
    /// page-edit plugin read `ref`), and the host's
    /// `insertedPageObjectNumbers` given as `insertedPages` refs.
    PageMutation,
    /// A byte payload (an extracted PDF), parked in the `HostTable` and
    /// answered as `{"$hostBytes": id}` for `Bytes::from_json` to take.
    HostBytes,
    /// A form read: every widget given the `ref` and `page` the kernel
    /// reads, every other page reference nested as `Annotations` does.
    Forms,
    /// A form mutation: `Forms` over the inner result and its event. A
    /// `forms.applyEffects` batch that wrote nothing answers a bare result
    /// with no `mutationSequence`; that answers itself with no event.
    FormMutation,
    /// `forms.applyEffects`: `FormMutation`, with the event the host
    /// publishes as `form.valueChanged` renamed `form.effectsApplied`, as
    /// the TypeScript engine publishes it. The batch result has no `field`.
    FormEffects,
    /// A field edit (`createField`, `updateField`, a widget link):
    /// `FormMutation`, with the bare field the host answers put under
    /// `field` in the result and in the event, where the host spreads it
    /// beside `type`. The TypeScript engine sends `{field, meta}`, and the
    /// plugin reads `field` from both.
    FormFieldMutation,
    /// Bytes in the first payload, put back into the result as `bytes`
    /// (`{"$bytes": base64}`), as the form plugin reads an export.
    ExportBytes,
}

/// A decoded answer: the call's value, and the document event a mutation
/// published, if it published one.
#[derive(Debug)]
pub struct Decoded {
    pub value: Value,
    pub event: Option<Value>,
}

fn coded(code: &str, message: String) -> Error {
    Error::new(message).with("code", code)
}

/// Every kernel path `route` maps, for `maps`' namespace check. A path added
/// to `route` goes here too, one per line, in plan order;
/// `every_routed_path_is_listed` checks the two agree.
pub const PATHS: &[&str] = &[
    "close",
    "pages.list",
    "metadata.read",
    "metadata.update",
    "page.text.read",
    "page.geometry.read",
    "page.render.image",
    "search.query",
    "page.annotations.list",
    "annotations.listRawAll",
    "page.annotations.create",
    "page.annotations.update",
    "page.annotations.delete",
    "page.annotations.downloadFile",
    "pages.move",
    "pages.rotate",
    "pages.delete",
    "pages.insert",
    "pages.insertBlank",
    "pages.extract",
    "actions.read",
    "forms.list",
    "forms.setValue",
    "forms.reset",
    "forms.applyEffects",
    "forms.exportData",
    "forms.importData",
    "forms.repair",
    "forms.createField",
    "forms.updateField",
    "forms.deleteField",
    "forms.attachWidget",
    "forms.detachWidget",
];

/// Feature names a plugin checks with `doc.has` that are neither a path nor
/// a namespace of one (`document.versioned`). One per line, in plan order.
pub const FLAGS: &[&str] = &[];

/// Whether `translate` maps `path`; or, for a bare namespace such as
/// `redaction`, any listed path under it; or a listed flag. This is what
/// `DocumentHandle::has` answers, and the plugins ask all three ways
/// (`doc.has("forms.applyEffects")`, `doc.has("redaction")`,
/// `doc.has("document.versioned")`).
pub fn maps(path: &str) -> bool {
    route(path).is_some()
        || FLAGS.contains(&path)
        || PATHS.iter().any(|p| p.strip_prefix(path).is_some_and(|rest| rest.starts_with('.')))
}

/// The wire kind and result decoding for a kernel path.
fn route(path: &str) -> Option<(&'static str, Decode)> {
    Some(match path {
        "close" => ("close", Decode::Unit),
        "pages.list" => ("pages.list", Decode::PageList),
        "metadata.read" => ("metadata.read", Decode::Result),
        "metadata.update" => ("metadata.update", Decode::Mutation),
        "page.text.read" => ("pages.text", Decode::Result),
        "page.geometry.read" => ("pages.geometry", Decode::Result),
        "page.render.image" => ("pages.render", Decode::Raster),
        "search.query" => ("search.query", Decode::SearchSlice),
        "page.annotations.list" => ("annotations.listRawPage", Decode::Annotations),
        "annotations.listRawAll" => ("annotations.listRawAll", Decode::Annotations),
        "page.annotations.create" => ("annotations.create", Decode::AnnotationMutation),
        "page.annotations.update" => ("annotations.update", Decode::AnnotationMutation),
        "page.annotations.delete" => ("annotations.delete", Decode::AnnotationMutation),
        "page.annotations.downloadFile" => ("annotations.readFile", Decode::AttachmentContent),
        "pages.move" => ("pages.move", Decode::PageMutation),
        "pages.rotate" => ("pages.rotate", Decode::PageMutation),
        "pages.delete" => ("pages.delete", Decode::PageMutation),
        "pages.insert" => ("pages.insert", Decode::PageMutation),
        "pages.insertBlank" => ("pages.insertBlank", Decode::PageMutation),
        "pages.extract" => ("pages.extract", Decode::HostBytes),
        "actions.read" => ("actions.read", Decode::Result),
        "forms.list" => ("forms.list", Decode::Forms),
        "forms.setValue" => ("forms.setValue", Decode::FormMutation),
        "forms.reset" => ("forms.reset", Decode::FormMutation),
        "forms.applyEffects" => ("forms.applyEffects", Decode::FormEffects),
        "forms.exportData" => ("forms.export", Decode::ExportBytes),
        "forms.importData" => ("forms.import", Decode::FormMutation),
        "forms.repair" => ("forms.repair", Decode::FormMutation),
        "forms.createField" => ("forms.createField", Decode::FormFieldMutation),
        "forms.updateField" => ("forms.updateField", Decode::FormFieldMutation),
        "forms.deleteField" => ("forms.deleteField", Decode::FormMutation),
        "forms.attachWidget" => ("forms.attachWidget", Decode::FormFieldMutation),
        "forms.detachWidget" => ("forms.detachWidget", Decode::FormFieldMutation),
        _ => return None,
    })
}

pub fn translate(
    doc: &str,
    path: &str,
    pon: Option<u32>,
    args: &[Value],
    job: u64,
) -> Result<WireCall, Error> {
    let (kind, decode) = route(path).ok_or_else(|| {
        coded("unsupported", format!("[epdf-viewer-host] the native engine has no \"{path}\" yet"))
    })?;
    let page = || {
        pon.map(Value::from).ok_or_else(|| {
            coded("invalid-input", format!("[epdf-viewer-host] {path} needs a page"))
        })
    };
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let mut payloads = Vec::new();
    let extra: Vec<(&str, Value)> = match kind {
        "metadata.update" => vec![("patch", arg(0))],
        "pages.text" | "pages.geometry" => vec![("pageObjectNumber", page()?)],
        "pages.render" => vec![("pageObjectNumber", page()?), ("options", render_options(arg(0)))],
        "search.query" => vec![("request", search_request(arg(0))?)],
        "annotations.listRawPage" => vec![("pageObjectNumber", page()?)],
        "annotations.create" => {
            let (draft, resources) = normalize_binary(arg(0), &mut payloads)?;
            let draft = wire_refs_in(path, draft)?;
            vec![("pageObjectNumber", page()?), ("draft", draft), ("resources", resources)]
        }
        "annotations.update" => {
            let (patch, resources) = normalize_binary(arg(1), &mut payloads)?;
            let patch = wire_refs_in(path, patch)?;
            vec![("ref", wire_ref(path, arg(0))?), ("patch", patch), ("resources", resources)]
        }
        "annotations.delete" => vec![("ref", wire_ref(path, arg(0))?)],
        "annotations.readFile" => {
            vec![("pageObjectNumber", page()?), ("ref", wire_ref(path, arg(0))?)]
        }
        "pages.move" => vec![
            ("pageObjectNumbers", object_numbers(path, &arg(0))?),
            ("destIndex", whole(path, "destIndex", &arg(1))?),
        ],
        "pages.rotate" => vec![
            ("pageObjectNumbers", object_numbers(path, &arg(0))?),
            ("rotation", whole(path, "rotation", &arg(1))?),
        ],
        "pages.delete" | "pages.extract" => {
            vec![("pageObjectNumbers", object_numbers(path, &arg(0))?)]
        }
        "pages.insertBlank" => {
            let spec = arg(0);
            vec![
                ("size", spec.get("size").cloned().unwrap_or(Value::Null)),
                ("count", optional_whole(path, "count", spec.get("count"))?),
                ("destIndex", optional_whole(path, "destIndex", args.get(1))?),
            ]
        }
        "pages.insert" => {
            payloads.push(pdf_bytes(path, &arg(0))?);
            vec![("destIndex", optional_whole(path, "destIndex", args.get(1))?)]
        }
        "forms.setValue" => {
            vec![("ref", wire_field_ref(path, arg(0))?), ("value", arg(1))]
        }
        "forms.reset" | "forms.deleteField" => vec![("ref", wire_field_ref(path, arg(0))?)],
        "forms.applyEffects" => {
            let mut effects = wire_refs_in(path, arg(0))?;
            whole_field_refs(path, &mut effects)?;
            vec![("effects", effects)]
        }
        "forms.export" => vec![("format", arg(0))],
        "forms.import" => {
            let data = epdf_kernel::bytes::Bytes::from_json(&arg(0)).ok_or_else(|| {
                coded("invalid-input", format!("[epdf-viewer-host] {path} needs bytes"))
            })?;
            payloads.push(data.0);
            vec![("format", arg(1))]
        }
        "forms.repair" => {
            let bake = arg(0).get("bakeAppearances").cloned().unwrap_or(Value::Bool(false));
            vec![("bakeAppearances", bake)]
        }
        "forms.createField" => {
            let mut draft = arg(0);
            flatten_placements(&mut draft);
            vec![("draft", draft)]
        }
        "forms.updateField" => {
            vec![("ref", wire_field_ref(path, arg(0))?), ("patch", arg(1))]
        }
        "forms.attachWidget" | "forms.detachWidget" => {
            let widget = arg(1);
            vec![
                ("ref", wire_field_ref(path, arg(0))?),
                (
                    "widget",
                    whole(path, "widget", widget.get("annotObjectNumber").unwrap_or(&Value::Null))?,
                ),
                ("onState", widget.get("onState").cloned().unwrap_or(Value::Null)),
            ]
        }
        _ => vec![],
    };
    let mut request = Map::new();
    request.insert("kind".into(), kind.into());
    request.insert("jobId".into(), job.into());
    request.insert("docId".into(), doc.into());
    for (key, value) in extra {
        if !value.is_null() && !epdf_kernel::is_undefined(&value) {
            request.insert(key.into(), value);
        }
    }
    Ok(WireCall { request: Value::Object(request), payloads, decode })
}

/// A JavaScript number as the whole, non-negative integer the host parses.
/// Plugins send `90.0` where the host's serde wants `90`; anything else (a
/// fraction, a negative, `{"$number": "NaN"}`) is refused with a message
/// that names the field.
pub fn whole(path: &str, field: &str, value: &Value) -> Result<Value, Error> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX))
        .map(|n| Value::from(n as u64))
        .ok_or_else(|| {
            coded(
                "invalid-input",
                format!("[epdf-viewer-host] {path}'s {field} is not a whole number: {value}"),
            )
        })
}

/// `whole`, with an absent, `null` or `undefined` value left out.
pub fn optional_whole(path: &str, field: &str, value: Option<&Value>) -> Result<Value, Error> {
    match value {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(v) if epdf_kernel::is_undefined(v) => Ok(Value::Null),
        Some(v) => whole(path, field, v),
    }
}

/// `[PageRef]` as the host's `[pageObjectNumber]`.
fn object_numbers(path: &str, refs: &Value) -> Result<Value, Error> {
    let refs = refs.as_array().ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a list of pages: {refs}"))
    })?;
    refs.iter()
        .map(|r| {
            PageRef::from_json(r).map(|p| Value::from(p.page_object_number)).ok_or_else(|| {
                coded("invalid-input", format!("[epdf-viewer-host] {path}: not a PageRef: {r}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// A PDF given as `Bytes` JSON: `{"$bytes": base64}` from the caller, or
/// `{"$hostBytes": id}` from an extract, which this takes out of the table.
fn pdf_bytes(path: &str, value: &Value) -> Result<Vec<u8>, Error> {
    epdf_kernel::bytes::Bytes::from_json(value).map(|b| b.0).ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a PDF's bytes: {value}"))
    })
}

/// A `SearchRequest` the host reads: `startPage` is a `PageObjectNumber`
/// there, where the plugin sends a `PageRef`. The rest goes as it is.
fn search_request(mut request: Value) -> Result<Value, Error> {
    let Some(start) = request.get_mut("startPage") else { return Ok(request) };
    if start.is_object() {
        let pon = PageRef::from_json(start).ok_or_else(|| {
            coded(
                "invalid-input",
                format!("[epdf-viewer-host] search.query's startPage is not a PageRef: {start}"),
            )
        })?;
        *start = pon.page_object_number.into();
    }
    Ok(request)
}

/// `PageImageOptions` as `pages.render` reads them. The viewport, target and
/// `includeAnnotations` go as they are (the host spells them the same); the
/// encode options do not, because `pages.render` answers raw pixels and has
/// no encoder to hand them to.
fn render_options(mut options: Value) -> Value {
    if let Value::Object(map) = &mut options {
        map.remove("format");
        map.remove("quality");
    }
    options
}

fn malformed(what: String) -> Error {
    Error::new(format!("[epdf-viewer-host] the host answered {what}"))
}

/// The `page: PageRef` a host page object (a page, a search match) lacks,
/// from its `pageObjectNumber`.
fn add_page_ref(object: &mut Value, key: &str, what: &str) -> Result<(), Error> {
    let pon = object
        .get("pageObjectNumber")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| malformed(format!("{what} without a pageObjectNumber: {object}")))?;
    match object {
        Value::Object(map) => {
            map.insert(key.into(), PageRef::new(pon).to_json());
            Ok(())
        }
        other => Err(malformed(format!("{what} that is not an object: {other}"))),
    }
}

/// A `PageRef` itself, `{kind: "objectNumber", pageObjectNumber}`, which
/// both spellings share.
fn is_page_ref(map: &Map<String, Value>) -> bool {
    map.len() == 2
        && map.get("kind").and_then(Value::as_str) == Some("objectNumber")
        && map.get("pageObjectNumber").is_some_and(Value::is_u64)
}

fn page_number(value: Option<&Value>) -> Option<u32> {
    value.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())
}

/// Every page `value` names, kernel spelling to host spelling: a
/// `page: PageRef` member (an annotation ref's, a destination's, a revision
/// token's, a DTO's) becomes `pageObjectNumber`. A `PageRef` itself keeps
/// its spelling.
pub fn flatten_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let nested = match map.get("page") {
                Some(Value::Object(page)) if is_page_ref(page) => {
                    page_number(page.get("pageObjectNumber"))
                }
                _ => None,
            };
            if let Some(pon) = nested
                && !is_page_ref(map)
            {
                map.remove("page");
                map.insert("pageObjectNumber".into(), pon.into());
            }
            map.values_mut().for_each(flatten_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(flatten_refs),
        _ => {}
    }
}

/// Every page `value` names, host spelling to kernel spelling. The host
/// spells a page as `pageObjectNumber` wherever the kernel's vectors spell
/// `page: PageRef`: an annotation ref, a destination (in a link's target and
/// in its action tree), a DTO, a page state and its revision token. A bare
/// numeric `page` becomes a `PageRef` too. A `PageRef` itself keeps its
/// spelling.
pub fn nest_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if !is_page_ref(map)
                && let Some(pon) = page_number(map.get("pageObjectNumber"))
            {
                map.remove("pageObjectNumber");
                map.insert("page".into(), PageRef::new(pon).to_json());
            }
            if let Some(pon) = page_number(map.get("page")) {
                map.insert("page".into(), PageRef::new(pon).to_json());
            }
            map.values_mut().for_each(nest_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(nest_refs),
        _ => {}
    }
}

/// An annotation ref the host's way: flat, and with its JavaScript numbers
/// (`annotObjectNumber`, `index`, a revision's `generation`, each sent as
/// `17.0`) as the whole integers the host's serde parses.
fn wire_ref(path: &str, mut target: Value) -> Result<Value, Error> {
    flatten_refs(&mut target);
    whole_refs(path, &mut target)?;
    Ok(target)
}

/// A draft or patch the host's way: every ref inside it (a reply's
/// `inReplyTo`, a link's `goto` destination) flat and whole, as
/// `wire_ref` makes the call's own ref. Other members keep their numbers.
fn wire_refs_in(path: &str, mut value: Value) -> Result<Value, Error> {
    flatten_refs(&mut value);
    whole_refs(path, &mut value)?;
    Ok(value)
}

/// The JavaScript numbers of every annotation ref (an object whose `kind`
/// is `objectNumber`, `nm` or `index`, beside a `pageObjectNumber`) and
/// of its revision token, as whole integers.
fn whole_refs(path: &str, value: &mut Value) -> Result<(), Error> {
    fn whole_keys(path: &str, map: &mut Map<String, Value>, keys: &[&str]) -> Result<(), Error> {
        for key in keys {
            if let Some(n) = map.get(*key).filter(|n| n.is_f64()) {
                let n = whole(path, key, n)?;
                map.insert((*key).into(), n);
            }
        }
        Ok(())
    }
    match value {
        Value::Object(map) => {
            let is_ref = matches!(
                map.get("kind").and_then(Value::as_str),
                Some("objectNumber" | "nm" | "index")
            ) && map.contains_key("pageObjectNumber");
            if is_ref {
                whole_keys(path, map, &["annotObjectNumber", "index", "pageObjectNumber"])?;
                if let Some(Value::Object(revision)) = map.get_mut("revision") {
                    whole_keys(path, revision, &["generation", "pageObjectNumber"])?;
                }
            }
            for member in map.values_mut() {
                whole_refs(path, member)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                whole_refs(path, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A form field ref the host's way: an object-number ref's
/// `fieldObjectNumber`, sent as a JavaScript number, whole.
fn wire_field_ref(path: &str, mut target: Value) -> Result<Value, Error> {
    whole_field_refs(path, &mut target)?;
    Ok(target)
}

/// Every form field ref inside `value` (`{kind: "objectNumber",
/// fieldObjectNumber}`) with a whole `fieldObjectNumber`.
fn whole_field_refs(path: &str, value: &mut Value) -> Result<(), Error> {
    match value {
        Value::Object(map) => {
            if let Some(n) = map.get("fieldObjectNumber").filter(|n| n.is_f64()) {
                let n = whole(path, "fieldObjectNumber", n)?;
                map.insert("fieldObjectNumber".into(), n);
            }
            for member in map.values_mut() {
                whole_field_refs(path, member)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                whole_field_refs(path, item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A form widget the host spells `{annotObjectNumber, pageObjectNumber}`,
/// given the `ref` and `page` the plugin reads. A toggle widget's
/// `onState`, `exportValue` and `checked` stay beside them.
pub fn nest_widgets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let widget = map.contains_key("annotObjectNumber")
                && !map.contains_key("kind")
                && !map.contains_key("ref");
            if widget
                && let (Some(annot), Some(pon)) = (
                    map.get("annotObjectNumber").and_then(Value::as_u64),
                    page_number(map.get("pageObjectNumber")),
                )
            {
                map.remove("pageObjectNumber");
                let page = PageRef::new(pon).to_json();
                map.insert("page".into(), page.clone());
                map.insert(
                    "ref".into(),
                    serde_json::json!({ "kind": "objectNumber", "page": page, "annotObjectNumber": annot }),
                );
            }
            map.values_mut().for_each(nest_widgets);
        }
        Value::Array(items) => items.iter_mut().for_each(nest_widgets),
        _ => {}
    }
}

/// A field draft's widget placements: `page: PageRef` out as
/// `pageObjectNumber`, which `WidgetPlacement` reads.
fn flatten_placements(draft: &mut Value) {
    let flatten = |placement: &mut Value| {
        if let Value::Object(map) = placement
            && let Some(pon) = map.get("page").and_then(|p| p.get("pageObjectNumber")).cloned()
        {
            map.remove("page");
            map.insert("pageObjectNumber".into(), pon);
        }
    };
    if let Some(widget) = draft.get_mut("widget") {
        flatten(widget);
    }
    if let Some(Value::Array(widgets)) = draft.get_mut("widgets") {
        widgets.iter_mut().for_each(flatten);
    }
}

/// `normalizeAnnotationDraft` (`packages/engine/core/src/annotation/
/// normalize.ts`): a stamp's `source` and an attachment's `file`, each a
/// `BinarySource` the kernel spells `{data: {"$bytes": …}, mimeType?,
/// name?}` or bare `{"$bytes": …}`, become `{resource: "rN"}` with the
/// bytes moved into a payload. Every other draft goes as it is. The
/// resources answer `Null` when nothing moved, so the request leaves them
/// out.
fn normalize_binary(
    mut draft: Value,
    payloads: &mut Vec<Vec<u8>>,
) -> Result<(Value, Value), Error> {
    let mut resources = Map::new();
    match draft.get("subtype").and_then(Value::as_str) {
        Some("stamp") => {
            if let Some(source) = draft.get("source").cloned() {
                let (key, _, _) = take_binary(&source, &mut resources, payloads)?;
                draft["source"] = serde_json::json!({ "resource": key });
            }
        }
        Some("file-attachment") => {
            if let Some(file) = draft.get("file").cloned() {
                let (key, mime, name) = take_binary(&file, &mut resources, payloads)?;
                let name = name.ok_or_else(|| {
                    coded(
                        "invalid-input",
                        "[epdf-viewer-host] an attachment file needs a name".into(),
                    )
                })?;
                let mut wire = serde_json::json!({ "resource": key, "name": name });
                if let Some(mime) = mime {
                    wire["mimeType"] = mime.into();
                }
                if let Some(description) = file.get("description") {
                    wire["description"] = description.clone();
                }
                draft["file"] = wire;
            }
        }
        _ => {}
    }
    let resources = if resources.is_empty() { Value::Null } else { Value::Object(resources) };
    Ok((draft, resources))
}

/// Move one `BinarySource`'s bytes into a payload and declare it as the
/// next resource. Answers the resource key, the MIME type and the name.
fn take_binary(
    source: &Value,
    resources: &mut Map<String, Value>,
    payloads: &mut Vec<Vec<u8>>,
) -> Result<(String, Option<String>, Option<String>), Error> {
    let data = source.get("data").unwrap_or(source);
    let bytes = epdf_kernel::bytes::Bytes::from_json(data).ok_or_else(|| {
        coded(
            "invalid-input",
            format!("[epdf-viewer-host] a binary source without bytes: {source}"),
        )
    })?;
    let key = format!("r{}", resources.len());
    let mime = source.get("mimeType").and_then(Value::as_str).map(str::to_string);
    let name = source.get("name").and_then(Value::as_str).map(str::to_string);
    let mut resource = Map::new();
    resource.insert("payload".into(), payloads.len().into());
    if let Some(mime) = &mime {
        resource.insert("mimeType".into(), mime.clone().into());
    }
    resources.insert(key.clone(), Value::Object(resource));
    payloads.push(bytes.0);
    Ok((key, mime, name))
}

/// `ref` on every page of `object.layout.pages`, which must be there.
fn add_layout_refs(object: &mut Value, what: &str) -> Result<(), Error> {
    let Some(Value::Array(pages)) = object.get_mut("layout").and_then(|l| l.get_mut("pages"))
    else {
        return Err(malformed(format!("{what} without a layout")));
    };
    for page in pages {
        add_page_ref(page, "ref", "a layout page")?;
    }
    Ok(())
}

/// Read a host response into what the call's caller expects.
pub fn decode(decode: &Decode, response: HostResponse) -> Result<Decoded, Error> {
    let mut envelope: Value = serde_json::from_str(&response.json)
        .map_err(|e| malformed(format!("unreadable JSON: {e}")))?;
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("Unknown");
        let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
        return Err(Error::engine(code, format!("{code}: {message}")));
    }
    if let Decode::Unit = decode {
        return Ok(Decoded { value: epdf_kernel::undefined(), event: None });
    }
    let mut result = match envelope.get_mut("result").map(Value::take) {
        Some(Value::Object(result)) => result,
        Some(other) => return Err(malformed(format!("a result that is not an object: {other}"))),
        None => return Err(malformed("neither a result nor an error".into())),
    };
    let take = |result: &mut Map<String, Value>, name: &str| {
        result.remove(name).ok_or_else(|| malformed(format!("a result without \"{name}\"")))
    };
    let value = match decode {
        Decode::Unit => unreachable!("answered above"),
        Decode::Result => Value::Object(result),
        Decode::Field(name) => take(&mut result, name)?,
        Decode::PageList => {
            let Some(Value::Array(pages)) = result.get_mut("pages") else {
                return Err(malformed("a page list without \"pages\"".into()));
            };
            for page in pages {
                add_page_ref(page, "ref", "a page")?;
            }
            Value::Object(result)
        }
        Decode::SearchSlice => {
            let Some(Value::Array(matches)) = result.get_mut("matches") else {
                return Err(malformed("a search slice without \"matches\"".into()));
            };
            for hit in matches {
                add_page_ref(hit, "page", "a search match")?;
            }
            Value::Object(result)
        }
        Decode::Annotations => {
            let mut value = Value::Object(result);
            nest_refs(&mut value);
            value
        }
        Decode::AnnotationMutation => {
            let mut value = take(&mut result, "result")?;
            nest_refs(&mut value);
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                nest_refs(event);
            }
            return Ok(Decoded { value, event });
        }
        Decode::AttachmentContent => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("a file without its payload".into()))?;
            result
                .insert("data".into(), serde_json::json!({ "$hostBytes": HostTable::put(bytes) }));
            Value::Object(result)
        }
        Decode::Forms => {
            let mut value = Value::Object(result);
            nest_widgets(&mut value);
            nest_refs(&mut value);
            value
        }
        Decode::FormMutation | Decode::FormEffects => {
            if !result.contains_key("mutationSequence") {
                let mut value = Value::Object(result);
                nest_widgets(&mut value);
                nest_refs(&mut value);
                return Ok(Decoded { value, event: None });
            }
            let mut value = take(&mut result, "result")?;
            nest_widgets(&mut value);
            nest_refs(&mut value);
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                nest_widgets(event);
                nest_refs(event);
                if let (Decode::FormEffects, Value::Object(map)) = (decode, event) {
                    map.insert("type".into(), "form.effectsApplied".into());
                }
            }
            return Ok(Decoded { value, event });
        }
        Decode::FormFieldMutation => {
            let mut field = take(&mut result, "result")?;
            nest_widgets(&mut field);
            nest_refs(&mut field);
            let event = match result.remove("event") {
                Some(Value::Object(mut spread)) => {
                    let mut event = Map::new();
                    for key in ["type", "origin"] {
                        if let Some(value) = spread.remove(key) {
                            event.insert(key.into(), value);
                        }
                    }
                    let mut field = Value::Object(spread);
                    nest_widgets(&mut field);
                    nest_refs(&mut field);
                    event.insert("field".into(), field);
                    Some(Value::Object(event))
                }
                other => other,
            };
            return Ok(Decoded { value: serde_json::json!({ "field": field }), event });
        }
        Decode::ExportBytes => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("an export without its bytes".into()))?;
            result.insert("bytes".into(), epdf_kernel::bytes::Bytes(bytes).to_json());
            Value::Object(result)
        }
        Decode::Mutation => {
            let value = take(&mut result, "result")?;
            return Ok(Decoded { value, event: result.remove("event") });
        }
        Decode::PageMutation => {
            let mut value = take(&mut result, "result")?;
            add_layout_refs(&mut value, "a page mutation's result")?;
            if let Some(pons) = value.get("insertedPageObjectNumbers").and_then(Value::as_array) {
                let refs = pons
                    .iter()
                    .map(|n| {
                        n.as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .map(|n| PageRef::new(n).to_json())
                            .ok_or_else(|| {
                                malformed(format!("an inserted page that is not a number: {n}"))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                value["insertedPages"] = Value::Array(refs);
            }
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                add_layout_refs(event, "a page event")?;
            }
            return Ok(Decoded { value, event });
        }
        Decode::HostBytes => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("bytes without their payload".into()))?;
            serde_json::json!({ "$hostBytes": HostTable::put(bytes) })
        }
        Decode::Raster => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("a raster without its pixels".into()))?;
            let dimension = |key: &str| {
                result.get(key).and_then(Value::as_u64).ok_or_else(|| {
                    malformed(format!("a raster without a whole \"{key}\": {result:?}"))
                })
            };
            let (width, height, stride) =
                (dimension("width")?, dimension("height")?, dimension("stride")?);
            let fits = width.checked_mul(4).is_some_and(|row| stride >= row)
                && stride.checked_mul(height).is_some_and(|needed| bytes.len() as u64 >= needed);
            if !fits {
                return Err(malformed(format!(
                    "a {width}x{height} raster with stride {stride} in {} bytes",
                    bytes.len()
                )));
            }
            // `PageRaster`'s pixel format: the host renders through
            // `FPDF_REVERSE_BYTE_ORDER` into RGBA with straight alpha
            // (`epdf_engine::features::render::raster::PageRaster`).
            result.insert("color".into(), "rgba8".into());
            result.insert("premultipliedAlpha".into(), false.into());
            result.insert(
                "source".into(),
                serde_json::json!({ "kind": "pixels", "$byteLength": bytes.len() }),
            );
            result.insert("$host".into(), HostTable::put(bytes).into());
            Value::Object(result)
        }
    };
    Ok(Decoded { value, event: None })
}
