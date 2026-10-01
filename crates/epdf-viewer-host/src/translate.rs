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

use epdf_engine::host::HostResponse;
use epdf_kernel::Error;
use serde_json::{Map, Value};

use crate::host_table::HostTable;

pub struct WireCall {
    pub request: Value,
    pub payloads: Vec<Vec<u8>>,
    pub decode: Decode,
}

pub enum Decode {
    Field(&'static str),
    Raster,
    Unit,
}

fn coded(code: &str, message: String) -> Error {
    Error::new(message).with("code", code)
}

/// Whether `translate` maps `path`: what `DocumentHandle::has` answers.
pub fn maps(path: &str) -> bool {
    route(path).is_some()
}

/// The wire kind and result decoding for a kernel path.
fn route(path: &str) -> Option<(&'static str, Decode)> {
    Some(match path {
        "close" => ("close", Decode::Unit),
        "pages.list" => ("pages.list", Decode::Field("snapshot")),
        "metadata.read" => ("metadata.read", Decode::Field("metadata")),
        "metadata.update" => ("metadata.update", Decode::Field("metadata")),
        "page.text.read" => ("pages.text", Decode::Field("snapshot")),
        "page.geometry.read" => ("pages.geometry", Decode::Field("snapshot")),
        "page.render.image" => ("pages.render", Decode::Raster),
        "search.query" => ("search.query", Decode::Field("slice")),
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
    let extra: Vec<(&str, Value)> = match kind {
        "metadata.update" => vec![("patch", arg(0))],
        "pages.text" | "pages.geometry" => vec![("pageObjectNumber", page()?)],
        "pages.render" => vec![("pageObjectNumber", page()?), ("options", arg(0))],
        "search.query" => vec![("request", arg(0))],
        _ => vec![],
    };
    let mut request = Map::new();
    request.insert("kind".into(), kind.into());
    request.insert("jobId".into(), job.into());
    request.insert("docId".into(), doc.into());
    for (key, value) in extra {
        if !value.is_null() {
            request.insert(key.into(), value);
        }
    }
    Ok(WireCall { request: Value::Object(request), payloads: Vec::new(), decode })
}

/// Read a host response. The host answers with the result itself where the
/// TypeScript worker protocol tags it (`{tag, snapshot}`), so a `Field` that
/// is absent reads as the whole result.
pub fn decode(decode: &Decode, response: HostResponse) -> Result<Value, Error> {
    let envelope: Value = serde_json::from_str(&response.json)
        .map_err(|e| Error::new(format!("[epdf-viewer-host] unreadable response: {e}")))?;
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("Unknown");
        let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
        return Err(Error::engine(code, format!("{code}: {message}")));
    }
    let result = envelope.get("result").cloned().unwrap_or(Value::Null);
    match decode {
        Decode::Unit => Ok(epdf_kernel::undefined()),
        Decode::Field(name) => Ok(result.get(*name).cloned().unwrap_or(result)),
        Decode::Raster => {
            let mut raster = result.get("raster").cloned().unwrap_or(result);
            let bytes = response.payloads.into_iter().next().unwrap_or_default();
            let id = HostTable::put(bytes);
            if let Value::Object(map) = &mut raster {
                map.insert("$host".into(), id.into());
            }
            Ok(raster)
        }
    }
}
