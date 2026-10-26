//! `sync/viewports.ts`: the pages' viewports, a page mirror re-read
//! whenever a confirmed `pages.scaleSet` names a loaded page, from this
//! session or another (stream gaps and version moves re-read every loaded
//! page). Its `changed` callback tells the annotation plugin a page's
//! viewports, and a new `defaultScale` setting tells it again. `connect`
//! loads every page in the registry, including pages inserted later.

use std::rc::Rc;

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::mirror::ResourceStatus;
use epdf_kernel::page_mirror::{Affected, PageMirror, PageMirrorChange, PageMirrorSpec};
use epdf_kernel::rt::Promise;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_annotation::AnnotationCapability;
use serde_json::{Value, json};

use crate::contract::{OperationOptions, PageInput, PageScale, no_page_scale};
use crate::controller::{MeasurementCapability, MeasurementContext};
use crate::model::{PageScaleInputs, clear_load_error, page_scale_of, record_load_error};
use crate::scale::default_measure;
use crate::services::Registry;
use crate::services::store::{CAPABILITY, PageRow};
use crate::units::serialize_error;

/// The inputs a page's public scale was projected from, and the scale:
/// `memoByKey`, so a page's scale is the same `Rc` until its viewports, its
/// read state, its layout or the default scale change.
pub(crate) struct ScaleMemo {
    viewports: Option<Rc<Value>>,
    status: ResourceStatus,
    error: Option<Rc<Value>>,
    /// The registry the page's layout row came from. A registry is replaced
    /// whole, every row new (`kernel.ts:1178-1186` takes `layout.pages` from
    /// the event), so this is the TypeScript's key on the layout object.
    pages: Option<Rc<Value>>,
    persistent: bool,
    settings: Rc<Value>,
    scale: Rc<PageScale>,
}

