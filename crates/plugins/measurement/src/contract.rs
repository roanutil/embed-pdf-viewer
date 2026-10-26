//! `contract.ts`: the public measurement vocabulary. Measures, viewports and
//! re-measurement reports are the engine's JSON (`PdfMeasure`,
//! `PageMeasurementViewport`, the annotation plugin's `RecalibrationReport`),
//! carried as the caller or the engine wrote them.

use epdf_kernel::Error;
use epdf_kernel::geometry::{PageRef, Point};
use epdf_kernel::signal::AbortSignal;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

use crate::scale::default_presets;

/// Changing a page's scale rewrites its measurement annotations
/// (`services/store.ts`).
pub const SCALE_PERMISSION: &str = "doc.annotate.modify";

/// A page argument: a ref, or a zero-based index into the document's pages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageInput {
    Ref(PageRef),
    Index(f64),
}

impl From<PageRef> for PageInput {
    fn from(page: PageRef) -> Self {
        PageInput::Ref(page)
    }
}

impl From<u32> for PageInput {
    fn from(index: u32) -> Self {
        PageInput::Index(index as f64)
    }
}

impl PageInput {
    /// A `PageRef | number` value. Anything else names no page.
    pub fn from_json(value: &Value) -> PageInput {
        match value {
            Value::Number(n) => PageInput::Index(n.as_f64().unwrap_or(f64::NAN)),
            other => PageRef::from_json(other).map_or(PageInput::Index(f64::NAN), PageInput::Ref),
        }
    }
}

/// `PageTarget`: one page, several pages, or every page of the document.
#[derive(Clone, Debug, PartialEq)]
pub enum PageTarget {
    All,
    Page(PageInput),
    Pages(Vec<PageInput>),
}

impl From<PageInput> for PageTarget {
    fn from(page: PageInput) -> Self {
        PageTarget::Page(page)
    }
}

impl From<PageRef> for PageTarget {
    fn from(page: PageRef) -> Self {
        PageTarget::Page(PageInput::Ref(page))
    }
}

impl PageTarget {
    /// `'all'`, a list, or one page.
    pub fn from_json(value: &Value) -> PageTarget {
        match value {
            Value::String(s) if s == "all" => PageTarget::All,
            Value::Array(pages) => {
                PageTarget::Pages(pages.iter().map(PageInput::from_json).collect())
            }
            other => PageTarget::Page(PageInput::from_json(other)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ScalePreset {
    pub id: String,
    pub label: String,
    pub paper: f64,
    pub real: f64,
    pub unit: String,
}

/// `MeasurementSettings['defaultScale']`.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultScale {
    Metric,
    Imperial,
    /// A scale of the app's own, as JSON.
    Measure(Value),
}

impl DefaultScale {
    pub fn to_json(&self) -> Value {
        match self {
            DefaultScale::Metric => json!("metric"),
            DefaultScale::Imperial => json!("imperial"),
            DefaultScale::Measure(measure) => measure.clone(),
        }
    }

    pub fn from_json(value: &Value) -> DefaultScale {
        match value {
            Value::String(s) if s == "imperial" => DefaultScale::Imperial,
            Value::Object(_) | Value::Array(_) | Value::Null => {
                DefaultScale::Measure(value.clone())
            }
            _ => DefaultScale::Metric,
        }
    }
}

/// The plugin's settings, registered with the kernel and shared by every
/// document.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementSettings {
    pub default_scale: DefaultScale,
    pub presets: Vec<ScalePreset>,
}

impl MeasurementSettings {
    /// The settings as the kernel's JSON store holds them.
    pub fn of(value: &Value) -> MeasurementSettings {
        MeasurementSettings {
            default_scale: DefaultScale::from_json(
                value.get("defaultScale").unwrap_or(&Value::Null),
            ),
            presets: value
                .get("presets")
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default(),
        }
    }
}

/// `MEASUREMENT_DEFAULTS`, as JSON.
pub fn measurement_defaults() -> Value {
    json!({ "defaultScale": "metric", "presets": default_presets() })
}

/// `MeasurementConfig`: either setting, each a whole value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementConfig {
    pub default_scale: Option<DefaultScale>,
    pub presets: Option<Vec<ScalePreset>>,
}

impl MeasurementConfig {
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        if let Some(scale) = &self.default_scale {
            out.insert("defaultScale".into(), scale.to_json());
        }
        if let Some(presets) = &self.presets {
            out.insert("presets".into(), json!(presets));
        }
        Value::Object(out)
    }
}

/// `OperationOptions`.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `ScaleChangeOptions`: `recalculate` re-measures the page's measurement
/// annotations with the new scale; only `Some(false)` skips it.
#[derive(Clone, Default)]
pub struct ScaleChangeOptions {
    pub signal: Option<AbortSignal>,
    pub recalculate: Option<bool>,
}

/// `calibrate`'s options: `apply_to` widens the write (default: the input
/// page).
#[derive(Clone, Default)]
pub struct CalibrateOptions {
    pub change: ScaleChangeOptions,
    pub apply_to: Option<PageTarget>,
}

