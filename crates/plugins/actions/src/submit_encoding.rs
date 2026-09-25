//! `submit-encoding.ts`: a resolved submission dataset as
//! `application/x-www-form-urlencoded`, for embedder handlers. The plugin
//! itself never calls it.

use epdf_acrojs::js::{encode_uri_component, string_of};
use serde_json::Value;

use crate::types::FormSubmissionEntry;

/// A name-only entry (`value` null) encodes as a bare name; a multi-select
/// repeats the name per value, the HTML-form convention.
pub fn submit_entries_to_url_encoded(entries: &[FormSubmissionEntry]) -> String {
    entries
        .iter()
        .flat_map(|entry| {
            let name = encode_uri_component(&entry.name);
            match &entry.value {
                Value::Null => vec![name],
                Value::Array(items) => items
                    .iter()
                    .map(|item| format!("{name}={}", encode_uri_component(&string_of(item))))
                    .collect(),
                value => vec![format!("{name}={}", encode_uri_component(&string_of(value)))],
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}
