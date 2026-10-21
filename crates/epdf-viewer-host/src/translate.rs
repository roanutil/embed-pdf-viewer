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
//! catch-up: a `PageRef`'s number is `pageObjectNumber`, an annotation
//! ref's is `annotObjectNumber` and a form field ref's is
//! `fieldObjectNumber`. The host spells all three `objectNumber`, as
//! upstream does (595bdc6c7). `translate` respells a request on the way out
//! and `decode` respells an answer on the way back, so the kernel keeps its
//! spelling until Task 13 ports it.
//!
//! A kernel path that maps onto a kind upstream removed goes to the host
//! through `Host::call_legacy` (see [`call_host`]), each until the sub-task
//! named beside it in `route` moves it to the kind that replaced it. The
//! kernel's page structure edits are `document.apply` changes of one page
//! op each ([`kernel_pages`]).
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
    /// The first of the result's `pages`: one page's annotation listing.
    FirstPage,
    /// One member of the result, which must be there.
    Field(&'static str),
    /// A `PageListSnapshot`, which must name every page by its `ref`.
    PageList,
    /// A `SearchSlice`, which must name every match's `page`.
    SearchSlice,
    /// A mutation: `{result, mutationSequence, event}`. Answers `result`
    /// and hands back the event to publish.
    Mutation,
    /// [`Decode::Result`] for an answer that holds form records (a form,
    /// signatures): the field records and field refs go back to the
    /// kernel's spelling first ([`to_kernel_form_records`]).
    FormResult,
    /// [`Decode::Mutation`] for a form or signature write, whose result and
    /// event hold form records.
    FormMutation,
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
        // Answered in the kernel's shape by `call_host` (`kernel_annotations`).
        "page.annotations.list" => ("annotations.list", Decode::FirstPage),
        "annotations.listRawAll" => ("annotations.list", Decode::Result),
        "page.annotations.create" => ("annotations.create", Decode::Mutation),
        "page.annotations.update" => ("annotations.update", Decode::Mutation),
        "page.annotations.delete" => ("annotations.delete", Decode::Mutation),
        "page.annotations.downloadFile" => ("annotations.readFile", Decode::AttachmentContent),
        "redaction.apply" => ("redaction.apply", Decode::Mutation),
        "page.measure.viewports" => ("measure.viewports", Decode::Field("viewports")),
        "page.measure.setScale" => ("measure.setScale", Decode::Mutation),
        // Page structure: one page op of a `document.apply` change each
        // (119685ea4), answered in the kernel's shape by `kernel_pages`.
        "pages.move" | "pages.rotate" | "pages.delete" | "pages.insert" | "pages.insertBlank" => {
            ("document.apply", Decode::PageMutation)
        }
        "pages.extract" => ("pages.extract", Decode::HostBytes),
        "actions.read" => ("actions.read", Decode::Result),
        "forms.list" => ("forms.list", Decode::FormResult),
        "forms.setValue" => ("forms.setValue", Decode::FormMutation),
        "forms.reset" => ("forms.reset", Decode::FormMutation),
        // Removed upstream (98e5c270c): a `document.apply` change, answered
        // in the kernel's shape by `call_host` (`kernel_forms`).
        "forms.applyEffects" => ("forms.applyEffects", Decode::FormMutation),
        "forms.exportData" => ("forms.export", Decode::ExportBytes),
        "forms.importData" => ("forms.import", Decode::FormMutation),
        "forms.repair" => ("forms.repair", Decode::FormMutation),
        "forms.createField" => ("forms.createField", Decode::FormMutation),
        "forms.updateField" => ("forms.updateField", Decode::FormMutation),
        "forms.deleteField" => ("forms.deleteField", Decode::FormMutation),
        // Removed upstream; owner 8.21 (`forms.addWidget`).
        "forms.attachWidget" => ("forms.attachWidget", Decode::FormMutation),
        "forms.detachWidget" => ("forms.detachWidget", Decode::FormMutation),
        "pieceInfo.read" | "page.pieceInfo.read" => ("pieceInfo.read", Decode::OptionalResult),
        // `void`, as the TypeScript resolves it, and no event to hand back.
        "pieceInfo.update" | "page.pieceInfo.update" => ("pieceInfo.update", Decode::Unit),
        "pages.setName" => ("pages.setName", Decode::PageMutation),
        "pages.removeName" => ("pages.removeName", Decode::PageMutation),
        // Not `PageMutation`: a flatten answers per-page results, each with
        // the `page` the stamp plugin reads, and no layout.
        "pages.flatten" => ("pages.flatten", Decode::Mutation),
        "download" => ("document.saveBuffer", Decode::HostBytes),
        "signatures.list" => ("signatures.list", Decode::FormResult),
        "signatures.contents" => ("signatures.contents", Decode::InlineBytes),
        "signatures.digest" => ("signatures.digest", Decode::InlineBytes),
        "signatures.revisionBytes" => ("signatures.revisionBytes", Decode::InlineBytes),
        "signatures.analyze" => ("signatures.analyze", Decode::FormResult),
        "signatures.prepare" => ("signatures.prepare", Decode::ResultWithBytes("digest")),
        "signatures.complete" => ("signatures.complete", Decode::FormMutation),
        // Renamed `signatures.cancel` upstream; owner 8.26.
        "signatures.abort" => ("signatures.abort", Decode::Result),
        // `{ tag: 'document.version'; version }`: the kernel takes the version.
        "document.version" => ("document.version", Decode::Field("version")),
        "forms.setSignatureAppearance" => ("forms.setSignatureAppearance", Decode::FormMutation),
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
    if kind == "document.apply" {
        let op = kernel_pages::op(path, args, &mut payloads)?;
        let mut request = serde_json::json!({
            "kind": kind, "effect": "write", "jobId": job, "docId": doc,
            "change": { "ops": [op] },
        });
        to_wire_refs(&mut request);
        return Ok(WireCall { request, payloads, decode });
    }
    let extra: Vec<(&str, Value)> = match kind {
        "metadata.update" => vec![("patch", arg(0))],
        "pages.text" | "pages.geometry" => vec![("page", page()?)],
        "pages.render" => vec![("page", page()?), ("options", render_options(arg(0)))],
        "search.query" => vec![("request", search_request(arg(0))?)],
        "annotations.list" if path == "page.annotations.list" => {
            vec![("pages", Value::Array(vec![page()?]))]
        }
        "annotations.list" => Vec::new(),
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
        "pages.extract" => vec![("pages", page_refs(path, &arg(0))?)],
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

/// The host kinds whose answers hold form field records, decoded as
/// [`Decode::FormResult`] or [`Decode::FormMutation`].
const FORM_RECORD_KINDS: &[&str] = &[
    "forms.list",
    "forms.setValue",
    "forms.reset",
    "forms.applyEffects",
    "forms.import",
    "forms.repair",
    "forms.createField",
    "forms.updateField",
    "forms.attachWidget",
    "forms.detachWidget",
    "forms.setSignatureAppearance",
];

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
    // `translate` builds a `document.apply` for the kernel's page edits
    // only.
    if kind == "document.apply" {
        return kernel_pages::call(host, &request, payloads);
    }
    if let Some(response) = metadata::kernel_call(host, &kind, &request) {
        return response;
    }
    if kernel_forms::KINDS.contains(&kind.as_str()) {
        let response = kernel_forms::call(host, &kind, &request, payloads);
        return page_space::with_raw_field_flags(host, &request, response);
    }
    if kind == "annotations.list" {
        return kernel_annotations::listing(host, &request);
    }
    let respelled = base_index_refs(host, &mut request);
    if kind == "annotations.create" {
        return kernel_create::call(host, &request, payloads);
    }
    if kind == "annotations.update" {
        return kernel_update::call(host, &request, payloads);
    }
    if kind == "annotations.delete" {
        return kernel_delete::call(host, &request, payloads);
    }
    let moved = page_space::render_target_to_page_space(host, &mut request);
    let response = if respelled || moved {
        host.call(&request.to_string(), payloads)
    } else {
        host.call(json, payloads)
    };
    let response = page_space::answer_to_file_space(host, &kind, &request, response);
    if FORM_RECORD_KINDS.contains(&kind.as_str()) {
        page_space::with_raw_field_flags(host, &request, response)
    } else {
        response
    }
}

/// The kernel's metadata, and the host's (Task 8.6).
///
/// The kernel reads one `DocumentMetadata` with the custom keys under
/// `custom` and the dates as `created` and `modified`, and writes one patch
/// in the same spelling. The host answers upstream's: `metadata.read` the
/// standard fields with `createdAt` and `modifiedAt`, `metadata.readCustom`
/// the custom keys, and the two writes apart (a4b8259f7, fd175ba50).
/// [`call_host`] reads both and folds them, and runs a kernel patch as one
/// `document.apply` change, so a patch touching both halves is still one
/// write. Dead once Task 13 ports the kernel.
mod metadata {
    use epdf_engine::host::{Host, HostResponse};
    use serde_json::{Map, Value, json};

    /// The kernel's `metadata.read` and `metadata.update`, answered in the
    /// kernel's spelling; `None` for any other kind.
    pub(super) fn kernel_call(
        host: &mut Host,
        kind: &str,
        request: &Value,
    ) -> Option<HostResponse> {
        let answer = match kind {
            "metadata.read" => {
                read(host, request).map(|metadata| (json!({ "result": metadata }), Vec::new()))
            }
            "metadata.update" => update(host, request),
            _ => return None,
        };
        Some(match answer {
            Ok((envelope, payloads)) => HostResponse { json: envelope.to_string(), payloads },
            Err(response) => response,
        })
    }

    /// The same document as `request`, and its job id.
    fn on_document(request: &Value, kind: &str) -> Value {
        let mut call = json!({ "kind": kind, "jobId": request.get("jobId").cloned() });
        for key in ["docId", "layerName"] {
            if let Some(value) = request.get(key) {
                call[key] = value.clone();
            }
        }
        call
    }

    /// One call, answering its `result`, or the whole response when it is
    /// an error.
    fn send(host: &mut Host, call: &Value) -> Result<(Value, HostResponse), HostResponse> {
        let response = host.call(&call.to_string(), Vec::new());
        let envelope: Value = serde_json::from_str(&response.json).unwrap_or(Value::Null);
        match envelope.get("result") {
            Some(result) if envelope.get("error").is_none() => Ok((result.clone(), response)),
            _ => Err(response),
        }
    }

    /// Both halves of the dictionary, in the kernel's spelling.
    fn read(host: &mut Host, request: &Value) -> Result<Value, HostResponse> {
        let (mut metadata, _) = send(host, &on_document(request, "metadata.read"))?;
        let (custom, _) = send(host, &on_document(request, "metadata.readCustom"))?;
        if let Some(fields) = metadata.as_object_mut() {
            let created = fields.remove("createdAt").unwrap_or(Value::Null);
            let modified = fields.remove("modifiedAt").unwrap_or(Value::Null);
            fields.insert("created".into(), created);
            fields.insert("modified".into(), modified);
            fields.insert("custom".into(), custom);
        }
        Ok(metadata)
    }

    /// A kernel patch as one change: the standard fields as a
    /// `metadata.update` op (the dates renamed), the custom keys as a
    /// `metadata.updateCustom` op. Answers the mutation the kernel decodes:
    /// the whole dictionary, the sequence, and one `metadata.updated` event,
    /// with the change's payloads (a layer's artifact).
    fn update(host: &mut Host, request: &Value) -> Result<(Value, Vec<Vec<u8>>), HostResponse> {
        let mut standard = match request.get("patch") {
            Some(Value::Object(patch)) => patch.clone(),
            _ => Map::new(),
        };
        let custom = standard.remove("custom");
        for (kernel, wire) in [("created", "createdAt"), ("modified", "modifiedAt")] {
            if let Some(value) = standard.remove(kernel) {
                standard.insert(wire.into(), value);
            }
        }
        let mut ops = Vec::new();
        if !standard.is_empty() || !matches!(custom, Some(Value::Object(_))) {
            ops.push(json!({ "type": "metadata.update", "patch": standard }));
        }
        if let Some(custom @ Value::Object(_)) = custom {
            ops.push(json!({ "type": "metadata.updateCustom", "patch": custom }));
        }
        let mut apply = on_document(request, "document.apply");
        apply["effect"] = json!("write");
        apply["change"] = json!({ "ops": ops });
        let (applied, response) = send(host, &apply)?;
        let metadata = read(host, request)?;
        let mut answer = json!({
            "result": { "metadata": metadata },
            "mutationSequence": applied["mutationSequence"],
        });
        if let Some(origin) =
            applied["events"].as_array().and_then(|e| e.last()).map(|e| &e["origin"])
        {
            answer["event"] =
                json!({ "type": "metadata.updated", "metadata": metadata, "origin": origin });
        }
        Ok((json!({ "result": answer }), response.payloads))
    }
}

/// The kernel's form value writes (Task 8.20).
///
/// The kernel writes a value in this branch's old spelling
/// (`{type: "text", value}`, `{type: "toggle", state}` by on-state,
/// `{type: "choice", values}`), resets one field, and sends a script's
/// effects as one `forms.applyEffects` batch that answers each effect's
/// status. Upstream takes a value as a read returns it (`{value}`,
/// `{checked}`, `{selectedValues}`), resets `refs`, and runs a script's
/// effects as `document.apply` ops, all or none (98e5c270c). [`call`]
/// reads the form to spell each value as its field takes it, sends the
/// upstream request, and answers in the kernel's shape: `{field,
/// changedWidgets}` and a `form.valueChanged` event for a value write or a
/// reset; `{results, changedWidgets, meta}` and `form.effectsApplied` for a
/// batch, every effect `rejected` with the refusal when the change was
/// refused, since nothing of it landed; and `form.repaired` for a repair,
/// with its counter `needAppearancesCleared`. Field records go back to the
/// file's coordinates. Dead once Task 13 ports the kernel.
mod kernel_forms {
    use epdf_engine::host::{Host, HostResponse};
    use serde_json::{Value, json};

    /// The kinds [`call`] answers.
    pub(super) const KINDS: &[&str] =
        &["forms.setValue", "forms.reset", "forms.applyEffects", "forms.repair"];

    /// The kernel's call `kind`, one of [`KINDS`], answered in its shape.
    pub(super) fn call(
        host: &mut Host,
        kind: &str,
        request: &Value,
        payloads: Vec<Vec<u8>>,
    ) -> HostResponse {
        let answer = match kind {
            "forms.setValue" => set_value(host, request),
            "forms.reset" => reset(host, request),
            "forms.applyEffects" => apply_effects(host, request),
            _ => repair(host, request, payloads),
        };
        match answer {
            Ok((envelope, payloads)) => HostResponse { json: envelope.to_string(), payloads },
            Err(response) => response,
        }
    }

    type Answer = Result<(Value, Vec<Vec<u8>>), HostResponse>;

    /// One call, answering the envelope's `result` and the response, or the
    /// whole response when it is an error.
    fn send(host: &mut Host, call: &Value) -> Result<(Value, HostResponse), HostResponse> {
        let response = host.call(&call.to_string(), Vec::new());
        let mut envelope: Value = serde_json::from_str(&response.json).unwrap_or(Value::Null);
        if envelope.get("error").is_some() {
            return Err(response);
        }
        match envelope.get_mut("result") {
            Some(result) => {
                let result = result.take();
                Ok((result, response))
            }
            None => Err(response),
        }
    }

    fn on_document(request: &Value, kind: &str) -> Value {
        let mut call = json!({ "kind": kind, "jobId": request.get("jobId").cloned() });
        for key in ["docId", "layerName", "opId", "authority"] {
            if let Some(value) = request.get(key) {
                call[key] = value.clone();
            }
        }
        call
    }

    /// The form's fields, as `forms.list` reads them.
    fn fields(host: &mut Host, request: &Value) -> Result<Vec<Value>, HostResponse> {
        let (form, _) = send(host, &on_document(request, "forms.list"))?;
        Ok(form["fields"].as_array().cloned().unwrap_or_default())
    }

    fn field_of<'f>(fields: &'f [Value], reference: &Value) -> Option<&'f Value> {
        fields.iter().find(|field| {
            field["ref"] == *reference
                || (reference["kind"] == "fqn" && field["name"] == reference["name"])
        })
    }

    /// A kernel value as its field takes it now. A value already in
    /// upstream's spelling goes as it came; one that fits no field goes in
    /// the shape the host refuses, so the refusal is the host's.
    pub(super) fn wire_value(field: Option<&Value>, value: &Value) -> Value {
        let family = field.and_then(|f| f["family"].as_str()).unwrap_or_default();
        match value.get("type").and_then(Value::as_str) {
            Some("text") => json!({ "value": value.get("value").cloned().unwrap_or(Value::Null) }),
            Some("toggle") => {
                let state = value.get("state").and_then(Value::as_str);
                let Some(state) = state else { return json!({ "value": null }) };
                // The widget with that on-state, by its export value.
                let export = field
                    .and_then(|f| f["widgets"].as_array())
                    .and_then(|w| w.iter().find(|w| w["onState"] == state))
                    .and_then(|w| w["exportValue"].as_str())
                    .unwrap_or(state);
                json!({ "value": export })
            }
            Some("choice") => {
                let values = value.get("values").cloned().unwrap_or_else(|| json!([]));
                match (family, values.as_array().map(Vec::len)) {
                    ("combobox", Some(0)) => json!({ "value": null }),
                    ("combobox", Some(1)) => json!({ "value": values[0] }),
                    _ => json!({ "selectedValues": values }),
                }
            }
            _ => value.clone(),
        }
    }

    /// A field record answered in page space, in the file's coordinates.
    fn field_in_file_space(
        host: &Host,
        request: &Value,
        field: Value,
    ) -> Result<Value, HostResponse> {
        let mut records = json!({ "fields": [field] });
        records = super::page_space::form_records_in_file_space(host, request, records)
            .map_err(|message| refusal(&message))?;
        Ok(records["fields"][0].take())
    }

    fn refusal(message: &str) -> HostResponse {
        HostResponse {
            json: json!({ "error": { "name": "EngineError", "code": "WireFormat",
                "message": message } })
            .to_string(),
            payloads: Vec::new(),
        }
    }

    /// The kernel's mutation envelope.
    fn mutation(result: Value, sequence: &Value, event: Option<Value>) -> Value {
        let mut answer = json!({ "result": result, "mutationSequence": sequence });
        if let Some(event) = event {
            answer["event"] = event;
        }
        json!({ "result": answer })
    }

    /// `{type: kernel, origin, ...result}`.
    fn kernel_event(kind: &str, origin: Option<&Value>, result: &Value) -> Option<Value> {
        let origin = origin?;
        let mut event = json!({ "type": kind, "origin": origin });
        if let (Some(event), Some(fields)) = (event.as_object_mut(), result.as_object()) {
            for (key, value) in fields {
                event.insert(key.clone(), value.clone());
            }
        }
        Some(event)
    }

    fn set_value(host: &mut Host, request: &Value) -> Answer {
        let fields = fields(host, request)?;
        let reference = request.get("ref").cloned().unwrap_or(Value::Null);
        let value = wire_value(field_of(&fields, &reference), &request["value"]);
        let mut call = on_document(request, "forms.setValue");
        call["effect"] = json!("write");
        call["ref"] = reference;
        call["value"] = value;
        let (answer, response) = send(host, &call)?;
        let field = field_in_file_space(host, request, answer["result"]["field"].clone())?;
        let result = json!({
            "field": field,
            "changedWidgets": answer["result"]["meta"]["changedWidgets"],
        });
        let event = kernel_event("form.valueChanged", answer["event"].get("origin"), &result);
        Ok((mutation(result, &answer["mutationSequence"], event), response.payloads))
    }

    fn reset(host: &mut Host, request: &Value) -> Answer {
        let reference = request.get("ref").cloned().unwrap_or(Value::Null);
        let mut call = on_document(request, "forms.reset");
        call["effect"] = json!("write");
        call["refs"] = json!([reference]);
        let (answer, response) = send(host, &call)?;
        // The field read back: the one the reset changed, or as it is.
        let field = match answer["result"]["fields"].get(0) {
            Some(field) => field.clone(),
            None => {
                let fields = fields(host, request)?;
                field_of(&fields, &reference).cloned().unwrap_or(Value::Null)
            }
        };
        let field = field_in_file_space(host, request, field)?;
        let result = json!({
            "field": field,
            "changedWidgets": answer["result"]["meta"]["changedWidgets"],
        });
        let origin = answer["events"].as_array().and_then(|e| e.last()).map(|e| &e["origin"]);
        let event = kernel_event("form.valueChanged", origin, &result);
        Ok((mutation(result, &answer["mutationSequence"], event), response.payloads))
    }

    /// An effect as the op upstream runs for it (`formEffectOps`).
    fn op_of(effect: &Value, fields: &[Value]) -> Value {
        let reference = effect.get("ref").cloned().unwrap_or(Value::Null);
        match effect.get("kind").and_then(Value::as_str) {
            Some("setValue") => json!({ "type": "forms.setValue", "field": reference,
                "value": wire_value(field_of(fields, &reference), &effect["value"]) }),
            Some("setDisplay") => json!({ "type": "forms.setDisplay", "field": reference,
                "display": effect["display"] }),
            Some("setAppearanceText") => json!({ "type": "forms.setAppearanceText",
                "field": reference, "text": effect["text"] }),
            Some("reset") => json!({ "type": "forms.reset", "fields": effect["refs"] }),
            _ => effect.clone(),
        }
    }

    fn apply_effects(host: &mut Host, request: &Value) -> Answer {
        let effects = request["effects"].as_array().cloned().unwrap_or_default();
        let fields = fields(host, request)?;
        let ops: Vec<Value> = effects.iter().map(|effect| op_of(effect, &fields)).collect();
        let mut call = on_document(request, "document.apply");
        call["effect"] = json!("write");
        call["change"] = json!({ "ops": ops });
        let (answer, response) = match send(host, &call) {
            Ok(sent) => sent,
            Err(refused) => {
                // Refused whole: nothing landed, and every effect says why.
                let envelope: Value = serde_json::from_str(&refused.json).unwrap_or(Value::Null);
                let error = envelope.get("error").cloned().unwrap_or(Value::Null);
                let results: Vec<Value> = (0..effects.len())
                    .map(|index| {
                        json!({ "index": index, "status": "rejected", "fields": [],
                        "changedWidgets": [], "error": error })
                    })
                    .collect();
                let result = json!({ "results": results, "changedWidgets": [], "meta": null });
                return Ok((mutation(result, &Value::Null, None), Vec::new()));
            }
        };
        let items = answer["result"]["items"].as_array().cloned().unwrap_or_default();
        let mut results = Vec::new();
        let mut all_changed: Vec<Value> = Vec::new();
        let mut wrote = false;
        for (index, item) in items.iter().enumerate() {
            let changed = item["meta"]["changedWidgets"].as_array().cloned().unwrap_or_default();
            let touched: Vec<Value> = match item.get("field") {
                Some(field) => vec![field.clone()],
                None => item["fields"].as_array().cloned().unwrap_or_default(),
            };
            let applied = match item["type"].as_str() {
                Some("forms.reset") => !touched.is_empty(),
                _ => !changed.is_empty(),
            };
            wrote |= applied;
            let mut converted = Vec::new();
            for field in touched {
                converted.push(field_in_file_space(host, request, field)?);
            }
            for widget in &changed {
                if !all_changed.contains(widget) {
                    all_changed.push(widget.clone());
                }
            }
            results.push(json!({
                "index": index,
                "status": if applied { "applied" } else { "unchanged" },
                "fields": converted,
                "changedWidgets": changed,
            }));
        }
        let meta = if wrote { answer["result"]["meta"].clone() } else { Value::Null };
        let result = json!({ "results": results, "changedWidgets": all_changed, "meta": meta });
        let origin = answer["events"].as_array().and_then(|e| e.last()).map(|e| &e["origin"]);
        let event = if wrote { kernel_event("form.effectsApplied", origin, &result) } else { None };
        Ok((mutation(result, &answer["mutationSequence"], event), response.payloads))
    }

    fn repair(host: &mut Host, request: &Value, payloads: Vec<Vec<u8>>) -> Answer {
        let response = host.call(&request.to_string(), payloads);
        let mut envelope: Value = serde_json::from_str(&response.json).unwrap_or(Value::Null);
        if envelope.get("error").is_some() {
            return Err(response);
        }
        let rename = |object: &mut Value| {
            if let Some(map) = object.as_object_mut()
                && let Some(cleared) = map.remove("needsAppearancesCleared")
            {
                map.insert("needAppearancesCleared".into(), cleared);
            }
        };
        let answer = &mut envelope["result"];
        rename(&mut answer["result"]);
        if let Some(event) = answer.get_mut("event").filter(|e| e.is_object()) {
            rename(event);
            event["type"] = json!("form.repaired");
        }
        Ok((envelope, response.payloads))
    }
}

