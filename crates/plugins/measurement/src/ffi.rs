//! The measurement plugin through uniffi: typed records, one handle, one
//! event enum. Every method hops to the kernel thread through
//! `KernelBound`.
//!
//! Compiled and tested since Task 13.33. Since Task 13.22
//! the plugin carries measures, viewports, readouts and re-measurement
//! reports as the engine's records (JSON), so the records here decode them.
//! A value that does not decode is an `invalid-state` error: the record and
//! the engine disagree, and a default would hide it. Annotation refs cross
//! as JSON strings (`annotation_json`), as the link and annotation handles
//! carry them since 13.17.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    CalibrateInput, CalibrateOptions, CreateMeasurementInput as CoreCreate, DefaultScale,
    KnownLength, MeasurementCapability, OperationOptions, PageInput, PageTarget,
    ScaleChangeOptions, ScalePreset,
};

// ── measures ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum MeasureFraction {
    Decimal,
    Fraction,
    Round,
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum MeasureLabelPosition {
    Suffix,
    Prefix,
}

/// One ISO 32000 number format (`/NumberFormat`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct PdfNumberFormat {
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub conversion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub fraction: Option<MeasureFraction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub precision: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub fixed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub thousands: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub decimal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub prefix_spacing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub suffix_spacing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub label_position: Option<MeasureLabelPosition>,
}

/// A point of a measure's `origin`, as the engine writes it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct MeasureOrigin {
    pub x: f64,
    pub y: f64,
}

/// A `/Measure` dictionary. `subtype` is `rectilinear` for the only kind
/// EmbedPDF writes; any other subtype (`geospatial`) is foreign and has no
/// format lists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct PdfMeasurement {
    pub subtype: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub x: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub y: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub distance: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub area: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub angle: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub slope: Option<Vec<PdfNumberFormat>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub origin: Option<MeasureOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[uniffi(default = None)]
    pub cyx: Option<f64>,
}

fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[measurement] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

fn decode_measure(value: &Value) -> Result<Option<PdfMeasurement>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value::<PdfMeasurement>(value.clone())
        .map(Some)
        .map_err(|e| invalid_state("a measure", e))
}

fn measure_json(measure: &PdfMeasurement) -> Result<Value, Error> {
    serde_json::to_value(measure).map_err(|e| invalid_state("a measure", e))
}

// ── config ──────────────────────────────────────────────────────────────────

/// The fallback scale for a page with no viewport.
// UniFFI carries the variants by value; a `Box` does not cross.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementDefaultScale {
    Metric,
    Imperial,
    Measure { measure: PdfMeasurement },
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MeasurementConfig {
    /// `None` is metric.
    #[uniffi(default = None)]
    pub default_scale: Option<MeasurementDefaultScale>,
    /// `None` is the default picker: metric 1:1 to 1:200, imperial 1/4 and
    /// 1/8 in.
    #[uniffi(default = None)]
    pub presets: Option<Vec<ScalePreset>>,
}

impl TryFrom<MeasurementConfig> for crate::MeasurementConfig {
    type Error = Error;

    fn try_from(config: MeasurementConfig) -> Result<Self, Error> {
        Ok(crate::MeasurementConfig {
            default_scale: config
                .default_scale
                .map(|s| {
                    Ok::<_, Error>(match s {
                        MeasurementDefaultScale::Metric => DefaultScale::Metric,
                        MeasurementDefaultScale::Imperial => DefaultScale::Imperial,
                        MeasurementDefaultScale::Measure { measure } => {
                            DefaultScale::Measure(measure_json(&measure)?)
                        }
                    })
                })
                .transpose()?,
            presets: config.presets,
        })
    }
}

// ── targets, scales, readouts ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementPageTarget {
    Page { page: PageRef },
    Pages { pages: Vec<PageRef> },
    All,
}

fn core_page(page: PageRef) -> epdf_kernel::geometry::PageRef {
    epdf_kernel::geometry::PageRef::new(page.page_object_number)
}

fn page_input(page: PageRef) -> PageInput {
    PageInput::Ref(core_page(page))
}

fn ffi_page(page: epdf_kernel::geometry::PageRef) -> PageRef {
    PageRef { page_object_number: page.page_object_number }
}

fn page_of_json(value: Option<&Value>) -> Result<PageRef, Error> {
    value
        .and_then(epdf_kernel::geometry::PageRef::from_json)
        .map(ffi_page)
        .ok_or_else(|| invalid_state("a page ref", value.cloned().unwrap_or(Value::Null)))
}

