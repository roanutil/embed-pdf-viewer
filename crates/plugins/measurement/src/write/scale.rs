//! `write/scale.ts`: scale writes. A scale change is one change of the
//! document: the pages' `pages.setScale` ops and, unless told not to, the
//! re-measure of the measurements on them, staged in one group, so it is one
//! request and one undo step. A multi-page change reports per page; a
//! single-page change rejects.

use std::collections::HashMap;
use std::rc::Rc;

use epdf_annotation_core::json::truthy;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::changes::StagedChange;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::page_predict::{scale_back_of, viewports_with_scale};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, js};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::MeasurementCapability;
use crate::model::{
    begin_scale_change, end_scale_change, set_calibration, set_local_viewports, set_reports,
};
use crate::scale::{default_measure, with_area_unit, with_precision, with_unit};
use crate::services::store::CAPABILITY;
use crate::sync::viewports::is_persistent;
use crate::units::{
    assert_writable_measure, measure_from_known_length, measure_from_ratio, measurement_point,
    serialize_error,
};

/// The scale a change writes to a page: `null` is back to the default.
type MeasureOf = Rc<dyn Fn(&MeasurementCapability, PageRef) -> Result<Value, Error>>;

/// `PageScaleWrite`: a page and the scale a change gives it (`null`: back to
/// the default).
type PageScaleWrite = (PageRef, Value);

fn cancelled(signal: &AbortSignal, message: &str) -> Error {
    plugin_error(code::OPERATION_CANCELLED, CAPABILITY, message, signal.reason_value(), None)
}

/// `refusedReport(page, error)`: what a page that didn't get its scale
/// reports: why.
fn refused_report(page: PageRef, error: &Error) -> ScaleChangeReport {
    ScaleChangeReport::ScaleError { page, scale_error: serialize_error(error) }
}

/// `opsOf(writes)`: one `pages.setScale` op per scale; the pages that get
/// the same one go together.
fn ops_of(writes: &[PageScaleWrite]) -> Vec<Value> {
    let mut groups: Vec<(String, Value, Vec<Value>)> = Vec::new();
    for (page, scale) in writes {
        // `JSON.stringify(scale)`: only compared with the other keys made here.
        let key = serde_json::to_string(scale).unwrap_or_default();
        match groups.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, pages)) => pages.push(page.to_json()),
            None => groups.push((key, scale.clone(), vec![page.to_json()])),
        }
    }
    groups
        .into_iter()
        .map(|(_, measure, pages)| {
            json!({ "type": "pages.setScale", "pages": pages, "measure": measure })
        })
        .collect()
}

impl MeasurementCapability {
    /// `effectiveScale(page, scale)`: the scale a page measures in once it
    /// has `scale`: the default where `null` removes it.
    fn effective_scale(&self, page: PageRef, scale: &Value) -> Result<Value, Error> {
        if !(scale.is_null() || epdf_kernel::is_undefined(scale)) {
            return Ok(scale.clone());
        }
        let core = self.core();
        let layout = core.registry.page_of(PageInput::Ref(page))?;
        let settings = core.settings.get();
        default_measure(
            settings.get("defaultScale").unwrap_or(&Value::Null),
            layout.user_unit().unwrap_or(1.0),
        )
    }

    /// `writeLocal(write)`: a session-only scale: the engine can't keep one,
    /// so the session's state holds it.
    fn write_local(&self, (page, scale): &PageScaleWrite) -> Result<(), Error> {
        let core = self.core();
        let layout = core.registry.page_of(PageInput::Ref(*page))?;
        let pon = page.page_object_number;
        let viewports = core
            .cx
            .state()
            .local_viewports
            .get(&pon)
            .and_then(|list| list.as_array().cloned())
            .unwrap_or_default();
        let size = layout.row().get("size").cloned().unwrap_or(Value::Null);
        let next = viewports_with_scale(&viewports, &size, scale);
        core.cx.update_state(|s| set_local_viewports(&s, pon, Value::Array(next)));
        Ok(())
    }

