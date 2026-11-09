//! `{ kind: 'url' }` sources: the kernel downloads them under the loading
//! tab before the engine opens them (`fetchUrlSource` in
//! `packages/core/main/src/kernel.ts`), with the host's [`HostFetch`].
//!
//! The TypeScript falls back to the global `fetch` when the host gives
//! none. A native kernel has no global `fetch`, so without a host's one a
//! URL source fails `unsupported`, as the TypeScript does where there is no
//! `fetch()` at all.

use std::rc::Rc;

use serde_json::{Map, Value};

use crate::bytes::Bytes;
use crate::error::{code, plugin_error};
use crate::rt::Promise;
use crate::signal::AbortSignal;
use crate::{Error, js};

/// `HostFetch`: how a `{ kind: 'url' }` source downloads, `fetch`'s own
/// shape. Called with the URL and the document's cancel signal, and again
/// on `retry()`. A host's adds headers or cookies, or answers from
/// somewhere else.
pub trait HostFetch {
    /// `fetch(url, { signal })`. A synchronous refusal is a rejected
    /// promise, as an `async` caller sees it.
    fn fetch(&self, url: &str, signal: AbortSignal) -> Promise<Rc<dyn FetchResponse>>;
}

/// What [`HostFetch::fetch`] answers: a `Response`'s `ok`, `status` and
/// `arrayBuffer()`.
pub trait FetchResponse {
    fn ok(&self) -> bool;
    fn status(&self) -> f64;
    /// `arrayBuffer()`: the whole file.
    fn array_buffer(&self) -> Promise<FetchedBytes>;
}

/// A downloaded file: `bytes` as the engine takes them in an open input
/// (`{"$bytes": ...}`, or a host's own value, such as a web host's
/// `{"$host": n}`, that the kernel releases with the last session that can
/// open it), and its first bytes, at least the 1028 [`is_pdf`] reads (all
/// of them for a shorter file).
#[derive(Clone, Debug)]
pub struct FetchedBytes {
    pub bytes: Value,
    pub head: Vec<u8>,
}

impl FetchedBytes {
    /// A file held in Rust: its bytes inline.
    pub fn from_bytes(bytes: Vec<u8>) -> FetchedBytes {
        let head = bytes[..bytes.len().min(HEAD_BYTES)].to_vec();
        FetchedBytes { bytes: Bytes(bytes).to_json(), head }
    }
}

/// How many leading bytes [`is_pdf`] can read: the header within the first
/// 1024, and its 4 bytes.
pub const HEAD_BYTES: usize = 1024 + PDF_HEADER.len();

/// `%PDF`, which a PDF has in its first 1024 bytes, as PDFium looks for it.
const PDF_HEADER: [u8; 4] = [0x25, 0x50, 0x44, 0x46];

/// `isPdf(bytes)` (`packages/core/main/src/pdf-header.ts`): whether bytes
/// are a PDF's, by its header. A web page or an image a URL gave instead
/// isn't.
pub fn is_pdf(bytes: &[u8]) -> bool {
    if bytes.len() < PDF_HEADER.len() {
        return false;
    }
    let end = (bytes.len() - PDF_HEADER.len()).min(1024);
    (0..=end).any(|start| bytes[start..start + PDF_HEADER.len()] == PDF_HEADER)
}

/// What a download's HTTP status says: the file isn't there, the user may
/// not have it, or neither.
fn download_error_code(status: f64) -> &'static str {
    if status == 404.0 {
        code::NOT_FOUND
    } else if status == 401.0 || status == 403.0 {
        code::PERMISSION_DENIED
    } else {
        code::OPERATION_FAILED
    }
}

/// The refusal of a host with no `fetch()` at all.
pub fn no_fetch(url: &str) -> Error {
    plugin_error(
        code::UNSUPPORTED,
        "documents",
        format!("can't download {url}: no fetch()"),
        None,
        None,
    )
}

/// `source.kind === 'url'`.
pub fn is_url_source(source: &Value) -> bool {
    source.get("kind").and_then(Value::as_str) == Some("url")
}

/// `${value}` for the URL a source names.
fn url_text(source: &Value) -> String {
    match source.get("url") {
        Some(Value::String(url)) => url.clone(),
        Some(Value::Number(n)) => js::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".into(),
        _ => "undefined".into(),
    }
}

/// `fetchUrlSource(source, signal, hostFetch)`: download a URL source's
/// file with the host's fetch; its tab shows why when the server says no,
/// or answers with something that isn't a PDF. Resolves the bytes input
/// the engine opens, with the source's `id`.
pub fn fetch_url_source(
    source: &Value,
    signal: AbortSignal,
    host_fetch: Option<Rc<dyn HostFetch>>,
) -> Promise<Value> {
    let url = url_text(source);
    let id = source.get("id").filter(|id| !crate::is_undefined(id)).cloned();
    Promise::spawn(async move {
        let Some(fetch_file) = host_fetch else { return Err(no_fetch(&url)) };
        let response = fetch_file.fetch(&url, signal).await?;
        if !response.ok() {
            let status = response.status();
            return Err(plugin_error(
                download_error_code(status),
                "documents",
                format!("couldn't download {url} (HTTP {})", js::number_to_string(status)),
                None,
                None,
            ));
        }
        let file = response.array_buffer().await?;
        // Said here, where it's known: the engine says only that it couldn't
        // open the file. A sign-in page a server answers with instead is the
        // usual one.
        if !is_pdf(&file.head) {
            crate::bytes::release_host(&file.bytes);
            return Err(plugin_error(
                code::INVALID_INPUT,
                "documents",
                format!("{url} isn't a PDF"),
                None,
                None,
            ));
        }
        let mut input = Map::new();
        input.insert("kind".into(), Value::String("bytes".into()));
        if let Some(id) = id {
            input.insert("id".into(), id);
        }
        input.insert("bytes".into(), file.bytes);
        Ok(Value::Object(input))
    })
}

/// A [`HostFetch`] over a function, for a host that has no type of its own.
pub struct FetchFn(pub Rc<dyn Fn(&str, AbortSignal) -> Promise<Rc<dyn FetchResponse>>>);

impl HostFetch for FetchFn {
    fn fetch(&self, url: &str, signal: AbortSignal) -> Promise<Rc<dyn FetchResponse>> {
        (self.0)(url, signal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pdf_finds_the_header_in_the_first_kilobyte() {
        assert!(is_pdf(b"%PDF-1.7\n"));
        assert!(is_pdf(&[b" ".repeat(1024), b"%PDF".to_vec()].concat()));
        assert!(!is_pdf(&[b" ".repeat(1025), b"%PDF".to_vec()].concat()));
        assert!(!is_pdf(b"<html>"));
        assert!(!is_pdf(b"%PD"));
        assert!(!is_pdf(b""));
    }

    #[test]
    fn a_status_names_its_code() {
        assert_eq!(download_error_code(404.0), code::NOT_FOUND);
        assert_eq!(download_error_code(401.0), code::PERMISSION_DENIED);
        assert_eq!(download_error_code(403.0), code::PERMISSION_DENIED);
        assert_eq!(download_error_code(500.0), code::OPERATION_FAILED);
    }

    #[test]
    fn from_bytes_keeps_the_head() {
        let file = FetchedBytes::from_bytes(vec![1; 2000]);
        assert_eq!(file.head.len(), HEAD_BYTES);
        assert_eq!(Bytes::from_json(&file.bytes).unwrap().0.len(), 2000);
    }
}
