//! The engine's own thread. PDFium never leaves `OwnerThread`; this client
//! thread queues calls to it so the kernel thread never blocks on a render.
//! A PDFium call cannot be interrupted, so a kernel abort only drops the
//! reply.

use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread::JoinHandle;

use epdf_engine::dispatch::OwnerThread;
use epdf_engine::error::{EngineError, EngineErrorCode};
use epdf_engine::host::{Host, HostResponse};

type Reply = Box<dyn FnOnce(HostResponse) + Send>;

enum Request {
    Call { json: String, payloads: Vec<Vec<u8>>, reply: Reply },
    /// Answered once every request queued before it has been answered.
    Drained(Box<dyn FnOnce() + Send>),
}

struct State {
    tx: Option<Sender<Request>>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct EngineWorker {
    state: Arc<Mutex<State>>,
    /// Set by `join`: what is still queued is answered without PDFium.
    closing: Arc<AtomicBool>,
    /// Held by the client thread until PDFium is closed: a test's view of
    /// whether the thread outlived its host.
    alive: Weak<()>,
}

impl EngineWorker {
    pub fn spawn() -> EngineWorker {
        EngineWorker::spawn_with(Host::new)
    }

    /// `spawn`, with the host built by `setup` on the owner thread: the
    /// stamp plugin's asset engine installs an image encoder there.
    pub fn spawn_with(setup: impl FnOnce() -> Host + Send + 'static) -> EngineWorker {
        let (tx, rx) = mpsc::channel::<Request>();
        let alive = Arc::new(());
        let witness = Arc::downgrade(&alive);
        let closing = Arc::new(AtomicBool::new(false));
        let skip = closing.clone();
        let handle = std::thread::Builder::new()
            .name("epdf-engine-client".into())
            .spawn(move || {
                let mut owner = OwnerThread::start(setup);
                for request in rx {
                    match request {
                        Request::Call { reply, .. } if skip.load(Ordering::SeqCst) => {
                            reply(shut_down());
                        }
                        Request::Call { json, payloads, reply } => {
                            let response = owner
                                .run(move |host: &mut Host| {
                                    crate::translate::call_host(host, &json, payloads)
                                })
                                .unwrap_or_else(|error| HostResponse {
                                    json: serde_json::json!({ "error": error }).to_string(),
                                    payloads: Vec::new(),
                                });
                            reply(response);
                        }
                        Request::Drained(reply) => reply(),
                    }
                }
                owner.close();
                drop(alive);
            })
            .expect("the engine client thread");
        EngineWorker {
            state: Arc::new(Mutex::new(State { tx: Some(tx), handle: Some(handle) })),
            closing,
            alive: witness,
        }
    }

    /// Queue a call. After `shutdown` the reply still comes, at once and as
    /// an error, so a caller waiting on it is never left pending.
    pub fn submit(&self, json: String, payloads: Vec<Vec<u8>>, reply: Reply) {
        let request = Request::Call { json, payloads, reply };
        if let Some(Request::Call { reply, .. }) = self.send(request) {
            reply(shut_down());
        }
    }

    /// Refuse further calls. `drained` runs, on the client thread, once
    /// every call queued before this one has been answered; at once, on
    /// this thread, if the worker was already shut down.
    pub fn shutdown_after(&self, drained: Box<dyn FnOnce() + Send>) {
        if let Some(Request::Drained(drained)) = self.send(Request::Drained(drained)) {
            drained();
        }
        self.shutdown();
    }

    /// Refuse further calls. Calls already queued still run.
    pub fn shutdown(&self) {
        self.state.lock().unwrap().tx.take();
    }

    /// `shutdown`, then wait for the client thread to answer what is queued
    /// (as shut down, without running it), close PDFium and exit. A call
    /// already inside PDFium finishes first: it cannot be interrupted. Never called on the client thread: only the
    /// kernel thread owns a worker, and the client thread's replies only
    /// post to the kernel thread (a post that never blocks, and fails once
    /// the kernel thread is stopping).
    pub fn join(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let handle = {
            let mut state = self.state.lock().unwrap();
            state.tx.take();
            state.handle.take()
        };
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    #[doc(hidden)]
    pub fn alive(&self) -> Weak<()> {
        self.alive.clone()
    }

    /// The request back if the thread no longer takes requests.
    fn send(&self, request: Request) -> Option<Request> {
        match self.state.lock().unwrap().tx.as_ref() {
            Some(tx) => tx.send(request).err().map(|e| e.0),
            None => Some(request),
        }
    }
}

fn shut_down() -> HostResponse {
    HostResponse {
        json: serde_json::json!({
            "error": EngineError::with_message(EngineErrorCode::DocNotOpen, "the engine is shut down")
        })
        .to_string(),
        payloads: Vec::new(),
    }
}