fn core_point(p: Point) -> epdf_kernel::geometry::Point {
    epdf_kernel::geometry::Point { x: p.x, y: p.y }
}

fn ffi_point(p: epdf_kernel::geometry::Point) -> Point {
    Point { x: p.x, y: p.y }
}

impl From<MeasurementPageTarget> for PageTarget {
    fn from(t: MeasurementPageTarget) -> Self {
        match t {
            MeasurementPageTarget::Page { page } => PageTarget::Page(page_input(page)),
            MeasurementPageTarget::Pages { pages } => {
                PageTarget::Pages(pages.into_iter().map(page_input).collect())
            }
            MeasurementPageTarget::All => PageTarget::All,
        }
    }
}

/// `ScaleChangeOptions` without the signal, which the handle's
/// `CancelToken` carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct MeasurementScaleChangeOptions {
    /// Re-measure the pages' measurement annotations. `None` is true.
    #[uniffi(default = None)]
    pub recalculate: Option<bool>,
}

impl From<MeasurementScaleChangeOptions> for ScaleChangeOptions {
    fn from(options: MeasurementScaleChangeOptions) -> Self {
        ScaleChangeOptions { signal: None, recalculate: options.recalculate }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementScaleSource {
    /// EmbedPDF's own viewport.
    Owned,
    /// Another producer's viewport.
    Foreign,
    /// No viewport: the configured default.
    Default,
}

/// An engine error as the plugin keeps it (`SerializedEngineError`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeasurementErrorInfo {
    pub code: String,
    pub detail: String,
}

fn decode_error(value: &Value) -> Result<MeasurementErrorInfo, Error> {
    let code = value.get("code").and_then(Value::as_str);
    let message = value.get("message").and_then(Value::as_str);
    match (code, message) {
        (Some(code), Some(message)) => {
            Ok(MeasurementErrorInfo { code: code.into(), detail: message.into() })
        }
        _ => Err(invalid_state("an engine error", value)),
    }
}

/// A page's resolved scale.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementPageScale {
    pub measure: Option<PdfMeasurement>,
    pub source: MeasurementScaleSource,
    /// The page's viewports are known.
    pub ready: bool,
    /// The engine can persist a scale into the document.
    pub persistent: bool,
    pub error: Option<MeasurementErrorInfo>,
}

impl TryFrom<&crate::PageScale> for MeasurementPageScale {
    type Error = Error;

