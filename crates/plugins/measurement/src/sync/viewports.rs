//! `sync/viewports.ts`: the page's viewports into the slice. Refresh epochs
//! keep an old read from replacing a newer scale. `connect` watches the
//! document for viewport changes, version moves and inserted pages.
//!
//! A refresh without the engine's measure service runs its whole `async`
//! body at call time, so its `finally` drops the read before the caller
//! records it, and the settled read stays recorded: a later `ensureLoaded`
//! joins it. The port keeps that order.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::PageRef;
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_plugin_annotation::host_contract::serialize_error;
use epdf_plugin_annotation::promise::await_engine;
use epdf_plugin_annotation::value::str_of;
use serde_json::Value;

use crate::contract::*;
use crate::controller::MeasurementCapability;
use crate::model::MeasurementAction;
use crate::scale::{default_measure, select_page_scale};
use crate::services::store::not_found;

/// `!!doc.page(page).measure`: `page` throws for a page the engine does not
/// know.
fn has_measure(
    doc: &dyn epdf_kernel::engine::DocumentHandle,
    page: &PageRef,
) -> Result<bool, Error> {
    doc.page_index(page.page_object_number)?;
    Ok(doc.has("page.measure"))
}

impl MeasurementCapability {
    /// `publish(page, viewports)`: hand the viewports to the annotation
    /// plugin and store them with the scale they select.
    pub(crate) fn publish(
        &self,
        page: PageRef,
        viewports: Vec<MeasurementViewport>,
    ) -> Result<(), Error> {
        let s = self.s();
        if s.is_disposed() {
            return Ok(());
        }
        let Some(layout) = s.meta(&page) else { return Ok(()) };
        let fallback = default_measure(&s.config, layout.user_unit)?;
        s.annotation.set_page_viewports(
            page,
            Some(viewports.iter().map(MeasurementViewport::for_annotation).collect()),
            Some(fallback.clone()),
        );
        let persistent = match s.cx.doc() {
            Ok(doc) => has_measure(&*doc, &page)?,
            Err(_) => false,
        };
        let scale = select_page_scale(&viewports, layout.crop, &fallback, persistent);
        s.cx.dispatch(MeasurementAction::PageScale { page, viewports, scale });
        Ok(())
    }

    /// `refresh(page)`: read the page's viewports. `Err` is a synchronous
    /// throw (the plugin is disposed, the engine does not know the page).
    pub(crate) fn refresh(&self, page: PageRef) -> Result<Promise<()>, Error> {
        let s = self.s();
        let doc = s.live()?;
        let Some(layout) = s.meta(&page) else {
            return Ok(Promise::rejected(not_found("no such page")));
        };
        let pon = page.page_object_number;
        let epoch = {
            let mut epochs = self.0.epochs.borrow_mut();
            let next = epochs.get(&pon).copied().unwrap_or(0) + 1;
            epochs.insert(pon, next);
            next
        };
        // Disable creation until calibration is known; never race a default
        // into an imported viewport.
        s.annotation.set_page_viewports(
            page,
            None,
            Some(default_measure(&s.config, layout.user_unit)?),
        );
        let previous = s.state().pages.get(&pon).cloned();
        let scale = match &previous {
            Some(entry) => PageScale { ready: false, ..entry.scale.clone() },
            None => PageScale {
                measure: None,
                source: ScaleSource::Default,
                persistent: has_measure(&*doc, &page)?,
                ready: false,
                error: None,
            },
        };
        s.cx.dispatch(MeasurementAction::PageScale {
            page,
            viewports: previous.map(|p| p.viewports).unwrap_or_default(),
            scale,
        });
        let service = has_measure(&*doc, &page)?;
        let current = {
            let epochs = self.0.epochs.clone();
            move || epochs.borrow().get(&pon) == Some(&epoch)
        };
        let this = self.clone();
        // The `catch` and `finally` of the TypeScript's async body.
        let settle = move |read: Result<Vec<MeasurementViewport>, Error>| -> Result<(), Error> {
            let out = read.and_then(|viewports| {
                if !this.s().is_disposed() && current() {
                    this.publish(page, viewports)
                } else {
                    Ok(())
                }
            });
            let out = out.inspect_err(|error| {
                if !this.s().is_disposed() && current() {
                    this.s().cx.dispatch(MeasurementAction::PageScale {
                        page,
                        viewports: Vec::new(),
                        scale: PageScale {
                            measure: None,
                            source: ScaleSource::Default,
                            ready: false,
                            persistent: service,
                            error: Some(serialize_error(error)),
                        },
                    });
                }
            });
            if current() {
                this.0.reading.borrow_mut().remove(&pon);
            }
            out
        };
        let request = if service {
            let call = doc.call("page.measure.viewports", Some(pon), Vec::new());
            Promise::spawn(async move {
                let read = await_engine(call).await.and_then(|v| {
                    serde_json::from_value::<Vec<MeasurementViewport>>(v)
                        .map_err(|e| Error::new(format!("[measurement] malformed viewports: {e}")))
                });
                settle(read)
            })
        } else {
            let viewports =
                s.state().pages.get(&pon).map(|p| p.viewports.clone()).unwrap_or_default();
            match settle(Ok(viewports)) {
                Ok(()) => Promise::resolved(()),
                Err(error) => Promise::rejected(error),
            }
        };
        self.0.reading.borrow_mut().insert(pon, request.clone());
        Ok(request)
    }

