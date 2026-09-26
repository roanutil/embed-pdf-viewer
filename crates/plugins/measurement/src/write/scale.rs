//! `write/scale.ts`: scale writes. Every change serializes per page, loads
//! the page first, and re-measures the page's measurement annotations
//! unless told not to. A multi-page change reports per-page failures; a
//! single-page change rejects.
//!
//! The turns follow the TypeScript. `enqueue` is
//! `(previous ?? Promise.resolve()).catch(() => {}).then(action)`: one turn
//! for the `catch` reaction, one for the `then` reaction that calls the
//! action, and one for adopting the promise the action returns.

use std::rc::Rc;

use epdf_annotation_core::measure::PdfMeasurement;
use epdf_annotation_core::{PageObjectNumber, PageRef};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::rt::{self, Promise};
use epdf_plugin_annotation::RecalibrationReport;
use epdf_plugin_annotation::host_contract::serialize_error;
use epdf_plugin_annotation::promise::{all, await_engine};
use serde_json::Value;

use crate::contract::*;
use crate::controller::MeasurementCapability;
use crate::model::MeasurementAction;
use crate::scale::{default_measure, with_area_unit, with_precision, with_unit};
use crate::services::store::not_found;
use crate::units::{
    assert_writable_measure, decode_measure, measure_from_known_length, measure_from_ratio,
    measurement_point,
};

/// The scale a change writes to one page: a measure as JSON, or `None` for
/// the default. `Err` is a page the edit cannot apply to.
type MeasureOf = Rc<dyn Fn(&PageRef) -> Result<Option<Value>, Error>>;

fn to_json(measure: &PdfMeasurement) -> Value {
    epdf_kernel::js::to_value(measure)
}

impl MeasurementCapability {
    fn enqueue(
        &self,
        pon: PageObjectNumber,
        action: impl FnOnce() -> Promise<ScaleChangeReport> + 'static,
    ) -> Promise<ScaleChangeReport> {
        let previous = self.0.queues.borrow().get(&pon).cloned();
        let result = Promise::spawn(async move {
            match previous {
                Some(previous) => {
                    let _ = previous.await;
                }
                None => rt::tick().await,
            }
            rt::tick().await;
            let adopted = action();
            rt::tick().await;
            adopted.await
        });
        self.0.queues.borrow_mut().insert(pon, result.clone());
        let (queues, settled) = (self.0.queues.clone(), result.clone());
        rt::spawn(async move {
            let _ = settled.clone().await;
            let mut queues = queues.borrow_mut();
            if queues.get(&pon).is_some_and(|q| q.same(&settled)) {
                queues.remove(&pon);
            }
        });
        result
    }

    /// `save(page, scale, opts)`: write one page's scale (`None` = back to
    /// the default) and re-measure.
    fn save(
        &self,
        page: PageRef,
        scale: Option<Value>,
        opts: ScaleChangeOptions,
    ) -> Promise<ScaleChangeReport> {
        let this = self.clone();
        Promise::spawn(async move {
            let s = this.s();
            s.assert_allowed()?;
            this.ensure_loaded(&page)?.await?;
            s.assert_allowed()?;
            let doc = s.live()?;
            let pon = page.page_object_number;
            doc.page_index(pon)?;
            if doc.has("page.measure") {
                let arg = scale.clone().unwrap_or(Value::Null);
                await_engine(doc.call("page.measure.setScale", Some(pon), vec![arg])).await?;
                s.live()?;
                this.refresh(page)?.await?;
            } else {
                let layout = s.require_meta(&page)?;
                let old =
                    s.state().pages.get(&pon).map(|p| p.viewports.clone()).unwrap_or_default();
                let mut next: Vec<MeasurementViewport> =
                    old.into_iter().filter(|v| !v.owned).collect();
                if let Some(scale) = &scale {
                    next.push(MeasurementViewport {
                        bbox: layout.crop,
                        name: Some("EmbedPDF".into()),
                        owned: true,
                        measure: Some(decode_measure(scale)?),
                    });
                }
                this.publish(page, next)?;
            }
            s.live()?;
            let effective = match scale {
                Some(scale) => scale,
                None => to_json(&default_measure(&s.config, s.require_meta(&page)?.user_unit)?),
            };
            let report = if opts.recalculate == Some(false) {
                RecalibrationReport {
                    page,
                    scale: effective,
                    error: None,
                    updated: Vec::new(),
                    skipped: Vec::new(),
                    failed: Vec::new(),
                }
            } else {
                s.annotation.remeasure_page(page, effective).await?
            };
            let report = ScaleChangeReport::Recalibrated(report);
            s.live()?;
            s.events
                .scale_changed
                .emit(&MeasurementScaleChangedEvent { page, report: report.clone() });
            Ok(report)
        })
    }

