//! Kernel engine calls as host wire requests, and back.
//!
//! The kernel speaks member chains (`crates/epdf-kernel/src/engine.rs`); the
//! host speaks wire kinds (`crates/epdf-engine/src/host.rs`). On the web the
//! two hops are `engine-bridge.ts` and `@embedpdf/engine`'s worker protocol;
//! here they are this one table.
//!
//! A page travels as the `page: PageRef` (or `pages: PageRef[]`) that
//! `worker-protocol.ts` spells and the host reads (`host.rs` `page_field`);
//! `tooling/engine-conformance/vectors/wire/requests.json` pins the keys. The
//! host answers pages the same way (`PageLayout.ref`, `SearchMatch.page`, an
//! annotation's `page`, a widget's `page` and `ref`), so the kernel reads
//! them as they arrive.
//!
//! The kernel still spells refs as this branch's wire did before the
//! catch-up: a `PageRef`'s number is `pageObjectNumber` and an annotation
//! ref's is `annotObjectNumber`. The host spells both `objectNumber`, as
//! upstream does (595bdc6c7). `translate` respells a request on the way out
//! and `decode` respells an answer on the way back, so the kernel keeps its
//! spelling until Task 13 ports it.
//!
//! A kernel path that maps onto a kind upstream removed goes to the host
//! through `Host::call_legacy` (see [`call_host`]), each until the sub-task
//! named beside it in `route` moves it to the kind that replaced it.
//!
//! Answers are read the host's way, which is not the TypeScript worker's in
//! two respects: the host sends a result bare where the worker tags it
//! (`{tag, snapshot}`), and wraps a mutation's result with its sequence and
//! event. Where the host and the `engine-core` DTOs still differ for a mapped
//! kind (a raster's pixel format, a field edit's bare field) it is filled in
//! here, and a render request leaves its encode options behind. A shape the
//! host did not send is an error, never a guess.

use epdf_engine::host::{Host, HostResponse};
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
    /// A `PageListSnapshot`, which must name every page by its `ref`.
    PageList,
    /// A `SearchSlice`, which must name every match's `page`.
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
    /// An annotation's embedded file: the result (`name`, `mimeType`,
    /// `size`), with the payload parked in the `HostTable` as `data`.
    AttachmentContent,
    /// A page mutation: `Mutation`, whose result and event must carry a
    /// layout that names every page by its `ref` (the kernel's page registry
    /// and the page-edit plugin read it).
    PageMutation,
    /// A byte payload (an extracted PDF), parked in the `HostTable` and
    /// answered as `{"$hostBytes": id}` for `Bytes::from_json` to take.
    HostBytes,
    /// Bytes in the first payload, put back into the result as `bytes`
    /// (`{"$bytes": base64}`), as the form plugin reads an export.
    ExportBytes,
    /// `pages.renderEncoded`: width, height and type in the result, the
    /// encoded bytes as the payload, answered as the `PageImage` a local
    /// engine gives the stamp plugin (`source.kind == "bytes"`).
    Encoded,
    /// A read that answers `null` for nothing there (`pieceInfo.read` for
    /// an application with no entry), else the result as the host sent it.
    OptionalResult,
    /// The first payload itself, as `{"$bytes": base64}`: a signature's
    /// `/Contents`, a digest, a revision's bytes.
    InlineBytes,
    /// The result, with the first payload added under this key as
    /// `{"$bytes": base64}` (`signatures.prepare`'s digest).
    ResultWithBytes(&'static str),
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
    "redaction.apply",
    "page.measure.viewports",
    "page.measure.setScale",
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
    "pieceInfo.read",
    "page.pieceInfo.read",
    "pieceInfo.update",
    "page.pieceInfo.update",
    "pages.setName",
    "pages.removeName",
    "pages.flatten",
    "download",
    "signatures.list",
    "signatures.contents",
    "signatures.digest",
    "signatures.revisionBytes",
    "signatures.analyze",
    "signatures.prepare",
    "signatures.complete",
    "signatures.abort",
    "document.version",
    "forms.setSignatureAppearance",
];