/// The kernel's page structure edits (Task 8.18).
///
/// Upstream runs them as ops of `document.apply` (119685ea4): `pages.move`
/// is `pages.reorder`, and every position names a neighbour page or an end
/// (`ListPosition`, 1233d1349). The kernel still names an index, as this
/// branch's wire did: `pages.move`'s in the pages left once the block is
/// out, an insert's among the pages there are. [`op`] builds the page op
/// with the kernel's `destIndex` where its `position` goes, and [`call`]
/// turns it into the neighbour at that index of the document as it is now,
/// sends the change, and answers it as the kernel decodes a page mutation:
/// the op's item as the result (its layout in the file's coordinates) and
/// its one event under the kernel's name (`pages.moved` for upstream's
/// `pages.reordered`). Dead once Task 13 ports the kernel.
mod kernel_pages {
    use epdf_engine::error::{EngineError, EngineErrorCode};
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::session::registry::SessionKey;
    use epdf_kernel::Error;
    use serde_json::{Value, json};

    /// The page op a kernel page path names, its bytes moved into
    /// `payloads`.
    pub(super) fn op(
        path: &str,
        args: &[Value],
        payloads: &mut Vec<Vec<u8>>,
    ) -> Result<Value, Error> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        let mut op = match path {
            "pages.rotate" => json!({
                "type": "pages.rotate",
                "pages": super::page_refs(path, &arg(0))?,
                "rotation": super::whole(path, "rotation", &arg(1))?,
            }),
            "pages.move" => json!({
                "type": "pages.reorder",
                "pages": super::page_refs(path, &arg(0))?,
                "destIndex": super::whole(path, "destIndex", &arg(1))?,
            }),
            "pages.delete" => {
                json!({ "type": "pages.delete", "pages": super::page_refs(path, &arg(0))? })
            }
            "pages.insertBlank" => {
                let spec = arg(0);
                let mut wire = json!({ "size": spec.get("size").cloned().unwrap_or(Value::Null) });
                let count = super::optional_whole(path, "count", spec.get("count"))?;
                if !count.is_null() {
                    wire["count"] = count;
                }
                json!({ "type": "pages.insertBlank", "spec": wire })
            }
            "pages.insert" => {
                payloads.push(super::pdf_bytes(path, &arg(0))?);
                json!({ "type": "pages.insert", "source": { "payload": 0 } })
            }
            _ => unreachable!("routed to document.apply: {path}"),
        };
        if matches!(path, "pages.insertBlank" | "pages.insert") {
            let at = super::optional_whole(path, "destIndex", args.get(1))?;
            if !at.is_null() {
                op["destIndex"] = at;
            }
        }
        Ok(op)
    }

    /// Send the change, its op's `destIndex` turned into a `position`, and
    /// answer it as the kernel decodes a page mutation.
    pub(super) fn call(host: &mut Host, request: &Value, payloads: Vec<Vec<u8>>) -> HostResponse {
        match send(host, request, payloads) {
            Ok(response) => response,
            Err(error) => HostResponse {
                json: json!({ "error": error.to_wire() }).to_string(),
                payloads: Vec::new(),
            },
        }
    }

    fn send(
        host: &mut Host,
        request: &Value,
        payloads: Vec<Vec<u8>>,
    ) -> Result<HostResponse, EngineError> {
        let mut request = request.clone();
        let index = request
            .pointer_mut("/change/ops/0")
            .and_then(Value::as_object_mut)
            .and_then(|op| op.remove("destIndex"));
        if let Some(at) = index {
            let order = page_order(host, &request)?;
            let at = at.as_u64().ok_or_else(|| malformed("destIndex is a whole number"))? as usize;
            let op = &mut request["change"]["ops"][0];
            op["position"] = position_at(op, &order, at)?;
        }
        let response = host.call(&request.to_string(), payloads);
        let mut envelope: Value =
            serde_json::from_str(&response.json).map_err(|e| malformed(e.to_string()))?;
        if envelope.get("error").is_some() {
            return Ok(response);
        }
        let answer = &mut envelope["result"];
        let mut item = answer["result"]["items"][0].take();
        let mut event = answer["events"][0].take();
        if let Some(fields) = item.as_object_mut() {
            fields.remove("type");
        }
        super::page_space::layout_in_file_space(&mut item)?;
        if !event.is_null() {
            super::page_space::layout_in_file_space(&mut event)?;
            if event["type"] == "pages.reordered" {
                event["type"] = json!("pages.moved");
            }
        }
        let kernel = json!({ "result": {
            "result": item,
            "mutationSequence": answer["mutationSequence"].take(),
            "event": event,
        } });
        Ok(HostResponse { json: kernel.to_string(), payloads: response.payloads })
    }

    /// The neighbour at the kernel's index: for a move, among the pages left
    /// once the block is out; for an insert, among the pages there are. 0
    /// is the start; an index past the end is refused, as this branch's
    /// wire refused it.
    fn position_at(op: &Value, order: &[Value], at: usize) -> Result<Value, EngineError> {
        let moving = op["type"] == "pages.reorder";
        let left: Vec<&Value> = if moving {
            let block: Vec<&Value> =
                op["pages"].as_array().map(|p| p.iter().collect()).unwrap_or_default();
            let number = |page: &Value| page.get("objectNumber").cloned();
            order.iter().filter(|page| !block.iter().any(|b| number(b) == number(page))).collect()
        } else {
            order.iter().collect()
        };
        if at > left.len() {
            let message = if moving {
                format!(
                    "pages.move destination {at} is past the end: {} pages remain after the \
                     block is detached",
                    left.len()
                )
            } else {
                format!(
                    "{} destination {at} is outside 0..={}",
                    op["type"].as_str().unwrap_or("pages.insert"),
                    left.len()
                )
            };
            return Err(EngineError::with_message(EngineErrorCode::InvalidArg, message));
        }
        Ok(match at {
            0 => json!("start"),
            at => json!({ "after": left[at - 1] }),
        })
    }

    /// The document's pages, as refs, in display order.
    fn page_order(host: &mut Host, request: &Value) -> Result<Vec<Value>, EngineError> {
        let doc = request
            .get("docId")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("the change names no document"))?;
        let key = match request.get("layerName").and_then(Value::as_str) {
            Some(layer) => SessionKey::layer(doc, layer),
            None => SessionKey::document(doc),
        };
        let records = host.sessions_mut().session_mut(&key)?.all_records()?;
        Ok(records
            .iter()
            .map(|record| {
                json!({ "kind": "objectNumber", "objectNumber": record.page_object_number.get() })
            })
            .collect())
    }

    fn malformed(message: impl Into<String>) -> EngineError {
        EngineError::with_message(EngineErrorCode::WireFormat, message)
    }
}