fn same(a: &Option<Rc<Value>>, b: &Option<Rc<Value>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// `fallbackOf(layout)`: the `defaultScale` setting, in the page's user
/// unit.
fn fallback_of(settings: &Value, layout: Option<&PageRow>) -> Result<Value, Error> {
    let user_unit = layout.map_or(1.0, PageRow::user_unit_or_one);
    default_measure(settings.get("defaultScale").unwrap_or(&Value::Null), user_unit)
}

/// The engine can persist a scale into the document (`doc.page(page).measure
/// !== undefined`). `Err` where reading `ctx.doc` throws: the registry still
/// names the page after the handle is gone.
fn is_persistent(cx: &MeasurementContext) -> Result<bool, Error> {
    Ok(cx.doc()?.has("page.measure"))
}

/// A mirrored value that is the TypeScript's `undefined`: a read whose
/// answer had no `viewports`.
fn absent(value: &Value) -> bool {
    epdf_kernel::is_undefined(value)
}

/// The page's viewports as the TypeScript spreads them (`[...next]`).
fn viewport_list(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

/// `ctx.pageMirror({ name: 'viewports', load, affected, changed })`.
pub(crate) fn create_viewport_mirror(
    cx: &MeasurementContext,
    registry: &Rc<Registry>,
    settings: &InstanceSettings,
    annotation: &AnnotationCapability,
) -> Result<PageMirror, Error> {
    let reader = cx.clone();
    let pages = registry.clone();
    let load = Rc::new(
        move |doc: Option<Rc<dyn epdf_kernel::engine::DocumentHandle>>,
              page: Value,
              signal: AbortSignal| {
            let cx = reader.clone();
            let registry = pages.clone();
            Promise::spawn(async move {
                let Some(page) = PageRef::from_json(&page) else {
                    return Err(plugin_error(
                        code::NOT_FOUND,
                        CAPABILITY,
                        "page undefined is not in this document",
                        None,
                        None,
                    ));
                };
                registry.assert_page_ref(page)?;
                let pon = page.page_object_number;
                let doc = doc.ok_or_else(|| {
                    plugin_error(code::NOT_READY, CAPABILITY, "no document", None, None)
                })?;
                // Without a page measure service, scales live for the session
                // only.
                if !doc.has("page.measure") {
                    return Ok(cx.state().local_viewports.get(&pon).cloned().unwrap_or(json!([])));
                }
                match doc.call("page.measure.listViewports", Some(pon), Vec::new()).await {
                    // An answer without `viewports` is `undefined` there:
                    // the page stays not ready and `changed` skips it.
                    Ok(list) => {
                        Ok(list.get("viewports").cloned().unwrap_or_else(epdf_kernel::undefined))
                    }
                    Err(error) => {
                        // A read the closing document cut short is no page's
                        // error, and there's no state left.
                        if !signal.aborted() {
                            let serialized = serialize_error(&error);
                            cx.update_state(|s| record_load_error(&s, pon, serialized));
                        }
                        Err(error)
                    }
                }
            })
        },
    );
    let writer = cx.clone();
    let rows = registry.clone();
    let store = settings.clone();
    let annotation = annotation.clone();
    let changed = Rc::new(move |change: PageMirrorChange| {
        let Some(next) = change.next.filter(|next| change.cause != "drop" && !absent(next)) else {
            return Ok(());
        };
        let Some(page) = PageRef::from_json(&change.page) else { return Ok(()) };
        writer.update_state(|s| clear_load_error(&s, page.page_object_number));
        if let Some(layout) = rows.find(PageInput::Ref(page)) {
            let fallback = fallback_of(&store.get(), Some(&layout))?;
            annotation.set_page_viewports(&change.page, Some(viewport_list(&next)), fallback);
        }
        Ok(())
    });
    cx.page_mirror(PageMirrorSpec {
        name: "viewports".into(),
        load,
        affected: Rc::new(|event| {
            if event.get("type").and_then(Value::as_str) == Some("pages.scaleSet") {
                Affected::Pages(vec![event.get("page").cloned().unwrap_or(Value::Null)])
            } else {
                Affected::None
            }
        }),
        fold: None,
        predict: None,
        changed: Some(changed),
    })
}

impl MeasurementCapability {
    /// `scaleOfPage(pageObjectNumber)`: the page's public scale, the same
    /// `Rc` until its viewports, read state or layout change.
    fn scale_of_page(&self, page: PageRef) -> Result<Rc<PageScale>, Error> {
        let core = self.core();
        let pon = page.page_object_number;
        let key = page.to_json();
        let viewports = core.viewports.get(&key).filter(|v| !absent(v));
        let status = core.viewports.get_status(&key);
        let error = core.cx.state().load_errors.get(&pon).cloned();
        let layout = core.registry.find(PageInput::Ref(page));
        let pages = layout.as_ref().and_then(|_| core.registry.pages());
        let persistent = match layout {
            Some(_) => is_persistent(&core.cx)?,
            None => false,
        };
        let settings = core.settings.get();
        if let Some(memo) = core.scales.borrow().get(&pon)
            && same(&memo.viewports, &viewports)
            && memo.status == status
            && same(&memo.error, &error)
            && same(&memo.pages, &pages)
            && memo.persistent == persistent
            && Rc::ptr_eq(&memo.settings, &settings)
        {
            return Ok(memo.scale.clone());
        }
        let fallback = fallback_of(&settings, layout.as_ref())?;
        let scale = Rc::new(page_scale_of(&PageScaleInputs {
            viewports: viewports.as_deref(),
            status,
            error: error.as_deref(),
            size: layout.as_ref().map(PageRow::size),
            fallback: &fallback,
            persistent,
        }));
        core.scales.borrow_mut().insert(
            pon,
            ScaleMemo {
                viewports,
                status,
                error,
                pages,
                persistent,
                settings,
                scale: scale.clone(),
            },
        );
        Ok(scale)
    }

    /// `scaleOf(page)`: a page's scale, by ref or index; a page that isn't
    /// there has none, and isn't ready. `Err` where the TypeScript throws:
    /// a `defaultScale` that cannot be built in the page's user unit.
    pub(crate) fn scale_of(&self, page: PageInput) -> Result<Rc<PageScale>, Error> {
        match page {
            PageInput::Ref(page) => self.scale_of_page(page),
            PageInput::Index(_) => match self.core().registry.find(page) {
                Some(layout) => self.scale_of_page(layout.page()),
                None => Ok(Rc::new(no_page_scale())),
            },
        }
    }

    /// `ensureLoaded(page, { signal })` of the viewport sync: rejects
    /// `not-found` for a page that isn't there.
    pub(crate) fn ensure_page_loaded(
        &self,
        page: PageInput,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let Some(layout) = self.core().registry.find(page) else {
            return Promise::rejected(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                "no such page",
                None,
                None,
            ));
        };
        cancellable(
            CAPABILITY,
            signal,
            self.core().viewports.ensure_loaded(layout.page().to_json()),
        )
    }

    /// Load the page's viewports. Resolves at once when already known;
    /// rejects `not-found` for a page that isn't there, and
    /// `operation-cancelled` when the signal fires.
    pub fn ensure_loaded(
        &self,
        page: impl Into<PageInput>,
        options: OperationOptions,
    ) -> Promise<()> {
        self.ensure_page_loaded(page.into(), options.signal)
    }

    /// `refresh(page)`: re-read one page, for session-only scales, which no
    /// engine event announces.
    pub(crate) fn refresh_page(&self, page: PageRef) -> Promise<()> {
        self.core().viewports.refresh(Some(page.to_json()))
    }

    /// `connect()` of the viewport sync: load every page in the registry,
    /// now and as pages are inserted, and tell the annotation plugin a new
    /// default scale.
    pub(crate) fn connect_viewports(&self) {
        let core = self.core();
        let hydrate = {
            let mirror = core.viewports.clone();
            move |pages: Option<Rc<Value>>| {
                for row in pages.as_deref().and_then(Value::as_array).into_iter().flatten() {
                    let Some(page) = row.get("ref").and_then(PageRef::from_json) else { continue };
                    let key = page.to_json();
                    if mirror.get_status(&key) != ResourceStatus::Idle {
                        continue;
                    }
                    // Reported through `getPageScale(page).error`.
                    mirror.ensure_loaded(key).ignore();
                }
            }
        };
        hydrate(core.registry.pages());
        // `ctx.watch(() => ctx.document()?.pages, hydrate)`.
        let registry = core.registry.clone();
        let previous = std::cell::RefCell::new(registry.pages());
        let watch = core.cx.subscribe(Rc::new(move || {
            let next = registry.pages();
            if same(&previous.borrow(), &next) {
                return Ok(());
            }
            *previous.borrow_mut() = next.clone();
            hydrate(next);
            Ok(())
        }));
        core.cx.listen(watch);
        // A new default scale reaches the pages without one: the annotation
        // plugin measures new drawings on them with it.
        let this = std::rc::Rc::downgrade(&self.0);
        let off: Unsubscribe = core.settings.events.on(move |event| {
            if !event.changed.iter().any(|key| key == "defaultScale") {
                return Ok(());
            }
            let Some(core) = this.upgrade() else { return Ok(()) };
            for page in core.registry.refs() {
                let key = page.to_json();
                let Some(known) = core.viewports.get(&key).filter(|v| !absent(v)) else {
                    continue;
                };
                let layout = core.registry.find(PageInput::Ref(page));
                let fallback = fallback_of(&event.settings, layout.as_ref())?;
                core.annotation.set_page_viewports(&key, Some(viewport_list(&known)), fallback);
            }
            Ok(())
        });
        core.cx.listen(off);
    }
}
