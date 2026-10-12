//! What a plugin crate implements to be replayed, and what the runner hands
//! it: the per-run [`Harness`].

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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

    /// A promise whose value serializes. A value that does not rejects,
    /// rather than resolving to a `null` the TypeScript never produced.
    pub fn promise<T: serde::Serialize + Clone + 'static>(promise: Promise<T>) -> Outcome {
        Outcome::Promise(promise.map(|v| {
            serde_json::to_value(v)
                .map_err(|e| Error::new(format!("[harness] a result does not serialize: {e}")))
        }))
    }

    /// A promise of nothing: resolves to `undefined`, in the turn `promise`
    /// does. The TypeScript hands the caller the kernel's own promise, so a
    /// `.map` here would put a step's turn meter one turn behind it.
    pub fn promise_unit(promise: Promise<()>) -> Outcome {
        let (unit, resolver) = Promise::pending();
        promise.on_settle(move |result| resolver.settle(result.clone().map(|_| undefined())));
        Outcome::Promise(unit)
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
    /// A member that is not a method (a nested object, a value), printed as
    /// the TypeScript prints it; `None` prints the member as a callable ref.
    /// Called in member order, so refs registered here number in place.
    fn describe_member(
        &self,
        _capability: &Rc<dyn Any>,
        _member: &'static str,
        _h: &Harness,
    ) -> Option<Value> {
        None
    }
}

#[derive(Default)]
pub(crate) struct Entry {
    pub outcome: Option<Value>,
    pub settled: Vec<Value>,
    pub actions: Vec<Value>,
    pub callbacks: Vec<Value>,
    pub reports: Vec<Value>,
}

/// A table's count of what it still holds live, not how far it has grown:
/// a scripted table's `Vec` length, a live table's slot count.
trait LiveCount {
    fn live_count(&self) -> usize;
}

impl<T> LiveCount for Vec<T> {
    fn live_count(&self) -> usize {
        self.len()
    }
}

/// `promises`' storage: a scripted harness keeps the growing `Vec` replay
/// always had, since a vector's `nth` scripted assertions and `scenario.rs`
/// expect. A live harness keeps a slot table instead, so
/// [`Harness::take_promise`] can free a promise's slot the moment
/// JavaScript takes it, rather than holding every settled value forever.
pub(crate) enum PromiseTable {
    Scripted(Vec<Promise<Value>>),
    Live { next: u32, slots: HashMap<u32, Promise<Value>> },
}

impl LiveCount for PromiseTable {
    fn live_count(&self) -> usize {
        match self {
            PromiseTable::Scripted(vec) => vec.len(),
            PromiseTable::Live { slots, .. } => slots.len(),
        }
    }
}

pub type RefFn = Rc<dyn Fn(Vec<Value>) -> Outcome>;

/// `refs`' storage, split as [`PromiseTable`] is: a scripted harness keeps
/// the growing `Vec` a vector's `{"ref": n}` targets index. A live harness
/// keeps a slot table, so [`Harness::release_ref`] can free a function
/// JavaScript has dropped, and a `pinned` set of the ids it must never
/// free: a capability's description, which `WasmKernel` memoizes and hands
/// out again with the same ids.
pub(crate) enum RefTable {
    Scripted(Vec<RefFn>),
    Live { next: u32, slots: HashMap<u32, RefFn>, pinned: HashSet<u32> },
}

impl LiveCount for RefTable {
    fn live_count(&self) -> usize {
        match self {
            RefTable::Scripted(vec) => vec.len(),
            RefTable::Live { slots, .. } => slots.len(),
        }
    }
}

/// A live callback: the `$fn` spec (so the host can find its function by
/// id) and the call's arguments, answered synchronously.
pub type LiveFn = Rc<dyn Fn(&Value, Vec<Value>) -> Answer>;

/// A live host's hook for a function the Rust side no longer holds: called
/// with a `$fn` name once the last [`Callback`] made from a spec naming it
/// has dropped. The same name can back several callbacks at once (the host
/// passed one function twice), so the harness counts them per name and
/// calls this only when the count reaches zero.
pub type ReleaseFn = Rc<dyn Fn(&str)>;

