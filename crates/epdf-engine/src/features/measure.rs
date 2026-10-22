//! `measure.viewports` and `measure.setScale`: a page's measurement
//! viewports (`/VP`), and EmbedPDF's full-page calibration among them.
//!
//! A transcription of `packages/engine/services/src/features/measure/`:
//! `MeasureReader.ts`, `MeasureMutator.ts`, `internal/readViewports.ts` and
//! `internal/measureCodec.ts`, function for function, plus
//! `packages/engine/core/src/measure/validate.ts` (`assertWritableMeasure`)
//! and the `validNumberFormat` it calls. Other producers' viewports survive
//! every write; `/VP` itself is never rewritten.
//!
//! [`derive`] is the label a dimension annotation's points and scale work
//! out (`core/src/measure/compute.ts`, `format.ts`, `derive.ts`).

pub mod derive;

use pdfium::measure::{Measure, MeasureAxis, MeasureSubtype, MeasureText, Viewport};
use pdfium::{Document, PageView, PointF, RectF};
use serde_json::{Map, Value, json};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};

/// `readViewports.ts` `CALIBRATION_NAME`.
pub const CALIBRATION_NAME: &str = "EmbedPDF";

/// `MEASURE_AXES`, in its declaration order (the order a write appends in).
const MEASURE_AXES: [(&str, MeasureAxis); 6] = [
    ("x", MeasureAxis::X),
    ("y", MeasureAxis::Y),
    ("distance", MeasureAxis::Distance),
    ("area", MeasureAxis::Area),
    ("angle", MeasureAxis::Angle),
    ("slope", MeasureAxis::Slope),
];
const FRACTIONS: [&str; 4] = ["decimal", "fraction", "round", "truncate"];
const TEXT: [(MeasureText, &str); 4] = [
    (MeasureText::ThousandsSeparator, "thousands"),
    (MeasureText::DecimalSeparator, "decimal"),
    (MeasureText::PrefixSpacing, "prefixSpacing"),
    (MeasureText::SuffixSpacing, "suffixSpacing"),
];

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

/// `requireMeasureWrite`: a native rejection after validated input is an
/// internal invariant, never caller validation.
pub fn require_measure_write(ok: bool) -> Result<(), EngineError> {
    if ok {
        Ok(())
    } else {
        Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "Measurement native write failed after preflight",
        ))
    }
}

fn text(read: Result<Option<String>, pdfium::PdfiumError>) -> Result<Option<String>, EngineError> {
    read.map_err(|e| EngineError::with_message(EngineErrorCode::Unknown, e.to_string()))
}

/// `normalizePdfRect(readRectF(...))`, in the wire's `PdfRect` spelling.
fn normalized(r: RectF) -> Value {
    let (l, r_, t, b) =
        (f64::from(r.left), f64::from(r.right), f64::from(r.top), f64::from(r.bottom));
    json!({ "left": l.min(r_), "right": l.max(r_), "bottom": t.min(b), "top": t.max(b) })
}

// ── MeasureReader / readViewports ───────────────────────────────────────────

/// `MeasureReader.viewports` over `readViewports`: every viewport in
/// drawing order, in the file's coordinates, a malformed one or one whose
/// `/BBox` doesn't read skipped. A viewport with no name or no measure
/// reads `null` for it.
pub fn read_viewports(
    page: &PageView<'_>,
    cancel: &CancelToken,
) -> Result<Vec<Value>, EngineError> {
    cancel.check()?;
    let mut viewports = Vec::new();
    for i in 0..page.viewport_count() {
        let Some(viewport) = page.viewport(i) else { continue };
        let Some(bbox) = viewport.bbox() else { continue };
        let name = text(viewport.name())?;
        let measure = read_measure(viewport.measure())?;
        let owned = name.as_deref() == Some(CALIBRATION_NAME)
            && measure.as_ref().is_none_or(|m| m["subtype"] == "rectilinear");
        viewports.push(json!({
            "bbox": normalized(bbox),
            "owned": owned,
            "name": name,
            "measure": measure,
        }));
    }
    Ok(viewports)
}

