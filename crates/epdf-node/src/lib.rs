//! Server binding layer: `napi-rs` exports for Linux and macOS.
//!
//! The Phase 5 server milestone hosts this crate from
//! `cloudpdf/server/src/runtime/worker-entry.ts:18`, behind a supervised host
//! with `QuarantiningEnginePool` and `EngineRecycler`.
//!
//! Exports must return `Result` rather than panic: `napi-derive` only wraps
//! functions marked `#[napi(catch_unwind)]`, which this crate does not use.
//!
//! # One export, not sixty-five
//!
//! [`Engine::call`] takes the wire envelope as a string and the binary
//! arguments as buffers, and hands both back. The routing is
//! `epdf_engine::host::Host`'s, shared with the WASM and uniffi bindings, so
//! there is one match statement for the protocol rather than one per host
//! language. A misspelled argument key in a per-operation export would
//! compile here and fail at runtime; there is nothing in this file for it to
//! be misspelled in.
//!
//! # Threads
//!
//! An `Engine` is pinned to the thread that constructed it, and the type
//! system here cannot say so: `napi` requires an exported object to be
//! `Send`, and every PDFium handle beneath this is thread-confined. The
//! server's worker model is what provides the confinement, one engine per
//! worker thread, and this crate states the requirement rather than
//! enforcing it.

use napi::bindgen_prelude::{Buffer, Error, Result, Status};
use napi_derive::napi;

/// What one request produced.
///
/// The JSON and the binaries travel separately for the same reason the web
/// wire separates them: a saved PDF or a rendered page is megabytes, and
/// base64 through a string would double it on a path that does not need to.
#[napi(object)]
pub struct EngineResponse {
    /// Either `{"result": ...}` or `{"error": ...}`. One shape for both
    /// outcomes, so a caller has one parse and one branch.
    pub json: String,
    /// Binary results, positional: the operation names the first and the
    /// second rather than keying them.
    pub payloads: Vec<Buffer>,
}

/// One engine and every document it holds open.
#[napi]
pub struct Engine {
    host: epdf_engine::host::Host,
}

#[napi]
impl Engine {
    /// Start an engine on the calling thread.
    #[napi(constructor)]
    pub fn new() -> Result<Self> {
        // Constructed inside `catch_unwind` because a panic crossing the
        // Node-API boundary is undefined behaviour, and acquiring the PDFium
        // library is the one thing here that can fail that way.
        std::panic::catch_unwind(epdf_engine::host::Host::new)
            .map(|host| Self { host })
            .map_err(|_| {
                Error::new(Status::GenericFailure, "the Rust engine could not be created")
            })
    }

    /// Run one request.
    ///
    /// Returns `Err` only when the engine panicked. An operation's own
    /// refusal is an `error` inside the JSON, because a thrown exception and
    /// a returned error would be two ways to say the same thing and a caller
    /// would have to handle both.
    #[napi]
    pub fn call(&mut self, request: String, payloads: Vec<Buffer>) -> Result<EngineResponse> {
        let payloads: Vec<Vec<u8>> = payloads.into_iter().map(|b| b.to_vec()).collect();
        let host = &mut self.host;
        let response =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                host.call(&request, payloads)
            }))
            .map_err(|_| {
                // Reported as a thrown error rather than as an envelope,
                // because a panic means the session state is unknown and the
                // supervisor's job is to recycle the worker rather than to
                // retry the operation.
                Error::new(
                    Status::GenericFailure,
                    "the Rust engine panicked; this worker should be recycled",
                )
            })?;

        Ok(EngineResponse {
            json: response.json,
            payloads: response.payloads.into_iter().map(Buffer::from).collect(),
        })
    }

    /// Cancel everything, close every session, and release every document.
    ///
    /// TERMINAL and idempotent. The engine refuses every request afterwards,
    /// which is deliberate: a shutdown happens because the worker is going
    /// away or because something went wrong, and an engine that quietly came
    /// back to life would let a supervisor keep using one it had decided to
    /// discard.
    ///
    /// A pool returning a healthy engine to the queue closes its documents
    /// with `close` instead, one per document.
    #[napi]
    pub fn shutdown(&mut self) {
        self.host.sessions_mut().shutdown();
    }

    /// How many sessions are open, for a pool deciding whether to recycle.
    #[napi]
    pub fn session_count(&self) -> u32 {
        self.host.sessions().len() as u32
    }
}
