//! The measurement plugin through uniffi: typed records, one handle, one
//! event enum. Every method hops to the kernel thread through
//! `KernelBound`.
//!
//! The plugin keeps a measure and engine errors as JSON (`PageScale.error`,
//! `RecalibrationReport.scale`), so the records here decode them. A value
//! that does not decode is an `invalid-state` error: the record and the
//! engine disagree, and a default would hide it.

use std::sync::Arc;

use epdf_annotation_core::measure as core;
use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::Value;

use crate::{
    CalibrateInput, CalibrateOptions, CreateMeasurementInput as CoreCreate, DefaultScale,
    KnownDistance, MeasurementCapability, PageTarget, ScaleChangeOptions, ScalePreset,
};

// ── measures ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasureFraction {
    Decimal,
    Fraction,
    Round,
    Truncate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MeasureLabelPosition {
    Suffix,
    Prefix,
}

/// One ISO 32000 number format (`/NumberFormat`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PdfNumberFormat {
    pub unit: String,
    #[uniffi(default = None)]
    pub conversion: Option<f64>,
    #[uniffi(default = None)]
    pub fraction: Option<MeasureFraction>,
    #[uniffi(default = None)]
    pub precision: Option<f64>,
    #[uniffi(default = None)]
    pub fixed: Option<bool>,
    #[uniffi(default = None)]
    pub thousands: Option<String>,
    #[uniffi(default = None)]
    pub decimal: Option<String>,
    #[uniffi(default = None)]
    pub prefix_spacing: Option<String>,
    #[uniffi(default = None)]
    pub suffix_spacing: Option<String>,
    #[uniffi(default = None)]
    pub label_position: Option<MeasureLabelPosition>,
}

/// A `/Measure` dictionary. `subtype` is `RL` for a rectilinear measure
/// (the only kind EmbedPDF writes); any other subtype is foreign and has
/// no format lists.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PdfMeasurement {
    pub subtype: String,
    #[uniffi(default = None)]
    pub ratio: Option<String>,
    #[uniffi(default = None)]
    pub x: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub y: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub distance: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub area: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub angle: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub slope: Option<Vec<PdfNumberFormat>>,
    #[uniffi(default = None)]
    pub origin: Option<Point>,
    #[uniffi(default = None)]
    pub cyx: Option<f64>,
}

impl From<core::MeasureFraction> for MeasureFraction {
    fn from(f: core::MeasureFraction) -> Self {
        match f {
            core::MeasureFraction::Decimal => MeasureFraction::Decimal,
            core::MeasureFraction::Fraction => MeasureFraction::Fraction,
            core::MeasureFraction::Round => MeasureFraction::Round,
            core::MeasureFraction::Truncate => MeasureFraction::Truncate,
        }
    }
}

impl From<MeasureFraction> for core::MeasureFraction {
    fn from(f: MeasureFraction) -> Self {
        match f {
            MeasureFraction::Decimal => core::MeasureFraction::Decimal,
            MeasureFraction::Fraction => core::MeasureFraction::Fraction,
            MeasureFraction::Round => core::MeasureFraction::Round,
            MeasureFraction::Truncate => core::MeasureFraction::Truncate,
        }
    }
}

impl From<core::LabelPosition> for MeasureLabelPosition {
    fn from(p: core::LabelPosition) -> Self {
        match p {
            core::LabelPosition::Suffix => MeasureLabelPosition::Suffix,
            core::LabelPosition::Prefix => MeasureLabelPosition::Prefix,
        }
    }
}

impl From<MeasureLabelPosition> for core::LabelPosition {
    fn from(p: MeasureLabelPosition) -> Self {
        match p {
            MeasureLabelPosition::Suffix => core::LabelPosition::Suffix,
            MeasureLabelPosition::Prefix => core::LabelPosition::Prefix,
        }
    }
}

impl From<core::PdfNumberFormat> for PdfNumberFormat {
    fn from(f: core::PdfNumberFormat) -> Self {
        PdfNumberFormat {
            unit: f.unit,
            conversion: f.conversion,
            fraction: f.fraction.map(Into::into),
            precision: f.precision,
            fixed: f.fixed,
            thousands: f.thousands,
            decimal: f.decimal,
            prefix_spacing: f.prefix_spacing,
            suffix_spacing: f.suffix_spacing,
            label_position: f.label_position.map(Into::into),
        }
    }
}