// ── measureCodec ────────────────────────────────────────────────────────────

/// `readMeasure`: a foreign measure is its subtype name and nothing else
/// (`geospatial` or `unknown`); a `rectilinear` one always carries `x`, `distance` and `area` (possibly empty)
/// and `y`, `angle`, `slope` only when non-empty.
pub fn read_measure(handle: Option<Measure<'_>>) -> Result<Option<Value>, EngineError> {
    let Some(m) = handle else { return Ok(None) };
    match m.subtype() {
        MeasureSubtype::Rl => {}
        MeasureSubtype::Geo => return Ok(Some(json!({ "subtype": "geospatial" }))),
        MeasureSubtype::Unknown => return Ok(Some(json!({ "subtype": "unknown" }))),
    }
    let formats = |axis: MeasureAxis| -> Result<Vec<Value>, EngineError> {
        let mut list = Vec::new();
        for i in 0..m.format_count(axis) {
            // `EPDFMeasure_GetFormat` on an index below the count; a null
            // here is a malformed entry, read as the TypeScript reads a
            // null handle: every getter fails, leaving only the unit `''`.
            let Some(f) = m.format(axis, i) else {
                list.push(json!({ "unit": "" }));
                continue;
            };
            let mut out = Map::new();
            out.insert("unit".into(), text(f.unit())?.unwrap_or_default().into());
            if let Some(conversion) = f.conversion() {
                out.insert("conversion".into(), f64::from(conversion).into());
            }
            if let Some(fraction) = f.fraction().and_then(|c| FRACTIONS.get(c as usize)) {
                out.insert("fraction".into(), (*fraction).into());
            }
            if let Some(precision) = f.precision() {
                out.insert("precision".into(), precision.into());
            }
            if let Some(fixed) = f.fixed_denominator() {
                out.insert("fixed".into(), fixed.into());
            }
            if let Some(position) = f.label_position() {
                out.insert(
                    "labelPosition".into(),
                    if position == 1 { "prefix" } else { "suffix" }.into(),
                );
            }
            for (code, key) in TEXT {
                if let Some(value) = text(f.text(code))? {
                    out.insert(key.into(), value.into());
                }
            }
            list.push(Value::Object(out));
        }
        Ok(list)
    };
    let ratio = text(m.ratio())?;
    let mut result = Map::new();
    result.insert("subtype".into(), "rectilinear".into());
    result.insert("x".into(), formats(MeasureAxis::X)?.into());
    result.insert("distance".into(), formats(MeasureAxis::Distance)?.into());
    result.insert("area".into(), formats(MeasureAxis::Area)?.into());
    if let Some(ratio) = ratio {
        result.insert("ratio".into(), ratio.into());
    }
    if let Some(origin) = m.origin() {
        result
            .insert("origin".into(), json!({ "x": f64::from(origin.x), "y": f64::from(origin.y) }));
    }
    if let Some(cyx) = m.cyx() {
        result.insert("cyx".into(), f64::from(cyx).into());
    }
    for (key, axis) in
        [("y", MeasureAxis::Y), ("angle", MeasureAxis::Angle), ("slope", MeasureAxis::Slope)]
    {
        let list = formats(axis)?;
        if !list.is_empty() {
            result.insert(key.into(), list.into());
        }
    }
    Ok(Some(Value::Object(result)))
}

