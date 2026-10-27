//! Kernel engine calls as host wire requests, and back: the job each method
//! of upstream's local document handle builds (`packages/engine/main/src/
//! document/Local*Service.ts`, `wirePack({...})`), for `epdf-engine`'s host,
//! which answers upstream's worker payloads without their tags.
//!
//! The kernel speaks member chains (`crates/epdf-kernel/src/engine.rs`) with
//! the engine's current DTOs, so a request is the method's arguments under
//! the keys its `wirePack` names, and an answer is the payload member its
//! service returns. What remains here is host wire adaptation, not shape
//! conversion:
//!
//! - the job envelope each `wirePack` carries: `effect`, `jobId`, an `opId`
//!   for a write (the caller's `WriteOptions.opId`, else a fresh one, as
//!   `opIdOf` does) and the handle's `authority` where the kind takes one;
//! - bytes: a `{"$bytes"}` (or `{"$hostBytes"}`) value inside a request goes
//!   as a payload the request names `{"payload": n}`, as `wirePack`'s
//!   transfer list does, and a `{"payload": n}` in an answer comes back as
//!   `{"$bytes"}`; a whole-file answer (a download, an extract) is parked in
//!   the `HostTable` as `{"$hostBytes": id}`;
//! - numbers: the kernel's numbers are JavaScript numbers (`17.0`), and the
//!   host's serde reads a whole one as an integer, so whole numbers go as
//!   integers;
//! - a page raster stays raw pixels in the `HostTable` behind `$host`, as
//!   the render plugin and its uniffi export read them, where the
//!   TypeScript encodes an image; the stamp plugin's asset engine encodes.

use epdf_engine::host::{AUTHORITY_KINDS, HostResponse, RequestEffect, WRITE_KINDS};
use epdf_kernel::Error;
use epdf_kernel::geometry::PageRef;
use serde_json::{Map, Value, json};

use crate::host_table::HostTable;

pub struct WireCall {
    pub request: Value,
    pub payloads: Vec<Vec<u8>>,
    pub decode: Decode,
}