    fn try_from(s: &crate::PageScale) -> Result<Self, Error> {
        Ok(MeasurementPageScale {
            measure: decode_measure(&s.measure)?,
            source: match s.source {
                "owned" => MeasurementScaleSource::Owned,
                "foreign" => MeasurementScaleSource::Foreign,
                _ => MeasurementScaleSource::Default,
            },
            ready: s.ready,
            persistent: s.persistent,
            error: s.error.as_ref().map(decode_error).transpose()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementReadoutKind {
    Distance,
    Perimeter,
    Area,
}

/// A value in the scale's first format unit, and its label.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementReadout {
    pub kind: MeasurementReadoutKind,
    pub label: String,
    pub value: f64,
    pub perimeter: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementUnavailable {
    NotDimension,
    NoMeasure,
    ForeignMeasure,
    NoScale,
    NoDistanceFormat,
    NoAreaFormat,
    InvalidFormat,
    InvalidGeometry,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementReading {
    Value { readout: MeasurementReadout },
    Unavailable { reason: MeasurementUnavailable },
}

/// A readout as the plugin answers it: `{ kind, value, label, perimeter? }`
/// or `{ unavailable }`.
fn decode_reading(value: &Value) -> Result<MeasurementReading, Error> {
    if let Some(reason) = value.get("unavailable").and_then(Value::as_str) {
        let reason = match reason {
            "not-dimension" => MeasurementUnavailable::NotDimension,
            "no-measure" => MeasurementUnavailable::NoMeasure,
            "foreign-measure" => MeasurementUnavailable::ForeignMeasure,
            "no-scale" => MeasurementUnavailable::NoScale,
            "no-distance-format" => MeasurementUnavailable::NoDistanceFormat,
            "no-area-format" => MeasurementUnavailable::NoAreaFormat,
            "invalid-format" => MeasurementUnavailable::InvalidFormat,
            "invalid-geometry" => MeasurementUnavailable::InvalidGeometry,
            other => return Err(invalid_state("a readout", other)),
        };
        return Ok(MeasurementReading::Unavailable { reason });
    }
    let kind = match value.get("kind").and_then(Value::as_str) {
        Some("distance") => MeasurementReadoutKind::Distance,
        Some("perimeter") => MeasurementReadoutKind::Perimeter,
        Some("area") => MeasurementReadoutKind::Area,
        _ => return Err(invalid_state("a readout", value)),
    };
    Ok(MeasurementReading::Value {
        readout: MeasurementReadout {
            kind,
            label: value.get("label").and_then(Value::as_str).unwrap_or_default().into(),
            value: value.get("value").and_then(Value::as_f64).unwrap_or(f64::NAN),
            perimeter: value.get("perimeter").and_then(Value::as_str).map(str::to_string),
        },
    })
}

// ── calibration and creation ────────────────────────────────────────────────

/// Two points the calibrate tool captured, awaiting the real length. Page
/// space.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrationRequest {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    /// The captured length in PDF user space.
    pub user_space_length: f64,
}

impl From<crate::CalibrationRequest> for MeasurementCalibrationRequest {
    fn from(r: crate::CalibrationRequest) -> Self {
        MeasurementCalibrationRequest {
            page: ffi_page(r.page),
            from: ffi_point(r.from),
            to: ffi_point(r.to),
            user_space_length: r.user_space_length,
        }
    }
}

/// Two page points and the real length between them, `value` in `unit`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrateInput {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    pub value: f64,
    pub unit: String,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MeasurementCalibrateOptions {
    /// Re-measure the pages' measurement annotations. `None` is true.
    #[uniffi(default = None)]
    pub recalculate: Option<bool>,
    /// Write the scale to these pages. `None` is the input's page.
    #[uniffi(default = None)]
    pub apply_to: Option<MeasurementPageTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasurementKind {
    Distance,
    Perimeter,
    Area,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CreateMeasurementInput {
    pub kind: MeasurementKind,
    pub page: PageRef,
    /// Page-space points: two for a distance, the vertices otherwise.
    pub points: Vec<Point>,
    /// The authoring tool whose defaults apply. `None` is the kind's own.
    #[uniffi(default = None)]
    pub tool: Option<String>,
}

// ── reports ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ScaleSkipReason {
    NoAuthority,
    Locked,
    ForeignMeasure,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleSkippedAnnotation {
    /// The `AnnotationRef` record.
    pub annotation_json: String,
    pub reason: ScaleSkipReason,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleFailedAnnotation {
    /// The `AnnotationRef` record.
    pub annotation_json: String,
    pub error: MeasurementErrorInfo,
}

/// One page's re-measurement after a scale change.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleRecalibration {
    pub page: PageRef,
    pub scale: Option<PdfMeasurement>,
    pub error: Option<MeasurementErrorInfo>,
    /// The `AnnotationRef` records re-measured.
    pub updated_json: Vec<String>,
    pub skipped: Vec<ScaleSkippedAnnotation>,
    pub failed: Vec<ScaleFailedAnnotation>,
}

// UniFFI carries the variants by value; a `Box` does not cross.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ScaleChangeReport {
    Recalibrated {
        report: ScaleRecalibration,
    },
    /// The page's viewport write failed; nothing on it was re-measured.
    ScaleFailed {
        page: PageRef,
        error: MeasurementErrorInfo,
    },
}

fn list<'a>(report: &'a Value, key: &str) -> &'a [Value] {
    report.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn ref_json(value: Option<&Value>) -> String {
    value.cloned().unwrap_or(Value::Null).to_string()
}

impl TryFrom<&crate::ScaleChangeReport> for ScaleChangeReport {
    type Error = Error;

    fn try_from(r: &crate::ScaleChangeReport) -> Result<Self, Error> {
        Ok(match r {
            crate::ScaleChangeReport::ScaleError { page, scale_error } => {
                ScaleChangeReport::ScaleFailed {
                    page: ffi_page(*page),
                    error: decode_error(scale_error)?,
                }
            }
            crate::ScaleChangeReport::Recalibrated(r) => ScaleChangeReport::Recalibrated {
                report: ScaleRecalibration {
                    page: page_of_json(r.get("page"))?,
                    scale: decode_measure(r.get("scale").unwrap_or(&Value::Null))?,
                    error: r.get("error").map(decode_error).transpose()?,
                    updated_json: list(r, "updated").iter().map(Value::to_string).collect(),
                    skipped: list(r, "skipped")
                        .iter()
                        .map(|entry| {
                            Ok(ScaleSkippedAnnotation {
                                annotation_json: ref_json(entry.get("ref")),
                                reason: match entry.get("reason").and_then(Value::as_str) {
                                    Some("no-authority") => ScaleSkipReason::NoAuthority,
                                    Some("locked") => ScaleSkipReason::Locked,
                                    Some("foreign-measure") => ScaleSkipReason::ForeignMeasure,
                                    Some("unavailable") => ScaleSkipReason::Unavailable,
                                    _ => return Err(invalid_state("a skip reason", entry)),
                                },
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                    failed: list(r, "failed")
                        .iter()
                        .map(|entry| {
                            Ok(ScaleFailedAnnotation {
                                annotation_json: ref_json(entry.get("ref")),
                                error: decode_error(entry.get("error").unwrap_or(&Value::Null))?,
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                },
            },
        })
    }
}

fn reports(list: Rc<Vec<crate::ScaleChangeReport>>) -> Result<Vec<ScaleChangeReport>, ViewerError> {
    list.iter().map(ScaleChangeReport::try_from).collect::<Result<_, _>>().map_err(Into::into)
}

// ── events ──────────────────────────────────────────────────────────────────

// UniFFI carries the variants by value; a `Box` does not cross.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementEvent {
    ScaleChanged { page: PageRef, report: ScaleChangeReport },
    CalibrationRequested { request: MeasurementCalibrationRequest },
    CalibrationCompleted { page: PageRef },
    CalibrationDismissed,
}

#[uniffi::export(with_foreign)]
pub trait MeasurementEventListener: Send + Sync {
    fn on_event(&self, event: MeasurementEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

// ── the handle ──────────────────────────────────────────────────────────────

/// The capability, if its document is still open.
fn open(cap: &MeasurementCapability) -> Result<&MeasurementCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "measurement",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct MeasurementHandle {
    cap: KernelBound<MeasurementCapability>,
    post: KernelPost,
}

impl MeasurementHandle {
    /// On the kernel thread.
    pub fn new(cap: MeasurementCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(MeasurementHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl MeasurementHandle {
    /// Whether this caller may measure `page`: `annotations:create`, and the
    /// page's scale has loaded.
    pub async fn can_measure(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| open(c)?.can_measure(page_input(page))).await
    }

    /// `ready` is false until the page's viewports load (`ensure_loaded`).
    pub async fn page_scale(&self, page: PageRef) -> Result<MeasurementPageScale, ViewerError> {
        self.cap
            .call(move |c| {
                MeasurementPageScale::try_from(&*open(c)?.get_page_scale(page_input(page))?)
            })
            .await
    }

    pub async fn is_busy(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_busy())).await
    }

    /// The reports of the last scale change.
    pub async fn last_reports(&self) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.list_last_reports().iter().map(ScaleChangeReport::try_from).collect()
            })
            .await
    }

    pub async fn presets(&self) -> Result<Vec<ScalePreset>, ViewerError> {
        self.cap.call(|c| open(c)?.list_presets()).await
    }

    pub async fn units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_units().into_iter().map(String::from).collect())).await
    }

    pub async fn area_units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_area_units())).await
    }

    /// A measurement annotation's value in its own scale. `annotation_json`
    /// is the `AnnotationRef` record.
    pub async fn readout(
        &self,
        annotation_json: String,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                let reference: Value = serde_json::from_str(&annotation_json).map_err(|e| {
                    plugin_error(code::INVALID_INPUT, "measurement", e.to_string(), None, None)
                })?;
                decode_reading(&open(c)?.get_readout(&reference))
            })
            .await
    }

    /// A distance in the page's scale, with no annotation.
    pub async fn measure_distance(
        &self,
        page: PageRef,
        from: Point,
        to: Point,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                decode_reading(&open(c)?.measure_distance(
                    page_input(page),
                    core_point(from),
                    core_point(to),
                )?)
            })
            .await
    }