/// `writeMeasure`: into a freshly created (reset, empty) measure, after
/// preflight. Every native refusal is `requireMeasureWrite`'s `Unknown`.
pub fn write_measure(handle: Option<Measure<'_>>, value: &Value) -> Result<(), EngineError> {
    let Some(m) = handle else { return require_measure_write(false) };
    if let Some(ratio) = value.get("ratio").and_then(Value::as_str) {
        require_measure_write(m.set_ratio(ratio))?;
    }
    if let Some(origin) = value.get("origin").filter(|o| o.is_object()) {
        let at = |k: &str| origin.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN) as f32;
        require_measure_write(m.set_origin(PointF { x: at("x"), y: at("y") }))?;
    }
    if let Some(cyx) = value.get("cyx").and_then(Value::as_f64) {
        require_measure_write(m.set_cyx(cyx as f32))?;
    }
    for (key, axis) in MEASURE_AXES {
        let Some(list) = value.get(key).and_then(Value::as_array) else { continue };
        for nf in list {
            let unit = nf.get("unit").and_then(Value::as_str).unwrap_or_default();
            let conversion = nf.get("conversion").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let Some(f) = m.add_format(axis, unit, conversion as f32) else {
                return require_measure_write(false);
            };
            if let Some(fraction) = nf.get("fraction").and_then(Value::as_str) {
                let code = FRACTIONS.iter().position(|f| *f == fraction).unwrap_or(usize::MAX);
                require_measure_write(f.set_fraction(u32::try_from(code).unwrap_or(u32::MAX)))?;
            }
            if let Some(precision) = nf.get("precision").and_then(Value::as_f64) {
                require_measure_write(f.set_precision(precision as i32))?;
            }
            if let Some(fixed) = nf.get("fixed").and_then(Value::as_bool) {
                require_measure_write(f.set_fixed_denominator(fixed))?;
            }
            if let Some(position) = nf.get("labelPosition").and_then(Value::as_str) {
                require_measure_write(f.set_label_position(u32::from(position == "prefix")))?;
            }
            for (code, key) in TEXT {
                if let Some(value) = nf.get(key).and_then(Value::as_str) {
                    require_measure_write(f.set_text(code, value))?;
                }
            }
        }
    }
    Ok(())
}

// ── validate.ts ─────────────────────────────────────────────────────────────

/// `Math.fround`.
fn fround(n: f64) -> f64 {
    f64::from(n as f32)
}

/// A member read the way `typeof x === 'number'` reads it: absent is
/// `None`, a non-number is `NaN`.
fn num_member(nf: &Value, key: &str) -> Option<f64> {
    match nf.get(key) {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().unwrap_or(f64::NAN)),
    }
}

fn is_integer(n: f64) -> bool {
    n.is_finite() && n.trunc() == n
}

/// `validNumberFormat` (`packages/engine/core/src/measure/format.ts`).
fn valid_number_format(nf: &Value) -> bool {
    let c = fround(num_member(nf, "conversion").unwrap_or(f64::NAN));
    if !c.is_finite() || c <= 0.0 {
        return false;
    }
    let fraction = nf.get("fraction").filter(|f| !f.is_null());
    let d = num_member(nf, "precision")
        .unwrap_or(if fraction.and_then(Value::as_str) == Some("fraction") { 16.0 } else { 100.0 });
    if !is_integer(d) || !(1.0..=2147483647.0).contains(&d) {
        return false;
    }
    let decimal = fraction.is_none_or(|f| f.as_str() == Some("decimal"));
    !(decimal && !is_integer(d.log10()))
}

fn text_ok(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(Value::String(s)) => !s.contains('\0'),
        Some(_) => false,
    }
}

/// `assertPdfFloat`.
pub fn assert_pdf_float(n: Option<&Value>) -> Result<(), String> {
    match n.and_then(Value::as_f64) {
        Some(n) if fround(n).is_finite() => Ok(()),
        _ => Err("Value is outside PDF float range".into()),
    }
}