/// The kernel's annotation listings (Task 8.12).
///
/// The kernel reads the annotation record this branch's wire had before the
/// catch-up: `{pageState, annotations}` per page, refs by object number,
/// `/NM` or revisioned index, an `index` per annotation, the `/F` bits
/// under `flags`, PDF-space places, `{r, g, b}` colours, widgets among the
/// rest, rotation from our `/EMBD_Metadata`. Upstream's `annotations.list`
/// answers none of that, and parts of it (the index, the weak state, the
/// recorded rotation and `/RD`) can't be rebuilt from its answer. So the
/// kernel's two listings are read here from the host's session with the
/// engine's previous reader (`read::page`, which the annotation writes still
/// answer with), never crossing the wire. Dead once Task 13 ports the
/// kernel onto `annotations.list`.
mod kernel_annotations {
    use epdf_engine::cancel::CancelToken;
    use epdf_engine::error::{EngineError, EngineErrorCode};
    use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
    use epdf_engine::features::annotation::read::page::read_page_annotations;
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::identity::{PageObjectNumber, PageRef};
    use epdf_engine::session::registry::SessionKey;
    use serde_json::{Value, json};

    /// `{pages: [{pageState, annotations}]}` for the request's `pages`, or
    /// for every page in document order, as the removed
    /// `annotations.listRawAll` answered.
    pub(super) fn listing(host: &mut Host, request: &Value) -> HostResponse {
        let json = match read(host, request) {
            Ok(pages) => json!({ "result": { "pages": pages } }),
            Err(error) => json!({ "error": error.to_wire() }),
        };
        HostResponse { json: json.to_string(), payloads: Vec::new() }
    }

