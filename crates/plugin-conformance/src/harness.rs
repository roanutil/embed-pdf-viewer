//! What a plugin crate implements to be replayed, and what the runner hands
//! it: the per-run [`Harness`].

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, undefined};
use serde_json::{Value, json};

use crate::fake_engine::FakeEngine;

/// What a call did, before the harness writes it into the trace.
pub enum Outcome {
    Returned(Value),
    Threw(Error),
    Promise(Promise<Value>),
}

impl Outcome {
    pub fn undefined() -> Outcome {
        Outcome::Returned(undefined())
    }

    /// A `Result` of a synchronous call.
    pub fn from_result(result: Result<Value, Error>) -> Outcome {
        match result {
            Ok(value) => Outcome::Returned(value),
            Err(error) => Outcome::Threw(error),
        }
    }

    /// A promise whose value serializes.
    pub fn promise<T: serde::Serialize + Clone + 'static>(promise: Promise<T>) -> Outcome {
        Outcome::Promise(promise.map(|v| Ok(serde_json::to_value(v).unwrap_or(Value::Null))))
    }

    /// A promise of nothing: resolves to `undefined`.
    pub fn promise_unit(promise: Promise<()>) -> Outcome {
        Outcome::Promise(promise.map(|_| Ok(undefined())))
    }
}

/// A plugin crate's replay adapter.
pub trait HarnessPlugin {
    /// The factory names a vector's `plugins[].factory` may use.
    fn factories(&self) -> Vec<&'static str>;
    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error>;
    /// Tokens a vector may name, beyond the ones this run makes.
    fn tokens(&self) -> Vec<AnyToken> {
        Vec::new()
    }
    /// Call a method on a capability this adapter owns; `None` if it does not
    /// own this capability's type. `args` are canonical JSON, `$fn` specs
    /// included: [`Harness::callback`] turns one into a function.
    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome>;
    /// The capability's own members, in the order the TypeScript object
    /// literal defines them, for printing the object when a call returns it.
    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>>;
}

#[derive(Default)]
pub(crate) struct Entry {
    pub outcome: Option<Value>,
    pub settled: Vec<Value>,
    pub actions: Vec<Value>,
    pub callbacks: Vec<Value>,
    pub reports: Vec<Value>,
}

pub type RefFn = Rc<dyn Fn(Vec<Value>) -> Outcome>;

pub(crate) struct Inner {
    pub entry: RefCell<Entry>,
    pub refs: RefCell<Vec<RefFn>>,
    pub promises: RefCell<Vec<Promise<Value>>>,
    pub tokens: RefCell<HashMap<String, AnyToken>>,
    pub fake: RefCell<Option<Rc<FakeEngine>>>,
    pub adapters: Vec<Rc<dyn HarnessPlugin>>,
    /// Plugins built in this run, by id, for a probe's `sameAs`.
    pub built: RefCell<HashMap<String, AnyPlugin>>,
}

/// The per-run harness. Cheap to clone.
#[derive(Clone)]
pub struct Harness(pub(crate) Rc<Inner>);

impl Harness {
    pub(crate) fn new(
        adapters: Vec<Rc<dyn HarnessPlugin>>,
        tokens: HashMap<String, AnyToken>,
    ) -> Harness {
        Harness(Rc::new(Inner {
            entry: RefCell::new(Entry::default()),
            refs: RefCell::new(Vec::new()),
            promises: RefCell::new(Vec::new()),
            tokens: RefCell::new(tokens),
            fake: RefCell::new(None),
            adapters,
            built: RefCell::new(HashMap::new()),
        }))
    }

    /// Log a callback into the current trace entry.
    pub fn log(&self, name: &str, args: Vec<Value>) {
        let args = args.into_iter().map(|a| crate::scrub(&a)).collect::<Vec<_>>();
        self.0.entry.borrow_mut().callbacks.push(json!({ "fn": name, "args": args }));
    }

    /// Report an error into the current trace entry, as the kernel's
    /// `report` does.
    pub fn report(&self, error: &Error) {
        self.0.entry.borrow_mut().reports.push(error.to_canonical());
    }

    /// The token with this name: a registered plugin's, else one made for
    /// this run (the first request decides its hint).
    pub fn token(&self, name: &str, hint: Option<&str>) -> AnyToken {
        if let Some(token) = self.0.tokens.borrow().get(name) {
            return token.clone();
        }
        let token = AnyToken::new(name, hint.map(str::to_string));
        self.0.tokens.borrow_mut().insert(name.to_string(), token.clone());
        token
    }

    /// Register a function handed back to the caller; `{"$ref": n}`.
    pub fn register_fn(&self, f: RefFn) -> Value {
        let mut refs = self.0.refs.borrow_mut();
        refs.push(f);
        json!({ "$ref": refs.len() - 1 })
    }

    pub(crate) fn ref_fn(&self, id: usize) -> Option<RefFn> {
        self.0.refs.borrow().get(id).cloned()
    }

