//! A kernel over the fake engine with one open document, for tests that
//! drive a plugin's capability directly instead of replaying a vector: a
//! plugin's uniffi exports, tested without PDFium.
//!
//! The fake engine answers from tables, which suits a vector that scripts
//! every step. A capability called from a test needs an engine that behaves:
//! a write that the next read sees, and the document event a real engine
//! publishes for a write. A [`TestSpec`] adds that as responders, one per
//! engine path, ahead of the fake's tables. Everything else (open, the page
//! list, security, close) is the fake's, as the engine spec describes.
//!
//! Build it on the thread that will own it (a `KernelThread`'s build
//! closure): the kernel and its runtime are thread-local.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{
    AnyPlugin, Error, Kernel, KernelOptions, OpenOptions, OpenSource, Promise, Token, rt,
};
use serde_json::{Map, Value, json};

use crate::fake_engine::{AsEngine, FakeEngine};

/// What a responder answers a call with, and the document events to publish
/// before the answer, as a real engine publishes a mutation's event first.
pub struct Reply {
    pub value: Value,
    pub events: Vec<Value>,
}

impl Reply {
    pub fn value(value: Value) -> Self {
        Reply { value, events: Vec::new() }
    }
}

pub type Responder = Rc<dyn Fn(&[Value]) -> Result<Reply, Error>>;

/// The fake engine's spec, plus responders that answer document calls by
/// path before the fake's tables are consulted.
pub struct TestSpec {
    pub engine: Value,
    responders: Vec<(String, Responder)>,
}

impl TestSpec {
    pub fn new(engine: Value) -> Self {
        TestSpec { engine, responders: Vec::new() }
    }

    /// Answer the document call `path` (not a page call) with `responder`.
    pub fn respond(
        mut self,
        path: &str,
        responder: impl Fn(&[Value]) -> Result<Reply, Error> + 'static,
    ) -> Self {
        self.responders.push((path.to_string(), Rc::new(responder)));
        self
    }
}

impl From<Value> for TestSpec {
    fn from(engine: Value) -> Self {
        TestSpec::new(engine)
    }
}

/// The key of the one document [`TestKernel::open`] opens.
pub const DOCUMENT: &str = "a";

/// An engine spec with one document, [`DOCUMENT`], that allows everything.
pub fn one_document() -> Value {
    json!({
        "documents": {
            DOCUMENT: { "id": DOCUMENT, "security": { "allows": "*" }, "pages": [] }
        }
    })
}

pub struct TestKernel {
    pub kernel: Kernel,
    pub engine: Rc<FakeEngine>,
    /// The kernel's id for the open document (`a#1`).
    pub document_id: String,
}

impl TestKernel {
    /// Start a kernel with `plugins` and open [`DOCUMENT`], running the
    /// runtime until the open settles.
    pub fn open(plugins: Vec<AnyPlugin>, spec: impl Into<TestSpec>) -> Result<Self, Error> {
        let TestSpec { engine: spec, responders } = spec.into();
        let fake = FakeEngine::new(spec);
        let engine = Rc::new(TestEngine { fake: fake.clone(), responders: Rc::new(responders) });
        let kernel = Kernel::new(engine, plugins, KernelOptions::default())?;
        settle(&kernel.start())?;
        let opened = kernel.documents().open(
            OpenSource::Input(
                json!({ "kind": "bytes", "id": DOCUMENT, "bytes": { "$bytes": "" } }),
            ),
            OpenOptions::default(),
        );
        let document_id = settle(&opened)?
            .as_str()
            .ok_or_else(|| Error::new("[test-kernel] open answered something that is not an id"))?
            .to_string();
        Ok(TestKernel { kernel, engine: fake, document_id })
    }

    /// The open document's capability for `token` (or the workspace one).
    pub fn capability<C: Clone + 'static>(&self, token: &Token<C>) -> Result<C, Error> {
        self.kernel.capability(token, Some(&self.document_id))
    }

    /// Close the document, running the runtime until the close settles.
    pub fn close(&self) -> Result<(), Error> {
        settle(&self.kernel.documents().close(&self.document_id))
    }

    /// Deliver a document event to the open document's subscribers.
    pub fn emit(&self, event: &Value) -> Result<(), Error> {
        self.engine.emit(DOCUMENT, event)
    }
}

/// Run the runtime, firing timers as they come due, until `promise` settles.
pub fn settle<T: Clone + 'static>(promise: &Promise<T>) -> Result<T, Error> {
    loop {
        rt::run_until_stalled();
        if let Some(result) = promise.peek() {
            return result;
        }
        match rt::next_deadline() {
            Some(deadline) => rt::advance((deadline - rt::now()).max(0.0)),
            None => return Err(Error::new("[test-kernel] the promise can never settle")),
        }
    }
}

