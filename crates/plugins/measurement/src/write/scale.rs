//! `write/scale.ts`: scale writes. Every change serializes per page, loads
//! the page first, and re-measures the page's measurement annotations unless
//! told not to. A multi-page change reports per-page failures; a single-page
//! change rejects.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::json::truthy;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::Promise;
use epdf_kernel::serial_queue::SerialQueue;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, js};
use epdf_plugin_annotation::promise::all;
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::{MeasurementCapability, MeasurementContext};
use crate::model::{
    begin_scale_change, end_scale_change, set_calibration, set_local_viewports, set_reports,
};
use crate::scale::{default_measure, with_area_unit, with_precision, with_unit};
use crate::services::store::CAPABILITY;
use crate::units::{
    assert_writable_measure, measure_from_known_length, measure_from_ratio, measurement_point,
    serialize_error,
};

/// The scale a change writes to a page: `null` is back to the default.
type MeasureOf = Rc<dyn Fn(&MeasurementCapability, PageRef) -> Result<Value, Error>>;

/// `pageQueues`: one write queue per page, shared with the settle flush.
pub(crate) type PageQueues = Rc<RefCell<HashMap<u32, SerialQueue>>>;

/// `ctx.onSettle(...)`: a download waits for the scale changes on their
/// way (`Promise.all` over every page queue's `idle()`).
pub(crate) fn wire_settle(cx: &MeasurementContext) -> Result<PageQueues, Error> {
    let queues: PageQueues = Rc::default();
    let pending = queues.clone();
    cx.on_settle(Rc::new(move |_signal| {
        let idle: Vec<Promise<()>> = pending.borrow().values().map(SerialQueue::idle).collect();
        let flush = all(idle);
        Ok(Some(Promise::spawn(async move {
            flush.await?;
            Ok(())
        })))
    }))?;
    Ok(queues)
}

fn cancelled(signal: &AbortSignal, message: &str) -> Error {
    plugin_error(code::OPERATION_CANCELLED, CAPABILITY, message, signal.reason_value(), None)
}

impl MeasurementCapability {
    /// `queueOf(page)(operation, { signal })`: one write queue per page, so a
    /// page's scale changes land in order. An operation whose signal fired
    /// while it waited never starts.
    fn enqueue(
        &self,
        page: PageRef,
        signal: Option<AbortSignal>,
        operation: impl FnOnce() -> Promise<ScaleChangeReport> + 'static,
    ) -> Promise<ScaleChangeReport> {
        let core = self.core();
        let pon = page.page_object_number;
        let queue = core
            .queues
            .borrow_mut()
            .entry(pon)
            .or_insert_with(|| core.cx.serial_queue(&format!("scale:{pon}")))
            .clone();
        queue.run_with(move || Ok(operation()), signal)
    }

