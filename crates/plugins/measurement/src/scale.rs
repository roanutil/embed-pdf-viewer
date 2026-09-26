//! `scale.ts`: the default presets and scale, page-scale selection, and the
//! unit, area-unit and precision edits of a rectilinear scale.

use epdf_annotation_core::PdfPoint;
use epdf_annotation_core::PdfRect;
use epdf_annotation_core::measure::{MeasureFraction, PdfMeasurement, PdfNumberFormat};
use epdf_kernel::Error;

use crate::contract::{
    DefaultScale, MeasurementConfig, MeasurementViewport, PageScale, ScalePreset, ScaleSource,
};
use crate::units::{
    area_unit_label, measure_from_ratio_typed, metres, range_error, square_metres, square_of,
    viewport_for_point,
};

/// `DEFAULT_PRESETS`: metric 1:1 to 1:200, then imperial 1/4 and 1/8 in to
/// the foot.
pub fn default_presets() -> Vec<ScalePreset> {
    let metric = [1.0, 10.0, 20.0, 50.0, 100.0, 200.0].map(|real: f64| ScalePreset {
        id: format!("metric-{real}"),
        label: format!("1:{real}"),
        paper: 1.0,
        real,
        unit: "m".into(),
    });
    let imperial = |id: &str, label: &str, real: f64| ScalePreset {
        id: id.into(),
        label: label.into(),
        paper: 1.0,
        real,
        unit: "ft".into(),
    };
    let mut presets = metric.to_vec();
    presets.push(imperial("imperial-quarter", "1/4 in = 1 ft", 48.0));
    presets.push(imperial("imperial-eighth", "1/8 in = 1 ft", 96.0));
    presets
}

/// `defaultMeasure(config, userUnit)`: the configured measure as it is,
/// else 1:1 in feet (`'imperial'`) or metres.
pub fn default_measure(
    config: &MeasurementConfig,
    user_unit: f64,
) -> Result<PdfMeasurement, Error> {
    match &config.default_scale {
        Some(DefaultScale::Measure(measure)) => Ok((**measure).clone()),
        Some(DefaultScale::Imperial) => measure_from_ratio_typed(1.0, 1.0, "ft", user_unit),
        _ => measure_from_ratio_typed(1.0, 1.0, "m", user_unit),
    }
}

/// `selectPageScale(viewports, crop, fallback, persistent)`: the owned
/// viewport wherever it is, else the one under the crop's centre, else the
/// fallback.
pub fn select_page_scale(
    viewports: &[MeasurementViewport],
    crop: PdfRect,
    fallback: &PdfMeasurement,
    persistent: bool,
) -> PageScale {
    let centre = PdfPoint { x: (crop.left + crop.right) / 2.0, y: (crop.top + crop.bottom) / 2.0 };
    let selected = viewports
        .iter()
        .find(|v| v.owned)
        .or_else(|| viewport_for_point(viewports, |v| v.bbox, centre));
    PageScale {
        measure: match selected {
            Some(v) => v.measure.clone(),
            None => Some(fallback.clone()),
        },
        source: match selected {
            Some(v) if v.owned => ScaleSource::Owned,
            Some(_) => ScaleSource::Foreign,
            None => ScaleSource::Default,
        },
        ready: true,
        persistent,
        error: None,
    }
}

/// `m.x[0]?.unit.trim()`, when it is a length unit.
fn basis_of(m: &PdfMeasurement) -> Result<f64, Error> {
    m.x.as_deref()
        .and_then(|x| x.first())
        .and_then(|f| metres(f.unit.trim()))
        .ok_or_else(|| range_error("This scale has an unknown base unit; calibrate it first"))
}

/// The precision a unit edit keeps: the last format's, or 100 after a
/// fractional one.
fn kept_precision(formats: Option<&Vec<PdfNumberFormat>>) -> f64 {
    let previous = formats.and_then(|f| f.last());
    match previous {
        Some(f) if f.fraction == Some(MeasureFraction::Fraction) => 100.0,
        Some(f) => f.precision.unwrap_or(100.0),
        None => 100.0,
    }
}

fn decimal(unit: String, conversion: f64, precision: f64) -> PdfNumberFormat {
    PdfNumberFormat {
        unit,
        conversion: Some(conversion),
        fraction: Some(MeasureFraction::Decimal),
        precision: Some(precision),
        fixed: None,
        thousands: None,
        decimal: None,
        prefix_spacing: None,
        suffix_spacing: None,
        label_position: None,
    }
}

/// `withUnit(m, unit, area = squareOf(unit))`: one decimal distance format
/// in `unit` and one area format in `area`, both from the base unit.
pub fn with_unit(
    m: &PdfMeasurement,
    unit: &str,
    area: Option<&str>,
) -> Result<PdfMeasurement, Error> {
    let basis = basis_of(m)?;
    let area = area.map_or_else(|| square_of(unit), str::to_string);
    let precision = kept_precision(m.distance.as_ref());
    let to = metres(unit).unwrap_or(f64::NAN);
    Ok(PdfMeasurement {
        distance: Some(vec![decimal(unit.to_string(), basis / to, precision)]),
        area: Some(vec![decimal(
            area_unit_label(&area),
            basis.powi(2) / square_metres(&area),
            precision,
        )]),
        ..m.clone()
    })
}

/// `withPrecision(m, precision)`: the last distance and area formats become
/// decimal at `precision` (1, 10, 100 ... 10^9).
pub fn with_precision(m: &PdfMeasurement, precision: f64) -> Result<PdfMeasurement, Error> {
    let log = precision.log10();
    if !(log.is_finite() && log.trunc() == log) || !(1.0..=1e9).contains(&precision) {
        return Err(range_error("Invalid decimal precision"));
    }
    let update = |formats: &Option<Vec<PdfNumberFormat>>| {
        formats.as_ref().map(|formats| {
            let last = formats.len().wrapping_sub(1);
            formats
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    if i == last {
                        PdfNumberFormat {
                            precision: Some(precision),
                            fraction: Some(MeasureFraction::Decimal),
                            ..f.clone()
                        }
                    } else {
                        f.clone()
                    }
                })
                .collect()
        })
    };
    Ok(PdfMeasurement { distance: update(&m.distance), area: update(&m.area), ..m.clone() })
}

/// `withAreaUnit(measure, unit)`: change the area display unit, keeping the
/// distance formats.
pub fn with_area_unit(measure: &PdfMeasurement, unit: &str) -> Result<PdfMeasurement, Error> {
    let basis = basis_of(measure)?;
    let precision = kept_precision(measure.area.as_ref());
    Ok(PdfMeasurement {
        area: Some(vec![decimal(
            area_unit_label(unit),
            basis.powi(2) / square_metres(unit),
            precision,
        )]),
        ..measure.clone()
    })
}