    /// Track a promise handed back to the caller; `{"$promise": n}`. Its
    /// settlement lands in whichever trace entry is current when it settles.
    pub fn track(&self, promise: Promise<Value>) -> Value {
        let id = {
            let mut promises = self.0.promises.borrow_mut();
            promises.push(promise.clone());
            promises.len() - 1
        };
        let h = Rc::downgrade(&self.0);
        promise.on_settle(move |result| {
            let Some(h) = h.upgrade() else { return };
            let record = match result {
                Ok(value) => json!({ "promise": id, "resolved": crate::scrub(value) }),
                Err(error) => json!({ "promise": id, "rejected": error.to_canonical() }),
            };
            h.entry.borrow_mut().settled.push(record);
        });
        json!({ "$promise": id })
    }

    pub(crate) fn promise(&self, id: usize) -> Option<Promise<Value>> {
        self.0.promises.borrow().get(id).cloned()
    }

    /// A promise a `settle` step with this label settles.
    pub fn defer(&self, label: &str) -> Promise<Value> {
        let (promise, resolver) = Promise::pending();
        if let Some(fake) = self.0.fake.borrow().as_ref() {
            fake.defer(label, Box::new(move |result| resolver.settle(result)));
        }
        promise
    }

    /// Print a capability object: its members as refs that call back into
    /// the adapter that owns it.
    pub fn describe(&self, capability: Rc<dyn Any>) -> Option<Value> {
        for adapter in &self.0.adapters {
            if let Some(members) = adapter.members(&capability) {
                let mut out = serde_json::Map::new();
                for member in members {
                    let adapter = adapter.clone();
                    let capability = capability.clone();
                    let h = self.clone();
                    let f: RefFn = Rc::new(move |args| {
                        adapter.call(&capability, member, args, &h).unwrap_or_else(|| {
                            Outcome::Threw(Error::new(format!(
                                "[scenario] \"{member}\" is not a function"
                            )))
                        })
                    });
                    out.insert(member.to_string(), self.register_fn(f));
                }
                return Some(Value::Object(out));
            }
        }
        None
    }

    /// Call a method on any capability, through whichever adapter owns it.
    pub fn call(&self, capability: &Rc<dyn Any>, method: &str, args: Vec<Value>) -> Outcome {
        for adapter in &self.0.adapters {
            if let Some(outcome) = adapter.call(capability, method, args.clone(), self) {
                return outcome;
            }
        }
        Outcome::Threw(Error::new("[scenario] no adapter owns this capability"))
    }

    /// A scripted callback from a `$fn` spec: each call logs `{fn, args}`
    /// and answers with the next `seq` element (the last repeats) or the
    /// spec itself.
    pub fn callback(&self, spec: &Value) -> Callback {
        Callback {
            harness: self.clone(),
            spec: Rc::new(spec.clone()),
            calls: Rc::new(RefCell::new(0)),
        }
    }

    /// Record a value the way an outcome prints: scrubbed.
    pub fn outcome_json(&self, outcome: Outcome) -> Value {
        match outcome {
            Outcome::Returned(value) => json!({ "returned": crate::scrub(&value) }),
            Outcome::Threw(error) => json!({ "threw": error.to_canonical() }),
            Outcome::Promise(promise) => {
                let tracked = self.track(promise);
                json!({ "promise": tracked["$promise"] })
            }
        }
    }
}

/// What a scripted callback answered.
pub enum Answer {
    Returns(Value),
    Throws(Error),
    Promise(Promise<Value>),
}

#[derive(Clone)]
pub struct Callback {
    harness: Harness,
    spec: Rc<Value>,
    calls: Rc<RefCell<usize>>,
}

impl Callback {
    pub fn name(&self) -> &str {
        self.spec.get("$fn").and_then(Value::as_str).unwrap_or_default()
    }

    pub fn call(&self, args: Vec<Value>) -> Answer {
        self.harness.log(self.name(), args);
        let index = {
            let mut calls = self.calls.borrow_mut();
            let i = *calls;
            *calls += 1;
            i
        };
        let answer = match self.spec.get("seq").and_then(Value::as_array) {
            Some(seq) if !seq.is_empty() => seq[index.min(seq.len() - 1)].clone(),
            _ => (*self.spec).clone(),
        };
        if let Some(error) = answer.get("throws") {
            return Answer::Throws(Error::from_spec(error));
        }
        if let Some(label) = answer.get("defer").and_then(Value::as_str) {
            return Answer::Promise(self.harness.defer(label));
        }
        if let Some(error) = answer.get("rejects") {
            return Answer::Promise(Promise::rejected(Error::from_spec(error)));
        }
        if let Some(value) = answer.get("resolves") {
            return Answer::Promise(Promise::resolved(value.clone()));
        }
        Answer::Returns(answer.get("returns").cloned().unwrap_or_else(undefined))
    }
}
