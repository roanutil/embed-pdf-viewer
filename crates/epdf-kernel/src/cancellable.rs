//! Caller cancellation is distinct from an instance being closed.
use crate::{
    Promise,
    error::{plugin_error, to_plugin_error},
    rt,
    signal::AbortSignal,
};
use std::rc::Rc;
pub fn cancellable<T: Clone + 'static>(
    capability: &str,
    signal: Option<AbortSignal>,
    task: Promise<T>,
) -> Promise<T> {
    let Some(signal) = signal else {
        return task;
    };
    let (result, resolver) = Promise::pending();
    let abort = task.abort_handle();
    let capability = capability.to_string();
    let s = signal.clone();
    let r = resolver.clone();
    let c = capability.clone();
    let cancel: Rc<dyn Fn(&crate::Error)> = Rc::new(move |_| {
        if let Some(abort) = &abort {
            abort(s.reason_value());
        }
        r.reject(plugin_error(
            "operation-cancelled",
            &c,
            "operation cancelled",
            s.reason_value(),
            None,
        ));
    });
    let registration = if signal.aborted() {
        cancel(&signal.reason().unwrap());
        None
    } else {
        Some(signal.on_abort(cancel))
    };
    rt::spawn(async move {
        let answer = task.reaction().await;
        if let Some(id) = registration {
            signal.remove(id);
        }
        resolver.settle(answer.map_err(|e| to_plugin_error(&capability, &e)));
    });
    result
}