/// Feature names a plugin checks with `doc.has` that are neither a path nor
/// a namespace of one (`document.versioned`). One per line, in plan order.
pub const FLAGS: &[&str] = &["document.versioned"];

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
        // Removed upstream; owner 8.12 (`annotations.list`).
        "page.annotations.list" => ("annotations.listRawPage", Decode::Result),
        "annotations.listRawAll" => ("annotations.listRawAll", Decode::Result),
        "page.annotations.create" => ("annotations.create", Decode::Mutation),
        "page.annotations.update" => ("annotations.update", Decode::Mutation),
        "page.annotations.delete" => ("annotations.delete", Decode::Mutation),
        "page.annotations.downloadFile" => ("annotations.readFile", Decode::AttachmentContent),
        "redaction.apply" => ("redaction.apply", Decode::Mutation),
        "page.measure.viewports" => ("measure.viewports", Decode::Field("viewports")),
        "page.measure.setScale" => ("measure.setScale", Decode::Mutation),
        // Removed upstream; owner 8.18 (page structure through
        // `document.apply`).
        "pages.move" => ("pages.move", Decode::PageMutation),
        "pages.rotate" => ("pages.rotate", Decode::PageMutation),
        "pages.delete" => ("pages.delete", Decode::PageMutation),
        "pages.insert" => ("pages.insert", Decode::PageMutation),
        "pages.insertBlank" => ("pages.insertBlank", Decode::PageMutation),
        "pages.extract" => ("pages.extract", Decode::HostBytes),
        "actions.read" => ("actions.read", Decode::Result),
        "forms.list" => ("forms.list", Decode::Result),
        "forms.setValue" => ("forms.setValue", Decode::Mutation),
        "forms.reset" => ("forms.reset", Decode::Mutation),
        // Removed upstream; owner 8.20 (`document.apply`).
        "forms.applyEffects" => ("forms.applyEffects", Decode::Mutation),
        "forms.exportData" => ("forms.export", Decode::ExportBytes),
        "forms.importData" => ("forms.import", Decode::Mutation),
        "forms.repair" => ("forms.repair", Decode::Mutation),
        "forms.createField" => ("forms.createField", Decode::Mutation),
        "forms.updateField" => ("forms.updateField", Decode::Mutation),
        "forms.deleteField" => ("forms.deleteField", Decode::Mutation),
        // Removed upstream; owner 8.21 (`forms.addWidget`).
        "forms.attachWidget" => ("forms.attachWidget", Decode::Mutation),
        "forms.detachWidget" => ("forms.detachWidget", Decode::Mutation),
        "pieceInfo.read" | "page.pieceInfo.read" => ("pieceInfo.read", Decode::OptionalResult),
        // `void`, as the TypeScript resolves it, and no event to hand back.
        "pieceInfo.update" | "page.pieceInfo.update" => ("pieceInfo.update", Decode::Unit),
        "pages.setName" => ("pages.setName", Decode::PageMutation),
        "pages.removeName" => ("pages.removeName", Decode::PageMutation),
        // Not `PageMutation`: a flatten answers per-page results, each with
        // the `page` the stamp plugin reads, and no layout.
        "pages.flatten" => ("pages.flatten", Decode::Mutation),
        "download" => ("document.saveBuffer", Decode::HostBytes),
        "signatures.list" => ("signatures.list", Decode::Result),
        "signatures.contents" => ("signatures.contents", Decode::InlineBytes),
        "signatures.digest" => ("signatures.digest", Decode::InlineBytes),
        "signatures.revisionBytes" => ("signatures.revisionBytes", Decode::InlineBytes),
        "signatures.analyze" => ("signatures.analyze", Decode::Result),
        "signatures.prepare" => ("signatures.prepare", Decode::ResultWithBytes("digest")),
        "signatures.complete" => ("signatures.complete", Decode::Mutation),
        // Renamed `signatures.cancel` upstream; owner 8.26.
        "signatures.abort" => ("signatures.abort", Decode::Result),
        // `{ tag: 'document.version'; version }`: the kernel takes the version.
        "document.version" => ("document.version", Decode::Field("version")),
        "forms.setSignatureAppearance" => ("forms.setSignatureAppearance", Decode::Mutation),
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
        pon.map(|pon| PageRef::new(pon).to_json()).ok_or_else(|| {
            coded("invalid-input", format!("[epdf-viewer-host] {path} needs a page"))
        })
    };
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let mut payloads = Vec::new();
    let extra: Vec<(&str, Value)> = match kind {
        "metadata.update" => vec![("patch", arg(0))],
        "pages.text" | "pages.geometry" => vec![("page", page()?)],
        "pages.render" => vec![("page", page()?), ("options", render_options(arg(0)))],
        "search.query" => vec![("request", search_request(arg(0))?)],
        "annotations.listRawPage" => vec![("page", page()?)],
        "annotations.create" => {
            let (draft, resources) = normalize_binary(arg(0), &mut payloads)?;
            let draft = wire_refs_in(path, draft)?;
            vec![("page", page()?), ("draft", draft), ("resources", resources)]
        }
        "annotations.update" => {
            let (patch, resources) = normalize_binary(arg(1), &mut payloads)?;
            let patch = wire_refs_in(path, patch)?;
            vec![("ref", wire_ref(path, arg(0))?), ("patch", patch), ("resources", resources)]
        }
        "annotations.delete" => vec![("ref", wire_ref(path, arg(0))?)],
        "annotations.readFile" => {
            vec![("page", page()?), ("ref", wire_ref(path, arg(0))?)]
        }
        "redaction.apply" => vec![("scope", redaction_scope_out(path, arg(0))?)],
        "measure.viewports" => vec![("page", page()?)],
        "measure.setScale" => vec![("page", page()?)],
        "pages.move" => vec![
            ("pages", page_refs(path, &arg(0))?),
            ("destIndex", whole(path, "destIndex", &arg(1))?),
        ],
        "pages.rotate" => vec![
            ("pages", page_refs(path, &arg(0))?),
            ("rotation", whole(path, "rotation", &arg(1))?),
        ],
        "pages.delete" | "pages.extract" => vec![("pages", page_refs(path, &arg(0))?)],
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
            whole_placements(path, &mut draft)?;
            vec![("draft", draft)]
        }
        "forms.updateField" => {
            vec![("ref", wire_field_ref(path, arg(0))?), ("patch", arg(1))]
        }
        "forms.attachWidget" | "forms.detachWidget" => {
            // The widget's `AnnotationRef`, as `FormMutator` takes it; the
            // engine refuses one that is not by object number.
            let widget = arg(1);
            let on_state = widget.get("onState").cloned().unwrap_or(Value::Null);
            vec![
                ("ref", wire_field_ref(path, arg(0))?),
                ("widget", wire_ref(path, without(widget, "onState"))?),
                ("onState", on_state),
            ]
        }
        "pieceInfo.read" if path == "page.pieceInfo.read" => {
            vec![("page", page()?), ("application", arg(0))]
        }
        "pieceInfo.read" => vec![("application", arg(0))],
        "pieceInfo.update" if path == "page.pieceInfo.update" => {
            vec![("page", page()?), ("application", arg(0)), ("patch", arg(1))]
        }
        "pieceInfo.update" => vec![("application", arg(0)), ("patch", arg(1))],
        "pages.setName" => {
            let input = arg(0);
            let page = input.get("page").and_then(PageRef::from_json).ok_or_else(|| {
                coded("invalid-input", format!("[epdf-viewer-host] {path} needs a page"))
            })?;
            vec![
                ("name", input.get("name").cloned().unwrap_or(Value::Null)),
                ("page", page.to_json()),
                ("replace", input.get("replace").cloned().unwrap_or(Value::Null)),
            ]
        }
        "pages.removeName" => vec![("name", arg(0).get("name").cloned().unwrap_or(Value::Null))],
        "pages.flatten" => {
            vec![("pages", page_refs(path, &arg(0))?), ("usage", arg(1))]
        }
        "document.saveBuffer" => vec![("mode", save_mode(path, &arg(0))?)],
        "signatures.list" => {
            vec![("workingCopy", arg(0).get("workingCopy").cloned().unwrap_or(Value::Null))]
        }
        "signatures.contents" => vec![("ref", wire_field_ref(path, arg(0))?)],
        "signatures.digest" => {
            vec![("ref", wire_field_ref(path, arg(0))?), ("algorithm", arg(1))]
        }
        "signatures.revisionBytes" => {
            vec![("revisionIndex", whole(path, "revisionIndex", &arg(0))?)]
        }
        "signatures.analyze" => {
            let mut input = arg(0);
            for pointer in ["/since/signatureIndex", "/since/revisionIndex", "/until/revisionIndex"]
            {
                whole_at(path, &mut input, pointer)?;
            }
            vec![("input", input)]
        }
        "signatures.prepare" => {
            let mut input = wire_field_ref(path, arg(0))?;
            for pointer in ["/certify/permission", "/lock/permission", "/contentsSize"] {
                whole_at(path, &mut input, pointer)?;
            }
            // The artwork travels as a payload; no artwork, no appearance.
            if let Some(appearance) = input.get_mut("appearance") {
                let pdf = appearance.get("pdf").and_then(epdf_kernel::bytes::Bytes::from_json);
                match pdf {
                    Some(pdf) => {
                        payloads.push(pdf.0);
                        let page = optional_whole(path, "pageIndex", appearance.get("pageIndex"))?;
                        *appearance = serde_json::json!({ "pageIndex": if page.is_null() { 0.into() } else { page } });
                    }
                    None => {
                        input.as_object_mut().map(|m| m.remove("appearance"));
                    }
                }
            }
            vec![("input", input)]
        }
        "signatures.complete" => {
            let mut input = arg(0);
            payloads.push(bytes_at(&input, "cms", path)?);
            whole_at(path, &mut input, "/expectedVersion/editsVersion")?;
            vec![("input", without(input, "cms"))]
        }
        "signatures.abort" => vec![("signingId", arg(0))],
        "forms.setSignatureAppearance" => {
            payloads.push(bytes_at(&arg(1), "pdf", path)?);
            vec![
                ("ref", wire_field_ref(path, arg(0))?),
                ("pageIndex", optional_whole(path, "pageIndex", arg(1).get("pageIndex"))?),
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
    // `PdfMeasure | null`: a cleared scale is an explicit `null`, which the
    // loop above would drop.
    if kind == "measure.setScale" {
        let measure = arg(0);
        let measure = if epdf_kernel::is_undefined(&measure) { Value::Null } else { measure };
        request.insert("measure".into(), measure);
    }
    let mut request = Value::Object(request);
    to_wire_refs(&mut request);
    Ok(WireCall { request, payloads, decode })
}

/// Send one translated request to `host`: through [`Host::call`], or through
/// [`Host::call_legacy`] for a kind upstream removed, which the wire answers
/// as unknown. `route` names each one's owner sub-task.
pub fn call_host(host: &mut Host, json: &str, payloads: Vec<Vec<u8>>) -> HostResponse {
    let Ok(mut request) = serde_json::from_str::<Value>(json) else {
        // Not JSON: the host's own refusal says so.
        return host.call(json, payloads);
    };
    let kind = request.get("kind").and_then(Value::as_str).unwrap_or_default().to_owned();
    if epdf_engine::host::LEGACY_KINDS.contains(&kind.as_str()) {
        return host.call_legacy(json, payloads);
    }
    if base_index_refs(host, &mut request) {
        return host.call(&request.to_string(), payloads);
    }
    host.call(json, payloads)
}

/// The kernel's `nm` and `index` annotation refs, which the wire no longer
/// takes (bc0057f3a), as the `baseIndex` refs upstream names the same
/// annotations by. Answers whether it changed anything.
///
/// The host's annotation listing still answers an inline annotation (one
/// with no object number) by `nm`, or by `index` when it has no `/NM`, and
/// the kernel sends those back. Upstream names it by its position in the
/// page's `/Annots` instead, which the host resolves on a page it has not
/// restructured (`resolve::base_index_position`). An `index` ref is that
/// position already; an `nm` ref's position is looked up in the page's
/// listing. Its revision token is dropped: upstream has none.
///
/// Dead once Task 8.12 makes the listing answer `baseIndex` refs and Task 13
/// moves the kernel onto them.
fn base_index_refs(host: &mut Host, request: &mut Value) -> bool {
    let doc = request.get("docId").cloned().unwrap_or(Value::Null);
    let layer = request.get("layerName").cloned();
    let mut listings: Vec<(Value, Value)> = Vec::new();
    let mut changed = false;
    let mut respell = |target: &mut Value| -> bool {
        let Value::Object(map) = target else { return false };
        let position = match map.get("kind").and_then(Value::as_str) {
            Some("index") if map.contains_key("page") => map.get("index").cloned(),
            Some("nm") if map.contains_key("page") => {
                let page = map["page"].clone();
                let listed = match listings.iter().find(|(p, _)| *p == page) {
                    Some((_, listed)) => listed.clone(),
                    None => {
                        let mut list = serde_json::json!({ "kind": "annotations.listRawPage",
                            "docId": doc, "page": page });
                        if let Some(layer) = &layer {
                            list["layerName"] = layer.clone();
                        }
                        let response = host.call_legacy(&list.to_string(), Vec::new());
                        let listed = serde_json::from_str::<Value>(&response.json)
                            .ok()
                            .and_then(|mut e| e.get_mut("result").map(Value::take))
                            .unwrap_or(Value::Null);
                        listings.push((page, listed.clone()));
                        listed
                    }
                };
                let wanted = Value::Object(map.clone());
                listed["annotations"]
                    .as_array()
                    .and_then(|all| all.iter().find(|a| a["ref"] == wanted))
                    .and_then(|a| a.get("index").cloned())
            }
            _ => None,
        };
        let Some(position) = position.filter(Value::is_u64) else { return false };
        let page = map["page"].clone();
        *target = serde_json::json!({ "kind": "baseIndex", "page": page, "baseIndex": position });
        true
    };
    fn walk(value: &mut Value, respell: &mut dyn FnMut(&mut Value) -> bool) -> bool {
        if respell(value) {
            return true;
        }
        let mut changed = false;
        match value {
            Value::Object(map) => map.values_mut().for_each(|v| changed |= walk(v, respell)),
            Value::Array(items) => items.iter_mut().for_each(|v| changed |= walk(v, respell)),
            _ => {}
        }
        changed
    }
    changed |= walk(request, &mut respell);
    changed
}

/// The kernel's ref spelling as the host reads it: in every object tagged
/// `objectNumber`, a `PageRef`'s `pageObjectNumber` and an annotation ref's
/// `annotObjectNumber` become `objectNumber`. A form field ref keeps
/// `fieldObjectNumber`, which is still what the host reads.
fn to_wire_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("objectNumber")
                && !map.contains_key("objectNumber")
                && let Some(number) =
                    map.remove("pageObjectNumber").or_else(|| map.remove("annotObjectNumber"))
            {
                map.insert("objectNumber".into(), number);
            }
            map.values_mut().for_each(to_wire_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(to_wire_refs),
        _ => {}
    }
}