    fn read(host: &mut Host, request: &Value) -> Result<Vec<Value>, EngineError> {
        let doc = request.get("docId").and_then(Value::as_str).ok_or_else(|| {
            EngineError::with_message(EngineErrorCode::WireFormat, "the listing names no document")
        })?;
        let key = match request.get("layerName").and_then(Value::as_str) {
            Some(layer) => SessionKey::layer(doc, layer),
            None => SessionKey::document(doc),
        };
        let session = host.sessions_mut().session_mut(&key)?;
        let pages: Vec<PageObjectNumber> = match request.get("pages") {
            Some(Value::Array(pages)) => pages
                .iter()
                .map(|page| PageRef::from_value(page, "pages").map(PageRef::object_number))
                .collect::<Result<_, _>>()?,
            _ => {
                let document = session.document()?.document();
                (0..document.page_count())
                    .filter_map(|index| {
                        PageObjectNumber::new(document.page_object_number_by_index(index)).ok()
                    })
                    .collect()
            }
        };
        // One budget across the listing, as the removed whole-document
        // listing kept one.
        let mut budget = ActionReadBudgetTracker::default();
        let mut snapshots = Vec::with_capacity(pages.len());
        for page in pages {
            let state = session.page_state(page)?;
            let owned = session.document()?;
            let index = owned.page_index(page)?;
            let snapshot = read_page_annotations(
                owned.document(),
                index,
                page,
                &state.revision,
                &mut budget,
                &CancelToken::never(),
            )?;
            snapshots.push(
                serde_json::to_value(&snapshot).map_err(|e| {
                    EngineError::with_message(EngineErrorCode::Unknown, e.to_string())
                })?,
            );
        }
        Ok(snapshots)
    }
}

/// The kernel's annotation create, on the host's (Task 8.14).
///
/// The kernel sends the draft it always has: the old spelling, in the file's
/// coordinates (`rect` edges, `unrotatedRect` beside a counterclockwise
/// `rotation`, `p1`..`p4` quads, `{r, g, b}` colours, a `flags` object,
/// `inReplyTo`), its bytes as keyed resources. The host now takes upstream's
/// v3 draft in page space with its bytes by role, and answers the new record
/// and an `annotations.created` event. [`call`] respells the draft and the
/// resources, sends the create, and answers the kernel the annotation the
/// previous reader reads (the shape `sync` takes) under `created`, with an
/// `annotation.created` event carrying it. Fields the v3 kind doesn't
/// declare (a measurement's scale and caption, `/RD`) are left out, as the
/// previous create ignored them. Dead once Task 13 ports the kernel.
mod kernel_create {
    use epdf_engine::error::{EngineError, EngineErrorCode};
    use epdf_engine::features::annotation::subtype::AnnotationSubtype;
    use epdf_engine::features::annotation::write::prepare::declared_fields;
    use epdf_engine::geom::{PdfPoint, PdfRect};
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::identity::PageRef;
    use epdf_engine::page_space::destinations::page_destination_of;
    use epdf_engine::page_space::{page_point_of, page_rect_of};
    use serde_json::{Map, Value, json};

    pub(super) fn malformed(message: impl Into<String>) -> EngineError {
        EngineError::with_message(EngineErrorCode::WireFormat, message)
    }

    pub(super) fn refused(error: EngineError) -> HostResponse {
        HostResponse { json: json!({ "error": error.to_wire() }).to_string(), payloads: Vec::new() }
    }

    /// The kernel's create, answered in the kernel's shape.
    pub(super) fn call(host: &mut Host, request: &Value, payloads: Vec<Vec<u8>>) -> HostResponse {
        let (wire, payloads) = match respell(host, request, payloads) {
            Ok(respelled) => respelled,
            Err(error) => return refused(error),
        };
        let response = host.call(&wire.to_string(), payloads);
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        if envelope.get("error").is_some() {
            return response;
        }
        match answer_for_kernel(host, request, &mut envelope) {
            Ok(()) => HostResponse { json: envelope.to_string(), payloads: response.payloads },
            Err(error) => refused(error),
        }
    }

    /// The v3 request, in page space, with its bytes by role.
    fn respell(
        host: &Host,
        request: &Value,
        payloads: Vec<Vec<u8>>,
    ) -> Result<(Value, Vec<Vec<u8>>), EngineError> {
        let page: PageRef =
            serde_json::from_value(request.get("page").cloned().unwrap_or_default())
                .map_err(|e| malformed(format!("the create names no page: {e}")))?;
        let mut box_of = super::page_space::visible_boxes(host, request);
        let visible = box_of(page.object_number())?;
        let draft = match request.get("draft") {
            Some(Value::Object(draft)) => draft.clone(),
            _ => return Err(malformed("the create carries no draft")),
        };
        let keyed = request.get("resources").cloned().unwrap_or(Value::Null);
        let payload_of = |resource: Option<&Value>| -> Option<Value> {
            let key = resource?.as_str()?;
            keyed.get(key)?.get("payload").cloned()
        };
        let mut roles = Map::new();
        if let Some(index) = payload_of(draft.get("source").and_then(|s| s.get("resource"))) {
            roles.insert("appearance".into(), json!({ "payload": index }));
        }
        if let Some(index) = payload_of(draft.get("file").and_then(|f| f.get("resource"))) {
            roles.insert("file".into(), json!({ "payload": index }));
        }
        let draft = v3_draft(draft, visible, &mut box_of, false)?;
        let mut wire = Map::new();
        for key in ["kind", "jobId", "docId", "layerName", "opId", "page", "objectNumberFloor"] {
            if let Some(value) = request.get(key).filter(|v| !v.is_null()) {
                wire.insert(key.into(), value.clone());
            }
        }
        wire.insert("draft".into(), Value::Object(draft));
        if !roles.is_empty() {
            wire.insert("resources".into(), Value::Object(roles));
        }
        Ok((Value::Object(wire), payloads))
    }

    fn rect(value: &Value) -> Option<PdfRect> {
        serde_json::from_value(value.clone()).ok()
    }

    fn page_point(value: &Value, visible: PdfRect) -> Value {
        match serde_json::from_value::<PdfPoint>(value.clone()) {
            Ok(point) => json!(page_point_of(point, visible)),
            Err(_) => value.clone(),
        }
    }

    fn page_points(value: &Value, visible: PdfRect) -> Value {
        match value {
            Value::Array(points) => {
                Value::Array(points.iter().map(|p| page_point(p, visible)).collect())
            }
            other => other.clone(),
        }
    }

    fn page_box(value: &Value, visible: PdfRect) -> Value {
        match rect(value) {
            Some(rect) => json!(page_rect_of(epdf_engine::geom::normalize_pdf_rect(rect), visible)),
            None => value.clone(),
        }
    }

    fn hex(value: &Value) -> Value {
        let channel = |key: &str| value.get(key).and_then(Value::as_f64);
        match (channel("r"), channel("g"), channel("b")) {
            (Some(r), Some(g), Some(b)) => {
                Value::String(format!("#{:02x}{:02x}{:02x}", r as u8, g as u8, b as u8))
            }
            _ => value.clone(),
        }
    }