pub(crate) struct Inner {
    pub entry: RefCell<Entry>,
    pub refs: RefCell<RefTable>,
    pub promises: RefCell<PromiseTable>,
    pub tokens: RefCell<HashMap<String, AnyToken>>,
    pub fake: RefCell<Option<Rc<FakeEngine>>>,
    pub adapters: Vec<Rc<dyn HarnessPlugin>>,
    /// Plugins built in this run, by id, for a probe's `sameAs`.
    pub built: RefCell<HashMap<String, AnyPlugin>>,
    /// When set, every callback calls this instead of replaying a scripted
    /// answer, and the trace does not accumulate: `log` and `track`'s
    /// settlement recording are skipped.
    pub live: Option<LiveFn>,
    /// Set on a live harness that can release functions: every callback
    /// made from a named `$fn` spec then holds a [`FnGuard`].
    pub release: Option<ReleaseFn>,
    /// Live callbacks by `$fn` name, counted by their guards.
    pub held: RefCell<HashMap<String, usize>>,
    /// Set while a step's turn meter runs (`meter` in the README): a
    /// tracked promise that settles then also logs `@@settled`.
    pub metering: std::cell::Cell<bool>,
}

/// The per-run harness. Cheap to clone.
#[derive(Clone)]
pub struct Harness(pub(crate) Rc<Inner>);

impl Harness {
    /// Shared by `new` and `live`: every field but `live` starts the same
    /// way regardless of which one a run wants.
    fn with_live(
        adapters: Vec<Rc<dyn HarnessPlugin>>,
        tokens: HashMap<String, AnyToken>,
        live: Option<LiveFn>,
        release: Option<ReleaseFn>,
    ) -> Harness {
        let (refs, promises) = if live.is_some() {
            (
                RefTable::Live { next: 0, slots: HashMap::new(), pinned: HashSet::new() },
                PromiseTable::Live { next: 0, slots: HashMap::new() },
            )
        } else {
            (RefTable::Scripted(Vec::new()), PromiseTable::Scripted(Vec::new()))
        };
        Harness(Rc::new(Inner {
            entry: RefCell::new(Entry::default()),
            refs: RefCell::new(refs),
            promises: RefCell::new(promises),
            tokens: RefCell::new(tokens),
            fake: RefCell::new(None),
            metering: std::cell::Cell::new(false),
            adapters,
            built: RefCell::new(HashMap::new()),
            live,
            release,
            held: RefCell::new(HashMap::new()),
        }))
    }

    /// A scripted harness: callbacks replay a `$fn` spec's recorded answer
    /// instead of reaching a live host, and the trace accumulates as
    /// `scenario::run` expects. Public so a plugin crate's own tests can
    /// build one directly, the way its harness module does at replay time.
    pub fn new(adapters: Vec<Rc<dyn HarnessPlugin>>, tokens: HashMap<String, AnyToken>) -> Harness {
        Harness::with_live(adapters, tokens, None, None)
    }

    /// A harness whose callbacks call straight into `live` instead of
    /// replaying a scripted `$fn` spec. The trace does not accumulate: `log`
    /// and `track`'s settlement recording are skipped, so a session of
    /// manual testing does not grow it forever.
    pub fn live(
        adapters: Vec<Rc<dyn HarnessPlugin>>,
        tokens: HashMap<String, AnyToken>,
        live: LiveFn,
    ) -> Harness {
        Harness::with_live(adapters, tokens, Some(live), None)
    }

    /// [`live`](Self::live), and `release` hears about each `$fn` name once
    /// no callback made from it is left (see [`ReleaseFn`]).
    pub fn live_with_release(
        adapters: Vec<Rc<dyn HarnessPlugin>>,
        tokens: HashMap<String, AnyToken>,
        live: LiveFn,
        release: ReleaseFn,
    ) -> Harness {
        Harness::with_live(adapters, tokens, Some(live), Some(release))
    }