    fn change(
        &self,
        pages: PageTarget,
        measure: MeasureOf,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let this = self.clone();
        Promise::spawn(async move {
            let s = this.s();
            s.assert_allowed()?;
            let target = s.targets(&pages);
            // One page: a failure rejects. Several: each page reports its own
            // outcome.
            let tolerant = pages == PageTarget::All || target.len() > 1;
            s.cx.dispatch(MeasurementAction::Pending { delta: 1.0 });
            let result = async {
                let queued = target
                    .iter()
                    .map(|&page| {
                        let (worker, measure, opts) = (this.clone(), measure.clone(), opts.clone());
                        this.enqueue(page.page_object_number, move || {
                            Promise::spawn(async move {
                                let attempt = async {
                                    worker.ensure_loaded(&page)?.await?;
                                    let scale = measure(&page)?.filter(|s| !s.is_null());
                                    if let Some(scale) = &scale {
                                        assert_writable_measure(scale)?;
                                    }
                                    worker.save(page, scale, opts).await
                                }
                                .await;
                                match attempt {
                                    Ok(report) => Ok(report),
                                    Err(error) if !tolerant => {
                                        Err(to_plugin_error("measurement", &error))
                                    }
                                    Err(error) => Ok(ScaleChangeReport::ScaleError {
                                        page,
                                        scale_error: serialize_error(&error),
                                    }),
                                }
                            })
                        })
                    })
                    .collect();
                let reports = all(queued).await?;
                s.live()?;
                s.cx.dispatch(MeasurementAction::Reports { reports: reports.clone() });
                Ok(reports)
            }
            .await;
            if !s.is_disposed() {
                s.cx.dispatch(MeasurementAction::Pending { delta: -1.0 });
            }
            result
        })
    }

    /// The page's current rectilinear scale, for unit and precision edits.
    fn rectilinear_of(&self, page: &PageRef) -> Result<PdfMeasurement, Error> {
        let scale = self
            .s()
            .state()
            .pages
            .get(&page.page_object_number)
            .and_then(|p| p.scale.measure.clone());
        match scale {
            Some(scale) if scale.subtype == "RL" => Ok(scale),
            _ => Err(plugin_error(
                code::NOT_READY,
                "measurement",
                "calibrate this page before changing its units or precision",
                None,
                None,
            )),
        }
    }

    /// Write `measure` to `pages`. The measure is JSON because it is
    /// validated as the TypeScript validates an untyped object.
    pub fn set_scale(
        &self,
        pages: PageTarget,
        measure: Value,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        self.change(pages, Rc::new(move |_| Ok(Some(measure.clone()))), opts)
    }

    /// Derive a scale from a known length between two page points.
    /// `apply_to` widens the write (default: the input page).
    pub fn calibrate(
        &self,
        input: CalibrateInput,
        opts: CalibrateOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let this = self.clone();
        Promise::spawn(async move {
            let s = this.s();
            let a = measurement_point(s.to_pdf(&input.page, input.from)?);
            let b = measurement_point(s.to_pdf(&input.page, input.to)?);
            let length = epdf_geometry::js::hypot(b.x - a.x, b.y - a.y);
            let scale =
                measure_from_known_length(length, input.distance.value, &input.distance.unit)
                    .map_err(|e| {
                        plugin_error(code::INVALID_INPUT, "measurement", e.message, None, None)
                    })?;
            let target = opts.apply_to.unwrap_or(PageTarget::Page(input.page));
            let rest = ScaleChangeOptions { recalculate: opts.recalculate };
            let reports =
                this.change(target, Rc::new(move |_| Ok(Some(scale.clone()))), rest).await?;
            if s.state().calibration.as_ref().is_some_and(|r| r.page.same_page(&input.page)) {
                s.cx.dispatch(MeasurementAction::Calibration { request: None });
            }
            s.events.calibration_completed.emit(&CalibrationCompletedEvent { page: input.page });
            Ok(reports)
        })
    }

    pub fn set_unit(
        &self,
        pages: PageTarget,
        unit: &str,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let (this, unit) = (self.clone(), unit.to_string());
        self.change(
            pages,
            Rc::new(move |p| Ok(Some(to_json(&with_unit(&this.rectilinear_of(p)?, &unit, None)?)))),
            opts,
        )
    }

    pub fn set_area_unit(
        &self,
        pages: PageTarget,
        unit: &str,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let (this, unit) = (self.clone(), unit.to_string());
        self.change(
            pages,
            Rc::new(move |p| Ok(Some(to_json(&with_area_unit(&this.rectilinear_of(p)?, &unit)?)))),
            opts,
        )
    }

    pub fn set_precision(
        &self,
        pages: PageTarget,
        precision: f64,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let this = self.clone();
        self.change(
            pages,
            Rc::new(move |p| {
                Ok(Some(to_json(&with_precision(&this.rectilinear_of(p)?, precision)?)))
            }),
            opts,
        )
    }

    pub fn set_preset(
        &self,
        pages: PageTarget,
        preset_id: &str,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        let Some(preset) = self.0.presets.iter().find(|p| p.id == preset_id).cloned() else {
            return Promise::rejected(not_found(&format!("unknown scale preset '{preset_id}'")));
        };
        let this = self.clone();
        self.change(
            pages,
            Rc::new(move |p| {
                let user_unit = this.s().meta(p).map_or(1.0, |m| m.user_unit);
                measure_from_ratio(preset.paper, preset.real, &preset.unit, user_unit).map(Some)
            }),
            opts,
        )
    }

    /// Back to the default scale (the owned viewport is removed).
    pub fn clear_scale(
        &self,
        pages: PageTarget,
        opts: ScaleChangeOptions,
    ) -> Promise<Vec<ScaleChangeReport>> {
        self.change(pages, Rc::new(|_| Ok(None)), opts)
    }
}
