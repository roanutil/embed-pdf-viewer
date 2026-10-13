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
    /// A page mutation: `Mutation`, with `ref` added to every layout page of
    /// the result and the event (the kernel's page registry and the
    /// page-edit plugin read `ref`), and the host's
    /// `insertedPageObjectNumbers` given as `insertedPages` refs.
    PageMutation,
    /// A byte payload (an extracted PDF), parked in the `HostTable` and
    /// answered as `{"$hostBytes": id}` for `Bytes::from_json` to take.
    HostBytes,
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
    "pages.move",
    "pages.rotate",
    "pages.delete",
    "pages.insert",
    "pages.insertBlank",
    "pages.extract",
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
        "pages.move" => ("pages.move", Decode::PageMutation),
        "pages.rotate" => ("pages.rotate", Decode::PageMutation),
        "pages.delete" => ("pages.delete", Decode::PageMutation),
        "pages.insert" => ("pages.insert", Decode::PageMutation),
        "pages.insertBlank" => ("pages.insertBlank", Decode::PageMutation),
        "pages.extract" => ("pages.extract", Decode::HostBytes),
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
