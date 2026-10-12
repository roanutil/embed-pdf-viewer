//! `sync/hydration.ts`: whole-document hydration. The plugin always
//! hydrates every page through `listRawAll()`. The snapshot arrives
//! cursor-stamped (`auditHead`); remote events arriving during the window
//! queue here and replay by cursor after ingest (events at or below the head
//! are already inside the snapshot and drop). `stream.desynced` re-runs the
//! same ingest with `bumpAp`.

use std::cell::{Cell, RefCell};

use epdf_annotation_core::{Id, Msg, PageObjectNumber, PageRef, Source};
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use serde_json::Value;

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::model::AnnotationAction;
use crate::promise::{self, await_engine};
use crate::value::get;

#[derive(Default)]
pub(crate) struct HydrationState {
    started: Cell<bool>,
    window: Cell<bool>,
    running: RefCell<Option<Promise<()>>>,
    again: Cell<bool>,
    pending: RefCell<Vec<Value>>,
}

/// JavaScript `a <= b` over two JSON values, as the replay compares a
/// `serverId` with the snapshot's `auditHead`.
fn js_le(a: &Value, b: &Value) -> bool {
    if let (Value::String(x), Value::String(y)) = (a, b) {
        return x.encode_utf16().cmp(y.encode_utf16()) != std::cmp::Ordering::Greater;
    }
    let n = |v: &Value| match v {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() { 0.0 } else { t.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    };
    n(a) <= n(b)
}

impl AnnotationCapability {
    fn set_hydration(&self, hydration: AnnotationHydration) {
        self.cx().dispatch(AnnotationAction::SetHydration { hydration });
    }

    fn ingest_snapshot(&self, snap: &Value, bump_ap: bool) -> Result<(), Error> {
        let mut annots = Vec::new();
        for page in get(snap, "pages").and_then(Value::as_array).into_iter().flatten() {
            let pon = get(page, "pageState")
                .and_then(|s| get(s, "page"))
                .and_then(|p| get(p, "pageObjectNumber"))
                .and_then(Value::as_f64)
                .map(|p| p as PageObjectNumber);
            let Some(crop) = pon.and_then(|p| self.crop_of(p)) else { continue };
            for d in get(page, "annotations").and_then(Value::as_array).into_iter().flatten() {
                annots.push(self.ingest(d, crop, Source::Baked)?);
            }
        }
        self.commit(Msg::Hydrated { annots, bump_ap: Some(bump_ap) });
        Ok(())
    }

    fn run_hydration(&self, bump_ap: bool) -> Promise<()> {
        let Some(doc) = self.doc() else { return Promise::resolved(()) };
        // No read grant: no doomed listRawAll, and a later rehydrate
        // re-checks.
        if !self.can_read() {
            self.set_hydration(AnnotationHydration::Forbidden);
            return Promise::resolved(());
        }
        self.0.hydration.window.set(true);
        self.set_hydration(AnnotationHydration::Loading);
        let call = doc.call("annotations.listRawAll", None, Vec::new());
        let this = self.clone();
        // `.then(ingest)`
        let ingested: Promise<()> = Promise::spawn(async move {
            let snap = call.await?;
            this.ingest_snapshot(&snap, bump_ap)?;
            let head = get(&snap, "auditHead").cloned();
            let queued = std::mem::take(&mut *this.0.hydration.pending.borrow_mut());
            this.0.hydration.window.set(false);
            for event in queued {
                let server_id = match event.get("origin") {
                    Some(origin) => {
                        origin.get("serverId").cloned().unwrap_or_else(epdf_kernel::undefined)
                    }
                    None => Value::Null,
                };
                if let Some(head) = &head
                    && !server_id.is_null()
                    && !epdf_kernel::is_undefined(&server_id)
                    && js_le(&server_id, head)
                {
                    continue;
                }
                this.apply_remote(&event)?;
            }
            this.set_hydration(AnnotationHydration::Complete);
            this.0.events.resynced.emit(&AnnotationResyncedEvent { pages: ResyncedPages::All });
            Ok(())
        });
        let this = self.clone();
        // `.catch(degrade)`: a replayed event that throws rejects it too.
        let caught: Promise<()> = Promise::spawn(async move {
            if let Err(error) = ingested.await {
                this.0.hydration.window.set(false);
                let queued = std::mem::take(&mut *this.0.hydration.pending.borrow_mut());
                for event in queued {
                    this.apply_remote(&event)?;
                }
                this.set_hydration(AnnotationHydration::Error(error));
            }
            Ok(())
        });
        let this = self.clone();
        // `.finally(chain)`: the callback one turn after, the result two
        // more (the resolved callback value is adopted).
        let run: Promise<()> = Promise::spawn(async move {
            let result = caught.await;
            *this.0.hydration.running.borrow_mut() = None;
            if this.0.hydration.again.get() {
                this.0.hydration.again.set(false);
                let again = this.run_hydration(true);
                *this.0.hydration.running.borrow_mut() = Some(again);
            }
            rt::tick().await;
            rt::tick().await;
            result
        });
        *self.0.hydration.running.borrow_mut() = Some(run.clone());
        run
    }

    /// `rehydrate()`: a desync during an in-flight hydration runs once more
    /// after it settles.
    pub(crate) fn rehydrate(&self) -> Promise<()> {
        let running = self.0.hydration.running.borrow().clone();
        if let Some(running) = running {
            self.0.hydration.again.set(true);
            return running;
        }
        let run = self.run_hydration(true);
        *self.0.hydration.running.borrow_mut() = Some(run.clone());
        run
    }

    /// `reloadPage(pon)`: replace one page's annotations from the engine.
    pub(crate) fn reload_page_pon(&self, pon: PageObjectNumber) -> Promise<()> {
        let this = self.clone();
        promise::spawn(async move {
            let (Some(doc), Some(crop)) = (this.doc(), this.crop_of(pon)) else { return Ok(()) };
            let run = async {
                doc.page_index(pon)?;
                let call = doc.call("page.annotations.list", Some(pon), Vec::new());
                let snap = await_engine(call).await?;
                let m = this.model();
                let stale: Vec<Id> = m
                    .order
                    .iter()
                    .filter(|id| m.by_id.get(*id).is_some_and(|a| a.page.page_object_number == pon))
                    .cloned()
                    .collect();
                if !stale.is_empty() {
                    this.commit(Msg::Remove { ids: stale });
                }
                let annots = get(&snap, "annotations")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|d| this.ingest(d, crop, Source::Baked))
                    .collect::<Result<Vec<_>, _>>()?;
                this.commit(Msg::Loaded { annots });
                this.0.events.resynced.emit(&AnnotationResyncedEvent {
                    pages: ResyncedPages::Pages(vec![PageRef::new(pon)]),
                });
                Ok::<(), epdf_kernel::Error>(())
            };
            let _ = run.await;
            Ok(())
        })
    }

    /// `getStatus()`: `ready` once hydration completed, else its status.
    pub fn get_status(&self) -> &'static str {
        match self.state().hydration {
            AnnotationHydration::Complete => "ready",
            AnnotationHydration::Loading => "loading",
            AnnotationHydration::Error(_) => "error",
            AnnotationHydration::Forbidden => "forbidden",
        }
    }

    /// `refresh()`: re-read every annotation from the engine.
    pub fn refresh(&self) -> Promise<()> {
        self.rehydrate()
    }

    /// `onResynced(listener)`.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&AnnotationResyncedEvent) -> Result<(), epdf_kernel::Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.resynced.on(listener)
    }

    /// `getHydration()` (host).
    pub fn get_hydration(&self) -> AnnotationHydration {
        self.state().hydration.clone()
    }

    /// `ensureHydrated()` (host): once per document.
    pub fn ensure_hydrated(&self) {
        if self.0.hydration.started.get() {
            return;
        }
        self.0.hydration.started.set(true);
        let _ = self.run_hydration(false);
    }

    /// `reloadPage(page)` (host).
    pub fn reload_page(&self, page: PageRef) -> Promise<()> {
        self.reload_page_pon(page.page_object_number)
    }

    /// `deliverRemoteEvent(event)` (host): queued during a hydration
    /// window, applied otherwise. Fails where applying throws: a record with
    /// no `ref`.
    pub fn deliver_remote_event(&self, event: Value) -> Result<(), Error> {
        if self.0.hydration.window.get() {
            self.0.hydration.pending.borrow_mut().push(event);
            return Ok(());
        }
        self.apply_remote(&event)
    }
}