struct TestEngine {
    fake: Rc<FakeEngine>,
    responders: Rc<Vec<(String, Responder)>>,
}

impl Engine for TestEngine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let opened = AsEngine(self.fake.clone()).open(input, options);
        let abort = opened.abort_handle();
        let (promise, resolver) = Promise::<Rc<dyn DocumentHandle>>::pending();
        let (fake, responders) = (self.fake.clone(), self.responders.clone());
        opened.on_settle(move |result| {
            resolver.settle(result.clone().map(|inner| {
                Rc::new(TestHandle { inner, fake, responders }) as Rc<dyn DocumentHandle>
            }))
        });
        match abort {
            Some(abort) => promise.with_abort(move |reason| abort(reason)),
            None => promise,
        }
    }

    fn destroy(&self) -> EngineCall<()> {
        AsEngine(self.fake.clone()).destroy()
    }
}

struct TestHandle {
    inner: Rc<dyn DocumentHandle>,
    fake: Rc<FakeEngine>,
    responders: Rc<Vec<(String, Responder)>>,
}

impl DocumentHandle for TestHandle {
    fn id(&self) -> String {
        self.inner.id()
    }

    fn capabilities(&self) -> Value {
        self.inner.capabilities()
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.inner.security()
    }

    fn has(&self, member: &str) -> bool {
        self.inner.has(member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        let responder = match pon {
            Some(_) => None,
            None => self.responders.iter().find(|(p, _)| p == path).map(|(_, r)| r.clone()),
        };
        let Some(responder) = responder else {
            return self.inner.call(path, pon, args);
        };
        match responder(&args) {
            Ok(Reply { value, events }) => {
                let id = self.inner.id();
                for event in &events {
                    if let Err(error) = self.fake.emit(&id, event) {
                        return Promise::rejected(error);
                    }
                }
                Promise::resolved(value)
            }
            Err(error) => Promise::rejected(error),
        }
    }

    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.inner.page_index(pon)
    }

    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.inner.subscribe(listener)
    }

    fn last_server_id(&self) -> Option<String> {
        self.inner.last_server_id()
    }
}

// ── metadata ──────────────────────────────────────────────────────────────────

/// The Info-dict keys `DocumentMetadata` always carries (`null` when unset).
const METADATA_TEXT_KEYS: [&str; 8] =
    ["title", "author", "subject", "keywords", "producer", "creator", "created", "modified"];

/// [`one_document`] whose `metadata.read` answers `metadata` and whose
/// `metadata.update` applies the patch as the engine does (absent leaves,
/// `null` clears, a value sets; `custom` per key), publishes
/// `metadata.updated`, then answers `{metadata}`. `metadata` is completed to
/// the engine's full shape: every text key, `trapped`, `custom`.
pub fn spec_with_metadata(metadata: Value) -> TestSpec {
    let mut full = Map::new();
    for key in METADATA_TEXT_KEYS {
        full.insert(key.into(), Value::Null);
    }
    full.insert("trapped".into(), json!("unknown"));
    full.insert("custom".into(), json!({}));
    if let Value::Object(given) = metadata {
        full.extend(given);
    }
    let stored = Rc::new(RefCell::new(full));
    let read = stored.clone();
    TestSpec::new(one_document())
        .respond("metadata.read", move |_| Ok(Reply::value(Value::Object(read.borrow().clone()))))
        .respond("metadata.update", move |args| {
            let patch = args.first().and_then(Value::as_object).ok_or_else(|| {
                Error::engine("InvalidArg", "metadata.update takes a patch object")
            })?;
            let mut metadata = stored.borrow_mut();
            for (key, value) in patch {
                match key.as_str() {
                    "custom" => {
                        let Some(Value::Object(custom)) = metadata.get_mut("custom") else {
                            unreachable!("completed above")
                        };
                        for (name, value) in value.as_object().into_iter().flatten() {
                            match value {
                                Value::Null => custom.remove(name),
                                value => custom.insert(name.clone(), value.clone()),
                            };
                        }
                    }
                    "trapped" => {
                        metadata.insert(key.clone(), value.clone());
                    }
                    key if METADATA_TEXT_KEYS.contains(&key) => {
                        metadata.insert(key.into(), value.clone());
                    }
                    other => {
                        return Err(Error::engine(
                            "InvalidArg",
                            format!("unknown metadata key \"{other}\""),
                        ));
                    }
                }
            }
            let metadata = Value::Object(metadata.clone());
            Ok(Reply {
                value: json!({ "metadata": metadata }),
                events: vec![json!({ "type": "metadata.updated", "metadata": metadata })],
            })
        })
}
