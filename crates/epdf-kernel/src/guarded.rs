//! `guarded-handle.ts`: the document handle a controller sees as `ctx.doc`.
//! Every call made through it is refused (rejected `instance-closed`) once
//! the instance has closed, raced against the instance lifetime (a call
//! still pending at close rejects `instance-closed`, its late answer is
//! dropped, and an abortable engine call is aborted), and mapped through
//! `toPluginError`. Synchronous reads pass straight through.
//!
//! The turns follow the TypeScript. The guard reads the engine promise with
//! `Promise.resolve(pending).then(...)`: an `AbortablePromise` is a
//! `Promise` subclass, so `Promise.resolve` adopts it as a thenable (one turn
//! for the adoption job, one for its reaction) before the guard's own
//! reaction runs; a plain `Promise` is returned as it is and reacts in one.

use std::rc::Rc;

use serde_json::Value;

use crate::engine::{Answer, CallFacts, DocumentHandle, DocumentSecurity, EngineCall};
use crate::error::{Error, code, plugin_error, to_plugin_error};
use crate::rt::{self, Promise};
use crate::signal::AbortSignal;
use crate::store::Unsubscribe;

/// What the guard knows about the instance it protects.
#[derive(Clone)]
pub struct GuardedLifetime {
    /// Aborted when the instance closes or the kernel is destroyed.
    pub signal: AbortSignal,
    pub instance_id: String,
}

#[derive(Clone)]
struct Guard {
    lifetime: GuardedLifetime,
    capability: String,
}

impl Guard {
    fn closed(&self) -> Error {
        plugin_error(
            code::INSTANCE_CLOSED,
            &self.capability,
            format!("document instance {} was closed", self.lifetime.instance_id),
            None,
            None,
        )
    }

    fn promise<T: Clone + 'static>(&self, pending: EngineCall<T>) -> Promise<T> {
        let (guarded, resolver) = Promise::<T>::pending();
        let signal = self.lifetime.signal.clone();
        let abort = pending.abort_handle();
        let on_abort: Rc<dyn Fn(&Error)> = {
            let resolver = resolver.clone();
            let abort = abort.clone();
            let guard = self.clone();
            Rc::new(move |_| {
                if let Some(abort) = &abort {
                    abort(guard.lifetime.signal.reason().map(|r| r.to_canonical()));
                }
                resolver.reject(guard.closed());
            })
        };
        let registration = if signal.aborted() {
            on_abort(&signal.reason().unwrap_or_else(|| Error::new("aborted")));
            None
        } else {
            Some(signal.on_abort(on_abort))
        };
        let guard = self.clone();
        rt::spawn(async move {
            // `Promise.resolve(pending).then(...)`: awaiting an
            // `AbortablePromise` takes the same adoption turns, and a plain
            // promise reacts in one either way.
            let result = pending.await;
            if let Some(id) = registration {
                guard.lifetime.signal.remove(id);
            }
            if guard.lifetime.signal.aborted() {
                return;
            }
            resolver.settle(result.map_err(|e| to_plugin_error(&guard.capability, &e)));
        });
        // `new Promise(...)` with an `abort` member: not a subclass.
        match abort {
            Some(abort) => guarded.with_abort_member(move |reason| abort(reason)),
            None => guarded,
        }
    }
}

/// `guardHandle(handle, lifetime, capability)`.
pub fn guard_handle(
    handle: Rc<dyn DocumentHandle>,
    lifetime: GuardedLifetime,
    capability: &str,
) -> Rc<dyn DocumentHandle> {
    Rc::new(GuardedHandle {
        inner: handle,
        guard: Guard { lifetime, capability: capability.to_string() },
    })
}

struct GuardedHandle {
    inner: Rc<dyn DocumentHandle>,
    guard: Guard,
}

impl DocumentHandle for GuardedHandle {
    fn is_local(&self) -> bool {
        self.inner.is_local()
    }
    fn held_object_numbers(&self) -> usize {
        self.inner.held_object_numbers()
    }
    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        self.inner.take_object_number().map_err(|e| to_plugin_error(&self.guard.capability, &e))
    }
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        self.guard.promise(self.inner.reserve_object_numbers(count))
    }
    fn id(&self) -> String {
        self.inner.id()
    }

    fn capabilities(&self) -> Value {
        self.inner.capabilities()
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(GuardedSecurity { inner: self.inner.security(), guard: self.guard.clone() })
    }

    fn has(&self, member: &str) -> bool {
        self.inner.has(member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.guard.promise(self.inner.call(path, pon, args))
    }

    /// The facts pass through to the handle, under the same guard as `call`.
    fn call_with(
        &self,
        facts: &CallFacts,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
    ) -> EngineCall<Value> {
        self.guard.promise(self.inner.call_with(facts, path, pon, args))
    }

    fn call_answer(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Answer> {
        self.guard.promise(self.inner.call_answer(path, pon, args))
    }

    /// `page(ref)` throws synchronously; the guard maps the throw.
    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.inner.page_index(pon).map_err(|e| to_plugin_error(&self.guard.capability, &e))
    }

    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        self.inner.subscribe(listener)
    }

    fn last_server_id(&self) -> Option<String> {
        self.inner.last_server_id()
    }

    /// A synchronous member: the TypeScript guard passes it through.
    fn set_working_set(&self, view: &str, pages: Value) {
        self.inner.set_working_set(view, pages)
    }
}

struct GuardedSecurity {
    inner: Rc<dyn DocumentSecurity>,
    guard: Guard,
}

impl DocumentSecurity for GuardedSecurity {
    fn allows_field(&self, action: &str, target: &Value) -> bool {
        self.inner.allows_field(action, target)
    }
    fn allows(&self, capability: &str) -> bool {
        self.inner.allows(capability)
    }
    fn allows_annotation(&self, action: &str, target: &Value) -> bool {
        self.inner.allows_annotation(action, target)
    }
    fn allows_annotation_create(&self) -> bool {
        self.inner.allows_annotation_create()
    }
    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool {
        self.inner.allows_annotation_mutation(action, target)
    }
    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool {
        self.inner.allows_annotation_group_assignment(group_id)
    }
    fn effective_scope(&self) -> Vec<String> {
        self.inner.effective_scope()
    }
    fn identity(&self) -> Value {
        self.inner.identity()
    }
    fn current(&self) -> Value {
        self.inner.current()
    }
    fn password_prompt(&self) -> Value {
        self.inner.password_prompt()
    }
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        self.guard.promise(self.inner.unlock(input))
    }
}
