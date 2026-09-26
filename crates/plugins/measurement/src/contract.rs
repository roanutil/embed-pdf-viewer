//! `contract.ts` and `host-contract.ts`: the public measurement vocabulary.
//! The host lens equals the public one, so there is one set of types.

use std::rc::Rc;

use epdf_annotation_core::measure::PdfMeasurement;
use epdf_annotation_core::{AnnotationRef, PageRef, PdfRect, Point};
use epdf_kernel::Error;
use epdf_plugin_annotation::{RecalibrationReport, SkipReason};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

pub use epdf_annotation_core::measure::{MeasurementReadout, Readout, Unavailable};

/// `PageTarget`: one page, several pages, or every page of the document.
#[derive(Clone, Debug, PartialEq)]
pub enum PageTarget {
    Page(PageRef),
    Pages(Vec<PageRef>),
    All,
}

impl<'de> Deserialize<'de> for PageTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        match &value {
            Value::String(s) if s == "all" => Ok(PageTarget::All),
            Value::Array(_) => serde_json::from_value(value).map(PageTarget::Pages),
            _ => serde_json::from_value(value).map(PageTarget::Page),
        }
        .map_err(D::Error::custom)
    }
}

impl From<PageRef> for PageTarget {
    fn from(page: PageRef) -> Self {
        PageTarget::Page(page)
    }
}

/// `ScalePreset`: one entry of the scale picker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalePreset {
    pub id: String,
    pub label: String,
    pub paper: f64,
    pub real: f64,
    pub unit: String,
}

/// `MeasurementConfig['defaultScale']`.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultScale {
    Metric,
    Imperial,
    Measure(Box<PdfMeasurement>),
}

impl<'de> Deserialize<'de> for DefaultScale {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        match &value {
            // `config.defaultScale === 'imperial' ? 'ft' : 'm'`: any other
            // string is metric.
            Value::String(s) if s == "imperial" => Ok(DefaultScale::Imperial),
            Value::String(_) => Ok(DefaultScale::Metric),
            _ => serde_json::from_value(value)
                .map(|m| DefaultScale::Measure(Box::new(m)))
                .map_err(D::Error::custom),
        }
    }
}

/// What the TypeScript reads from the global scope.
#[derive(Clone, Default)]
pub struct MeasurementHost {
    /// `console.error(message, error)`: receives a throwing event
    /// listener's error. `None` drops it.
    pub console_error: Option<Rc<dyn Fn(&str, &Error)>>,
}

/// `measurementPlugin(config)`. `host` is the Rust port's way in for what
/// the TypeScript reads from the global scope.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementConfig {
    /// Fallback scale for uncalibrated pages. Default metric.
    #[serde(default)]
    pub default_scale: Option<DefaultScale>,
    /// The scale picker. Default: metric 1:1 to 1:200, imperial 1/4 and 1/8 in.
    #[serde(default)]
    pub presets: Option<Vec<ScalePreset>>,
    #[serde(skip)]
    pub host: MeasurementHost,
}

/// `ScaleChangeOptions`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ScaleChangeOptions {
    /// Re-measure the page's measurement annotations with the new scale.
    /// Default true.
    #[serde(default)]
    pub recalculate: Option<bool>,
}

/// `calibrate`'s options: `applyTo` widens the write (default: the input
/// page).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrateOptions {
    #[serde(default)]
    pub recalculate: Option<bool>,
    #[serde(default)]
    pub apply_to: Option<PageTarget>,
}

/// `PageScale['source']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaleSource {
    Owned,
    Foreign,
    Default,
}

/// `PageScale`: the resolved scale, its source and readiness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageScale {
    pub measure: Option<PdfMeasurement>,
    pub source: ScaleSource,
    /// The page's viewports are known.
    pub ready: bool,
    /// The engine can persist a scale into the document.
    pub persistent: bool,
    /// A `SerializedEngineError`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

/// `LOADING`: a page whose viewports are not known.
pub const LOADING: PageScale = PageScale {
    measure: None,
    source: ScaleSource::Default,
    ready: false,
    persistent: false,
    error: None,
};

/// `PageMeasurementViewport`: a scale region in PDF space; `owned` marks
/// the one this plugin writes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeasurementViewport {
    pub bbox: PdfRect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<PdfMeasurement>,
    #[serde(default)]
    pub owned: bool,
}