impl From<PdfNumberFormat> for core::PdfNumberFormat {
    fn from(f: PdfNumberFormat) -> Self {
        core::PdfNumberFormat {
            unit: f.unit,
            conversion: f.conversion,
            fraction: f.fraction.map(Into::into),
            precision: f.precision,
            fixed: f.fixed,
            thousands: f.thousands,
            decimal: f.decimal,
            prefix_spacing: f.prefix_spacing,
            suffix_spacing: f.suffix_spacing,
            label_position: f.label_position.map(Into::into),
        }
    }
}

fn formats_in(list: Option<Vec<core::PdfNumberFormat>>) -> Option<Vec<PdfNumberFormat>> {
    list.map(|l| l.into_iter().map(Into::into).collect())
}

fn formats_out(list: Option<Vec<PdfNumberFormat>>) -> Option<Vec<core::PdfNumberFormat>> {
    list.map(|l| l.into_iter().map(Into::into).collect())
}

impl From<core::PdfMeasurement> for PdfMeasurement {
    fn from(m: core::PdfMeasurement) -> Self {
        PdfMeasurement {
            subtype: m.subtype,
            ratio: m.ratio,
            x: formats_in(m.x),
            y: formats_in(m.y),
            distance: formats_in(m.distance),
            area: formats_in(m.area),
            angle: formats_in(m.angle),
            slope: formats_in(m.slope),
            origin: m.origin.map(|p| Point { x: p.x, y: p.y }),
            cyx: m.cyx,
        }
    }
}

impl From<PdfMeasurement> for core::PdfMeasurement {
    fn from(m: PdfMeasurement) -> Self {
        core::PdfMeasurement {
            subtype: m.subtype,
            ratio: m.ratio,
            x: formats_out(m.x),
            y: formats_out(m.y),
            distance: formats_out(m.distance),
            area: formats_out(m.area),
            angle: formats_out(m.angle),
            slope: formats_out(m.slope),
            origin: m.origin.map(|p| epdf_annotation_core::PdfPoint { x: p.x, y: p.y }),
            cyx: m.cyx,
        }
    }
}

fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[measurement] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

fn decode_measure(value: &Value) -> Result<Option<PdfMeasurement>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    serde_json::from_value::<core::PdfMeasurement>(value.clone())
        .map(|m| Some(m.into()))
        .map_err(|e| invalid_state("a measure", e))
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

impl From<MeasurementConfig> for crate::MeasurementConfig {
    fn from(config: MeasurementConfig) -> Self {
        crate::MeasurementConfig {
            default_scale: config.default_scale.map(|s| match s {
                MeasurementDefaultScale::Metric => DefaultScale::Metric,
                MeasurementDefaultScale::Imperial => DefaultScale::Imperial,
                MeasurementDefaultScale::Measure { measure } => {
                    DefaultScale::Measure(Box::new(measure.into()))
                }
            }),
            presets: config.presets,
            host: crate::MeasurementHost::default(),
        }
    }
}

// ── targets, scales, readouts ───────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MeasurementPageTarget {
    Page { page: PageRef },
    Pages { pages: Vec<PageRef> },
    All,
}

fn core_page(page: PageRef) -> epdf_annotation_core::PageRef {
    epdf_annotation_core::PageRef::new(page.page_object_number)
}

fn ffi_page(page: epdf_annotation_core::PageRef) -> PageRef {
    PageRef { page_object_number: page.page_object_number }
}

fn core_point(p: Point) -> epdf_annotation_core::Point {
    epdf_annotation_core::Point { x: p.x, y: p.y }
}