pub enum Decode {
    /// No value (`void`).
    Unit,
    /// The result as the host sent it.
    Result,
    /// The result, or `null` when the host answered `null`.
    OptionalResult,
    /// One member of the result, which must be there.
    Field(&'static str),
    /// An object holding only this member of the result (`{ viewports }`).
    Only(&'static str),
    /// A write: `{result, mutationSequence, event?|events?}`. Answers
    /// `result` and hands back the events to publish first.
    Mutation,
    /// `{ result }` beside an optional notice event (`signatures.prepare`,
    /// `signatures.cancel`).
    Inner,
    /// The first payload as `{"$bytes"}`: a resource, a signature's
    /// `/Contents`, a digest.
    Bytes,
    /// The first payload parked in the `HostTable` as `{"$hostBytes": id}`:
    /// a whole file read once (a download, an extract).
    HostBytes,
    /// `pages.render`: the raster's fields beside `page` and `area`, the
    /// pixels in the `HostTable` behind `$host` with a raw-pixel `source`.
    Raster,
    /// `annotations.renderAppearances`: `{page, appearances}`, each
    /// appearance's raster as [`Decode::Raster`] makes a page's.
    Appearances,
    /// `pages.renderEncoded` on the asset engine: the `PageImage` a local
    /// engine gives (`source.kind == "bytes"`).
    Encoded,
    /// `pages.geometry` as `page.text.layout`'s `{"$textLayout": snapshot}`.
    TextLayout,
}

/// A decoded answer: the call's value, and the document events the write
/// published, in order.
#[derive(Debug)]
pub struct Decoded {
    pub value: Value,
    pub events: Vec<Value>,
}

/// What a request needs from the handle it is for.
pub struct Client {
    /// `guard.changeAuthority()`: the handle's authority, on the wire.
    pub authority: Value,
    /// `guard.can(capability)`.
    pub allows: Box<dyn Fn(&str) -> bool>,
}

fn coded(code: &str, message: String) -> Error {
    Error::new(message).with("code", code)
}

fn invalid(message: String) -> Error {
    coded("invalid-input", format!("[epdf-viewer-host] {message}"))
}

/// Every kernel path a request maps, one per line, for `maps`' namespace
/// check. `every_routed_path_is_listed` checks it against `route`.
pub const PATHS: &[&str] = &[
    "close",
    "pages.list",
    "metadata.get",
    "metadata.update",
    "metadata.custom.get",
    "metadata.custom.update",
    "actions.get",
    "outline.get",
    "attachments.list",
    "attachments.download",
    "annotations.list",
    "annotations.export",
    "annotations.import",
    "apply",
    "forms.list",
    "forms.export",
    "forms.import",
    "forms.repair",
    "forms.setSignatureAppearance",
    "pages.extract",
    "pages.flatten",
    "pages.setName",
    "pages.removeName",
    "pieceInfo.get",
    "pieceInfo.update",
    "redaction.apply",
    "search.query",
    "signatures.list",
    "signatures.getContents",
    "signatures.getDigest",
    "signatures.downloadRevision",
    "signatures.analyze",
    "signatures.prepare",
    "signatures.complete",
    "signatures.cancel",
    "download",
    "downloadLayer",
    "document.version",
    "page.annotations.list",
    "page.annotations.create",
    "page.annotations.downloadResource",
    "page.annotations.exportAppearance",
    "page.annotations.renderAppearances",
    "page.forms.renderAppearances",
    "page.measure.listViewports",
    "page.measure.setScale",
    "page.pieceInfo.get",
    "page.pieceInfo.update",
    "page.render.image",
    "page.text.get",
    "page.text.layout",
];

/// Paths the native handle answers itself, without one wire request each:
/// `render.getPolicy` (the local engine's own policy) and the page-structure
/// verbs, each a one-op `apply` (`pageVerbs`).
pub const LOCAL_PATHS: &[&str] = &[
    "render.getPolicy",
    "pages.reorder",
    "pages.rotate",
    "pages.delete",
    "pages.insert",
    "pages.insertBlank",
];

/// Whether the native handle has `path`; or, for a bare namespace such as
/// `redaction`, any path under it. This is what `DocumentHandle::has`
/// answers. `downloadLayer` is the local engine's, and every other optional
/// member the local engine has is listed (`page.measure`, `pages.extract`,
/// `pages.flatten`, `render`); `forms.submit` (the cloud's) is not.
pub fn maps(path: &str) -> bool {
    let listed = |p: &&str| *p == path || p.strip_prefix(path).is_some_and(|r| r.starts_with('.'));
    PATHS.iter().any(listed) || LOCAL_PATHS.iter().any(listed)
}

/// The wire kind and answer of one kernel path.
fn route(path: &str) -> Option<(&'static str, Decode)> {
    Some(match path {
        "close" => ("close", Decode::Unit),
        "pages.list" => ("pages.list", Decode::Result),
        "metadata.get" => ("metadata.read", Decode::Field("metadata")),
        "metadata.update" => ("metadata.update", Decode::Mutation),
        "metadata.custom.get" => ("metadata.readCustom", Decode::Field("custom")),
        "metadata.custom.update" => ("metadata.updateCustom", Decode::Mutation),
        "actions.get" => ("actions.read", Decode::Field("snapshot")),
        "outline.get" => ("outline.read", Decode::Field("outline")),
        "attachments.list" => ("attachments.list", Decode::Result),
        "attachments.download" => ("attachments.readFile", Decode::Bytes),
        "annotations.list" | "page.annotations.list" => ("annotations.list", Decode::Result),
        "annotations.export" => ("annotations.export", Decode::Field("bundle")),
        "annotations.import" => ("annotations.import", Decode::Mutation),
        "apply" => ("document.apply", Decode::Mutation),
        "forms.list" => ("forms.list", Decode::Result),
        "forms.export" => ("forms.export", Decode::Field("bundle")),
        "forms.import" => ("forms.import", Decode::Mutation),
        "forms.repair" => ("forms.repair", Decode::Mutation),
        "forms.setSignatureAppearance" => ("forms.setSignatureAppearance", Decode::Mutation),
        "pages.extract" => ("pages.extract", Decode::HostBytes),
        "pages.flatten" => ("pages.flatten", Decode::Mutation),
        "pages.setName" => ("pages.setName", Decode::Mutation),
        "pages.removeName" => ("pages.removeName", Decode::Mutation),
        "pieceInfo.get" | "page.pieceInfo.get" => ("pieceInfo.read", Decode::OptionalResult),
        "pieceInfo.update" | "page.pieceInfo.update" => ("pieceInfo.update", Decode::Mutation),
        "redaction.apply" => ("redaction.apply", Decode::Mutation),
        "search.query" => ("search.query", Decode::Result),
        "signatures.list" => ("signatures.list", Decode::Field("snapshot")),
        "signatures.getContents" => ("signatures.contents", Decode::Bytes),
        "signatures.getDigest" => ("signatures.digest", Decode::Bytes),
        "signatures.downloadRevision" => ("signatures.revisionBytes", Decode::Bytes),
        "signatures.analyze" => ("signatures.analyze", Decode::Field("analysis")),
        "signatures.prepare" => ("signatures.prepare", Decode::Inner),
        "signatures.complete" => ("signatures.complete", Decode::Mutation),
        "signatures.cancel" => ("signatures.cancel", Decode::Inner),
        "download" => ("document.saveBuffer", Decode::HostBytes),
        "downloadLayer" => ("document.saveLayerBuffer", Decode::HostBytes),
        "document.version" => ("document.version", Decode::Field("version")),
        "page.annotations.create" => ("annotations.create", Decode::Mutation),
        "page.annotations.exportAppearance" => ("annotations.exportAppearance", Decode::Bytes),
        "page.annotations.renderAppearances" | "page.forms.renderAppearances" => {
            ("annotations.renderAppearances", Decode::Appearances)
        }
        "page.measure.listViewports" => ("measure.viewports", Decode::Only("viewports")),
        "page.measure.setScale" => ("measure.setScale", Decode::Mutation),
        "page.render.image" => ("pages.render", Decode::Raster),
        "page.text.get" => ("pages.text", Decode::Result),
        "page.text.layout" => ("pages.geometry", Decode::TextLayout),
        // `downloadResource(ref, role)` reads the file or the appearance
        // stream; the role picks the kind (see `translate`).
        "page.annotations.downloadResource" => ("annotations.readFile", Decode::Bytes),
        _ => return None,
    })
}

/// `ATTACHMENT_LIMITS.decodedBytes`: the most a file read decodes.
const ATTACHMENT_DECODED_BYTES: u64 = 256 * 1024 * 1024;

pub fn translate(
    doc: &str,
    path: &str,
    pon: Option<u32>,
    args: &[Value],
    job: u64,
    client: &Client,
) -> Result<WireCall, Error> {
    let (mut kind, decode) = route(path).ok_or_else(|| {
        coded("unsupported", format!("[epdf-viewer-host] the native engine has no \"{path}\""))
    })?;
    let arg = |i: usize| args.get(i).filter(|v| !is_absent(v)).cloned();
    let page = || {
        pon.map(|pon| PageRef::new(pon).to_json())
            .ok_or_else(|| invalid(format!("{path} needs a page")))
    };
    // `WriteOptions` (`{opId}`), at the argument index each method takes it.
    let options_at = |i: usize| arg(i).unwrap_or(Value::Null);
    let mut body = Map::new();
    let mut payloads = Vec::new();
    let put = |body: &mut Map<String, Value>, key: &str, value: Option<Value>| {
        if let Some(value) = value {
            body.insert(key.into(), value);
        }
    };
    let options = match path {
        "metadata.update" | "metadata.custom.update" => {
            put(&mut body, "patch", arg(0));
            options_at(1)
        }
        "attachments.download" => {
            put(&mut body, "ref", arg(0));
            body.insert("maxDecodedBytes".into(), ATTACHMENT_DECODED_BYTES.into());
            Value::Null
        }
        "annotations.list" => {
            put(&mut body, "pages", arg(0).and_then(|o| o.get("pages").cloned()));
            Value::Null
        }
        "page.annotations.list" => {
            body.insert("pages".into(), json!([page()?]));
            Value::Null
        }
        "annotations.export" | "forms.export" => {
            body.insert("selection".into(), arg(0).unwrap_or_else(|| json!({})));
            Value::Null
        }
        "annotations.import" => {
            let options = options_at(1);
            put(&mut body, "bundle", arg(0));
            put(&mut body, "pages", member(&options, "pages"));
            body.insert(
                "attribution".into(),
                member(&options, "attribution").unwrap_or_else(|| "restore".into()),
            );
            options
        }
        "apply" => {
            put(&mut body, "change", arg(0));
            options_at(1)
        }
        "forms.import" => {
            let options = options_at(1);
            put(&mut body, "bundle", arg(0));
            put(&mut body, "pages", member(&options, "pages"));
            body.insert(
                "attribution".into(),
                member(&options, "attribution").unwrap_or_else(|| "restore".into()),
            );
            body.insert("values".into(), member(&options, "values").unwrap_or(Value::Bool(true)));
            body.insert("mayScript".into(), (client.allows)("doc.forms.script").into());
            options
        }
        "forms.repair" => {
            let options = options_at(0);
            body.insert(
                "bakeAppearances".into(),
                member(&options, "bakeAppearances").unwrap_or(Value::Bool(false)),
            );
            options
        }
        "forms.setSignatureAppearance" => {
            put(&mut body, "ref", arg(0));
            // The PDF is the request's payload 0 (`[pdf]`).
            let appearance = arg(1).unwrap_or(Value::Null);
            payloads.push(bytes_member(&appearance, "pdf", path)?);
            options_at(2)
        }
        "pages.extract" => {
            put(&mut body, "pages", arg(0));
            if let Some(flatten) = resolve_page_extract(&options_at(1), &*client.allows)? {
                body.insert("flatten".into(), flatten);
            }
            Value::Null
        }
        "pages.flatten" => {
            let options = options_at(1);
            put(&mut body, "pages", arg(0));
            body.insert(
                "usage".into(),
                member(&options, "usage").unwrap_or_else(|| "display".into()),
            );
            options
        }
        "pages.setName" => {
            if let Some(Value::Object(input)) = arg(0) {
                body.extend(input);
            }
            options_at(1)
        }
        "pages.removeName" => {
            put(&mut body, "name", arg(0).and_then(|i| i.get("name").cloned()));
            options_at(1)
        }
        "pieceInfo.get" | "page.pieceInfo.get" => {
            if path == "page.pieceInfo.get" {
                body.insert("page".into(), page()?);
            }
            put(&mut body, "application", arg(0));
            Value::Null
        }
        "pieceInfo.update" | "page.pieceInfo.update" => {
            if path == "page.pieceInfo.update" {
                body.insert("page".into(), page()?);
            }
            put(&mut body, "application", arg(0));
            put(&mut body, "patch", arg(1));
            options_at(2)
        }
        "redaction.apply" => {
            put(&mut body, "scope", arg(0));
            options_at(1)
        }
        "search.query" => {
            put(&mut body, "request", arg(0));
            Value::Null
        }
        "signatures.getContents" => {
            put(&mut body, "ref", arg(0));
            Value::Null
        }
        "signatures.getDigest" => {
            put(&mut body, "ref", arg(0));
            put(&mut body, "algorithm", arg(1));
            Value::Null
        }
        "signatures.downloadRevision" => {
            put(&mut body, "revisionIndex", arg(0));
            Value::Null
        }
        "signatures.analyze" => {
            put(&mut body, "input", arg(0));
            Value::Null
        }
        "signatures.prepare" => {
            put(&mut body, "input", arg(0));
            options_at(1)
        }
        "signatures.complete" => {
            // The CMS is the request's payload 0, beside the input.
            let mut input = arg(0).unwrap_or(Value::Null);
            payloads.push(bytes_member(&input, "cms", path)?);
            if let Some(input) = input.as_object_mut() {
                input.remove("cms");
            }
            body.insert("input".into(), input);
            options_at(1)
        }
        "signatures.cancel" => {
            put(&mut body, "signingId", arg(0));
            options_at(1)
        }
        "download" => {
            body.insert("mode".into(), save_mode(path, &options_at(0))?);
            Value::Null
        }
        "page.annotations.create" => {
            let options = options_at(1);
            body.insert("page".into(), page()?);
            put(&mut body, "draft", arg(0));
            put(&mut body, "objectNumber", member(&options, "objectNumber"));
            put(&mut body, "resources", member(&options, "resources"));
            options
        }
        "page.annotations.downloadResource" => {
            body.insert("page".into(), page()?);
            put(&mut body, "ref", arg(0));
            if arg(1).as_ref().and_then(Value::as_str) == Some("file") {
                body.insert("maxDecodedBytes".into(), ATTACHMENT_DECODED_BYTES.into());
            } else {
                kind = "annotations.readAppearance";
            }
            Value::Null
        }
        "page.annotations.exportAppearance" => {
            body.insert("page".into(), page()?);
            put(&mut body, "refs", arg(0));
            Value::Null
        }
        "page.annotations.renderAppearances" | "page.forms.renderAppearances" => {
            body.insert("page".into(), page()?);
            let family =
                if path == "page.forms.renderAppearances" { "widgets" } else { "annotations" };
            body.insert("family".into(), family.into());
            put(&mut body, "options", arg(0).map(render_options));
            Value::Null
        }
        "page.measure.listViewports" | "page.text.get" | "page.text.layout" => {
            body.insert("page".into(), page()?);
            Value::Null
        }
        "page.measure.setScale" => {
            body.insert("page".into(), page()?);
            // `PdfMeasure | null`: a cleared scale is an explicit `null`.
            body.insert("measure".into(), arg(0).unwrap_or(Value::Null));
            options_at(1)
        }
        "page.render.image" => {
            body.insert("page".into(), page()?);
            body.insert("options".into(), render_options(arg(0).unwrap_or_else(|| json!({}))));
            Value::Null
        }
        _ => Value::Null,
    };
    let mut request = Map::new();
    request.insert("kind".into(), kind.into());
    if let Some(effect) = RequestEffect::of(kind) {
        request.insert("effect".into(), effect.as_str().into());
    }
    request.insert("jobId".into(), job.into());
    if WRITE_KINDS.contains(&kind) {
        request.insert("opId".into(), op_id_of(&options)?.into());
    }
    request.insert("docId".into(), doc.into());
    request.extend(body);
    if AUTHORITY_KINDS.contains(&kind) {
        request.insert("authority".into(), client.authority.clone());
    }
    let mut request = Value::Object(request);
    to_payloads(&mut request, &mut payloads)?;
    whole_numbers(&mut request);
    Ok(WireCall { request, payloads, decode })
}

/// `close` for a document, with no handle to ask: the cleanup for an open
/// that will not hand out a document.
pub fn close_request(doc: &str, job: u64) -> WireCall {
    WireCall {
        request: json!({ "kind": "close", "effect": "close", "jobId": job, "docId": doc }),
        payloads: Vec::new(),
        decode: Decode::Unit,
    }
}

/// `pages.workingSet`: bookkeeping, never answered.
pub fn working_set_request(doc: &str, job: u64, view: &str, pages: Value) -> Value {
    let mut request = json!({
        "kind": "pages.workingSet", "jobId": job, "docId": doc, "view": view, "pages": pages,
    });
    whole_numbers(&mut request);
    request
}

/// `objectNumbers.reserve`: `count` more numbers for this session.
pub fn reserve_request(doc: &str, job: u64, count: u32) -> WireCall {
    WireCall {
        request: json!({
            "kind": "objectNumbers.reserve", "effect": "session", "jobId": job, "docId": doc,
            "count": count,
        }),
        payloads: Vec::new(),
        decode: Decode::Field("range"),
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
    let page = pon.ok_or_else(|| invalid("page.render.image needs a page".into()))?;
    let options = args.first().cloned().unwrap_or(Value::Null);
    let format = options.get("format").and_then(Value::as_str).unwrap_or("png");
    if format != "png" {
        return Err(coded(
            "unsupported",
            format!("[epdf-viewer-host] the asset engine encodes png only, not {format}"),
        ));
    }
    let mut request = json!({
        "kind": "pages.renderEncoded",
        "effect": "read",
        "jobId": job,
        "docId": doc,
        "page": PageRef::new(page).to_json(),
        "options": render_options(options),
        "encode": { "format": "png" },
    });
    whole_numbers(&mut request);
    Ok(WireCall { request, payloads: Vec::new(), decode: Decode::Encoded })
}

/// An absent member: missing, `null` or the kernel's `undefined`.
fn is_absent(value: &Value) -> bool {
    value.is_null() || epdf_kernel::is_undefined(value)
}

/// `options[key]`, unless absent.
fn member(options: &Value, key: &str) -> Option<Value> {
    options.get(key).filter(|v| !is_absent(v)).cloned()
}

/// `opIdOf(options)`: the caller's `opId`, else a fresh one. The host checks
/// it as `opIdOf` does.
fn op_id_of(options: &Value) -> Result<String, Error> {
    match options.get("opId").filter(|v| !is_absent(v)) {
        None => Ok(epdf_kernel::identity::generate_uuid()),
        Some(Value::String(op_id)) => Ok(op_id.clone()),
        Some(other) => Err(Error::engine(
            "InvalidArg",
            format!("opId must be 1 to 255 visible ASCII characters: {other}"),
        )
        .with("details", json!({ "field": "opId" }))),
    }
}

/// `download`'s `{mode}`: `incremental` (`DEFAULT_PDF_SAVE_MODE`) unless
/// the caller asks for `rewrite`.
fn save_mode(path: &str, options: &Value) -> Result<Value, Error> {
    match member(options, "mode") {
        None => Ok("incremental".into()),
        Some(mode) => match mode.as_str() {
            Some(mode @ ("incremental" | "rewrite")) => Ok(mode.into()),
            _ => Err(invalid(format!("{path}'s mode is not incremental or rewrite: {mode}"))),
        },
    }
}

/// `resolvePageExtract(options, may)` (`dto/PageExtract.ts:59`): the
/// flatten a copy paints, checked against what the handle may do, or none
/// for a plain copy.
fn resolve_page_extract(
    options: &Value,
    may: &dyn Fn(&str) -> bool,
) -> Result<Option<Value>, Error> {
    let include = |key: &str| member(options, key).and_then(|v| v.as_bool());
    let Some(flatten) = member(options, "flatten") else {
        for field in ["includeAnnotations", "includeFormFields"] {
            if member(options, field).is_some() {
                return Err(Error::engine("InvalidArg", format!("{field} takes flatten"))
                    .with("details", json!({ "field": field })));
            }
        }
        if !may("doc.download") {
            return Err(Error::permission_denied("doc.download", None));
        }
        return Ok(None);
    };
    let usage = member(&flatten, "usage").unwrap_or_else(|| "display".into());
    if !matches!(usage.as_str(), Some("display" | "print")) {
        return Err(Error::engine("InvalidArg", "usage must be \"display\" or \"print\"")
            .with("details", json!({ "field": "flatten.usage" })));
    }
    if !may("doc.download") && !may("doc.download.flattened") {
        return Err(Error::permission_denied("doc.download", Some("flatten")).with(
            "details",
            json!({ "required": "doc.download", "context": "flatten",
                    "anyOf": ["doc.download", "doc.download.flattened"] }),
        ));
    }
    // `resolvePageLayers` (`dto/PageRender.ts:42`).
    let (annotations, form_fields) = (may("doc.annotate.read"), may("doc.forms.read"));
    let (include_annotations, include_form_fields) =
        (include("includeAnnotations"), include("includeFormFields"));
    if include_annotations == Some(true) && !annotations {
        return Err(Error::permission_denied("doc.annotate.read", Some("includeAnnotations")));
    }
    if include_form_fields == Some(true) && !form_fields {
        return Err(Error::permission_denied("doc.forms.read", Some("includeFormFields")));
    }
    Ok(Some(json!({
        "usage": usage,
        "includeAnnotations": include_annotations.unwrap_or(annotations),
        "includeFormFields": include_form_fields
            .unwrap_or(include_annotations != Some(false) && form_fields),
    })))
}

/// The `Bytes` JSON under `key`, as bytes.
fn bytes_member(value: &Value, key: &str, path: &str) -> Result<Vec<u8>, Error> {
    value
        .get(key)
        .and_then(epdf_kernel::bytes::Bytes::from_json)
        .map(|b| b.0)
        .ok_or_else(|| invalid(format!("{path} needs {key} bytes")))
}

/// `PageImageOptions` as `pages.render` reads them: the encode options stay
/// behind, since `pages.render` answers raw pixels.
fn render_options(mut options: Value) -> Value {
    if let Value::Object(map) = &mut options {
        map.remove("format");
        map.remove("quality");
    }
    options
}

/// Every `{"$bytes"}` or `{"$hostBytes"}` value inside `value` as a payload
/// of the request, named `{"payload": n}`: `wirePack`'s transfer list.
fn to_payloads(value: &mut Value, payloads: &mut Vec<Vec<u8>>) -> Result<(), Error> {
    match value {
        Value::Object(map) if map.contains_key("$bytes") || map.contains_key("$hostBytes") => {
            let bytes = epdf_kernel::bytes::Bytes::from_json(value)
                .ok_or_else(|| invalid(format!("bytes that do not decode: {value}")))?;
            *value = json!({ "payload": payloads.len() });
            payloads.push(bytes.0);
        }
        Value::Object(map) => {
            for member in map.values_mut() {
                to_payloads(member, payloads)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                to_payloads(item, payloads)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Every whole JavaScript number (`17.0`) as the integer the host's serde
/// reads; the kernel's `{"$number"}` and `{"$undefined"}` markers out.
fn whole_numbers(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => {
            if let Some(f) =
                n.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15)
            {
                *value = if f < 0.0 { Value::from(f as i64) } else { Value::from(f as u64) };
            }
        }
        Value::Object(map) => {
            map.retain(|_, member| !epdf_kernel::is_undefined(member));
            map.values_mut().for_each(whole_numbers);
        }
        Value::Array(items) => items.iter_mut().for_each(whole_numbers),
        _ => {}
    }
}

/// Every `{"payload": n}` inside an answer as `{"$bytes"}` of that payload.
fn from_payloads(value: &mut Value, payloads: &[Vec<u8>]) {
    match value {
        Value::Object(map) if map.len() == 1 && map.get("payload").is_some_and(Value::is_u64) => {
            let index = map["payload"].as_u64().unwrap_or(u64::MAX) as usize;
            if let Some(bytes) = payloads.get(index) {
                *value = epdf_kernel::bytes::Bytes(bytes.clone()).to_json();
            }
        }
        Value::Object(map) => map.values_mut().for_each(|m| from_payloads(m, payloads)),
        Value::Array(items) => items.iter_mut().for_each(|i| from_payloads(i, payloads)),
        _ => {}
    }
}

fn malformed(what: String) -> Error {
    Error::new(format!("[epdf-viewer-host] the host answered {what}"))
}

/// A raster (`PageRaster`: width, height, stride and pixel format) with its
/// pixels in `payloads[index]`, as the render plugin reads a picture: the
/// raster's fields, a raw-pixel `source` carrying the byte count, and the
/// pixels in the `HostTable` behind `$host`.
fn raster_picture(raster: Value, payloads: &mut [Vec<u8>]) -> Result<Map<String, Value>, Error> {
    let Value::Object(raster) = raster else {
        return Err(malformed("a raster that is not an object".into()));
    };
    let index =
        raster.get("data").and_then(|d| d.get("payload")).and_then(Value::as_u64).unwrap_or(0)
            as usize;
    let bytes = payloads
        .get_mut(index)
        .map(std::mem::take)
        .ok_or_else(|| malformed("a raster without its pixels".into()))?;
    let mut picture: Map<String, Value> =
        raster.into_iter().filter(|(key, _)| key != "data").collect();
    let dimension = |key: &str| {
        picture
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| malformed(format!("a raster without a whole \"{key}\": {picture:?}")))
    };
    let (width, height, stride) = (dimension("width")?, dimension("height")?, dimension("stride")?);
    let fits = width.checked_mul(4).is_some_and(|row| stride >= row)
        && stride.checked_mul(height).is_some_and(|needed| bytes.len() as u64 >= needed);
    if !fits {
        return Err(malformed(format!(
            "a {width}x{height} raster with stride {stride} in {} bytes",
            bytes.len()
        )));
    }
    picture.insert("source".into(), json!({ "kind": "pixels", "$byteLength": bytes.len() }));
    picture.insert("$host".into(), HostTable::put(bytes).into());
    Ok(picture)
}

/// The first payload, or `malformed`.
fn first_payload(payloads: &mut Vec<Vec<u8>>, what: &str) -> Result<Vec<u8>, Error> {
    if payloads.is_empty() {
        return Err(malformed(format!("{what} without its payload")));
    }
    Ok(payloads.swap_remove(0))
}

/// Read a host response into what the call's caller expects.
pub fn decode(decode: &Decode, response: HostResponse) -> Result<Decoded, Error> {
    let mut envelope: Value = serde_json::from_str(&response.json)
        .map_err(|e| malformed(format!("unreadable JSON: {e}")))?;
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("Unknown");
        let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
        let mut refused = Error::engine(code, format!("{code}: {message}"));
        if let Some(details) = error.get("details") {
            refused = refused.with("details", details.clone());
        }
        return Err(refused);
    }
    let mut payloads = response.payloads;
    // A notice the host publishes beside a result that is not a write's
    // (`signatures.prepared`, `signatures.cancelled`).
    let notice: Vec<Value> = envelope.get_mut("event").map(Value::take).into_iter().collect();
    if let Decode::Unit = decode {
        return Ok(Decoded { value: epdf_kernel::undefined(), events: notice });
    }
    let result = match envelope.get_mut("result").map(Value::take) {
        Some(Value::Null) if matches!(decode, Decode::OptionalResult) => {
            return Ok(Decoded { value: Value::Null, events: notice });
        }
        Some(result) => result,
        None => return Err(malformed("neither a result nor an error".into())),
    };
    let Value::Object(mut result) = result else {
        return Err(malformed(format!("a result that is not an object: {result}")));
    };
    let take = |result: &mut Map<String, Value>, name: &str| {
        result.remove(name).ok_or_else(|| malformed(format!("a result without \"{name}\"")))
    };
    let mut value = match decode {
        Decode::Unit => unreachable!("answered above"),
        Decode::Result | Decode::OptionalResult => Value::Object(result),
        Decode::Field(name) => take(&mut result, name)?,
        Decode::Only(name) => json!({ *name: take(&mut result, name)? }),
        Decode::Inner => take(&mut result, "result")?,
        Decode::Mutation => {
            let mut value = take(&mut result, "result")?;
            let mut events: Vec<Value> = result.remove("event").into_iter().collect();
            if let Some(Value::Array(all)) = result.remove("events") {
                events.extend(all);
            }
            from_payloads(&mut value, &payloads);
            return Ok(Decoded { value, events });
        }
        Decode::Bytes => {
            epdf_kernel::bytes::Bytes(first_payload(&mut payloads, "bytes")?).to_json()
        }
        Decode::HostBytes => {
            json!({ "$hostBytes": HostTable::put(first_payload(&mut payloads, "bytes")?) })
        }
        Decode::Raster => {
            // `{page, area, raster}` (`handlePagesRender`): the raster's
            // fields beside `page` and `area`.
            let raster = take(&mut result, "raster")?;
            let picture = raster_picture(raster, &mut payloads)?;
            result.extend(picture);
            Value::Object(result)
        }
        Decode::Appearances => {
            // `{page, result: {page, appearances}}`, each appearance's
            // raster its own payload.
            let mut inner = take(&mut result, "result")?;
            if let Some(Value::Array(appearances)) = inner.get_mut("appearances") {
                for appearance in appearances {
                    let Some(entry) = appearance.as_object_mut() else { continue };
                    if let Some(raster) = entry.remove("raster") {
                        let picture = raster_picture(raster, &mut payloads)?;
                        entry.insert("image".into(), Value::Object(picture));
                    }
                }
            }
            inner
        }
        Decode::Encoded => {
            let bytes = first_payload(&mut payloads, "an encoded render")?;
            let Value::Object(mut image) = take(&mut result, "image")? else {
                return Err(malformed("an encoded render whose image is not an object".into()));
            };
            let content_type = take(&mut image, "contentType")?;
            json!({
                "width": image.remove("width").unwrap_or(Value::Null),
                "height": image.remove("height").unwrap_or(Value::Null),
                "format": "png",
                "contentType": content_type,
                "source": { "kind": "bytes", "bytes": epdf_kernel::bytes::Bytes(bytes).to_json() },
            })
        }
        Decode::TextLayout => json!({ "$textLayout": Value::Object(result) }),
    };
    from_payloads(&mut value, &payloads);
    Ok(Decoded { value, events: notice })
}
