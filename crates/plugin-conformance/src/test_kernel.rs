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

/// A responder for a page call: the page's object number, then the args.
pub type PageResponder = Rc<dyn Fn(u32, &[Value]) -> Result<Reply, Error>>;

/// The fake engine's spec, plus responders that answer document and page
/// calls by path before the fake's tables are consulted.
pub struct TestSpec {
    pub engine: Value,
    responders: Vec<(String, Responder)>,
    page_responders: Vec<(String, PageResponder)>,
    lacking: Vec<String>,
}

impl TestSpec {
    pub fn new(engine: Value) -> Self {
        TestSpec {
            engine,
            responders: Vec::new(),
            page_responders: Vec::new(),
            lacking: Vec::new(),
        }
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

    /// Answer the page call `path` (`page.render.image`, say) with
    /// `responder`, which gets the page's object number and the args. The
    /// responder answers any page; `page_index` still refuses one the
    /// spec's page list lacks.
    pub fn respond_page(
        mut self,
        path: &str,
        responder: impl Fn(u32, &[Value]) -> Result<Reply, Error> + 'static,
    ) -> Self {
        self.page_responders.push((path.to_string(), Rc::new(responder)));
        self
    }
}

impl TestSpec {
    /// A handle without the optional member `member` (`doc.has(member)` is
    /// false), for an engine that doesn't have it: a plugin's defensive
    /// absent-service branch. The fake's tables still answer a call to it.
    pub fn lacking(mut self, member: &str) -> Self {
        self.lacking.push(member.to_string());
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
        TestKernel::open_with(plugins, spec, KernelOptions::default())
    }

    /// [`open`](Self::open) with the kernel's options: a host that sets
    /// `release_closed_sessions`, say.
    pub fn open_with(
        plugins: Vec<AnyPlugin>,
        spec: impl Into<TestSpec>,
        options: KernelOptions,
    ) -> Result<Self, Error> {
        TestKernel::open_with_clock(plugins, spec, options, Rc::new(epdf_kernel::clock::TimerClock))
    }

    /// [`open_with`](Self::open_with) on the host clock `clock`: one with
    /// frames, for a plugin whose motion runs on them.
    pub fn open_with_clock(
        plugins: Vec<AnyPlugin>,
        spec: impl Into<TestSpec>,
        options: KernelOptions,
        clock: Rc<dyn epdf_kernel::clock::HostClock>,
    ) -> Result<Self, Error> {
        let TestSpec { engine: spec, responders, page_responders, lacking } = spec.into();
        let fake = FakeEngine::new(spec);
        let responders =
            Rc::new(Responders { document: responders, page: page_responders, lacking });
        let engine = Rc::new(TestEngine { fake: fake.clone(), responders });
        let kernel = Kernel::new_with_clock(engine, plugins, options, clock)?;
        settle(&kernel.start())?;
        let opened = kernel.documents().open(
            OpenSource::Input(
                json!({ "kind": "bytes", "id": DOCUMENT, "bytes": { "$bytes": "" } }),
            ),
            OpenOptions::default(),
        );
        // `documents.open` answers `{ document: { id, ... } }`.
        let document_id = settle(&opened)?["document"]["id"]
            .as_str()
            .ok_or_else(|| Error::new("[test-kernel] open answered no document id"))?
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

struct Responders {
    document: Vec<(String, Responder)>,
    page: Vec<(String, PageResponder)>,
    /// Members `has` answers false for ([`TestSpec::lacking`]).
    lacking: Vec<String>,
}

impl Responders {
    /// The reply for `path`, when a responder answers it.
    fn answer(&self, path: &str, pon: Option<u32>, args: &[Value]) -> Option<Result<Reply, Error>> {
        match pon {
            Some(pon) => {
                let (_, responder) = self.page.iter().find(|(p, _)| p == path)?;
                Some(responder(pon, args))
            }
            None => {
                let (_, responder) = self.document.iter().find(|(p, _)| p == path)?;
                Some(responder(args))
            }
        }
    }
}

struct TestEngine {
    fake: Rc<FakeEngine>,
    responders: Rc<Responders>,
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
    responders: Rc<Responders>,
}

impl DocumentHandle for TestHandle {
    fn id(&self) -> String {
        self.inner.id()
    }

    // The fake's object-number pool and locality, as the engine's handle
    // has them (the trait's defaults would hold no numbers).
    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    fn held_object_numbers(&self) -> usize {
        self.inner.held_object_numbers()
    }

    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        self.inner.take_object_number()
    }

    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        self.inner.reserve_object_numbers(count)
    }

