//! Flush deferred document work before reading its file.
use crate::{Error, Promise, error::plugin_error, signal::AbortSignal, store::Report};
use std::rc::Rc;

pub type SettleFlush = Rc<dyn Fn(AbortSignal) -> Result<Option<Promise<()>>, Error>>;
pub type DownloadRead = Rc<dyn Fn() -> Promise<serde_json::Value>>;
pub type DownloadWrap = Rc<dyn Fn(DownloadRead) -> Result<Promise<serde_json::Value>, Error>>;

/// Flush failures are reported after all flushes finish. Caller or document
/// cancellation stops waiting immediately; flushes receive the combined signal.
pub fn settle(flushes: Vec<SettleFlush>, signals: Vec<AbortSignal>, report: Report) -> Promise<()> {
    let stop = AbortSignal::new();
    let mut listeners = Vec::new();
    for signal in signals {
        if signal.aborted() {
            stop.abort(Error::new("settling stopped"));
        } else {
            let abort = stop.clone();
            let id = signal.on_abort(Rc::new(move |_| abort.abort(Error::new("settling stopped"))));
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

/// First registered wraps outermost, as in `context.ts`.
pub fn run_wrapped(wraps: Vec<DownloadWrap>, read: DownloadRead) -> Promise<serde_json::Value> {
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