    /// `write(writes, recalculate, signal)`: the pages' scales and the
    /// re-measure of their measurements, staged in one group: one change,
    /// which the engine applies all or none of.
    async fn write(
        &self,
        writes: &[PageScaleWrite],
        recalculate: bool,
        signal: Option<AbortSignal>,
    ) -> Result<Vec<ScaleChangeReport>, Error> {
        let core = self.core();
        let persistent = is_persistent(&core.cx)?;
        let kept: Vec<PageScaleWrite> = if persistent { writes.to_vec() } else { Vec::new() };
        if !persistent {
            for write in writes {
                self.write_local(write)?;
            }
        }
        let label = json!({ "key": "measurement.setScale", "count": writes.len() });
        let changes = core.cx.changes()?;
        let (change, reports) = changes.group(label.clone(), || {
            let change = if kept.is_empty() {
                None
            } else {
                let pages = core.registry.pages();
                let list = pages.as_deref().and_then(Value::as_array).cloned().unwrap_or_default();
                let refs: Vec<Value> = kept.iter().map(|(page, _)| page.to_json()).collect();
                let mut staged = StagedChange::ops(label.clone(), ops_of(&kept));
                staged.undo = scale_back_of(&list, &refs)?;
                Some(changes.stage(staged)?)
            };
            let reports = writes
                .iter()
                .map(|(page, scale)| {
                    let effective = self.effective_scale(*page, scale)?;
                    if recalculate {
                        let report = core.annotation.stage_remeasure(&page.to_json(), effective)?;
                        Ok(ScaleChangeReport::Recalibrated(report))
                    } else {
                        Ok(ScaleChangeReport::Recalibrated(json!({
                            "page": page,
                            "scale": effective,
                            "updated": [],
                            "skipped": [],
                            "failed": [],
                        })))
                    }
                })
                .collect::<Result<Vec<_>, Error>>()?;
            Ok((change, reports))
        })?;
        // Without a scale to write, the group may still hold the re-measure:
        // wait for all of it.
        match change {
            Some(change) => {
                cancellable(CAPABILITY, signal, change.result()).await?;
            }
            None => cancellable(CAPABILITY, signal, changes.when_settled()).await?,
        }
        Ok(reports)
    }

    /// `change(pages, measure, options)`: every page is resolved first, then
    /// the pages' scales are written as one change. One page: a failure
    /// rejects. Several: each page reports its own outcome, but a cancel
    /// ends the whole change.
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
            let signal = options.signal.clone();
            if let Some(signal) = signal.as_ref().filter(|s| s.aborted()) {
                return Err(cancelled(signal, "operation cancelled"));
            }
            // One page: a failure rejects. Several: each page reports its own outcome.
            let tolerant = pages == PageTarget::All || target.len() > 1;
            let recalculate = options.recalculate != Some(false);
            core.cx.update_state(|s| begin_scale_change(&s));
            let run = async {
                // Re-measuring takes every measurement on the pages.
                if recalculate {
                    cancellable(CAPABILITY, signal.clone(), core.annotation.when_remeasurable())
                        .await?;
                }
                core.cx.assert_allowed(SCALE_PERMISSION, "changing a scale")?;
                let mut reports: HashMap<u32, ScaleChangeReport> = HashMap::new();
                let mut writes: Vec<PageScaleWrite> = Vec::new();
                for &page in &target {
                    let attempt = measure(&this, page).and_then(|scale| {
                        if truthy(Some(&scale)) {
                            assert_writable_measure(&scale)?;
                        }
                        Ok(scale)
                    });
                    match attempt {
                        Ok(scale) => writes.push((page, scale)),
                        Err(error) => {
                            let refused = to_plugin_error(CAPABILITY, &error);
                            if !tolerant {
                                return Err(refused);
                            }
                            reports.insert(page.page_object_number, refused_report(page, &refused));
                        }
                    }
                }
                if !writes.is_empty() {
                    match this.write(&writes, recalculate, signal.clone()).await {
                        Ok(written) => {
                            for report in written {
                                if let Some(page) = report.page() {
                                    reports.insert(page.page_object_number, report);
                                }
                            }
                        }
                        Err(error) => {
                            let refused = to_plugin_error(CAPABILITY, &error);
                            // A cancel ends the whole change; a refusal fails every page of it.
                            if !tolerant || refused.plugin_code() == Some(code::OPERATION_CANCELLED)
                            {
                                return Err(refused);
                            }
                            for (page, _) in &writes {
                                reports.insert(
                                    page.page_object_number,
                                    refused_report(*page, &refused),
                                );
                            }
                        }
                    }
                }
                let ordered: Vec<ScaleChangeReport> = target
                    .iter()
                    .filter_map(|page| reports.get(&page.page_object_number).cloned())
                    .collect();
                for report in &ordered {
                    if let (ScaleChangeReport::Recalibrated(_), Some(page)) =
                        (report, report.page())
                    {
                        core.events
                            .scale_changed
                            .emit(&MeasurementScaleChangedEvent { page, report: report.clone() });
                    }
                }
                let ordered = Rc::new(ordered);
                core.cx.update_state(|s| set_reports(&s, ordered.clone()));
                Ok(ordered)
            };
            let result = run.await;
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
