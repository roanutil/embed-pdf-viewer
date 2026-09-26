//! `sync/hydration.ts`: whole-form hydration. Every engine read of the field
//! tree lands here.

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};

use crate::contract::{FormResyncedEvent, ResourceStatus};
use crate::controller::FormCapability;
use crate::core::model::Msg;
use crate::promise::await_engine;

/// `readSnapshot()`.
async fn read_snapshot(this: FormCapability) -> Result<(), Error> {
    let Some(doc) = this.doc() else { return Ok(()) };
    // No read authority: don't fire a doomed list.
    if !this.can("doc.forms.read") {
        this.set_status(ResourceStatus::Forbidden);
        return Ok(());
    }
    if this.model().snapshot.is_none() {
        this.set_status(ResourceStatus::Loading);
    }
    match await_engine(doc.call("forms.list", None, Vec::new())).await {
        Ok(snapshot) => {
            this.apply(Msg::Snapshot { snapshot: snapshot.clone() });
            this.set_status(ResourceStatus::Ready);
            this.0.events.resynced.emit(&FormResyncedEvent { snapshot });
        }
        Err(error) => {
            // A race with an access change 403s here; anything else is a
            // real load failure. Either way surfaced, never a rejection.
            this.warn("[form] form snapshot failed to load", &error);
            this.set_status(ResourceStatus::Error);
        }
    }
    Ok(())
}

impl FormCapability {
    /// `refresh()`: re-read the field tree. An invalidation that lands while
    /// a read is in flight queues one more read after it, so the snapshot
    /// never settles stale; every call during a run shares its promise.
    pub fn refresh(&self) -> Promise<()> {
        if let Some(run) = self.0.refresh_run.borrow().clone() {
            self.0.refresh_again.set(true);
            return run;
        }
        let this = self.clone();
        let reads: Promise<()> = Promise::spawn(async move {
            loop {
                this.0.refresh_again.set(false);
                Promise::spawn(read_snapshot(this.clone())).await?;
                if !this.0.refresh_again.get() {
                    return Ok(());
                }
            }
        });
        // `.finally(() => { refreshRun = null })`: the callback one turn
        // after the reads settle, the derived promise two turns after that.
        let this = self.clone();
        let run: Promise<()> = Promise::spawn(async move {
            let result = reads.await;
            *this.0.refresh_run.borrow_mut() = None;
            rt::tick().await;
            rt::tick().await;
            result
        });
        *self.0.refresh_run.borrow_mut() = Some(run.clone());
        run
    }

    /// `getStatus()`.
    pub fn get_status(&self) -> ResourceStatus {
        self.cx().state().status
    }
}