    fn on_lost_object_numbers(
        &self,
        listener: Rc<dyn Fn(&epdf_kernel::engine::ObjectNumbersLost)>,
    ) -> Unsubscribe {
        self.inner.on_lost_object_numbers(listener)
    }

    fn capabilities(&self) -> Value {
        self.inner.capabilities()
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        self.inner.security()
    }

    fn has(&self, member: &str) -> bool {
        !self.responders.lacking.iter().any(|m| m == member) && self.inner.has(member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        let Some(reply) = self.responders.answer(path, pon, &args) else {
            return self.inner.call(path, pon, args);
        };
        match reply {
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
    ["title", "author", "subject", "keywords", "producer", "creator", "createdAt", "modifiedAt"];

/// The `meta` of a metadata write's result and event.
fn metadata_meta() -> Value {
    json!({ "affectedPages": [], "cacheDelta": null, "opId": "metadata-write", "undoable": true })
}

/// [`one_document`] whose `metadata.get` answers `metadata` and whose
/// `metadata.update` applies the patch as the engine does (absent leaves,
/// `null` clears, a value sets), publishes `metadata.updated`, then answers
/// `{metadata, meta}`; `metadata.custom.get` and `metadata.custom.update`
/// do the same for the custom keys (`metadata.customUpdated`, `{custom,
/// meta}`). `metadata` is completed to the engine's full `DocumentMetadata`:
/// every text key and `trapped`; a `custom` member seeds the custom keys.
pub fn spec_with_metadata(metadata: Value) -> TestSpec {
    let mut full = Map::new();
    for key in METADATA_TEXT_KEYS {
        full.insert(key.into(), Value::Null);
    }
    full.insert("trapped".into(), json!("unknown"));
    let mut custom = Map::new();
    if let Value::Object(given) = metadata {
        for (key, value) in given {
            match (key.as_str(), value) {
                ("custom", Value::Object(keys)) => custom.extend(keys),
                (_, value) => {
                    full.insert(key, value);
                }
            }
        }
    }
    let stored = Rc::new(RefCell::new(full));
    let custom = Rc::new(RefCell::new(custom));
    let (read, read_custom, write_custom) = (stored.clone(), custom.clone(), custom);
    TestSpec::new(one_document())
        .respond("metadata.get", move |_| Ok(Reply::value(Value::Object(read.borrow().clone()))))
        .respond("metadata.update", move |args| {
            let patch = args.first().and_then(Value::as_object).ok_or_else(|| {
                Error::engine("InvalidArg", "metadata.update takes a patch object")
            })?;
            let mut metadata = stored.borrow_mut();
            for (key, value) in patch {
                if key != "trapped" && !METADATA_TEXT_KEYS.contains(&key.as_str()) {
                    return Err(Error::engine(
                        "InvalidArg",
                        format!("unknown metadata key \"{key}\""),
                    ));
                }
                metadata.insert(key.clone(), value.clone());
            }
            let metadata = Value::Object(metadata.clone());
            Ok(Reply {
                value: json!({ "metadata": metadata, "meta": metadata_meta() }),
                events: vec![json!({
                    "type": "metadata.updated", "metadata": metadata, "meta": metadata_meta()
                })],
            })
        })
        .respond("metadata.custom.get", move |_| {
            Ok(Reply::value(Value::Object(read_custom.borrow().clone())))
        })
        .respond("metadata.custom.update", move |args| {
            let patch = args.first().and_then(Value::as_object).ok_or_else(|| {
                Error::engine("InvalidArg", "metadata.custom.update takes a patch object")
            })?;
            let mut custom = write_custom.borrow_mut();
            for (name, value) in patch {
                match value {
                    Value::Null => custom.remove(name),
                    value => custom.insert(name.clone(), value.clone()),
                };
            }
            let custom = Value::Object(custom.clone());
            Ok(Reply {
                value: json!({ "custom": custom, "meta": metadata_meta() }),
                events: vec![json!({
                    "type": "metadata.customUpdated", "custom": custom, "meta": metadata_meta()
                })],
            })
        })
}
