//! Foreign URL transport for the kernel's loading, failed and retried tabs.
//! Completion and cancellation cross threads; URL policy stays in the kernel.

use std::rc::Rc;
use std::sync::Arc;

use crate::host_table::HostTable;
use epdf_ffi::{ForeignSettle, HostFailure, KernelPost};
use epdf_kernel::Promise;
use epdf_kernel::fetch::{FetchResponse, FetchedBytes, HEAD_BYTES, HostFetch};
use epdf_kernel::signal::AbortSignal;

/// Starts a request without blocking the kernel thread. Answer from any
/// thread; dropping an unanswered completion fails the open.
#[uniffi::export(with_foreign)]
pub trait DocumentFetcher: Send + Sync {
    fn fetch(
        &self,
        url: String,
        completion: Arc<DocumentFetchCompletion>,
    ) -> Arc<dyn DocumentFetchCancellation>;
}

/// Cancels the transport when its loading document closes. Must not block.
#[uniffi::export(with_foreign)]
pub trait DocumentFetchCancellation: Send + Sync {
    fn cancel(&self);
}

/// One request's answer. The first answer wins, including after cancellation.
#[derive(uniffi::Object)]
pub struct DocumentFetchCompletion {
    settle: ForeignSettle<Rc<dyn FetchResponse>>,
}

#[uniffi::export]
impl DocumentFetchCompletion {
    /// HTTP status and full response body. The kernel maps HTTP failures and
    /// checks the PDF header before asking the engine to open the bytes.
    pub fn finish(&self, status: u16, bytes: Vec<u8>) {
        self.settle
            .settle(move || Ok(Rc::new(Response { status, bytes }) as Rc<dyn FetchResponse>));
    }

    pub fn fail_with(&self, failure: HostFailure) {
        self.settle.fail(failure, "documents");
    }
}

struct Response {
    status: u16,
    bytes: Vec<u8>,
}
impl FetchResponse for Response {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    fn status(&self) -> f64 {
        self.status.into()
    }
    fn array_buffer(&self) -> Promise<FetchedBytes> {
        Promise::resolved(FetchedBytes {
            head: self.bytes[..self.bytes.len().min(HEAD_BYTES)].to_vec(),
            bytes: serde_json::json!({ "$host": HostTable::put(self.bytes.clone()) }),
        })
    }
}

pub(crate) struct NativeFetch {
    pub foreign: Arc<dyn DocumentFetcher>,
    pub post: KernelPost,
}

impl HostFetch for NativeFetch {
    fn fetch(&self, url: &str, signal: AbortSignal) -> Promise<Rc<dyn FetchResponse>> {
        let (promise, resolver) = Promise::pending();
        if let Some(reason) = signal.reason() {
            return Promise::rejected(reason);
        }
        let completion = Arc::new(DocumentFetchCompletion {
            settle: ForeignSettle::new(resolver, self.post.clone(), "a DocumentFetcher completion"),
        });
        let cancellation = self.foreign.fetch(url.to_owned(), completion.clone());
        // Weak completion: the abort registration must not keep an unanswered
        // foreign completion alive and prevent its drop from rejecting.
        let weak = Arc::downgrade(&completion);
        let registration = signal.on_abort(Rc::new(move |reason| {
            cancellation.cancel();
            if let Some(completion) = weak.upgrade() {
                let reason = reason.clone();
                completion.settle.settle(move || Err(reason));
            }
        }));
        promise.on_settle(move |_| signal.remove(registration));
        promise
    }
}