    /// A kernel draft (old spelling, the file's coordinates) as upstream's
    /// v3 draft in page space, keeping only the fields its kind declares.
    /// The kernel's patch is the same statement (`toPatch`): with `update`,
    /// it is respelled as a v3 patch, where a drawn kind's `rect` is a
    /// command, a turn of 0 and a cloud of 0 clear, and the shape the
    /// statement states wins over its `rect`.
    pub(super) fn v3_draft(
        mut draft: Map<String, Value>,
        visible: PdfRect,
        box_of: &mut dyn FnMut(
            epdf_engine::identity::PageObjectNumber,
        ) -> Result<PdfRect, EngineError>,
        update: bool,
    ) -> Result<Map<String, Value>, EngineError> {
        let subtype_name = draft.get("subtype").and_then(Value::as_str).unwrap_or("").to_owned();
        let subtype: AnnotationSubtype = serde_json::from_value(json!(subtype_name))
            .map_err(|_| malformed(format!("no annotation kind '{subtype_name}'")))?;
        use AnnotationSubtype as S;
        let mut out = Map::new();
        // The `/F` flags, each its own field.
        if let Some(Value::Object(flags)) = draft.remove("flags") {
            for (name, value) in flags {
                if value.is_boolean() {
                    out.insert(name, value);
                }
            }
        }
        // `inReplyTo` and `replyType` are `reply`.
        if let Some(to) = draft.remove("inReplyTo").filter(|to| !to.is_null()) {
            let reply_type = draft.remove("replyType").unwrap_or(json!("reply"));
            out.insert("reply".into(), json!({ "to": to, "type": reply_type }));
        }
        draft.remove("replyType");
        // A box kind's box: the turned-from box when it is turned, its rect
        // when upright. The kernel's turn is counterclockwise.
        let rotation_sent = draft.contains_key("rotation");
        let rotation = draft.remove("rotation").and_then(|r| r.as_f64()).filter(|r| *r != 0.0);
        let clockwise = rotation.map(|r| (360.0 - r.rem_euclid(360.0)).rem_euclid(360.0));
        let is_box_kind =
            matches!(subtype, S::Square | S::Circle | S::FreeText | S::Stamp | S::Caret);
        let unrotated = draft.remove("unrotatedRect");
        let given_rect = draft.remove("rect");
        let turned_from = unrotated.filter(|u| !u.is_null() && rotation.is_some());
        // Upright, the box is the rect less the statement's `/RD`, as the
        // kernel's `ingest` reads it (`inset_pdf_rect_by_rd`): a callout's
        // rect holds its line, a cloudy border's its bumps.
        let differences = draft.remove("rectDifferences");
        let upright_box = given_rect.as_ref().and_then(rect).map(|r| {
            let inset = |key: &str| {
                differences.as_ref().and_then(|d| d.get(key)).and_then(Value::as_f64).unwrap_or(0.0)
            };
            json!(PdfRect {
                left: r.left + inset("left"),
                bottom: r.bottom + inset("bottom"),
                right: r.right - inset("right"),
                top: r.top - inset("top"),
            })
        });
        if is_box_kind && let Some(content) = turned_from.or(upright_box) {
            out.insert("box".into(), page_box(&content, visible));
        } else if (update
            || matches!(subtype, S::Text | S::Link | S::Redact | S::FileAttachment | S::Popup))
            && let Some(rect) = given_rect
        {
            out.insert("rect".into(), page_box(&rect, visible));
        }
        if let Some(degrees) = clockwise.filter(|d| *d != 0.0)
            && matches!(
                subtype,
                S::Square
                    | S::Circle
                    | S::FreeText
                    | S::Stamp
                    | S::Caret
                    | S::Polygon
                    | S::Polyline
                    | S::Line
                    | S::Ink
            )
        {
            out.insert("rotation".into(), json!(degrees));
        } else if update && rotation_sent {
            out.insert("rotation".into(), Value::Null);
        }
        for (name, value) in draft {
            let converted = match name.as_str() {
                "color" | "fontColor" => hex(&value),
                "interiorColor" => {
                    if value.is_null() {
                        Value::Null
                    } else {
                        hex(&value)
                    }
                }
                "quadPoints" => match &value {
                    Value::Array(quads) => Value::Array(
                        quads
                            .iter()
                            .map(|q| {
                                json!({
                                    "upperLeft": page_point(&q["p1"], visible),
                                    "upperRight": page_point(&q["p2"], visible),
                                    "lowerLeft": page_point(&q["p3"], visible),
                                    "lowerRight": page_point(&q["p4"], visible),
                                })
                            })
                            .collect(),
                    ),
                    other => other.clone(),
                },
                "vertices" | "calloutLine" => page_points(&value, visible),
                "inkList" => match &value {
                    Value::Array(strokes) => {
                        Value::Array(strokes.iter().map(|s| page_points(s, visible)).collect())
                    }
                    other => other.clone(),
                },
                "linePoints" => json!({
                    "start": page_point(&value["start"], visible),
                    "end": page_point(&value["end"], visible),
                }),
                "cloudyIntensity" => match value.as_f64() {
                    Some(intensity) if intensity > 0.0 => value,
                    _ if update => Value::Null,
                    _ => continue,
                },
                "file" => {
                    let mut file = Map::new();
                    for key in ["name", "mimeType", "description"] {
                        if let Some(v) = value.get(key) {
                            file.insert(key.into(), v.clone());
                        }
                    }
                    Value::Object(file)
                }
                "target" => match value.get("destination") {
                    Some(destination) if value["kind"] == "goto" => {
                        let destination: epdf_engine::features::actions::destination::PdfDestination =
                            serde_json::from_value(destination.clone()).map_err(|e| {
                                malformed(format!("a link destination the host can't read: {e}"))
                            })?;
                        json!({ "kind": "goto",
                            "destination": page_destination_of(destination, &mut *box_of)? })
                    }
                    _ => value,
                },
                _ => value,
            };
            out.insert(name, converted);
        }
        // Only the fields the kind declares travel: the v3 create refuses
        // any other, where the previous create ignored them.
        let declared: Vec<&str> = declared_fields(subtype).map(|(name, _)| name).collect();
        out.retain(|name, _| declared.contains(&name.as_str()));
        // A drawn kind's `rect` beside the shape it states is the rect
        // around it, not a command.
        if update
            && epdf_engine::features::annotation::write::shape_for_rect::is_drawn_rect_kind(
                &subtype_name,
            )
            && epdf_engine::features::annotation::write::shape_for_rect::shape_fields_of(
                &subtype_name,
            )
            .iter()
            .any(|field| *field != "rotation" && out.contains_key(*field))
        {
            out.remove("rect");
        }
        out.insert("subtype".into(), json!(subtype_name));
        Ok(out)
    }

    /// The created annotation as the previous reader reads it, under
    /// `created`, and the event the kernel's sync takes.
    fn answer_for_kernel(
        host: &mut Host,
        request: &Value,
        envelope: &mut Value,
    ) -> Result<(), EngineError> {
        let object_number = envelope
            .pointer("/result/result/annotation/ref/objectNumber")
            .cloned()
            .ok_or_else(|| malformed("a create answered no annotation"))?;
        let page = request.get("page").cloned().unwrap_or_default();
        let listing = super::kernel_annotations::listing(
            host,
            &json!({ "kind": "annotations.list", "docId": request.get("docId"),
                "layerName": request.get("layerName"), "pages": [page] }),
        );
        let listed: Value = serde_json::from_str(&listing.json)
            .map_err(|e| malformed(format!("the listing is not JSON: {e}")))?;
        let created = listed
            .pointer("/result/pages/0/annotations")
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|a| a["ref"]["objectNumber"] == object_number))
            .cloned()
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the created annotation does not read back",
                )
            })?;
        let answer = &mut envelope["result"];
        let meta = answer["result"]["meta"].take();
        answer["result"] = json!({ "created": created, "meta": meta });
        if let Some(Value::Object(event)) = answer.get_mut("event") {
            event.remove("annotation");
            event.insert("type".into(), json!("annotation.created"));
            event.insert("created".into(), created);
        }
        Ok(())
    }
}

/// The kernel's annotation update, on the host's (Task 8.15).
///
/// The kernel sends its patch as the statement it sends a create
/// (`toPatch`): the old spelling, in the file's coordinates, its bytes as
/// keyed resources. The host now takes upstream's v3 patch in page space,
/// with its bytes by role, and answers the new record, its appearance
/// verdict and an `annotations.updated` event. [`call`] respells the patch
/// ([`super::kernel_create::v3_draft`] with `update`) and the resources,
/// sends the update, and answers the kernel the annotation the previous
/// reader reads (the shape `sync` takes) under `updated`, with an
/// `annotation.updated` event carrying it. Dead once Task 13 ports the
/// kernel.
mod kernel_update {
    use epdf_engine::error::{EngineError, EngineErrorCode};
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::identity::AnnotationRef;
    use epdf_engine::session::registry::SessionKey;
    use serde_json::{Map, Value, json};

    use super::kernel_create::{malformed, refused, v3_draft};

    /// The kernel's update, answered in the kernel's shape.
    pub(super) fn call(host: &mut Host, request: &Value, payloads: Vec<Vec<u8>>) -> HostResponse {
        let (wire, index) = match respell(host, request) {
            Ok(respelled) => respelled,
            Err(error) => return refused(error),
        };
        let response = host.call(&wire.to_string(), payloads);
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        if envelope.get("error").is_some() {
            return response;
        }
        match answer_for_kernel(host, request, index, &mut envelope) {
            Ok(()) => HostResponse { json: envelope.to_string(), payloads: response.payloads },
            Err(error) => refused(error),
        }
    }

    fn session_key(request: &Value) -> Result<SessionKey, EngineError> {
        let doc = request
            .get("docId")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("the update names no document"))?;
        Ok(match request.get("layerName").and_then(Value::as_str) {
            Some(layer) => SessionKey::layer(doc, layer),
            None => SessionKey::document(doc),
        })
    }

    /// The v3 request, in page space, with its bytes by role, and where the
    /// annotation sits in its page's `/Annots` (an update doesn't move it).
    fn respell(host: &Host, request: &Value) -> Result<(Value, u32), EngineError> {
        let reference: AnnotationRef =
            serde_json::from_value(request.get("ref").cloned().unwrap_or_default())
                .map_err(|e| malformed(format!("the update names no annotation: {e}")))?;
        let index = {
            let session = host.sessions().session(&session_key(request)?)?;
            let owned = session.document()?;
            let page_index = owned.page_index(reference.page())?;
            epdf_engine::features::annotation::write::resolve::resolve_annotation_index_raw(
                owned.document(),
                page_index,
                &reference,
            )?
        };
        let mut box_of = super::page_space::visible_boxes(host, request);
        let visible = box_of(reference.page())?;
        let patch = match request.get("patch") {
            Some(Value::Object(patch)) => patch.clone(),
            _ => return Err(malformed("the update carries no patch")),
        };
        let keyed = request.get("resources").cloned().unwrap_or(Value::Null);
        let payload_of = |resource: Option<&Value>| -> Option<Value> {
            let key = resource?.as_str()?;
            keyed.get(key)?.get("payload").cloned()
        };
        let mut roles = Map::new();
        if let Some(index) = payload_of(patch.get("source").and_then(|s| s.get("resource"))) {
            roles.insert("appearance".into(), json!({ "payload": index }));
        }
        if let Some(index) = payload_of(patch.get("file").and_then(|f| f.get("resource"))) {
            roles.insert("file".into(), json!({ "payload": index }));
        }
        let patch = v3_draft(patch, visible, &mut box_of, true)?;
        let mut wire = Map::new();
        for key in
            ["kind", "jobId", "docId", "layerName", "opId", "ref", "authority", "objectNumberFloor"]
        {
            if let Some(value) = request.get(key).filter(|v| !v.is_null()) {
                wire.insert(key.into(), value.clone());
            }
        }
        wire.insert("patch".into(), Value::Object(patch));
        if !roles.is_empty() {
            wire.insert("resources".into(), Value::Object(roles));
        }
        Ok((Value::Object(wire), index))
    }

    /// The updated annotation as the previous reader reads it, under
    /// `updated`, and the event the kernel's sync takes.
    fn answer_for_kernel(
        host: &mut Host,
        request: &Value,
        index: u32,
        envelope: &mut Value,
    ) -> Result<(), EngineError> {
        let page = request.get("ref").and_then(|r| r.get("page")).cloned().unwrap_or_default();
        let listing = super::kernel_annotations::listing(
            host,
            &json!({ "kind": "annotations.list", "docId": request.get("docId"),
                "layerName": request.get("layerName"), "pages": [page] }),
        );
        let listed: Value = serde_json::from_str(&listing.json)
            .map_err(|e| malformed(format!("the listing is not JSON: {e}")))?;
        let updated = listed
            .pointer("/result/pages/0/annotations")
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|a| a["index"] == json!(index)))
            .cloned()
            .ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    "the updated annotation does not read back",
                )
            })?;
        let answer = &mut envelope["result"];
        let appearance = answer["result"]["appearance"].take();
        let meta = answer["result"]["meta"].take();
        answer["result"] = json!({ "updated": updated, "appearance": appearance, "meta": meta });
        if let Some(Value::Object(event)) = answer.get_mut("event") {
            event.remove("annotation");
            event.insert("type".into(), json!("annotation.updated"));
            event.insert("updated".into(), updated);
        }
        Ok(())
    }
}

