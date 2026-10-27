//! `submit/intent.ts`: the one normalized submit intent, from either source
//! (an action node's payload or a script `doc.submitForm()` effect), and the
//! engine request shape the document's home receives. Pure.

use epdf_acrojs::ScriptUiEffect;
use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::contract::{ActionSubmitRequest, SubmitIntent};
use crate::services::{get, str_of};

/// `intentOfPayload(payload)`.
pub fn intent_of_payload(payload: &Value) -> SubmitIntent {
    let flags = get(payload, "flags").cloned().unwrap_or(Value::Null);
    SubmitIntent {
        url: get(payload, "url").and_then(Value::as_str).map(str::to_string),
        fields: get(payload, "fields").and_then(Value::as_array).cloned(),
        exclude: get(&flags, "exclude").and_then(Value::as_bool).unwrap_or(false),
        include_no_value_fields: get(&flags, "includeNoValueFields")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        format: str_of(&flags, "format").to_string(),
        method: str_of(&flags, "method").to_string(),
        flags_raw: get(&flags, "raw").and_then(Value::as_f64).unwrap_or(0.0),
        char_set: get(payload, "charSet").and_then(Value::as_str).map(str::to_string),
    }
}

/// `new Date(ms).toISOString()`: a `RangeError` for a time out of range.
pub fn iso_string(ms: f64) -> Result<String, Error> {
    const MAX: f64 = 8.64e15;
    if !ms.is_finite() || ms.abs() > MAX {
        return Err(Error::named("RangeError", "Invalid time value"));
    }
    let ms = ms.trunc() as i64;
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{}{:06}", if year < 0 { '-' } else { '+' }, year.abs())
    };
    Ok(format!(
        "{year}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    ))
}

/// `toFormSubmissionRequest(request, sentAt)`.
pub fn to_form_submission_request(request: &ActionSubmitRequest, sent_at: &str) -> Value {
    let mut intent = json!({
        "url": request.url,
        "format": request.format,
        "method": request.method,
        "flagsRaw": request.flags_raw,
    });
    if let Some(char_set) = &request.char_set {
        intent["charSet"] = json!(char_set);
    }
    json!({
        "entries": request.entries,
        "intent": intent,
        "origin": request.origin,
        "sentAt": sent_at,
    })
}

/// `intentOfSubmitEffect(effect)`: script field names are include-mode.
/// `None` for an effect that is not a submit.
pub fn intent_of_submit_effect(effect: &ScriptUiEffect) -> Option<SubmitIntent> {
    let ScriptUiEffect::SubmitForm { url, field_names, include_empty, format, method } = effect
    else {
        return None;
    };
    Some(SubmitIntent {
        url: url.clone(),
        fields: field_names.as_ref().map(|names| {
            names.iter().map(|name| json!({ "kind": "name", "name": name })).collect()
        }),
        exclude: false,
        include_no_value_fields: *include_empty,
        format: format.clone().unwrap_or_else(|| "fdf".into()),
        method: method.clone().unwrap_or_else(|| "post".into()),
        flags_raw: 0.0,
        char_set: None,
    })
}
