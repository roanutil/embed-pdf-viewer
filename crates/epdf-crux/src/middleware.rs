//! The engine as a Crux effect middleware.
//!
//! Crux's middleware layer exists for effects that should be handled in Rust
//! rather than by the shell. That is exactly the shape of this engine: the
//! app asks for an [`EngineRequest`], this resolves it on the PDFium owner
//! thread, and the shell never sees the request at all.
//!
//! # Threads
//!
//! Crux requires `try_process_effect` to return before the effect resolves
//! (it asserts against a synchronous resolve on the calling thread), so jobs
//! go to [`OwnerThread::submit`] rather than [`OwnerThread::run`]. The
//! resolve then happens ON the owner thread, and whatever the app asks for
//! next is processed there too: a follow-up engine request is submitted from
//! inside a job, which `run` would deadlock on, and a follow-up shell effect
//! reaches the shell's callback on the owner thread. A shell has to hop to
//! its UI thread in that callback.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use crux_core::middleware::{EffectMiddleware, EffectResolver};
use epdf_engine::dispatch::OwnerThread;
use epdf_engine::host::Host;

use crate::app::{EngineReply, EngineRequest};
use crate::blobs::{Blob, BlobStore};

pub struct EngineMiddleware {
    owner: Mutex<OwnerThread<Host>>,
    blobs: Arc<BlobStore>,
}

impl EngineMiddleware {
    /// Starts the owner thread. The host is built on it, because it holds
    /// PDFium handles once anything is open and those never cross threads.
    ///
    /// `blobs` is the store the shell puts payloads in and takes results
    /// out of, shared with `CoreFfi`.
    pub fn new(blobs: Arc<BlobStore>) -> Self {
        Self { owner: Mutex::new(OwnerThread::start(Host::new)), blobs }
    }
}

impl EffectMiddleware for EngineMiddleware {
    type Op = EngineRequest;

    fn try_process_effect(
        &self,
        request: EngineRequest,
        mut resolver: EffectResolver<EngineReply>,
    ) {
        let blobs = Arc::clone(&self.blobs);
        let owner = self.owner.lock().expect("owner");
        let submitted = owner.submit(move |host| {
            let reply = match take_all(&blobs, &request.payloads) {
                // `Host::call` reports every refusal in its envelope, but a
                // panic would end the thread with this effect unresolved, and
                // the app's command would wait for it forever.
                Ok(payloads) => {
                    catch_unwind(AssertUnwindSafe(|| host.call(&request.json, payloads)))
                        .map(|response| EngineReply {
                            json: response.json,
                            payloads: response.payloads.into_iter().map(|p| blobs.put(p)).collect(),
                        })
                        .unwrap_or_else(|_| failure("RuntimeUnavailable", "the engine panicked"))
                }
                Err(missing) => failure(
                    "WireFormat",
                    &format!("blob {} was never put, or was already taken", missing.id),
                ),
            };
            resolver.resolve(reply);
        });
        if submitted.is_err() {
            // Closed. The resolver went with the job; a command that asked a
            // closed engine for something stays pending, which matches a
            // shell that has already dropped the core.
        }
    }
}

/// Every payload's bytes, or the first blob that has none.
fn take_all(blobs: &BlobStore, payloads: &[Blob]) -> Result<Vec<Vec<u8>>, Blob> {
    payloads.iter().map(|&blob| blobs.take(blob).ok_or(blob)).collect()
}

fn failure(code: &str, message: &str) -> EngineReply {
    EngineReply {
        json: serde_json::json!({ "error": { "code": code, "message": message } }).to_string(),
        payloads: Vec::new(),
    }
}