/// The kernel's annotation delete, on the host's (Task 8.16).
///
/// The host now answers upstream's delete: `{meta}`, the thread and popups
/// gone with the annotation and named in `meta.changed`, and an
/// `annotations.deleted` event listing them. The kernel's sync takes the
/// previous answer: the durable id of what it asked to delete under
/// `deleted` (its object number, else its non-empty `/NM`, else `null`), in
/// the result and in an `annotation.deleted` event. [`call`] reads that id
/// before the delete and answers it so. Dead once Task 13 ports the kernel.
mod kernel_delete {
    use epdf_engine::error::EngineError;
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::identity::AnnotationRef;
    use serde_json::{Value, json};

    use super::kernel_create::{malformed, refused};

    /// The kernel's delete, answered in the kernel's shape.
    pub(super) fn call(host: &mut Host, request: &Value, payloads: Vec<Vec<u8>>) -> HostResponse {
        let deleted = match durable_id(host, request) {
            Ok(id) => id,
            Err(error) => return refused(error),
        };
        let response = host.call(&request.to_string(), payloads);
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        if envelope.get("error").is_some() {
            return response;
        }
        let answer = &mut envelope["result"];
        let meta = answer["result"]["meta"].take();
        answer["result"] = json!({ "deleted": deleted, "meta": meta });
        if let Some(Value::Object(event)) = answer.get_mut("event") {
            event.insert("type".into(), json!("annotation.deleted"));
            event.insert("deleted".into(), deleted);
        }
        HostResponse { json: envelope.to_string(), payloads: response.payloads }
    }

    /// The durable id of the annotation `request` names, as the previous
    /// delete answered it. A ref that names nothing answers `null`, and the
    /// host's own refusal follows.
    fn durable_id(host: &mut Host, request: &Value) -> Result<Value, EngineError> {
        let reference: AnnotationRef =
            serde_json::from_value(request.get("ref").cloned().unwrap_or_default())
                .map_err(|e| malformed(format!("the delete names no annotation: {e}")))?;
        let base_index = match reference {
            AnnotationRef::ObjectNumber { annot, .. } => {
                return Ok(json!({ "kind": "objectNumber", "value": annot }));
            }
            AnnotationRef::BaseIndex { base_index, .. } => base_index,
            AnnotationRef::Nm { .. } | AnnotationRef::Index { .. } => return Ok(Value::Null),
        };
        // Still inline: its name, if it has one, from the kernel's listing.
        let page = request.get("ref").and_then(|r| r.get("page")).cloned().unwrap_or_default();
        let mut list = json!({ "kind": "annotations.list", "docId": request.get("docId"),
            "pages": [page] });
        if let Some(layer) = request.get("layerName") {
            list["layerName"] = layer.clone();
        }
        let listing = super::kernel_annotations::listing(host, &list);
        let listed: Value = serde_json::from_str(&listing.json)
            .map_err(|e| malformed(format!("the listing is not JSON: {e}")))?;
        let name = listed
            .pointer("/result/pages/0/annotations")
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|a| a["index"] == json!(base_index)))
            .and_then(|a| a["nm"].as_str())
            .filter(|nm| !nm.is_empty())
            .map(str::to_owned);
        Ok(name.map_or(Value::Null, |nm| json!({ "kind": "nm", "value": nm })))
    }
}

/// The kernel's places in the file's coordinates, and the host's in page
/// space (Task 8.4's boundary, `epdf_engine::page_space`).
///
/// The kernel still reads and sends PDF space: a page list's boxes, a
/// destination, a render target, a page's text geometry. The host now
/// answers and takes page space, as upstream's worker does. [`call_host`]
/// converts a render target on the way out and a page list's, the catalog
/// actions' or the text geometry's places on the way back, reading each
/// page's visible box from the host's session (a page list carries its own),
/// so the kernel keeps its space until Task 13 ports it. Dead then.
mod page_space {
    use epdf_engine::error::EngineError;
    use epdf_engine::features::actions::destination::PdfDestination;
    use epdf_engine::features::actions::document::DocumentActionsSnapshot;
    use epdf_engine::features::annotation::read::base::AnnotationActions;
    use epdf_engine::features::forms::snapshot::FieldActions;
    use epdf_engine::features::geometry::{PageGeometryRun, PageGeometrySnapshot};
    use epdf_engine::features::pages::{PageLayout, PageListSnapshot};
    use epdf_engine::features::search::reader::SearchSlice;
    use epdf_engine::geom::{PageBoxes, PageQuad, PageRect, PdfRect, normalize_pdf_rect};
    use epdf_engine::host::{Host, HostResponse};
    use epdf_engine::identity::{PageObjectNumber, PageRef};
    use epdf_engine::page_space::destinations::{PageDestination, pdf_destination_of};
    use epdf_engine::page_space::{
        PageCoordinates, PdfCoordinates, page_rect_of, pdf_quad_of, pdf_rect_of,
    };
    use epdf_engine::session::registry::SessionKey;
    use serde_json::Value;
    use std::collections::HashMap;