    /// Load the page's viewports: the read in flight, or at once when the
    /// page is ready. `Err` is a synchronous throw from the read.
    pub fn ensure_loaded(&self, page: &PageRef) -> Result<Promise<()>, Error> {
        let pon = page.page_object_number;
        if let Some(reading) = self.0.reading.borrow().get(&pon).cloned() {
            return Ok(reading);
        }
        if self.s().state().pages.get(&pon).is_some_and(|p| p.scale.ready) {
            return Ok(Promise::resolved(()));
        }
        self.refresh(*page)
    }

    /// `connect()` of the viewport sync.
    pub(crate) fn connect_viewports(&self) -> Result<(), Error> {
        let s = self.s();
        let Ok(doc) = s.cx.doc() else { return Ok(()) };
        let this = self.clone();
        let off = doc.subscribe(Rc::new(move |event: &Value| {
            // A synchronous throw from a refresh has nowhere to go from an
            // engine listener; the TypeScript's would escape the engine's
            // emit. None of the vectors throws there.
            match str_of(event, "type") {
                Some("page.viewportsChanged") => {
                    if let Some(page) = event
                        .get("page")
                        .and_then(|p| serde_json::from_value::<PageRef>(p.clone()).ok())
                    {
                        let _ = this.refresh(page);
                    }
                }
                Some("stream.desynced" | "document.versioned") => {
                    for page in this.s().page_refs() {
                        let _ = this.refresh(page);
                    }
                }
                _ => {}
            }
        }));
        s.cx.defer(Box::new(move || {
            off.call();
            Ok(None)
        }));
        // Covers pages inserted after this document-scoped instance was
        // created.
        let pages = Rc::new(RefCell::new(String::new()));
        let this = self.clone();
        let reconcile = Rc::new(move || -> Result<(), Error> {
            let refs = this.s().page_refs();
            let next =
                refs.iter().map(|p| p.page_object_number.to_string()).collect::<Vec<_>>().join(",");
            if *pages.borrow() == next {
                return Ok(());
            }
            *pages.borrow_mut() = next;
            for page in refs {
                if !this.s().state().pages.contains_key(&page.page_object_number) {
                    this.ensure_loaded(&page)?;
                }
            }
            Ok(())
        });
        let listener = reconcile.clone();
        let off = s.cx.subscribe(Rc::new(move || listener()));
        s.cx.defer(Box::new(move || {
            off.call();
            Ok(None)
        }));
        reconcile()
    }
}
