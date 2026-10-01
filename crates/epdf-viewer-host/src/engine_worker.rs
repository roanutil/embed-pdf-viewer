//! The engine's own thread. PDFium never leaves `OwnerThread`; this client
//! thread queues calls to it so the kernel thread never blocks on a render.
//! A PDFium call cannot be interrupted, so a kernel abort only drops the
//! reply.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use epdf_engine::dispatch::OwnerThread;
use epdf_engine::host::{Host, HostResponse};

type Reply = Box<dyn FnOnce(HostResponse) + Send>;

struct Request {
    json: String,
    payloads: Vec<Vec<u8>>,
    reply: Reply,
}

#[derive(Clone)]
pub struct EngineWorker {
    tx: Arc<Mutex<Option<Sender<Request>>>>,
}

impl EngineWorker {
    pub fn spawn() -> EngineWorker {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("epdf-engine-client".into())
            .spawn(move || {
                let mut owner = OwnerThread::start(Host::new);
                for Request { json, payloads, reply } in rx {
                    let response = owner
                        .run(move |host: &mut Host| host.call(&json, payloads))
                        .unwrap_or_else(|error| HostResponse {
                            json: serde_json::json!({ "error": error }).to_string(),
                            payloads: Vec::new(),
                        });
                    reply(response);
                }
                owner.close();
            })
            .expect("the engine client thread");
        EngineWorker { tx: Arc::new(Mutex::new(Some(tx))) }
    }

    /// Queue a call. After `shutdown` the reply still comes, at once and as
    /// an error, so a caller waiting on it is never left pending.
    pub fn submit(&self, json: String, payloads: Vec<Vec<u8>>, reply: Reply) {
        let request = Request { json, payloads, reply };
        let refused = match self.tx.lock().unwrap().as_ref() {
            Some(tx) => tx.send(request).err().map(|e| e.0),
            None => Some(request),
        };
        if let Some(Request { reply, .. }) = refused {
            reply(HostResponse {
                json: serde_json::json!({
                    "error": { "code": "DocNotOpen", "message": "the engine is shut down" }
                })
                .to_string(),
                payloads: Vec::new(),
            });
        }
    }

    pub fn shutdown(&self) {
        self.tx.lock().unwrap().take();
    }
}