/// The host's ref spelling as the kernel reads it, the inverse of
/// [`to_wire_refs`]: an `objectNumber` beside a `page` is an annotation
/// ref's (`annotObjectNumber`), and one without is a `PageRef`'s
/// (`pageObjectNumber`). That rule holds while the host answers form field
/// refs as `fieldObjectNumber`; the sub-task that respells those changes it.
fn to_kernel_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("objectNumber")
                && let Some(number) = map.remove("objectNumber")
            {
                let key =
                    if map.contains_key("page") { "annotObjectNumber" } else { "pageObjectNumber" };
                map.insert(key.into(), number);
            }
            map.values_mut().for_each(to_kernel_refs);
        }
        Value::Array(items) => items.iter_mut().for_each(to_kernel_refs),
        _ => {}
    }
}

/// `page.render.image` on the stamp plugin's asset engine: the same options
/// as `pages.render`, plus the encode the plugin asked for (`format`,
/// default png). Png is the one encoder the asset engine has.
pub fn translate_encoded_render(
    doc: &str,
    pon: Option<u32>,
    args: &[Value],
    job: u64,
) -> Result<WireCall, Error> {
    let page = pon.ok_or_else(|| {
        coded("invalid-input", "[epdf-viewer-host] page.render.image needs a page".into())
    })?;
    let options = args.first().cloned().unwrap_or(Value::Null);
    let format = options.get("format").and_then(Value::as_str).unwrap_or("png");
    if format != "png" {
        return Err(coded(
            "unsupported",
            format!("[epdf-viewer-host] the asset engine encodes png only, not {format}"),
        ));
    }
    let mut request = serde_json::json!({
        "kind": "pages.renderEncoded",
        "jobId": job,
        "docId": doc,
        "page": PageRef::new(page).to_json(),
        "options": render_options(options),
        "encode": { "format": "png" },
    });
    to_wire_refs(&mut request);
    Ok(WireCall { request, payloads: Vec::new(), decode: Decode::Encoded })
}