/// `PageScale`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PageScale {
    /// The engine's `PdfMeasurement`, or `null`.
    pub measure: Value,
    /// `owned`, `foreign` or `default`.
    pub source: &'static str,
    /// The page's viewports are known.
    pub ready: bool,
    /// The engine can persist a scale into the document.
    pub persistent: bool,
    /// Why the last read of the page's viewports failed; the last known
    /// viewports stay in effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl PageScale {
    pub fn to_json(&self) -> Value {
        epdf_kernel::js::to_value(self)
    }
}

/// `NO_PAGE_SCALE`: a page's scale where there is none to read.
pub fn no_page_scale() -> PageScale {
    PageScale {
        measure: Value::Null,
        source: "default",
        ready: false,
        persistent: false,
        error: None,
    }
}

/// `NO_READOUT`.
pub fn no_readout() -> Value {
    json!({ "unavailable": "not-dimension" })
}

/// `CalibrationRequest`: two points captured by the calibrate tool,
/// awaiting the real length. Page space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationRequest {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    /// The captured length in PDF user space.
    pub user_space_length: f64,
}

/// `CalibrateInput['distance']`.
#[derive(Clone, Debug, PartialEq)]
pub struct KnownLength {
    pub value: f64,
    pub unit: String,
}

/// `CalibrateInput`.
#[derive(Clone, Debug, PartialEq)]
pub struct CalibrateInput {
    pub page: PageInput,
    pub from: Point,
    pub to: Point,
    pub distance: KnownLength,
}

/// `ScaleChangeReport`: the annotation plugin's `RecalibrationReport`, or a
/// page whose scale write failed in a multi-page change.
#[derive(Clone, Debug, PartialEq)]
pub enum ScaleChangeReport {
    /// `{ page, scale, updated, skipped, failed, error? }`, as
    /// `remeasurePage` answered it (or the same shape with nothing
    /// re-measured).
    Recalibrated(Value),
    /// `{ page, updated: [], skipped: [], failed: [], scaleError }`.
    ScaleError { page: PageRef, scale_error: Value },
}

impl ScaleChangeReport {
    /// The page the report is about.
    pub fn page(&self) -> Option<PageRef> {
        match self {
            ScaleChangeReport::Recalibrated(report) => {
                report.get("page").and_then(PageRef::from_json)
            }
            ScaleChangeReport::ScaleError { page, .. } => Some(*page),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            ScaleChangeReport::Recalibrated(report) => report.clone(),
            ScaleChangeReport::ScaleError { page, scale_error } => json!({
                "page": page,
                "updated": [],
                "skipped": [],
                "failed": [],
                "scaleError": scale_error,
            }),
        }
    }

    pub fn from_json(value: &Value) -> Result<ScaleChangeReport, Error> {
        match value.get("scaleError") {
            Some(scale_error) if !epdf_kernel::is_undefined(scale_error) => {
                let page = value.get("page").and_then(PageRef::from_json).ok_or_else(|| {
                    Error::named("TypeError", "a scale error report names no page")
                })?;
                Ok(ScaleChangeReport::ScaleError { page, scale_error: scale_error.clone() })
            }
            _ => Ok(ScaleChangeReport::Recalibrated(value.clone())),
        }
    }
}

impl Serialize for ScaleChangeReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ScaleChangeReport {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        ScaleChangeReport::from_json(&value).map_err(|e| D::Error::custom(e.message))
    }
}

/// `MeasurementKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasurementKind {
    Distance,
    Perimeter,
    Area,
}

impl MeasurementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MeasurementKind::Distance => "distance",
            MeasurementKind::Perimeter => "perimeter",
            MeasurementKind::Area => "area",
        }
    }

    pub fn parse(kind: &str) -> Option<MeasurementKind> {
        match kind {
            "distance" => Some(MeasurementKind::Distance),
            "perimeter" => Some(MeasurementKind::Perimeter),
            "area" => Some(MeasurementKind::Area),
            _ => None,
        }
    }
}

/// `CreateMeasurementInput`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateMeasurementInput {
    pub kind: MeasurementKind,
    pub page: PageInput,
    /// Page-space points: two for a distance, the vertices otherwise.
    pub points: Vec<Point>,
    /// The authoring tool whose defaults apply (defaults to the kind's own
    /// tool).
    pub tool: Option<String>,
}

// ── events ──

/// `MeasurementScaleChangedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementScaleChangedEvent {
    pub page: PageRef,
    pub report: ScaleChangeReport,
}

impl MeasurementScaleChangedEvent {
    pub fn to_json(&self) -> Value {
        json!({ "page": self.page, "report": self.report.to_json() })
    }
}

/// `CalibrationRequestedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationRequestedEvent {
    pub request: CalibrationRequest,
}

impl CalibrationRequestedEvent {
    pub fn to_json(&self) -> Value {
        json!({ "request": epdf_kernel::js::to_value(&self.request) })
    }
}

/// `CalibrationCompletedEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct CalibrationCompletedEvent {
    pub page: PageRef,
}

impl CalibrationCompletedEvent {
    pub fn to_json(&self) -> Value {
        json!({ "page": self.page })
    }
}

/// `CalibrationDismissedEvent`: `{}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CalibrationDismissedEvent;

impl CalibrationDismissedEvent {
    pub fn to_json(&self) -> Value {
        json!({})
    }
}

/// The settings-change event, as the kernel's store reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementSettingsChanged {
    pub settings: MeasurementSettings,
    pub changed: Vec<String>,
}