    /// `save(page, scale, options)`: write one page's scale (`null` = back to
    /// the default) and re-measure.
    fn save(
        &self,
        page: PageRef,
        scale: Value,
        options: ScaleChangeOptions,
    ) -> Promise<ScaleChangeReport> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            let signal = options.signal.clone();
            core.cx.assert_allowed(SCALE_PERMISSION, "changing a scale")?;
            this.ensure_page_loaded(PageInput::Ref(page), signal.clone()).await?;
            core.cx.assert_allowed(SCALE_PERMISSION, "changing a scale")?;
            let doc = core.cx.doc()?;
            let pon = page.page_object_number;
            if doc.has("page.measure") {
                let write = doc.call("page.measure.setScale", Some(pon), vec![scale.clone()]);
                cancellable(CAPABILITY, signal.clone(), write).await?;
                // The engine published `pages.scaleSet` before resolving, so
                // the mirror is already re-reading this page: join that read,
                // never start another.
                this.ensure_page_loaded(PageInput::Ref(page), None).await?;
            } else {
                // A session-only scale: no engine event announces it, so
                // re-read the page.
                let layout = core.registry.page_of(PageInput::Ref(page))?;
                let mut viewports: Vec<Value> = core
                    .cx
                    .state()
                    .local_viewports
                    .get(&pon)
                    .and_then(Value::as_array)
                    .map(|list| list.iter().filter(|v| !truthy(v.get("owned"))).cloned().collect())
                    .unwrap_or_default();
                if truthy(Some(&scale)) {
                    let mut bbox = serde_json::Map::new();
                    bbox.insert("x".into(), json!(0));
                    bbox.insert("y".into(), json!(0));
                    for (key, value) in
                        layout.row().get("size").and_then(Value::as_object).into_iter().flatten()
                    {
                        bbox.insert(key.clone(), value.clone());
                    }
                    viewports.push(json!({
                        "bbox": bbox,
                        "name": "EmbedPDF",
                        "owned": true,
                        "measure": scale,
                    }));
                }
                core.cx.update_state(|s| set_local_viewports(&s, pon, Value::Array(viewports)));
                this.refresh_page(page).await?;
            }
            let effective = if scale.is_null() || epdf_kernel::is_undefined(&scale) {
                let layout = core.registry.page_of(PageInput::Ref(page))?;
                let settings = core.settings.get();
                default_measure(
                    settings.get("defaultScale").unwrap_or(&Value::Null),
                    layout.user_unit().unwrap_or(1.0),
                )?
            } else {
                scale
            };
            let report = if options.recalculate == Some(false) {
                ScaleChangeReport::Recalibrated(json!({
                    "page": page,
                    "scale": effective,
                    "updated": [],
                    "skipped": [],
                    "failed": [],
                }))
            } else {
                let remeasure = core.annotation.remeasure_page(&page.to_json(), effective);
                ScaleChangeReport::Recalibrated(cancellable(CAPABILITY, signal, remeasure).await?)
            };
            core.events
                .scale_changed
                .emit(&MeasurementScaleChangedEvent { page, report: report.clone() });
            Ok(report)
        })
    }

    /// `change(pages, measure, options)`: every page is resolved first, then
    /// each page's write runs on its queue. One page: a failure rejects.
    /// Several: each page reports its own outcome, but a cancel ends the
    /// whole change.
    fn change(
        &self,
        pages: PageTarget,
        measure: MeasureOf,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            core.cx.assert_allowed(SCALE_PERMISSION, "changing a scale")?;
            let target = core.registry.targets(&pages)?;
            if let Some(signal) = options.signal.as_ref().filter(|s| s.aborted()) {
                return Err(cancelled(signal, "operation cancelled"));
            }
            let tolerant = pages == PageTarget::All || target.len() > 1;
            core.cx.update_state(|s| begin_scale_change(&s));
            let runs = target
                .iter()
                .map(|&page| {
                    let (writer, measure, options) =
                        (this.clone(), measure.clone(), options.clone());
                    this.enqueue(page, options.signal.clone(), move || {
                        Promise::spawn(async move {
                            let attempt = async {
                                writer
                                    .ensure_page_loaded(
                                        PageInput::Ref(page),
                                        options.signal.clone(),
                                    )
                                    .await?;
                                let scale = measure(&writer, page)?;
                                if truthy(Some(&scale)) {
                                    assert_writable_measure(&scale)?;
                                }
                                writer.save(page, scale, options).await
                            };
                            match attempt.await {
                                Ok(report) => Ok(report),
                                Err(error) => {
                                    let refused = to_plugin_error(CAPABILITY, &error);
                                    if !tolerant
                                        || refused.plugin_code() == Some(code::OPERATION_CANCELLED)
                                    {
                                        return Err(refused);
                                    }
                                    Ok(ScaleChangeReport::ScaleError {
                                        page,
                                        scale_error: serialize_error(&error),
                                    })
                                }
                            }
                        })
                    })
                })
                .collect();
            let settled = all(runs).await;
            let result = settled.map(|reports| {
                let reports = Rc::new(reports);
                core.cx.update_state(|s| set_reports(&s, reports.clone()));
                reports
            });
            core.cx.update_state(|s| end_scale_change(&s));
            result
        })
    }

    /// The page's current rectilinear scale, for unit and precision edits.
    fn rectilinear_of(&self, page: PageRef) -> Result<Value, Error> {
        let scale = self.scale_of(PageInput::Ref(page))?;
        if !truthy(Some(&scale.measure))
            || scale.measure.get("subtype").and_then(Value::as_str) != Some("rectilinear")
        {
            return Err(plugin_error(
                code::NOT_READY,
                CAPABILITY,
                "calibrate this page before changing its units or precision",
                None,
                None,
            ));
        }
        Ok(scale.measure.clone())
    }

    /// Write `measure` (any JSON; the preflight refuses what the engine
    /// could not write) to the pages.
    pub fn set_scale(
        &self,
        pages: impl Into<PageTarget>,
        measure: Value,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        self.change(pages.into(), Rc::new(move |_, _| Ok(measure.clone())), options)
    }

    /// Derive a scale from a known length between two page points and write
    /// it. `apply_to` widens the write (default: the input page). Fires
    /// `on_calibration_completed`.
    pub fn calibrate(
        &self,
        input: CalibrateInput,
        options: CalibrateOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        let this = self.clone();
        Promise::spawn(async move {
            let core = this.core();
            let start = measurement_point(input.from);
            let end = measurement_point(input.to);
            let length = epdf_geometry::js::hypot(end.x - start.x, end.y - start.y);
            let scale =
                measure_from_known_length(length, input.distance.value, &input.distance.unit)
                    .map_err(|error| {
                        plugin_error(code::INVALID_INPUT, CAPABILITY, error.message, None, None)
                    })?;
            let page = core.registry.page_of(input.page)?.page();
            let pages = options.apply_to.unwrap_or(PageTarget::Page(PageInput::Ref(page)));
            let reports =
                this.change(pages, Rc::new(move |_, _| Ok(scale.clone())), options.change).await?;
            if core.cx.state().calibration.as_ref().is_some_and(|request| request.page == page) {
                core.cx.update_state(|s| set_calibration(&s, None));
            }
            core.events.calibration_completed.emit(&CalibrationCompletedEvent { page });
            Ok(reports)
        })
    }

    /// Show the pages' distances in `unit` (areas in its square).
    pub fn set_unit(
        &self,
        pages: impl Into<PageTarget>,
        unit: &str,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        let unit = unit.to_string();
        self.change(
            pages.into(),
            Rc::new(move |this, page| with_unit(&this.rectilinear_of(page)?, &unit, None)),
            options,
        )
    }

    /// Show the pages' areas in `unit`.
    pub fn set_area_unit(
        &self,
        pages: impl Into<PageTarget>,
        unit: &str,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        let unit = unit.to_string();
        self.change(
            pages.into(),
            Rc::new(move |this, page| with_area_unit(&this.rectilinear_of(page)?, &unit)),
            options,
        )
    }

    /// Show the pages' values to `precision` (a power of ten).
    pub fn set_precision(
        &self,
        pages: impl Into<PageTarget>,
        precision: f64,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        self.change(
            pages.into(),
            Rc::new(move |this, page| with_precision(&this.rectilinear_of(page)?, precision)),
            options,
        )
    }

    /// A scale from the `presets` setting. Rejects `not-found` for an
    /// unknown preset.
    pub fn set_preset(
        &self,
        pages: impl Into<PageTarget>,
        preset_id: &str,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        let settings = self.core().settings.get();
        let preset = settings
            .get("presets")
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(preset_id))
            })
            .cloned();
        let Some(preset) = preset else {
            return Promise::rejected(plugin_error(
                code::NOT_FOUND,
                CAPABILITY,
                format!("unknown scale preset '{preset_id}'"),
                None,
                None,
            ));
        };
        let number = |key: &str| preset.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let (paper, real) = (number("paper"), number("real"));
        let unit = preset.get("unit").and_then(Value::as_str).unwrap_or("undefined").to_string();
        self.change(
            pages.into(),
            Rc::new(move |this, page| {
                let user_unit = this
                    .core()
                    .registry
                    .find(PageInput::Ref(page))
                    .map_or(1.0, |layout| layout.user_unit_or_one());
                measure_from_ratio(paper, real, &unit, user_unit)
            }),
            options,
        )
    }

    /// Back to the default scale (the owned viewport is removed).
    pub fn clear_scale(
        &self,
        pages: impl Into<PageTarget>,
        options: ScaleChangeOptions,
    ) -> Promise<Rc<Vec<ScaleChangeReport>>> {
        self.change(pages.into(), Rc::new(|_, _| Ok(Value::Null)), options)
    }
}

/// The reports as the TypeScript returns them.
pub fn reports_json(reports: &[ScaleChangeReport]) -> Value {
    Value::Array(reports.iter().map(js::to_value).collect())
}