impl From<MeasurementPageTarget> for PageTarget {
    fn from(t: MeasurementPageTarget) -> Self {
        match t {
            MeasurementPageTarget::Page { page } => PageTarget::Page(core_page(page)),
            MeasurementPageTarget::Pages { pages } => {
                PageTarget::Pages(pages.into_iter().map(core_page).collect())
            }
            MeasurementPageTarget::All => PageTarget::All,
        }
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

impl TryFrom<crate::PageScale> for MeasurementPageScale {
    type Error = Error;

    fn try_from(s: crate::PageScale) -> Result<Self, Error> {
        Ok(MeasurementPageScale {
            measure: s.measure.map(Into::into),
            source: match s.source {
                crate::ScaleSource::Owned => MeasurementScaleSource::Owned,
                crate::ScaleSource::Foreign => MeasurementScaleSource::Foreign,
                crate::ScaleSource::Default => MeasurementScaleSource::Default,
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

impl From<core::Readout> for MeasurementReading {
    fn from(r: core::Readout) -> Self {
        match r {
            core::Readout::Readout(r) => MeasurementReading::Value {
                readout: MeasurementReadout {
                    kind: match r.kind {
                        core::ReadoutKind::Distance => MeasurementReadoutKind::Distance,
                        core::ReadoutKind::Perimeter => MeasurementReadoutKind::Perimeter,
                        core::ReadoutKind::Area => MeasurementReadoutKind::Area,
                    },
                    label: r.label,
                    value: r.value,
                    perimeter: r.perimeter,
                },
            },
            core::Readout::Unavailable { unavailable } => MeasurementReading::Unavailable {
                reason: match unavailable {
                    core::Unavailable::NotDimension => MeasurementUnavailable::NotDimension,
                    core::Unavailable::NoMeasure => MeasurementUnavailable::NoMeasure,
                    core::Unavailable::ForeignMeasure => MeasurementUnavailable::ForeignMeasure,
                    core::Unavailable::NoScale => MeasurementUnavailable::NoScale,
                    core::Unavailable::NoDistanceFormat => MeasurementUnavailable::NoDistanceFormat,
                    core::Unavailable::NoAreaFormat => MeasurementUnavailable::NoAreaFormat,
                    core::Unavailable::InvalidFormat => MeasurementUnavailable::InvalidFormat,
                    core::Unavailable::InvalidGeometry => MeasurementUnavailable::InvalidGeometry,
                },
            },
        }
    }
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
            from: Point { x: r.from.x, y: r.from.y },
            to: Point { x: r.to.x, y: r.to.y },
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
    pub annotation: AnnotationRef,
    pub reason: ScaleSkipReason,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleFailedAnnotation {
    pub annotation: AnnotationRef,
    pub error: MeasurementErrorInfo,
}

/// One page's re-measurement after a scale change.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ScaleRecalibration {
    pub page: PageRef,
    pub scale: Option<PdfMeasurement>,
    pub error: Option<MeasurementErrorInfo>,
    pub updated: Vec<AnnotationRef>,
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

impl TryFrom<crate::ScaleChangeReport> for ScaleChangeReport {
    type Error = Error;

    fn try_from(r: crate::ScaleChangeReport) -> Result<Self, Error> {
        use epdf_plugin_annotation::SkipReason;
        Ok(match r {
            crate::ScaleChangeReport::ScaleError { page, scale_error } => {
                ScaleChangeReport::ScaleFailed {
                    page: ffi_page(page),
                    error: decode_error(&scale_error)?,
                }
            }
            crate::ScaleChangeReport::Recalibrated(r) => ScaleChangeReport::Recalibrated {
                report: ScaleRecalibration {
                    page: ffi_page(r.page),
                    scale: decode_measure(&r.scale)?,
                    error: r.error.as_ref().map(decode_error).transpose()?,
                    updated: r.updated.iter().map(ffi_ref).collect::<Result<_, Error>>()?,
                    skipped: r
                        .skipped
                        .iter()
                        .map(|(annotation, reason)| {
                            Ok(ScaleSkippedAnnotation {
                                annotation: ffi_ref(annotation)?,
                                reason: match reason {
                                    SkipReason::NoAuthority => ScaleSkipReason::NoAuthority,
                                    SkipReason::Locked => ScaleSkipReason::Locked,
                                    SkipReason::ForeignMeasure => ScaleSkipReason::ForeignMeasure,
                                    SkipReason::Unavailable => ScaleSkipReason::Unavailable,
                                },
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                    failed: r
                        .failed
                        .iter()
                        .map(|(annotation, error)| {
                            Ok(ScaleFailedAnnotation {
                                annotation: ffi_ref(annotation)?,
                                error: decode_error(error)?,
                            })
                        })
                        .collect::<Result<_, Error>>()?,
                },
            },
        })
    }
}

/// A plugin annotation ref as the ffi's; a ref whose numbers aren't whole
/// is `invalid-state`.
fn ffi_ref(r: &epdf_annotation_core::AnnotationRef) -> Result<AnnotationRef, Error> {
    AnnotationRef::try_from(r)
}

fn reports(list: Vec<crate::ScaleChangeReport>) -> Result<Vec<ScaleChangeReport>, ViewerError> {
    list.into_iter().map(ScaleChangeReport::try_from).collect::<Result<_, _>>().map_err(Into::into)
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
    /// Whether this caller may measure `page` (`doc.annotations.read`).
    pub async fn can_measure(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_measure(&core_page(page)))).await
    }

    /// `ready` is false until the page's viewports load (`ensure_loaded`).
    pub async fn page_scale(&self, page: PageRef) -> Result<MeasurementPageScale, ViewerError> {
        self.cap.call(move |c| open(c)?.get_page_scale(&core_page(page)).try_into()).await
    }

    pub async fn is_busy(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_busy())).await
    }

    /// The reports of the last scale change.
    pub async fn last_reports(&self) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.list_last_reports().into_iter().map(ScaleChangeReport::try_from).collect()
            })
            .await
    }

    pub async fn presets(&self) -> Result<Vec<ScalePreset>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_presets())).await
    }

    pub async fn units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.list_units().into_iter().map(String::from).collect())).await
    }

    pub async fn area_units(&self) -> Result<Vec<String>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_area_units().into_iter().map(String::from).collect()))
            .await
    }

    /// A measurement annotation's value in its own scale.
    pub async fn readout(
        &self,
        annotation: AnnotationRef,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_readout(&annotation.into()).into())).await
    }

    /// A distance in the page's scale, with no annotation. `not-found` for
    /// a page not in the document.
    pub async fn measure_distance(
        &self,
        page: PageRef,
        from: Point,
        to: Point,
    ) -> Result<MeasurementReading, ViewerError> {
        self.cap
            .call(move |c| {
                Ok(open(c)?
                    .measure_distance(&core_page(page), core_point(from), core_point(to))?
                    .into())
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
                Ok(open(c)?.measure_area(&core_page(page), &vertices)?.into())
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
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let measure = serde_json::to_value(core::PdfMeasurement::from(measure))
                        .map_err(|e| Error::new(e.to_string()))?;
                    Ok(open(c)?.set_scale(pages.into(), measure, options))
                },
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
                        page: core_page(input.page),
                        from: core_point(input.from),
                        to: core_point(input.to),
                        distance: KnownDistance { value: input.value, unit: input.unit },
                    };
                    let options = CalibrateOptions {
                        recalculate: options.recalculate,
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
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_unit(pages.into(), &unit, options)),
                reports,
            )
            .await?
    }

    pub async fn set_area_unit(
        &self,
        pages: MeasurementPageTarget,
        unit: String,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_area_unit(pages.into(), &unit, options)),
                reports,
            )
            .await?
    }

    pub async fn set_precision(
        &self,
        pages: MeasurementPageTarget,
        precision: f64,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_precision(pages.into(), precision, options)),
                reports,
            )
            .await?
    }

    /// `not-found` for a preset id `presets()` doesn't list.
    pub async fn set_preset(
        &self,
        pages: MeasurementPageTarget,
        preset_id: String,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_preset(pages.into(), &preset_id, options)),
                reports,
            )
            .await?
    }

    /// Back to the default scale: EmbedPDF's viewport is removed, another
    /// producer's stays.
    pub async fn clear_scale(
        &self,
        pages: MeasurementPageTarget,
        options: ScaleChangeOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<ScaleChangeReport>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.clear_scale(pages.into(), options)),
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
    pub async fn create_measurement(
        &self,
        input: CreateMeasurementInput,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
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
                        page: core_page(input.page),
                        points: input.points.into_iter().map(core_point).collect(),
                        tool: input.tool,
                    };
                    Ok(open(c)?.create_measurement(input))
                },
                |r| ffi_ref(&r).map_err(ViewerError::from),
            )
            .await?
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
                move |c| open(c)?.ensure_loaded(&core_page(page)),
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
                        let report = ScaleChangeReport::try_from(e.report.clone())?;
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
