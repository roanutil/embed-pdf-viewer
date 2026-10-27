//! Flush deferred document work before reading its file.
use crate::{Error, Promise, error::plugin_error, signal::AbortSignal, store::Report};
use std::rc::Rc;

pub type SettleFlush = Rc<dyn Fn(AbortSignal) -> Result<Option<Promise<()>>, Error>>;
/// What a wrap runs inside itself: a read of the file, or a print.
pub type WrappedRun<T> = Rc<dyn Fn() -> Promise<T>>;
/// Something of the document's own that runs around a run: it gets the run
/// and returns its promise (`DownloadWrap`, `PrintWrap` in `context.ts`).
pub type Wrap<T> = Rc<dyn Fn(WrappedRun<T>) -> Result<Promise<T>, Error>>;
pub type DownloadRead = WrappedRun<serde_json::Value>;
pub type DownloadWrap = Wrap<serde_json::Value>;
/// `PrintWrap`: what runs around each print of the document.
pub type PrintRun = WrappedRun<()>;
pub type PrintWrap = Wrap<()>;

/// Flush failures are reported after all flushes finish. Caller or document
/// cancellation stops waiting immediately; flushes receive the combined signal.
///
/// `stop.abort()` takes no reason in `settle.ts`, so the flushes' signal
/// carries the platform's: a `DOMException` named `AbortError`.
pub fn settle(flushes: Vec<SettleFlush>, signals: Vec<AbortSignal>, report: Report) -> Promise<()> {
    let stop = AbortSignal::new();
    let mut listeners = Vec::new();
    for signal in signals {
        if signal.aborted() {
            stop.abort(Error::dom_abort());
        } else {
            let abort = stop.clone();
            let id = signal.on_abort(Rc::new(move |_| abort.abort(Error::dom_abort())));
            listeners.push((signal, id));
        }
    }
    let running: Vec<_> = flushes
        .into_iter()
        .map(|flush| match flush(stop.clone()) {
            Ok(Some(p)) => p,
            Ok(None) => Promise::resolved(()),
            Err(e) => Promise::rejected(e),
        })
        .collect();
    let work = Promise::spawn(async move {
        let mut errors = Vec::new();
        for pending in running {
            if let Err(error) = pending.await {
                errors.push(error);
            }
        }
        for error in errors {
            report(&error);
        }
        Ok(())
    });
    let (aborted, reject) = Promise::pending();
    let abort: Rc<dyn Fn(&Error)> = Rc::new(move |_| {
        reject.reject(plugin_error(
            "operation-cancelled",
            "documents",
            "settling was cancelled",
            None,
            None,
        ))
    });
    let registration = if stop.aborted() {
        abort(&stop.reason().unwrap());
        None
    } else {
        Some(stop.on_abort(abort))
    };
    let race = Promise::race(vec![work, aborted]);
    Promise::spawn(async move {
        let result = race.await;
        for (signal, id) in listeners {
            signal.remove(id);
        }
        if let Some(id) = registration {
            stop.remove(id);
        }
        result
    })
}

/// `runWrapped`: first registered wraps outermost, as in `context.ts`.
pub fn run_wrapped<T: Clone + 'static>(wraps: Vec<Wrap<T>>, read: WrappedRun<T>) -> Promise<T> {
    let mut run = read;
    for wrap in wraps.into_iter().rev() {
        let next = run;
        run = Rc::new(move || match wrap(next.clone()) {
            Ok(p) => p,
            Err(e) => Promise::rejected(e),
        });
    }
    run()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::rt;

    /// `stop.abort()` with no reason (`settle.ts:28`): a flush's signal
    /// reads the platform's `AbortError`, whether the caller's signal fired
    /// before the settle or during it.
    #[test]
    fn a_flush_sees_the_platform_abort_reason() {
        for before in [true, false] {
            rt::reset();
            let seen: Rc<RefCell<Option<AbortSignal>>> = Rc::default();
            let keep = seen.clone();
            let (never, _hold) = Promise::<()>::pending();
            let flush: SettleFlush = Rc::new(move |signal| {
                *keep.borrow_mut() = Some(signal);
                Ok(Some(never.clone()))
            });
            let caller = AbortSignal::new();
            if before {
                caller.abort(Error::new("caller"));
            }
            let settled = settle(vec![flush], vec![caller.clone()], Rc::new(|_| {}));
            if !before {
                caller.abort(Error::new("caller"));
            }
            rt::run_until_stalled();
            let signal = seen.borrow().clone().expect("the flush ran");
            let reason = signal.reason().expect("aborted");
            assert_eq!(
                (reason.name.as_str(), reason.message.as_str()),
                ("AbortError", "This operation was aborted")
            );
            assert_eq!(settled.peek().unwrap().unwrap_err().message, "settling was cancelled");
        }
    }
}
