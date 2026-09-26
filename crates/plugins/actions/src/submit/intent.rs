//! `submit/intent.ts`: the one normalized submit intent, from either source
//! (an action node's payload or a script `doc.submitForm()` effect), and the
//! engine request shape the document's home receives. Pure.

use epdf_acrojs::ScriptUiEffect;
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

/// `toFormSubmissionRequest(request, clientTimeMs)`.
pub fn to_form_submission_request(request: &ActionSubmitRequest, client_time_ms: f64) -> Value {
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
        "clientTimeMs": client_time_ms,
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