/// `download`'s `{mode}` as the host's `SaveMode`, TypeScript's `PdfSaveMode`
/// (`incremental` or `rewrite`) on both sides. No options, or no mode, stays
/// the rewrite the plugins' own downloads (the stamp libraries) have always
/// written.
fn save_mode(path: &str, options: &Value) -> Result<Value, Error> {
    match options.get("mode").filter(|m| !m.is_null() && !epdf_kernel::is_undefined(m)) {
        None => Ok("rewrite".into()),
        Some(mode) => match mode.as_str() {
            Some(mode @ ("incremental" | "rewrite")) => Ok(mode.into()),
            _ => Err(coded(
                "invalid-input",
                format!("[epdf-viewer-host] {path}'s mode is not incremental or rewrite: {mode}"),
            )),
        },
    }
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

/// `value` without `key`.
fn without(mut value: Value, key: &str) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove(key);
    }
    value
}

/// The `Bytes` JSON under `key`, as bytes.
fn bytes_at(value: &Value, key: &str, path: &str) -> Result<Vec<u8>, Error> {
    value.get(key).and_then(epdf_kernel::bytes::Bytes::from_json).map(|b| b.0).ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} needs {key} bytes"))
    })
}

/// The number at a JSON `pointer` inside `value`, made whole; absent or
/// null is left as it is.
fn whole_at(path: &str, value: &mut Value, pointer: &str) -> Result<(), Error> {
    if let Some(slot) = value.pointer_mut(pointer)
        && !slot.is_null()
    {
        let field = pointer.rsplit('/').next().unwrap_or(pointer);
        *slot = whole(path, field, slot)?;
    }
    Ok(())
}

