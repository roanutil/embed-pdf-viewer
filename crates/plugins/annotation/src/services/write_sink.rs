//! `services/write-sink.ts`: awaitable writes. The effects a message
//! produces perform their engine calls synchronously inside `commit`; a
//! verb collects them here and settles on all of them. Programmatic creates
//! register a pending promise by their optimistic id and resolve when the
//! engine confirms.

use std::cell::RefCell;

use epdf_annotation_core::{AnnotationRef, Effect, Id};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::{Promise, Resolver};
use serde_json::Value;

use crate::contract::BatchResult;
use crate::controller::AnnotationCapability;
use crate::promise::{self, all_settled};
use crate::repository::annotation_key;

/// One engine write a message's effects started.
#[derive(Clone)]
pub(crate) struct WriteRecord {
    pub ref_: Option<AnnotationRef>,
    pub promise: Promise<Value>,
}

#[derive(Default)]
pub(crate) struct WriteSinkState {
    sink: RefCell<Option<Vec<WriteRecord>>>,
    /// Programmatic creates awaiting confirmation, by optimistic id, in
    /// `Map` insertion order.
    pending_creates: RefCell<Vec<(Id, Resolver<AnnotationRef>)>>,
}

/// `createEffectsOf(effects)`: the optimistic ids a message staged.
pub(crate) fn create_effects_of(effects: &[Effect]) -> Vec<Id> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Create { id } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// `groupEffectsOf(effects)`: the primaries of the groups a message staged.
pub(crate) fn group_effects_of(effects: &[Effect]) -> Vec<Id> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::CreateGroup { primary, .. } => Some(primary.clone()),
            _ => None,
        })
        .collect()
}

impl AnnotationCapability {
    /// `writes.note(ref, promise)`.
    pub(crate) fn note_write(&self, ref_: Option<AnnotationRef>, promise: Promise<Value>) {
        if let Some(sink) = self.0.writes.sink.borrow_mut().as_mut() {
            sink.push(WriteRecord { ref_, promise });
        }
    }

    /// `writes.collect(run)`.
    pub(crate) fn collect_writes<T>(&self, run: impl FnOnce() -> T) -> (T, Vec<WriteRecord>) {
        let outer = self.0.writes.sink.borrow_mut().replace(Vec::new());
        let result = run();
        let writes = std::mem::replace(&mut *self.0.writes.sink.borrow_mut(), outer);
        (result, writes.unwrap_or_default())
    }

    /// `writes.settle(refs, writes)`: the batch result, each refused member
    /// named once.
    pub(crate) fn settle_writes(
        &self,
        refs: Vec<AnnotationRef>,
        writes: Vec<WriteRecord>,
    ) -> Promise<BatchResult<AnnotationRef>> {
        promise::spawn(async move {
            let settled = all_settled(writes.iter().map(|w| w.promise.clone()).collect()).await?;
            let mut failed: Vec<(AnnotationRef, Error)> = Vec::new();
            let mut failed_keys: Vec<String> = Vec::new();
            for (i, r) in settled.iter().enumerate() {
                if let (Err(e), Some(ref_)) = (r, writes.get(i).and_then(|w| w.ref_.clone())) {
                    let key = annotation_key(&ref_);
                    if !failed_keys.contains(&key) {
                        failed_keys.push(key);
                        failed.push((ref_, to_plugin_error("annotation", e)));
                    }
                }
            }
            let applied =
                refs.into_iter().filter(|r| !failed_keys.contains(&annotation_key(r))).collect();
            Ok(BatchResult { applied, failed })
        })
    }

    /// `writes.awaitAll(writes)`: rejects with the first refusal.
    pub(crate) fn await_all_writes(&self, writes: Vec<WriteRecord>) -> Promise<()> {
        promise::spawn(async move {
            let settled = all_settled(writes.iter().map(|w| w.promise.clone()).collect()).await?;
            if let Some(Err(first)) = settled.iter().find(|r| r.is_err()) {
                return Err(to_plugin_error("annotation", first));
            }
            Ok(())
        })
    }

    /// `writes.awaitCreate(id)`: resolves once the entry with this id is
    /// confirmed by the engine.
    pub(crate) fn await_create(&self, id: Option<Id>) -> Promise<AnnotationRef> {
        let (out, resolver) = Promise::pending();
        match id {
            None => resolver.reject(plugin_error(
                code::OPERATION_FAILED,
                "annotation",
                "the annotation could not be staged",
                None,
                None,
            )),
            Some(id) => {
                let mut pending = self.0.writes.pending_creates.borrow_mut();
                match pending.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = resolver,
                    None => pending.push((id, resolver)),
                }
            }
        }
        out
    }

    pub(crate) fn is_create_awaited(&self, id: &str) -> bool {
        self.0.writes.pending_creates.borrow().iter().any(|(k, _)| k == id)
    }

    fn take_create(&self, id: &str) -> Option<Resolver<AnnotationRef>> {
        let mut pending = self.0.writes.pending_creates.borrow_mut();
        let i = pending.iter().position(|(k, _)| k == id)?;
        Some(pending.remove(i).1)
    }

    pub(crate) fn confirm_create(&self, id: &str, ref_: AnnotationRef) {
        if let Some(resolver) = self.take_create(id) {
            resolver.resolve(ref_);
        }
    }

    pub(crate) fn fail_create(&self, id: &str, error: &Error) {
        if let Some(resolver) = self.take_create(id) {
            resolver.reject(to_plugin_error("annotation", error));
        }
    }

    /// `writes.batchOver(refs, run, refOf)`: a verb per input, in order,
    /// refusals folded into the result.
    pub(crate) fn batch_over<R: Clone + 'static, T: Clone + 'static>(
        &self,
        inputs: Vec<R>,
        run: impl Fn(&AnnotationCapability, R) -> Promise<T> + 'static,
        ref_of: impl Fn(&R, T) -> Option<AnnotationRef> + 'static,
    ) -> Promise<BatchResult<R>> {
        let this = self.clone();
        promise::spawn(async move {
            let mut applied = Vec::new();
            let mut failed = Vec::new();
            for input in inputs {
                match run(&this, input.clone()).await {
                    Ok(out) => {
                        if let Some(r) = ref_of(&input, out) {
                            applied.push(r);
                        }
                    }
                    Err(error) => failed.push((input, to_plugin_error("annotation", &error))),
                }
            }
            Ok(BatchResult { applied, failed })
        })
    }
}