    /// The visible box of any page of the request's document, read one page
    /// at a time through the session's index, as the host's own boundary
    /// reads it (`page_space::boundary::session_boxes`). It takes `&Host`,
    /// so it can issue no call: a kernel tile costs one page's box reads,
    /// not a page list, and reads the document as it is now, so there is
    /// nothing to go stale after a write.
    pub(super) fn visible_boxes<'h>(
        host: &'h Host,
        request: &Value,
    ) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + 'h {
        let key = request.get("docId").and_then(Value::as_str).map(|doc| {
            match request.get("layerName").and_then(Value::as_str) {
                Some(layer) => SessionKey::layer(doc, layer),
                None => SessionKey::document(doc),
            }
        });
        epdf_engine::page_space::boundary::session_boxes(host.sessions(), key, "translate")
    }

    fn lookup(
        boxes: &HashMap<u32, PdfRect>,
    ) -> impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError> + '_ {
        move |page| {
            boxes.get(&page.get()).copied().ok_or_else(|| {
                EngineError::with_message(
                    epdf_engine::error::EngineErrorCode::NotFound,
                    format!("no page with object number {}", page.get()),
                )
            })
        }
    }

    /// A `pages.render` or `pages.renderEncoded` target the kernel sent in
    /// the file's coordinates (`{left, bottom, right, top}`), in page space
    /// on its page. Answers whether it changed anything; a target it cannot
    /// place goes as it came, for the host to refuse.
    pub(super) fn render_target_to_page_space(host: &Host, request: &mut Value) -> bool {
        let kind = request.get("kind").and_then(Value::as_str).unwrap_or_default();
        if kind != "pages.render" && kind != "pages.renderEncoded" {
            return false;
        }
        let Some(rect) =
            request.pointer("/options/target/rect").filter(|r| r.get("left").is_some())
        else {
            return false;
        };
        let Ok(rect) = serde_json::from_value::<PdfRect>(rect.clone()) else { return false };
        let Some(page) = request
            .pointer("/page/objectNumber")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .and_then(|n| PageObjectNumber::new(n).ok())
        else {
            return false;
        };
        let Ok(visible) = visible_boxes(host, request)(page) else { return false };
        let Ok(rect) = serde_json::to_value(page_rect_of(normalize_pdf_rect(rect), visible)) else {
            return false;
        };
        request["options"]["target"]["rect"] = rect;
        true
    }

    /// The host's page-space answer as the kernel reads it: `pages.list`'s
    /// boxes and page actions, `actions.read`'s destinations,
    /// `pages.geometry`'s runs, `search.query`'s match segments and
    /// `forms.list`'s field actions and widget rows, in the file's
    /// coordinates. Every other answer goes as it came.
    pub(super) fn answer_to_file_space(
        host: &Host,
        kind: &str,
        request: &Value,
        response: HostResponse,
    ) -> HostResponse {
        if matches!(kind, "pages.setName" | "pages.removeName") {
            return names_in_file_space(response);
        }
        if !matches!(
            kind,
            "pages.list" | "actions.read" | "pages.geometry" | "search.query" | "forms.list"
        ) {
            return response;
        }
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        let Some(result) = envelope.get_mut("result").filter(|r| r.is_object()) else {
            return response;
        };
        let converted = if kind == "forms.list" {
            let mut boxes = visible_boxes(host, request);
            form_in_file_space(result.take(), &mut boxes)
        } else if kind == "pages.geometry" {
            serde_json::from_value::<PageGeometrySnapshot<PageCoordinates>>(result.take())
                .map_err(|e| e.to_string())
                .and_then(|snapshot| {
                    let page = request
                        .get("page")
                        .cloned()
                        .and_then(|page| serde_json::from_value::<PageRef>(page).ok())
                        .ok_or("the request names no page")?;
                    let visible = visible_boxes(host, request)(page.object_number())
                        .map_err(|e| e.message)?;
                    Ok(kernel_geometry_of(snapshot, visible))
                })
        } else if kind == "search.query" {
            let mut boxes = visible_boxes(host, request);
            serde_json::from_value::<SearchSlice<PageCoordinates>>(result.take())
                .map_err(|e| e.to_string())
                .and_then(|slice| kernel_search_slice_of(slice, &mut boxes).map_err(|e| e.message))
        } else if kind == "pages.list" {
            serde_json::from_value::<PageListSnapshot<PageCoordinates>>(result.take())
                .map_err(|e| e.to_string())
                .and_then(|list| page_list_in_file_space(list).map_err(|e| e.message))
                .and_then(|list| serde_json::to_value(list).map_err(|e| e.to_string()))
        } else {
            let mut boxes = visible_boxes(host, request);
            serde_json::from_value::<DocumentActionsSnapshot<PageDestination>>(result.take())
                .map_err(|e| e.to_string())
                .and_then(|snapshot| {
                    snapshot
                        .try_map_destinations(&mut |d| pdf_destination_of(d, &mut boxes))
                        .map_err(|e| e.message)
                })
                .and_then(|snapshot| serde_json::to_value(snapshot).map_err(|e| e.to_string()))
        };
        let envelope = match converted {
            Ok(value) => {
                *result = value;
                envelope
            }
            // An answer the host should never give: refused, as the kernel's
            // decode reports any host error.
            Err(error) => serde_json::json!({ "error": {
                "name": "EngineError",
                "code": "WireFormat",
                "message": format!("the {kind} answer is not in page space: {error}"),
            } }),
        };
        HostResponse { json: envelope.to_string(), payloads: response.payloads }
    }

    /// The answer with each form field record's raw `/Ff` word, read from
    /// the session's form model by the field's object number, as
    /// `flags: {raw}`. Upstream's record has no raw word (c3b90c3a4) and the
    /// kernel's does; [`super::to_kernel_form_records`] keeps this one
    /// rather than rebuilding it. A field with no object number, or one the
    /// model no longer has, is left for that rebuild.
    pub(super) fn with_raw_field_flags(
        host: &Host,
        request: &Value,
        response: HostResponse,
    ) -> HostResponse {
        fn records(value: &mut Value, raw_of: &mut dyn FnMut(u32) -> Option<u32>) {
            match value {
                Value::Object(map) => {
                    if map.get("family").is_some_and(Value::is_string)
                        && map.contains_key("valueEntry")
                        && let Some(raw) = map
                            .get("ref")
                            .filter(|r| {
                                r.get("kind").and_then(Value::as_str) == Some("objectNumber")
                            })
                            .and_then(|r| r.get("objectNumber"))
                            .and_then(Value::as_u64)
                            .and_then(|n| u32::try_from(n).ok())
                            .and_then(&mut *raw_of)
                    {
                        map.insert("flags".into(), serde_json::json!({ "raw": raw }));
                    }
                    map.values_mut().for_each(|v| records(v, raw_of));
                }
                Value::Array(items) => items.iter_mut().for_each(|v| records(v, raw_of)),
                _ => {}
            }
        }
        if !response.json.contains("\"valueEntry\"") {
            return response;
        }
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        let Some(key) = request.get("docId").and_then(Value::as_str).map(|doc| {
            match request.get("layerName").and_then(Value::as_str) {
                Some(layer) => SessionKey::layer(doc, layer),
                None => SessionKey::document(doc),
            }
        }) else {
            return response;
        };
        let Ok(owned) = host.sessions().session(&key).and_then(|s| s.document()) else {
            return response;
        };
        let Some(model) = owned.document().form_model() else { return response };
        let mut raw_of = |n: u32| {
            model.field_index_by_object_number(n).and_then(|i| model.field(i)).map(|f| f.flags())
        };
        records(&mut envelope, &mut raw_of);
        HostResponse { json: envelope.to_string(), payloads: response.payloads }
    }

    /// Form records answered in page space (`{fields, widgets}`), in the
    /// file's coordinates, as [`form_in_file_space`] converts `forms.list`'s.
    pub(super) fn form_records_in_file_space(
        host: &Host,
        request: &Value,
        records: Value,
    ) -> Result<Value, String> {
        let mut boxes = visible_boxes(host, request);
        form_in_file_space(records, &mut boxes)
    }

    /// `forms.list`'s answer in the file's coordinates: each field's action
    /// destinations on the pages they go to, and each widget row's `rect`,
    /// `box` and action destinations, the inverse of the host's
    /// `page_form_snapshot_of`.
    fn form_in_file_space(
        mut snapshot: Value,
        boxes: &mut impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError>,
    ) -> Result<Value, String> {
        fn actions_in_file_space<A, F>(
            actions: &mut Value,
            boxes: &mut impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError>,
            map: impl FnOnce(
                A,
                &mut dyn FnMut(PageDestination) -> Result<PdfDestination, EngineError>,
            ) -> Result<F, EngineError>,
        ) -> Result<(), String>
        where
            A: serde::de::DeserializeOwned,
            F: serde::Serialize,
        {
            if actions.is_null() {
                return Ok(());
            }
            let read: A = serde_json::from_value(actions.take()).map_err(|e| e.to_string())?;
            let converted =
                map(read, &mut |d| pdf_destination_of(d, &mut *boxes)).map_err(|e| e.message)?;
            *actions = serde_json::to_value(converted).map_err(|e| e.to_string())?;
            Ok(())
        }

        for field in snapshot.get_mut("fields").and_then(Value::as_array_mut).into_iter().flatten()
        {
            if let Some(actions) = field.get_mut("actions") {
                actions_in_file_space(
                    actions,
                    boxes,
                    |a: FieldActions<PageDestination>, mut convert| {
                        a.try_map_destinations(&mut convert)
                    },
                )?;
            }
        }
        for row in snapshot.get_mut("widgets").and_then(Value::as_array_mut).into_iter().flatten() {
            let page = serde_json::from_value::<PageRef>(row["page"].clone())
                .map_err(|e| format!("a widget row's page: {e}"))?;
            let visible = boxes(page.object_number()).map_err(|e| e.message)?;
            for key in ["rect", "box"] {
                let place = serde_json::from_value::<PageRect>(row[key].take())
                    .map_err(|e| format!("a widget row's {key}: {e}"))?;
                row[key] =
                    serde_json::to_value(pdf_rect_of(place, visible)).map_err(|e| e.to_string())?;
            }
            if let Some(actions) = row.get_mut("actions") {
                actions_in_file_space(
                    actions,
                    boxes,
                    |a: AnnotationActions<PageDestination>, mut convert| {
                        a.try_map_destinations(&mut convert)
                    },
                )?;
            }
        }
        Ok(snapshot)
    }

    /// A page's text geometry as the kernel's selection reads it: the file's
    /// coordinates, and the spelling the wire had before upstream's a10ed2659
    /// and 6153c181e (`charStart`, `looseBox`/`tightBox` or
    /// `looseQuad`/`tightQuad` with corners `p1` to `p4`, flag bits, and a
    /// rotated run's baseline angle in radians counter-clockwise). A glyph or
    /// run with no box keeps the zeroed one.
    fn kernel_geometry_of(
        snapshot: PageGeometrySnapshot<PageCoordinates>,
        visible: PdfRect,
    ) -> Value {
        const ZERO: PdfRect = PdfRect { left: 0.0, bottom: 0.0, right: 0.0, top: 0.0 };
        let rect = |b: PageRect, empty: bool| if empty { ZERO } else { pdf_rect_of(b, visible) };
        let quad = |q: PageQuad, empty: bool| {
            if empty {
                let zero = serde_json::json!({ "x": 0.0, "y": 0.0 });
                return serde_json::json!({ "p1": zero, "p2": zero, "p3": zero, "p4": zero });
            }
            let q = pdf_quad_of(q, visible);
            serde_json::json!({ "p1": q.p1, "p2": q.p2, "p3": q.p3, "p4": q.p4 })
        };
        let flags = |space: bool, empty: bool| u32::from(space) | (u32::from(empty) << 1);
        let runs = snapshot
            .runs
            .into_iter()
            .map(|run| {
                let run_rect = rect(run.rect(), !run.has_box());
                let font_size = run.font_size();
                let mut out = match run {
                    PageGeometryRun::Upright { start, glyphs, .. } => serde_json::json!({
                        "rect": run_rect,
                        "charStart": start,
                        "glyphs": glyphs.into_iter().map(|g| {
                            let mut glyph = serde_json::json!({
                                "looseBox": rect(g.loose, g.empty),
                                "flags": flags(g.space, g.empty),
                            });
                            if let Some(tight) = g.tight {
                                glyph["tightBox"] = serde_json::json!(rect(tight, false));
                            }
                            glyph
                        }).collect::<Vec<_>>(),
                    }),
                    PageGeometryRun::Rotated { start, glyphs, rotation, ascent_flip, .. } => {
                        let mut radians = -rotation.to_radians();
                        if radians <= -std::f64::consts::PI {
                            radians += 2.0 * std::f64::consts::PI;
                        }
                        serde_json::json!({
                            "rect": run_rect,
                            "charStart": start,
                            "glyphs": glyphs.into_iter().map(|g| {
                                let mut glyph = serde_json::json!({
                                    "looseQuad": quad(g.loose, g.empty),
                                    "flags": flags(g.space, g.empty),
                                });
                                if let Some(tight) = g.tight {
                                    glyph["tightQuad"] = quad(tight, false);
                                }
                                glyph
                            }).collect::<Vec<_>>(),
                            "rotation": radians,
                            "ascentFlip": ascent_flip,
                        })
                    }
                };
                if let Some(size) = font_size {
                    out["fontSize"] = serde_json::json!(size);
                }
                out
            })
            .collect::<Vec<_>>();
        serde_json::json!({ "runs": runs })
    }

    /// A search slice as the kernel's search plugin reads it: segments in the
    /// file's coordinates with corners `p1` to `p4`, and the spelling before
    /// upstream's a10ed2659 (`charStart`/`charCount`, a snippet as `text`
    /// with the match at `matchStart` for `matchLength` UTF-16 units,
    /// `scannedPages`/`totalPages`). Each match is measured on its own page.
    fn kernel_search_slice_of(
        slice: SearchSlice<PageCoordinates>,
        box_of: &mut impl FnMut(PageObjectNumber) -> Result<PdfRect, EngineError>,
    ) -> Result<Value, EngineError> {
        let utf16 = |text: &str| text.encode_utf16().count();
        let matches = slice
            .matches
            .into_iter()
            .map(|found| {
                let visible = box_of(found.page)?;
                let segments = found
                    .segments
                    .iter()
                    .map(|segment| {
                        let q = pdf_quad_of(segment.quad, visible);
                        serde_json::json!({
                            "quad": { "p1": q.p1, "p2": q.p2, "p3": q.p3, "p4": q.p4 },
                            "rect": pdf_rect_of(segment.rect, visible),
                            "advance": segment.advance,
                        })
                    })
                    .collect::<Vec<_>>();
                let mut hit = serde_json::json!({
                    "page": PageRef::from(found.page),
                    "charStart": found.start,
                    "charCount": found.count,
                    "segments": segments,
                });
                if let Some(snippet) = found.snippet {
                    hit["snippet"] = serde_json::json!({
                        "text": format!("{}{}{}", snippet.before, snippet.matched, snippet.after),
                        "matchStart": utf16(&snippet.before),
                        "matchLength": utf16(&snippet.matched),
                    });
                }
                Ok(hit)
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        Ok(serde_json::json!({
            "matches": matches,
            "nextCursor": slice.next_cursor,
            "scannedPages": slice.pages_searched,
            "totalPages": slice.page_count,
        }))
    }

    /// A page name write's answer with its result's and its event's layout
    /// in the file's coordinates.
    fn names_in_file_space(response: HostResponse) -> HostResponse {
        let Ok(mut envelope) = serde_json::from_str::<Value>(&response.json) else {
            return response;
        };
        let converted =
            ["/result/result", "/result/event"].iter().try_for_each(|at| {
                match envelope.pointer_mut(at).filter(|answer| answer.get("layout").is_some()) {
                    Some(answer) => layout_in_file_space(answer),
                    None => Ok(()),
                }
            });
        let envelope = match converted {
            Ok(()) => envelope,
            Err(error) => serde_json::json!({ "error": error.to_wire() }),
        };
        HostResponse { json: envelope.to_string(), payloads: response.payloads }
    }

    /// `answer.layout`, a page list in page space, in the file's
    /// coordinates, as the kernel reads a layout.
    pub(super) fn layout_in_file_space(answer: &mut Value) -> Result<(), EngineError> {
        let Some(layout) = answer.get_mut("layout") else {
            return Ok(());
        };
        let list: PageListSnapshot<PageCoordinates> = serde_json::from_value(layout.take())
            .map_err(|e| {
                EngineError::with_message(
                    epdf_engine::error::EngineErrorCode::WireFormat,
                    format!("a layout is not in page space: {e}"),
                )
            })?;
        *layout = serde_json::to_value(page_list_in_file_space(list)?).map_err(|e| {
            EngineError::with_message(
                epdf_engine::error::EngineErrorCode::WireFormat,
                format!("a layout did not serialize: {e}"),
            )
        })?;
        Ok(())
    }

    fn page_list_in_file_space(
        list: PageListSnapshot<PageCoordinates>,
    ) -> Result<PageListSnapshot<PdfCoordinates>, EngineError> {
        let boxes: HashMap<u32, PdfRect> =
            list.pages.iter().map(|p| (p.page_object_number.get(), p.pdf_crop_box)).collect();
        let pages = list
            .pages
            .into_iter()
            .map(|page| {
                let visible = page.pdf_crop_box;
                let rect = |b| pdf_rect_of(b, visible);
                let actions = page
                    .actions
                    .map(|a| {
                        a.try_map_destinations(&mut |d| pdf_destination_of(d, &mut lookup(&boxes)))
                    })
                    .transpose()?;
                Ok(PageLayout::<PdfCoordinates> {
                    index: page.index,
                    page_object_number: page.page_object_number,
                    label: page.label,
                    size: page.size,
                    rotation: page.rotation,
                    user_unit: page.user_unit,
                    tab_order: page.tab_order,
                    boxes: PageBoxes {
                        media: rect(page.boxes.media),
                        crop: rect(page.boxes.crop),
                        bleed: rect(page.boxes.bleed),
                        trim: rect(page.boxes.trim),
                        art: rect(page.boxes.art),
                    },
                    pdf_crop_box: page.pdf_crop_box,
                    actions,
                })
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        Ok(PageListSnapshot { page_count: list.page_count, pages, named_pages: list.named_pages })
    }
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
                        let mut list = serde_json::json!({ "kind": "annotations.list",
                            "docId": doc, "pages": [page] });
                        if let Some(layer) = &layer {
                            list["layerName"] = layer.clone();
                        }
                        let response = kernel_annotations::listing(host, &list);
                        let listed = serde_json::from_str::<Value>(&response.json)
                            .ok()
                            .and_then(|mut e| e.pointer_mut("/result/pages/0").map(Value::take))
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
/// `objectNumber`, a `PageRef`'s `pageObjectNumber`, an annotation ref's
/// `annotObjectNumber` and a form field ref's `fieldObjectNumber` become
/// `objectNumber` (595bdc6c7).
fn to_wire_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("objectNumber")
                && !map.contains_key("objectNumber")
                && let Some(number) = map
                    .remove("pageObjectNumber")
                    .or_else(|| map.remove("annotObjectNumber"))
                    .or_else(|| map.remove("fieldObjectNumber"))
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
/// (`pageObjectNumber`). Form field refs, which also have no `page`, are
/// told apart by where they sit and respelled first
/// ([`to_kernel_form_records`]).
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

/// The host's form records as the kernel reads them, before
/// [`to_kernel_refs`] respells the rest. Upstream's records (595bdc6c7,
/// c3b90c3a4, bf7d4558e) differ from the kernel's in four ways, put back here until
/// Task 13 ports the kernel:
///
/// - A form field ref is `{kind, fieldObjectNumber}`: a field record's
///   `ref`, the ref under a `field` key (a signature's, a widget row's), and
///   each slot of `calculationOrder` and of a reset's `refs`.
/// - A field record (an object with a `family` and a `valueEntry`) carries
///   `fieldObjectNumber` beside its ref, and its `/Ff` bits as `flags:
///   {readOnly, required, noExport, raw}`. Upstream answers no raw word:
///   [`call_host`] reads it from the session's form model into
///   `flags.raw` ([`page_space::with_raw_field_flags`]). An answer that
///   did not come through it gets one rebuilt from the bits the record
///   reports, where a bit no record reports (`DoNotSpellCheck`, `Sort`)
///   reads clear.
/// - A list box's record has no `defaultValue`: upstream's (bf7d4558e) is
///   the option values a reset selects, where the kernel reads a text or
///   combo box's string, so it is left out.
/// - A field's widget (`FormWidget`, an object with `objectNumber` and
///   `page` and no `kind`) names its number `annotObjectNumber`.
fn to_kernel_form_records(value: &mut Value) {
    fn field_ref(value: &mut Value) {
        if let Value::Object(map) = value
            && map.get("kind").and_then(Value::as_str) == Some("objectNumber")
            && !map.contains_key("page")
            && let Some(number) = map.remove("objectNumber")
        {
            map.insert("fieldObjectNumber".into(), number);
        }
    }
    fn raw_flags(map: &Map<String, Value>) -> u64 {
        let set = |key: &str| map.get(key).and_then(Value::as_bool) == Some(true);
        let bit = |on: bool, at: u32| if on { 1u64 << at } else { 0 };
        let family = map.get("family").and_then(Value::as_str).unwrap_or_default();
        let mut raw = bit(set("readOnly"), 0) | bit(set("required"), 1) | bit(set("noExport"), 2);
        raw |= match family {
            "text" => bit(set("multiline"), 12) | bit(set("password"), 13) | bit(set("comb"), 24),
            "radio" => {
                bit(set("noToggleToOff"), 14) | bit(true, 15) | bit(set("radiosInUnison"), 25)
            }
            "pushbutton" => bit(true, 16),
            "combobox" => bit(true, 17) | bit(set("edit"), 18),
            "listbox" => bit(set("multiSelect"), 21),
            _ => 0,
        };
        raw
    }
    match value {
        Value::Object(map) => {
            let is_field =
                map.get("family").is_some_and(Value::is_string) && map.contains_key("valueEntry");
            if is_field {
                let number = map
                    .get("ref")
                    .filter(|r| r.get("kind").and_then(Value::as_str) == Some("objectNumber"))
                    .and_then(|r| r.get("objectNumber"))
                    .cloned()
                    .unwrap_or(Value::from(0));
                map.insert("fieldObjectNumber".into(), number);
                let flags = serde_json::json!({
                    "readOnly": map.get("readOnly").cloned().unwrap_or(Value::Bool(false)),
                    "required": map.get("required").cloned().unwrap_or(Value::Bool(false)),
                    "noExport": map.get("noExport").cloned().unwrap_or(Value::Bool(false)),
                    "raw": map
                        .get("flags")
                        .and_then(|flags| flags.get("raw"))
                        .filter(|raw| raw.is_u64())
                        .cloned()
                        .unwrap_or_else(|| Value::from(raw_flags(map))),
                });
                map.insert("flags".into(), flags);
                if map.get("family").and_then(Value::as_str) == Some("listbox") {
                    map.remove("defaultValue");
                }
                if let Some(reference) = map.get_mut("ref") {
                    field_ref(reference);
                }
            }
            if !map.contains_key("kind")
                && map.contains_key("page")
                && let Some(number) = map.remove("objectNumber")
            {
                map.insert("annotObjectNumber".into(), number);
            }
            if let Some(field) = map.get_mut("field") {
                field_ref(field);
            }
            for key in ["calculationOrder", "refs"] {
                if let Some(Value::Array(refs)) = map.get_mut(key) {
                    refs.iter_mut().for_each(field_ref);
                }
            }
            map.values_mut().for_each(to_kernel_form_records);
        }
        Value::Array(items) => items.iter_mut().for_each(to_kernel_form_records),
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

/// A `SearchRequest` as the plugin sends it (`{query, mode, startPage,
/// cursor, skip, budget}`, the spelling before upstream's a10ed2659), as the
/// host's `search.query` reads it: the flattened `SearchScanRequest`, with
/// the query's text and flags at the top level, `snippets` for `full` or no
/// mode and not for `rects` (any other mode is `invalid-input`), `from` for `startPage` as a `PageRef` with a whole object number,
/// and `limit` for `budget`.
fn search_request(request: Value) -> Result<Value, Error> {
    let invalid = |what: &str| {
        coded("invalid-input", format!("[epdf-viewer-host] search.query {what}: {request}"))
    };
    let Value::Object(mut old) = request.clone() else {
        return Err(invalid("takes a SearchRequest"));
    };
    let Some(Value::Object(mut flat)) = old.remove("query") else {
        return Err(invalid("needs a query"));
    };
    // An unknown mode is refused, not read as the full one: `rects` is the
    // tier that withholds text, and a misspelling must not leak snippets.
    let snippets = match old.remove("mode") {
        None | Some(Value::Null) => true,
        Some(Value::String(mode)) if mode == "full" => true,
        Some(Value::String(mode)) if mode == "rects" => false,
        Some(_) => return Err(invalid("takes a mode of full or rects")),
    };
    flat.insert("snippets".into(), Value::Bool(snippets));
    if let Some(start) = old.remove("startPage").filter(|s| !s.is_null()) {
        flat.insert("from".into(), page_ref("search.query's startPage", &start)?);
    }
    for key in ["cursor", "skip"] {
        if let Some(value) = old.remove(key).filter(|v| !v.is_null()) {
            flat.insert(key.into(), value);
        }
    }
    if let Some(Value::Object(budget)) = old.remove("budget") {
        let mut limit = Map::new();
        for (from, to) in [("maxMatches", "matches"), ("maxPages", "pages")] {
            if let Some(value) = budget.get(from).filter(|v| !v.is_null()) {
                limit.insert(to.into(), value.clone());
            }
        }
        flat.insert("limit".into(), Value::Object(limit));
    }
    Ok(Value::Object(flat))
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
    if matches!(decode, Decode::FormResult | Decode::FormMutation) {
        to_kernel_form_records(&mut envelope);
    }
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
        Decode::Result | Decode::OptionalResult | Decode::FormResult => Value::Object(result),
        Decode::FirstPage => match take(&mut result, "pages")? {
            Value::Array(mut pages) if !pages.is_empty() => pages.swap_remove(0),
            other => return Err(malformed(format!("a listing with no page: {other}"))),
        },
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
        Decode::Mutation | Decode::FormMutation => {
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