/// One `PageRef`, re-spelled so its object number is the whole integer the
/// host's serde parses (the kernel's numbers are JavaScript numbers).
fn page_ref(path: &str, value: &Value) -> Result<Value, Error> {
    PageRef::from_json(value).map(PageRef::to_json).ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path}: not a PageRef: {value}"))
    })
}

/// `PageRef[]`, each re-spelled as `page_ref` does.
fn page_refs(path: &str, refs: &Value) -> Result<Value, Error> {
    let refs = refs.as_array().ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a list of pages: {refs}"))
    })?;
    refs.iter().map(|r| page_ref(path, r)).collect::<Result<Vec<_>, _>>().map(Value::Array)
}

/// A PDF given as `Bytes` JSON: `{"$bytes": base64}` from the caller, or
/// `{"$hostBytes": id}` from an extract, which this takes out of the table.
fn pdf_bytes(path: &str, value: &Value) -> Result<Vec<u8>, Error> {
    epdf_kernel::bytes::Bytes::from_json(value).map(|b| b.0).ok_or_else(|| {
        coded("invalid-input", format!("[epdf-viewer-host] {path} takes a PDF's bytes: {value}"))
    })
}

/// A `SearchRequest` as the plugin sends it, its `startPage` a `PageRef`
/// with a whole object number. The rest goes as it is.
fn search_request(mut request: Value) -> Result<Value, Error> {
    let Some(start) = request.get_mut("startPage").filter(|s| !s.is_null()) else {
        return Ok(request);
    };
    *start = page_ref("search.query's startPage", start)?;
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

/// An annotation ref with its JavaScript numbers (`annotObjectNumber`,
/// `index`, a revision's `generation`, its pages' `pageObjectNumber`, each
/// sent as `17.0`) as the whole integers the host's serde parses.
fn wire_ref(path: &str, mut target: Value) -> Result<Value, Error> {
    whole_refs(path, &mut target)?;
    Ok(target)
}

/// A draft or patch with every ref inside it (a reply's `inReplyTo`, a
/// link's `goto` destination) whole, as `wire_ref` makes the call's own ref.
/// Other members keep their numbers.
fn wire_refs_in(path: &str, mut value: Value) -> Result<Value, Error> {
    whole_refs(path, &mut value)?;
    Ok(value)
}

/// The JavaScript numbers of every annotation ref (an object whose `kind`
/// is `objectNumber`, `nm` or `index`, beside a `page`), of its revision
/// token, and of every `PageRef`, as whole integers.
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
            let kind = map.get("kind").and_then(Value::as_str).map(str::to_owned);
            let kind = kind.as_deref();
            if kind == Some("objectNumber") && map.contains_key("pageObjectNumber") {
                whole_keys(path, map, &["pageObjectNumber"])?;
            }
            let is_ref =
                matches!(kind, Some("objectNumber" | "nm" | "index")) && map.contains_key("page");
            if is_ref {
                whole_keys(path, map, &["annotObjectNumber", "index"])?;
                if let Some(Value::Object(revision)) = map.get_mut("revision") {
                    whole_keys(path, revision, &["generation"])?;
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

/// A field draft's widget placements, each `page: PageRef` with the whole
/// object number `WidgetPlacement` parses.
fn whole_placements(path: &str, draft: &mut Value) -> Result<(), Error> {
    let whole_page = |placement: &mut Value| -> Result<(), Error> {
        if let Some(page) = placement.get_mut("page") {
            *page = page_ref(path, page)?;
        }
        Ok(())
    };
    if let Some(widget) = draft.get_mut("widget") {
        whole_page(widget)?;
    }
    if let Some(Value::Array(widgets)) = draft.get_mut("widgets") {
        for widget in widgets {
            whole_page(widget)?;
        }
    }
    Ok(())
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

/// The kernel's apply scope as the host's `RedactionScope` reads it:
/// `{kind: "pages", pages: [PageRef]}` with whole object numbers, or an
/// annotations scope whose refs go whole.
fn redaction_scope_out(path: &str, scope: Value) -> Result<Value, Error> {
    match scope.get("kind").and_then(Value::as_str) {
        Some("pages") => {
            let pages = scope.get("pages").cloned().unwrap_or(Value::Null);
            Ok(serde_json::json!({ "kind": "pages", "pages": page_refs(path, &pages)? }))
        }
        Some("annotations") => wire_refs_in(path, scope),
        _ => Err(coded(
            "invalid-input",
            format!("[epdf-viewer-host] {path} takes a pages or annotations scope: {scope}"),
        )),
    }
}

/// That `object[key]` is a `PageRef`, as the kernel's page registry, the
/// search plugin and the page-edit plugin read it.
fn require_page_ref(object: &Value, key: &str, what: &str) -> Result<(), Error> {
    match object.get(key) {
        Some(page) if PageRef::from_json(page).is_some() => Ok(()),
        _ => Err(malformed(format!("{what} without a PageRef \"{key}\": {object}"))),
    }
}

/// That `object.layout.pages` is there, each page named by its `ref`.
fn require_layout_refs(object: &Value, what: &str) -> Result<(), Error> {
    let Some(Value::Array(pages)) = object.get("layout").and_then(|l| l.get("pages")) else {
        return Err(malformed(format!("{what} without a layout")));
    };
    pages.iter().try_for_each(|page| require_page_ref(page, "ref", "a layout page"))
}

/// Read a host response into what the call's caller expects.
pub fn decode(decode: &Decode, response: HostResponse) -> Result<Decoded, Error> {
    let mut envelope: Value = serde_json::from_str(&response.json)
        .map_err(|e| malformed(format!("unreadable JSON: {e}")))?;
    to_kernel_refs(&mut envelope);
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("Unknown");
        let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
        return Err(Error::engine(code, format!("{code}: {message}")));
    }
    // A notice the host publishes beside a result that is not a
    // mutation's (`signature.prepared`, `signature.aborted`). A mutation's
    // event rides inside its result and is taken below.
    let notice = envelope.get_mut("event").map(Value::take);
    if let Decode::Unit = decode {
        return Ok(Decoded { value: epdf_kernel::undefined(), event: notice });
    }
    let mut result = match envelope.get_mut("result").map(Value::take) {
        Some(Value::Object(result)) => result,
        Some(Value::Null) if matches!(decode, Decode::OptionalResult) => {
            return Ok(Decoded { value: Value::Null, event: None });
        }
        Some(other) => return Err(malformed(format!("a result that is not an object: {other}"))),
        None => return Err(malformed("neither a result nor an error".into())),
    };
    let take = |result: &mut Map<String, Value>, name: &str| {
        result.remove(name).ok_or_else(|| malformed(format!("a result without \"{name}\"")))
    };
    let value = match decode {
        Decode::Unit => unreachable!("answered above"),
        Decode::Result | Decode::OptionalResult => Value::Object(result),
        Decode::Field(name) => take(&mut result, name)?,
        Decode::PageList => {
            let Some(Value::Array(pages)) = result.get("pages") else {
                return Err(malformed("a page list without \"pages\"".into()));
            };
            pages.iter().try_for_each(|page| require_page_ref(page, "ref", "a page"))?;
            Value::Object(result)
        }
        Decode::SearchSlice => {
            let Some(Value::Array(matches)) = result.get("matches") else {
                return Err(malformed("a search slice without \"matches\"".into()));
            };
            matches.iter().try_for_each(|hit| require_page_ref(hit, "page", "a search match"))?;
            Value::Object(result)
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
            let value = take(&mut result, "result")?;
            require_layout_refs(&value, "a page mutation's result")?;
            let event = result.remove("event");
            if let Some(event) = event.as_ref() {
                require_layout_refs(event, "a page event")?;
            }
            return Ok(Decoded { value, event });
        }
        Decode::InlineBytes => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("bytes without their payload".into()))?;
            epdf_kernel::bytes::Bytes(bytes).to_json()
        }
        Decode::ResultWithBytes(key) => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed(format!("a result without its {key} payload")))?;
            result.insert((*key).into(), epdf_kernel::bytes::Bytes(bytes).to_json());
            Value::Object(result)
        }
        Decode::HostBytes => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("bytes without their payload".into()))?;
            serde_json::json!({ "$hostBytes": HostTable::put(bytes) })
        }
        Decode::Encoded => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("an encoded render without its bytes".into()))?;
            let content_type = take(&mut result, "mimeType")?;
            serde_json::json!({
                "contentType": content_type,
                "width": result.remove("width").unwrap_or(Value::Null),
                "height": result.remove("height").unwrap_or(Value::Null),
                "source": { "kind": "bytes", "bytes": epdf_kernel::bytes::Bytes(bytes).to_json() },
            })
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
    Ok(Decoded { value, event: notice })
}