    /// Whether a live callback made from a spec naming `name` is still
    /// alive.
    pub fn holds_fn(&self, name: &str) -> bool {
        self.0.held.borrow().contains_key(name)
    }

    /// Whether this harness was made with [`live`](Self::live): callbacks
    /// reach a live host, so an adapter can hand them what a host can
    /// resolve instead of a trace placeholder.
    pub fn is_live(&self) -> bool {
        self.0.live.is_some()
    }

    /// Log a callback into the current trace entry. Skipped on a live
    /// harness.
    pub fn log(&self, name: &str, args: Vec<Value>) {
        if self.0.live.is_some() {
            return;
        }
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

    /// Register a function handed back to the caller; `{"$ref": n}`. A live
    /// harness never reuses an id, released or not.
    pub fn register_fn(&self, f: RefFn) -> Value {
        let id = match &mut *self.0.refs.borrow_mut() {
            RefTable::Scripted(vec) => {
                vec.push(f);
                (vec.len() - 1) as u32
            }
            RefTable::Live { next, slots, .. } => {
                let id = *next;
                *next += 1;
                slots.insert(id, f);
                id
            }
        };
        json!({ "$ref": id })
    }

    /// The function [`register_fn`](Self::register_fn) numbered `id`;
    /// `None` if it was never registered or has been released.
    pub fn ref_fn(&self, id: usize) -> Option<RefFn> {
        match &*self.0.refs.borrow() {
            RefTable::Scripted(vec) => vec.get(id).cloned(),
            RefTable::Live { slots, .. } => {
                u32::try_from(id).ok().and_then(|id| slots.get(&id).cloned())
            }
        }
    }

    /// Keep ref `id` for the life of this harness:
    /// [`release_ref`](Self::release_ref) ignores it from now on. A no-op on
    /// a scripted harness, which never releases anything.
    pub fn pin_ref(&self, id: u32) {
        if let RefTable::Live { pinned, .. } = &mut *self.0.refs.borrow_mut() {
            pinned.insert(id);
        }
    }

    /// Undo [`pin_ref`](Self::pin_ref): ref `id` is released like any
    /// other from now on. For a capability whose description will not be
    /// handed out again (its document closed).
    pub fn unpin_ref(&self, id: u32) {
        if let RefTable::Live { pinned, .. } = &mut *self.0.refs.borrow_mut() {
            pinned.remove(&id);
        }
    }

    /// Drop every ref and promise a live harness still holds, pinned or
    /// not, for a host that is done with it. The refs hold capabilities,
    /// and a plugin's store can hold callbacks that hold this harness, so
    /// until this runs the two keep each other alive. Ids still never
    /// repeat. A no-op on a scripted harness.
    pub fn clear_live(&self) {
        let refs = match &mut *self.0.refs.borrow_mut() {
            RefTable::Live { slots, pinned, .. } => {
                pinned.clear();
                std::mem::take(slots)
            }
            RefTable::Scripted(_) => HashMap::new(),
        };
        let promises = match &mut *self.0.promises.borrow_mut() {
            PromiseTable::Live { slots, .. } => std::mem::take(slots),
            PromiseTable::Scripted(_) => HashMap::new(),
        };
        // Dropped once no table is borrowed: a drop can reach back in.
        drop((refs, promises));
    }

    /// Free ref `id` on a live harness, once nothing on the caller's side
    /// can call it again. A pinned id, an unknown or already released one,
    /// and any id on a scripted harness are left alone.
    pub fn release_ref(&self, id: u32) {
        let removed = match &mut *self.0.refs.borrow_mut() {
            RefTable::Live { slots, pinned, .. } if !pinned.contains(&id) => slots.remove(&id),
            _ => None,
        };
        // Dropped once no table is borrowed: a drop can reach back in.
        drop(removed);
    }

    /// Whether a live harness handed out `id` and has since released it:
    /// ids only ever increase, so any id below the next one that is not in
    /// the table was released. Always `false` on a scripted harness.
    pub fn ref_was_released(&self, id: u32) -> bool {
        match &*self.0.refs.borrow() {
            RefTable::Live { next, slots, .. } => id < *next && !slots.contains_key(&id),
            RefTable::Scripted(_) => false,
        }
    }

    /// Track a promise handed back to the caller; `{"$promise": n}`. Its
    /// settlement lands in whichever trace entry is current when it settles,
    /// except on a live harness, where settlement records nothing (and the
    /// slot [`take_promise`](Self::take_promise) empties is not this: a
    /// promise that settles before JavaScript takes it is still there to
    /// take).
    pub fn track(&self, promise: Promise<Value>) -> Value {
        let id = {
            match &mut *self.0.promises.borrow_mut() {
                PromiseTable::Scripted(vec) => {
                    vec.push(promise.clone());
                    (vec.len() - 1) as u32
                }
                PromiseTable::Live { next, slots } => {
                    let id = *next;
                    *next += 1;
                    slots.insert(id, promise.clone());
                    id
                }
            }
        };
        let live = self.0.live.is_some();
        let h = Rc::downgrade(&self.0);
        promise.on_settle(move |result| {
            if live {
                return;
            }
            let Some(h) = h.upgrade() else { return };
            let record = match result {
                Ok(value) => json!({ "promise": id, "resolved": crate::scrub(value) }),
                Err(error) => json!({ "promise": id, "rejected": error.to_canonical() }),
            };
            h.entry.borrow_mut().settled.push(record);
            if h.metering.get() {
                // The TypeScript logs from the `.then` reaction it attached
                // when it tracked the promise: one turn after settlement.
                let h = Rc::downgrade(&h);
                epdf_kernel::rt::spawn(async move {
                    epdf_kernel::rt::tick().await;
                    if let Some(h) = h.upgrade()
                        && h.metering.get()
                    {
                        h.entry
                            .borrow_mut()
                            .callbacks
                            .push(json!({ "fn": "@@settled", "args": [id] }));
                    }
                });
            }
        });
        json!({ "$promise": id })
    }

    /// The promise [`track`](Self::track) numbered `id`, for a scripted
    /// harness's replay: the `Vec` it never removes from. `None` on a live
    /// harness, which never keeps a promise around to peek at; live code
    /// calls [`take_promise`](Self::take_promise) instead.
    pub fn promise(&self, id: usize) -> Option<Promise<Value>> {
        match &*self.0.promises.borrow() {
            PromiseTable::Scripted(vec) => vec.get(id).cloned(),
            PromiseTable::Live { .. } => None,
        }
    }

    /// The promise [`track`](Self::track) numbered `id`, taken: a live
    /// harness's table drops the slot, so nothing but the one JavaScript
    /// promise this becomes keeps it (and its settled value) alive. `None`
    /// if `id` is unknown, already taken, or this harness is scripted.
    pub fn take_promise(&self, id: u32) -> Option<Promise<Value>> {
        match &mut *self.0.promises.borrow_mut() {
            PromiseTable::Live { slots, .. } => slots.remove(&id),
            PromiseTable::Scripted(_) => None,
        }
    }

    /// Live entries by table, for the eval build's memory soak test:
    /// `{"refs": n, "promises": n, "heldFns": n}`, `heldFns` being the
    /// `$fn` names some live callback still holds.
    pub fn counts(&self) -> Value {
        json!({
            "refs": self.0.refs.borrow().live_count(),
            "promises": self.0.promises.borrow().live_count(),
            "heldFns": self.0.held.borrow().len(),
        })
    }

    /// A promise a `settle` step with this label settles.
    pub fn defer(&self, label: &str) -> Promise<Value> {
        let (promise, resolver) = Promise::pending();
        if let Some(fake) = self.0.fake.borrow().as_ref() {
            fake.defer(label, Box::new(move |result| resolver.settle(result)));
        }
        promise
    }

    /// [`defer`](Self::defer) for a promise of nothing, settled in the turn
    /// the settle step settles it: `new Promise((resolve) => defer(label,
    /// () => resolve()))`, with no `.then` between.
    pub fn defer_unit(&self, label: &str) -> Promise<()> {
        let (promise, resolver) = Promise::pending();
        if let Some(fake) = self.0.fake.borrow().as_ref() {
            fake.defer(label, Box::new(move |result| resolver.settle(result.map(|_| ()))));
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
                    if let Some(value) = adapter.describe_member(&capability, member, self) {
                        out.insert(member.to_string(), value);
                        continue;
                    }
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
    /// spec itself. On a harness made with
    /// [`live_with_release`](Self::live_with_release), a named spec's
    /// callback also holds its function until its last clone drops.
    ///
    /// So an adapter makes its callbacks when the spec arrives, and never
    /// keeps a `$fn` spec to make one from later: once the callbacks made
    /// from a name are gone, the host may have forgotten the function, and
    /// a callback made from it then calls a released id. To call a stored
    /// callback afresh, with its own `seq` count, use
    /// [`Callback::restarted`].
    pub fn callback(&self, spec: &Value) -> Callback {
        let name = spec.get("$fn").and_then(Value::as_str);
        let guard = match (&self.0.release, name) {
            (Some(_), Some(name)) => Some(Rc::new(FnGuard::new(self.clone(), name))),
            _ => None,
        };
        Callback {
            harness: self.clone(),
            spec: Rc::new(spec.clone()),
            calls: Rc::new(RefCell::new(0)),
            _guard: guard,
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

    /// [`outcome_json`](Self::outcome_json) without the scrub, for a live
    /// host: members that are `undefined` stay `{"$undefined": true}`.
    pub fn outcome_json_raw(&self, outcome: Outcome) -> Value {
        match outcome {
            Outcome::Returned(value) => json!({ "returned": value }),
            Outcome::Threw(error) => json!({ "threw": error.to_canonical() }),
            Outcome::Promise(promise) => {
                let tracked = self.track(promise);
                json!({ "promise": tracked["$promise"] })
            }
        }
    }

    /// [`outcome_json_raw`](Self::outcome_json_raw) as JSON text, written
    /// straight from the outcome. `json!` copies an interpolated `Value` by
    /// serializing it into a new one, which for a live host's every call
    /// was a deep copy of the answer it then only printed.
    pub fn outcome_text_raw(&self, outcome: Outcome) -> String {
        let (key, value) = match outcome {
            Outcome::Returned(value) => ("returned", value),
            Outcome::Threw(error) => ("threw", error.to_canonical()),
            Outcome::Promise(promise) => {
                let mut tracked = self.track(promise);
                ("promise", tracked["$promise"].take())
            }
        };
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(b"{\"");
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(b"\":");
        serde_json::to_writer(&mut out, &value).expect("a Value always serializes");
        out.push(b'}');
        String::from_utf8(out).expect("serde_json writes UTF-8")
    }
}

/// What a scripted callback answered.
pub enum Answer {
    Returns(Value),
    Throws(Error),
    Promise(Promise<Value>),
}

/// One live callback's hold on its `$fn` name: counted in
/// [`Inner::held`] while it lives, and the harness's [`ReleaseFn`] called
/// when the last hold on that name drops. A [`Callback`]'s clones share one.
struct FnGuard {
    harness: Harness,
    name: String,
}

impl FnGuard {
    fn new(harness: Harness, name: &str) -> FnGuard {
        *harness.0.held.borrow_mut().entry(name.to_string()).or_insert(0) += 1;
        FnGuard { harness, name: name.to_string() }
    }
}

impl Drop for FnGuard {
    fn drop(&mut self) {
        let last = {
            let mut held = self.harness.0.held.borrow_mut();
            let count = held.get_mut(&self.name).expect("a guard's name is counted");
            *count -= 1;
            let last = *count == 0;
            if last {
                held.remove(&self.name);
            }
            last
        };
        if let (true, Some(release)) = (last, &self.harness.0.release) {
            release(&self.name);
        }
    }
}

#[derive(Clone)]
pub struct Callback {
    harness: Harness,
    spec: Rc<Value>,
    calls: Rc<RefCell<usize>>,
    /// `None` on a scripted harness, and on a live one with no
    /// [`ReleaseFn`].
    _guard: Option<Rc<FnGuard>>,
}

impl Callback {
    /// The same function, holding it as this callback does, with its calls
    /// counted from zero: what [`Harness::callback`] on the same spec made,
    /// without reading the spec again.
    pub fn restarted(&self) -> Callback {
        Callback { calls: Rc::new(RefCell::new(0)), ..self.clone() }
    }

    pub fn name(&self) -> &str {
        self.spec.get("$fn").and_then(Value::as_str).unwrap_or_default()
    }

    pub fn call(&self, args: Vec<Value>) -> Answer {
        if let Some(live) = self.harness.0.live.as_ref() {
            return live(&self.spec, args);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_text_is_the_raw_outcome_printed() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        // Two harnesses, so each tracks its promise under the same id.
        let (text, raw) = (
            Harness::live(Vec::new(), HashMap::new(), live.clone()),
            Harness::live(Vec::new(), HashMap::new(), live),
        );
        let answer = json!({
            "pages": [{ "index": 0, "rect": { "x": 1.5, "y": -2, "w": 3e21 } }],
            "label": "a \"quoted\" é",
            "gone": { "$undefined": true },
            "none": null,
        });
        let outcomes: [fn(&Value) -> Outcome; 4] = [
            |v| Outcome::Returned(v.clone()),
            |_| Outcome::undefined(),
            |_| Outcome::Threw(Error::named("TypeError", "no page 7")),
            |v| Outcome::Promise(Promise::resolved(v.clone())),
        ];
        for outcome in outcomes {
            assert_eq!(
                text.outcome_text_raw(outcome(&answer)),
                raw.outcome_json_raw(outcome(&answer)).to_string()
            );
        }
    }

    #[test]
    fn a_live_harness_does_not_record_into_the_trace() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let harness = Harness::live(Vec::new(), HashMap::new(), live);

        harness.log("demo", vec![json!("x")]);
        harness.track(Promise::resolved(json!(1)));

        let entry = harness.0.entry.borrow();
        assert!(entry.callbacks.is_empty());
        assert!(entry.settled.is_empty());
    }

    #[test]
    fn a_live_harness_takes_a_promise_once_and_counts_it_gone() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let harness = Harness::live(Vec::new(), HashMap::new(), live);

        let tracked = harness.track(Promise::resolved(json!(1)));
        let id = tracked["$promise"].as_u64().unwrap() as u32;
        assert_eq!(harness.counts()["promises"], json!(1));

        let taken = harness.take_promise(id);
        assert!(taken.is_some());
        assert_eq!(harness.counts()["promises"], json!(0), "the take frees the slot");
        assert!(harness.take_promise(id).is_none(), "a second take finds nothing");
    }

    #[test]
    fn a_scripted_harness_keeps_its_vec_and_never_takes() {
        let harness = Harness::new(Vec::new(), HashMap::new());
        let tracked = harness.track(Promise::resolved(json!(1)));
        let id = tracked["$promise"].as_u64().unwrap() as usize;

        // The scripted path still peeks without removing, however many
        // times something asks.
        assert!(harness.promise(id).is_some());
        assert!(harness.promise(id).is_some());
        assert_eq!(harness.counts()["promises"], json!(1));
        // `take_promise` is a live-only operation; a scripted harness has no
        // slot table for it to take from.
        assert!(harness.take_promise(id as u32).is_none());
    }

    fn unit() -> RefFn {
        Rc::new(|_| Outcome::undefined())
    }

    fn ref_id(value: &Value) -> u32 {
        value["$ref"].as_u64().unwrap() as u32
    }

    #[test]
    fn a_live_harness_releases_a_ref_but_never_a_pinned_one() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let harness = Harness::live(Vec::new(), HashMap::new(), live);

        let loose = ref_id(&harness.register_fn(unit()));
        let kept = ref_id(&harness.register_fn(unit()));
        harness.pin_ref(kept);
        assert_eq!(harness.counts()["refs"], json!(2));

        harness.release_ref(loose);
        harness.release_ref(kept);
        assert_eq!(harness.counts()["refs"], json!(1), "only the loose ref is freed");
        assert!(harness.ref_fn(loose as usize).is_none());
        assert!(harness.ref_was_released(loose));
        assert!(harness.ref_fn(kept as usize).is_some(), "a pinned ref survives release");

        // Ids are never reused, so a released id cannot come back as a
        // different function.
        let next = ref_id(&harness.register_fn(unit()));
        assert!(next > kept);
        assert!(!harness.ref_was_released(next + 1), "an id never handed out");
    }

    #[test]
    fn a_function_is_released_once_its_last_callback_drops() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let released = Rc::new(RefCell::new(Vec::<String>::new()));
        let log = released.clone();
        let release: ReleaseFn = Rc::new(move |name| log.borrow_mut().push(name.to_string()));
        let harness = Harness::live_with_release(Vec::new(), HashMap::new(), live, release);
        let spec = json!({ "$fn": "js:1" });

        // Two callbacks from one name (the host passed one function twice),
        // and a clone of the first.
        let first = harness.callback(&spec);
        let clone = first.clone();
        let second = harness.callback(&spec);
        assert_eq!(harness.counts()["heldFns"], json!(1));

        drop(first);
        drop(second);
        assert!(released.borrow().is_empty(), "the clone still holds it");
        assert!(harness.holds_fn("js:1"));

        drop(clone);
        assert_eq!(*released.borrow(), vec!["js:1".to_string()]);
        assert!(!harness.holds_fn("js:1"));
        assert_eq!(harness.counts()["heldFns"], json!(0));
    }

    #[test]
    fn a_scripted_callback_holds_nothing() {
        let harness = Harness::new(Vec::new(), HashMap::new());
        let callback = harness.callback(&json!({ "$fn": "demo" }));
        assert!(!harness.holds_fn("demo"));
        drop(callback);
        assert_eq!(harness.counts()["heldFns"], json!(0));
    }

    #[test]
    fn a_scripted_harness_keeps_every_ref() {
        let harness = Harness::new(Vec::new(), HashMap::new());
        let id = ref_id(&harness.register_fn(unit()));
        harness.release_ref(id);
        assert!(harness.ref_fn(id as usize).is_some());
        assert_eq!(harness.counts()["refs"], json!(1));
    }

    /// A ref whose last strong count drops during `release_ref` can run
    /// arbitrary code, including code that reaches back into this harness
    /// (a capability's drop glue releasing another ref of its own). That
    /// must not panic on a `RefCell` already borrowed by `release_ref`
    /// itself: the removed `RefFn` has to drop only after the borrow it
    /// was found under ends.
    #[test]
    fn releasing_a_ref_whose_drop_reenters_the_harness_does_not_panic() {
        let live: LiveFn = Rc::new(|_spec, _args| Answer::Returns(json!(null)));
        let harness = Harness::live(Vec::new(), HashMap::new(), live);
        let inner = ref_id(&harness.register_fn(unit()));

        /// Drops into another `release_ref` call on the same harness.
        struct ReentrantDrop {
            harness: Harness,
            inner: u32,
        }
        impl Drop for ReentrantDrop {
            fn drop(&mut self) {
                self.harness.release_ref(self.inner);
            }
        }
        let reenter = ReentrantDrop { harness: harness.clone(), inner };
        let outer = ref_id(&harness.register_fn(Rc::new(move |_| {
            let _ = &reenter; // held only so releasing `outer` drops it
            Outcome::undefined()
        })));

        harness.release_ref(outer);
        assert!(harness.ref_fn(outer as usize).is_none());
        assert!(harness.ref_fn(inner as usize).is_none(), "the reentrant release still ran");
    }
}
