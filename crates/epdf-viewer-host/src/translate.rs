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
//! produces is what the plugins read, so the one place the host and the
//! `engine-core` DTOs differ for a mapped kind (`PageLayout.ref`) is filled
//! in here. A shape the host did not send is an error, never a guess.

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
    /// A mutation: `{result, mutationSequence, event}`. Answers `result`
    /// and hands back the event to publish.
    Mutation,
    /// Width, height and stride, with the pixels moved into the `HostTable`.
    Raster,
    Unit,
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

/// Whether `translate` maps `path`: what `DocumentHandle::has` answers.
pub fn maps(path: &str) -> bool {
    route(path).is_some()
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
        "search.query" => ("search.query", Decode::Result),
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

fn malformed(what: String) -> Error {
    Error::new(format!("[epdf-viewer-host] the host answered {what}"))
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
                let pon = page
                    .get("pageObjectNumber")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| {
                        malformed(format!("a page without a pageObjectNumber: {page}"))
                    })?;
                if let Value::Object(page) = page {
                    page.insert("ref".into(), PageRef::new(pon).to_json());
                }
            }
            Value::Object(result)
        }
        Decode::Mutation => {
            let value = take(&mut result, "result")?;
            return Ok(Decoded { value, event: result.remove("event") });
        }
        Decode::Raster => {
            let bytes = response
                .payloads
                .into_iter()
                .next()
                .ok_or_else(|| malformed("a raster without its pixels".into()))?;
            result.insert("$host".into(), HostTable::put(bytes).into());
            Value::Object(result)
        }
    };
    Ok(Decoded { value, event: None })
}