/// `assertWritableMeasure`. `Err` is the `RangeError`'s message, which
/// `MeasureMutator` rethrows as `InvalidArg`.
pub fn assert_writable_measure(value: &Value) -> Result<(), String> {
    if value.get("subtype").and_then(Value::as_str) != Some("rectilinear") {
        return Err("Only rectilinear measures can be authored".into());
    }
    if let Some(ratio) = value.get("ratio")
        && !matches!(ratio, Value::String(s) if !s.contains('\0'))
    {
        return Err("Invalid scale ratio".into());
    }
    for (key, _) in MEASURE_AXES {
        let list = value.get(key);
        if list.is_none() && matches!(key, "y" | "angle" | "slope") {
            continue;
        }
        let Some(Value::Array(list)) = list else {
            return Err(format!("Missing {key} formats"));
        };
        for nf in list {
            let usable = nf.is_object()
                && nf.get("unit").is_some_and(Value::is_string)
                && valid_number_format(nf);
            if !usable {
                return Err("Invalid number format".into());
            }
            if let Some(fraction) = nf.get("fraction")
                && !matches!(fraction.as_str(), Some("decimal" | "fraction" | "round" | "truncate"))
            {
                return Err("Invalid fraction mode".into());
            }
            if let Some(position) = nf.get("labelPosition")
                && !matches!(position.as_str(), Some("prefix" | "suffix"))
            {
                return Err("Invalid label position".into());
            }
            if let Some(fixed) = nf.get("fixed")
                && !fixed.is_boolean()
            {
                return Err("Invalid fixed precision".into());
            }
            for text in ["unit", "thousands", "decimal", "prefixSpacing", "suffixSpacing"] {
                if !text_ok(nf.get(text)) {
                    return Err("Invalid measurement text".into());
                }
            }
        }
    }
    if let Some(origin) = value.get("origin") {
        assert_pdf_float(origin.get("x"))?;
        assert_pdf_float(origin.get("y"))?;
    }
    if let Some(cyx) = value.get("cyx") {
        assert_pdf_float(Some(cyx))?;
        if fround(cyx.as_f64().unwrap_or(f64::NAN)) <= 0.0 {
            return Err("Invalid CYX".into());
        }
    }
    Ok(())
}

// ── MeasureMutator ──────────────────────────────────────────────────────────

/// Whether the viewport is one `setScale` owns: named `EmbedPDF`, with no
/// measure or a rectilinear one. Read as `MeasureMutator` reads it, which is not
/// `readViewports`: the `/BBox` doesn't matter here.
fn is_owned(viewport: &Viewport<'_>) -> Result<bool, EngineError> {
    if text(viewport.name())?.as_deref() != Some(CALIBRATION_NAME) {
        return Ok(false);
    }
    Ok(viewport.measure().is_none_or(|m| m.subtype() == MeasureSubtype::Rl))
}

/// `MeasureMutator.setScale`. `measure` is `None` for the wire's `null`:
/// remove every owned viewport, highest index first. Otherwise update the
/// last owned viewport, or append one over the visible page, and write the
/// measure into a fresh `/Measure`.
///
/// The visible page is `pageBoxesOf`'s crop box (`readBoxes(...).crop`,
/// `MeasureMutator.ts:52-56`): US Letter when the page has no media box,
/// and the crop box cut to the media box. `document` and `index` are the
/// page's, for that read.
pub fn set_scale(
    document: &Document,
    index: u32,
    page: &PageView<'_>,
    measure: Option<&Value>,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    cancel.check()?;
    if let Some(measure) = measure {
        assert_writable_measure(measure).map_err(invalid)?;
    }
    let mut owned = Vec::new();
    for i in 0..page.viewport_count() {
        let Some(viewport) = page.viewport(i) else { continue };
        if is_owned(&viewport)? {
            owned.push(i);
        }
    }
    // `MeasureMutator`: the page's visible box (`readBoxes(...).crop`).
    let crop = crate::features::pages::read_boxes(document, index).crop;
    let bbox = RectF {
        left: crop.left as f32,
        right: crop.right as f32,
        bottom: crop.bottom as f32,
        top: crop.top as f32,
    };
    if ![bbox.left, bbox.right, bbox.bottom, bbox.top].iter().all(|v| v.is_finite()) {
        return Err(invalid("Invalid calibration box"));
    }
    cancel.check()?;
    // Apply boundary. Descending removals keep original indices valid;
    // never rewrite /VP.
    let Some(measure) = measure else {
        for i in owned.into_iter().rev() {
            require_measure_write(page.remove_viewport(i))?;
        }
        return Ok(());
    };
    let viewport = match owned.last() {
        Some(&i) => page.viewport(i),
        None => page.add_viewport(bbox),
    };
    let Some(viewport) = viewport else { return require_measure_write(false) };
    require_measure_write(viewport.set_bbox(bbox))?;
    require_measure_write(viewport.set_name(CALIBRATION_NAME))?;
    write_measure(viewport.add_measure(), measure)
}