    pub async fn measure_area(
        &self,
        page: PageRef,
        vertices: Vec<Point>,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                let vertices: Vec<_> = vertices.into_iter().map(core_point).collect();
                decode_reading(&open(c)?.measure_area(page_input(page), &vertices)?)
            })
            .await
    }

    pub async fn calibration_request(
        &self,
    ) -> Result<Option<MeasurementCalibrationRequest>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_calibration_request().map(Into::into))).await
    }

    /// Write `measure` to `pages` and re-measure their dimensions. One
    /// report per page, in page order. Cancelling stops waiting; a page
    /// write already sent still lands.
    pub async fn set_scale(
        &self,
        pages: MeasurementPageTarget,
        measure: PdfMeasurement,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_scale(pages, measure_json(&measure)?, options.into())),
                reports,
            )
            .await?
    }

    /// Derive a scale from a known length and write it.
    pub async fn calibrate(
        &self,
        input: MeasurementCalibrateInput,
        options: MeasurementCalibrateOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = CalibrateInput {
                        page: page_input(input.page),
                        from: core_point(input.from),
                        to: core_point(input.to),
                        distance: KnownLength { value: input.value, unit: input.unit },
                    };
                    let options = CalibrateOptions {
                        change: ScaleChangeOptions {
                            signal: None,
                            recalculate: options.recalculate,
                        },
                        apply_to: options.apply_to.map(Into::into),
                    };
                    Ok(open(c)?.calibrate(input, options))
                },
                reports,
            )
            .await?
    }

    pub async fn set_unit(
        &self,
        pages: MeasurementPageTarget,
        unit: String,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_unit(pages, &unit, options.into())),
                reports,
            )
            .await?
    }

    pub async fn set_area_unit(
        &self,
        pages: MeasurementPageTarget,
        unit: String,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_area_unit(pages, &unit, options.into())),
                reports,
            )
            .await?
    }

    pub async fn set_precision(
        &self,
        pages: MeasurementPageTarget,
        precision: f64,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_precision(pages, precision, options.into())),
                reports,
            )
            .await?
    }

    /// `not-found` for a preset id `presets()` doesn't list.
    pub async fn set_preset(
        &self,
        pages: MeasurementPageTarget,
        preset_id: String,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_preset(pages, &preset_id, options.into())),
                reports,
            )
            .await?
    }

    /// Back to the default scale: EmbedPDF's viewport is removed, another
    /// producer's stays.
    pub async fn clear_scale(
        &self,
        pages: MeasurementPageTarget,
        options: MeasurementScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.clear_scale(pages, options.into())),
                reports,
            )
            .await?
    }

    /// Arm the calibrate tool; its two-point capture raises
    /// `CalibrationRequested`.
    pub async fn start_calibration(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| open(c)?.start_calibration()).await
    }

    pub async fn dismiss_calibration(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.dismiss_calibration();
                Ok(())
            })
            .await
    }

    /// Create a measurement annotation through the annotation plugin.
    /// Resolves the new annotation's `AnnotationRef` record.
    pub async fn create_measurement(
        &self,
        input: CreateMeasurementInput,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = CoreCreate {
                        kind: match input.kind {
                            MeasurementKind::Distance => crate::MeasurementKind::Distance,
                            MeasurementKind::Perimeter => crate::MeasurementKind::Perimeter,
                            MeasurementKind::Area => crate::MeasurementKind::Area,
                        },
                        page: page_input(input.page),
                        points: input.points.into_iter().map(core_point).collect(),
                        tool: input.tool,
                    };
                    Ok(open(c)?.create_measurement(input, OperationOptions::default()))
                },
                |created: Value| ref_json(created.get("annotation").and_then(|a| a.get("ref"))),
            )
            .await
    }

    pub async fn can_calibrate(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_calibrate())).await
    }

    /// Load the page's viewports. Shares a read in flight.
    pub async fn ensure_loaded(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.ensure_loaded(page_input(page), OperationOptions::default())),
                |()| (),
            )
            .await
    }

    /// Every hook in one registration. Gets `on_end` when the document
    /// closes (at once if it already has). An event that does not decode
    /// is reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn MeasurementEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let scale = c.on_scale_changed(move |e| {
                        let report = ScaleChangeReport::try_from(&e.report)?;
                        l.on_event(MeasurementEvent::ScaleChanged {
                            page: ffi_page(e.page),
                            report,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let requested = c.on_calibration_requested(move |e| {
                        l.on_event(MeasurementEvent::CalibrationRequested {
                            request: e.request.clone().into(),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let completed = c.on_calibration_completed(move |e| {
                        l.on_event(MeasurementEvent::CalibrationCompleted {
                            page: ffi_page(e.page),
                        });
                        Ok(())
                    });
                    let l = listener;
                    let dismissed = c.on_calibration_dismissed(move |_| {
                        l.on_event(MeasurementEvent::CalibrationDismissed);
                        Ok(())
                    });
                    epdf_kernel::Unsubscribe::new(move || {
                        for off in [&scale, &requested, &completed, &dismissed] {
                            off.call();
                        }
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