impl MeasurementViewport {
    /// The annotation plugin's view of it.
    pub fn for_annotation(&self) -> epdf_plugin_annotation::PageMeasurementViewport {
        epdf_plugin_annotation::PageMeasurementViewport {
            bbox: self.bbox,
            measure: self.measure.clone(),
            owned: self.owned,
        }
    }
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
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct KnownDistance {
    pub value: f64,
    pub unit: String,
}

/// `CalibrateInput`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CalibrateInput {
    pub page: PageRef,
    pub from: Point,
    pub to: Point,
    /// The real-world length between the two points.
    pub distance: KnownDistance,
}

/// `ScaleChangeReport`: a page's re-measurement, or the failed viewport
/// write of one page of a multi-page change.
#[derive(Clone, Debug, PartialEq)]
pub enum ScaleChangeReport {
    Recalibrated(RecalibrationReport),
    ScaleError { page: PageRef, scale_error: Value },
}

impl ScaleChangeReport {
    pub fn page(&self) -> PageRef {
        match self {
            ScaleChangeReport::Recalibrated(r) => r.page,
            ScaleChangeReport::ScaleError { page, .. } => *page,
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            ScaleChangeReport::Recalibrated(r) => r.to_json(),
            ScaleChangeReport::ScaleError { page, scale_error } => json!({
                "page": page,
                "updated": [],
                "skipped": [],
                "failed": [],
                "scaleError": scale_error,
            }),
        }
    }

    /// Read one back from its JSON form.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let page: PageRef = serde_json::from_value(value.get("page").cloned().unwrap_or_default())
            .map_err(|e| e.to_string())?;
        if let Some(scale_error) = value.get("scaleError") {
            return Ok(ScaleChangeReport::ScaleError { page, scale_error: scale_error.clone() });
        }
        let list =
            |key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        let aref = |v: &Value| -> Result<AnnotationRef, String> {
            serde_json::from_value(v.clone()).map_err(|e| e.to_string())
        };
        let reason = |v: Option<&Value>| match v.and_then(Value::as_str) {
            Some("no-authority") => Ok(SkipReason::NoAuthority),
            Some("locked") => Ok(SkipReason::Locked),
            Some("foreign-measure") => Ok(SkipReason::ForeignMeasure),
            Some("unavailable") => Ok(SkipReason::Unavailable),
            other => Err(format!("unknown skip reason {other:?}")),
        };
        Ok(ScaleChangeReport::Recalibrated(RecalibrationReport {
            page,
            scale: value.get("scale").cloned().unwrap_or(Value::Null),
            error: value.get("error").cloned(),
            updated: list("updated").iter().map(aref).collect::<Result<_, _>>()?,
            skipped: list("skipped")
                .iter()
                .map(|s| {
                    Ok((aref(s.get("ref").unwrap_or(&Value::Null))?, reason(s.get("reason"))?))
                })
                .collect::<Result<_, String>>()?,
            failed: list("failed")
                .iter()
                .map(|f| {
                    Ok((
                        aref(f.get("ref").unwrap_or(&Value::Null))?,
                        f.get("error").cloned().unwrap_or(Value::Null),
                    ))
                })
                .collect::<Result<_, String>>()?,
        }))
    }
}

impl Serialize for ScaleChangeReport {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for ScaleChangeReport {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ScaleChangeReport::from_json(&Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

/// `MeasurementKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
}

/// `CreateMeasurementInput`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CreateMeasurementInput {
    pub kind: MeasurementKind,
    pub page: PageRef,
    /// Page-space points: two for a distance, the vertices otherwise.
    pub points: Vec<Point>,
    /// The authoring tool whose defaults apply (defaults to the kind's own
    /// tool).
    #[serde(default)]
    pub tool: Option<String>,
}

// ── events ──

/// `MeasurementScaleChangedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MeasurementScaleChangedEvent {
    pub page: PageRef,
    pub report: ScaleChangeReport,
}

/// `CalibrationRequestedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CalibrationRequestedEvent {
    pub request: CalibrationRequest,
}

/// `CalibrationCompletedEvent`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CalibrationCompletedEvent {
    pub page: PageRef,
}

/// `CalibrationDismissedEvent`: an empty object.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CalibrationDismissedEvent {}
