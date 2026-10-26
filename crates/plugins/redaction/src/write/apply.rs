//! `write/apply.ts`: the destructive half, `doc.redaction.apply` in refs or
//! pages scope. Applies run one at a time, in call order, so each one sees
//! the marks the previous one left. The result reaches `last_result` and
//! `on_applied` through the confirmed `redaction.applied` event, which the
//! engine publishes before the apply resolves; this area only brackets the
//! call with the `applying` flag.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_annotation_core::record::annotation_key;
use epdf_kernel::Error;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Value, json};

use crate::contract::{OperationOptions, PageTarget, RedactionApplyResult};
use crate::controller::{RedactionCapability, RedactionContext};
use crate::model::{finish_apply, start_apply};
use crate::services::store::CAPABILITY;

/// `ctx.serialQueue('apply')` with the two members of the TypeScript
/// `SerialQueue` the kernel's does not carry (`serial-queue.ts`): an
/// operation whose signal fired while it waited never starts
/// (`operation-cancelled`), and `idle()`, which resolves once everything
/// queued so far has finished. The form and measurement plugins keep the
/// same wrapper (a Task 14 kernel carry).
pub(crate) struct ApplyQueue {
    queue: SerialQueue,
    tail: Rc<RefCell<Promise<()>>>,
}

impl ApplyQueue {
    /// The queue, and `ctx.onSettle(() => queue.idle())`: a download waits
    /// for a redaction being applied, whose content must not be in the file.
    pub(crate) fn new(cx: &RedactionContext) -> Result<ApplyQueue, Error> {
        let tail: Rc<RefCell<Promise<()>>> = Rc::new(RefCell::new(Promise::resolved(())));
        // `queue.idle()`: the tail, which settles once every apply queued so
        // far has finished, however it ended.
        let idle = tail.clone();
        cx.on_settle(Rc::new(move |_signal| Ok(Some(idle.borrow().clone()))))?;
        Ok(ApplyQueue { queue: cx.serial_queue("apply"), tail })
    }

    /// `queue(operation, { signal })`.
    fn run<T: Clone + 'static>(
        &self,
        operation: impl FnOnce() -> Result<Promise<T>, Error> + 'static,
        signal: Option<AbortSignal>,
    ) -> Promise<T> {
        let start = move || match signal.filter(AbortSignal::aborted) {
            // A rejected promise, not a throw: `tail.then(start, start)`
            // adopts what `start` returns.
            Some(signal) => Ok(Promise::rejected(plugin_error(
                code::OPERATION_CANCELLED,
                CAPABILITY,
                "cancelled before it started",
                signal.reason_value(),
                None,
            ))),
            None => operation(),
        };
        let result = self.queue.run(start);
        let done = result.clone();
        *self.tail.borrow_mut() = Promise::spawn(async move {
            let _ = done.await;
            Ok(())
        });
        result
    }
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `then` reaction that settles the function's own promise.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

impl RedactionCapability {
    /// `runApply(scopeOf, options)`: queue an apply whose scope is computed
    /// when its turn comes. One whose signal fired while it waited never
    /// starts; one cancelled while the engine applies rejects
    /// `operation-cancelled` at once, and the engine's `redaction.applied`
    /// says what it removed.
    fn run_apply(
        &self,
        scope_of: impl FnOnce(&RedactionCapability) -> Result<Value, Error> + 'static,
        options: OperationOptions,
    ) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        let signal = options.signal.clone();
        self.core().queue.run(
            move || {
                Ok(Promise::spawn(async move {
                    let core = this.core();
                    let service = core.store.require_service()?;
                    let scope = scope_of(&this)?;
                    core.cx.update_state(start_apply);
                    let call = service.call("redaction.apply", None, vec![scope]);
                    let outcome = cancellable(CAPABILITY, options.signal, call).await;
                    core.cx.update_state(finish_apply);
                    outcome.map(Rc::new)
                }))
            },
            signal,
        )
    }

    /// Apply specific pending marks. Per-page outcomes ride the result.
    /// Rejects `not-found` when none of the refs is a pending mark,
    /// `unsupported` without an engine redaction service, or the engine's
    /// `permission-denied`. Irreversible.
    pub fn apply(
        &self,
        refs: &[Value],
        options: OperationOptions,
    ) -> Promise<RedactionApplyResult> {
        let wanted: HashSet<String> = refs.iter().filter_map(annotation_key).collect();
        self.run_apply(
            move |this| {
                let marks: Vec<Value> = this
                    .list_pending(None)
                    .iter()
                    .filter(|mark| annotation_key(&mark.ref_).is_some_and(|k| wanted.contains(&k)))
                    .map(|mark| mark.ref_.clone())
                    .collect();
                if marks.is_empty() {
                    return Err(plugin_error(
                        code::NOT_FOUND,
                        CAPABILITY,
                        "no matching pending marks",
                        None,
                        None,
                    ));
                }
                Ok(json!({ "annotations": marks }))
            },
            options,
        )
    }

    /// Apply the marks on some pages. Rejects like [`apply`](Self::apply),
    /// with `invalid-input` for no pages, and `not-found` for a page that
    /// isn't there. Irreversible.
    pub fn apply_pages(
        &self,
        targets: &[PageTarget],
        options: OperationOptions,
    ) -> Promise<RedactionApplyResult> {
        let this = self.clone();
        let targets = targets.to_vec();
        Promise::spawn(async move {
            if targets.is_empty() {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    CAPABILITY,
                    "no pages given",
                    None,
                    None,
                ));
            }
            // Every page is resolved first: a page that isn't there refuses
            // the call.
            let pages: Vec<PageRef> = targets
                .into_iter()
                .map(|page| this.core().store.page_ref_of(page))
                .collect::<Result<_, _>>()?;
            adopt(this.run_apply(move |_| Ok(json!({ "pages": pages })), options)).await
        })
    }

    /// Apply every redaction in the document, in pages scope, including
    /// marks on pages this client never loaded. Rejects like
    /// [`apply`](Self::apply), and with `not-ready` when the document has no
    /// pages. Irreversible.
    pub fn apply_all(&self, options: OperationOptions) -> Promise<RedactionApplyResult> {
        self.run_apply(
            |this| {
                // Pages scope is authoritative for the whole document,
                // including marks on pages this client never loaded. Pages
                // without marks report 'unchanged'.
                let all = this.core().store.pages();
                if all.is_empty() {
                    return Err(plugin_error(
                        code::NOT_READY,
                        CAPABILITY,
                        "the document has no pages",
                        None,
                        None,
                    ));
                }
                Ok(json!({ "pages": all }))
            },
            options,
        )
    }
}
