//! `field-selection.ts`: ISO field selection, written once for ResetForm
//! (ISO 32000 Tables 241/242) and SubmitForm (Tables 239/240), and the pure
//! submission dataset builder.

use epdf_plugin_actions::{
    ActionDiagnostic, ActionDiagnosticCode, FormSubmissionEntry, PdfActionTargetRef, SubmitIntent,
};
use serde_json::Value;

use crate::value::{get, num, str_of, string_of, truthy};

/// What `resolveFieldSelection` resolves: the selection, and which fields
/// the target list addressed (directly or through a subtree).
pub struct FieldSelection {
    names: Vec<String>,
    /// `None` is a target without an object number (`undefined`).
    object_numbers: Vec<Option<f64>>,
    /// `targets === null`: nothing is ever listed.
    everything: bool,
    pub selected: Vec<Value>,
}

impl FieldSelection {
    /// `listed(field)`.
    pub fn listed(&self, field: &Value) -> bool {
        if self.everything {
            return false;
        }
        let r = get(field, "ref");
        let by_number = r.is_some_and(|r| {
            let number = num(r, "fieldObjectNumber");
            str_of(r, "kind") == Some("objectNumber") && self.object_numbers.contains(&number)
        });
        by_number || {
            let name = get(field, "name");
            self.names.iter().any(|target| match name.and_then(Value::as_str) {
                Some(name) => name == target || name.starts_with(&format!("{target}.")),
                None => false,
            })
        }
    }
}

/// `resolveFieldSelection(all, targets, exclude)`. `targets == None` (the key
/// absent) selects everything and ignores `exclude`; `[]` with include
/// selects nothing, `[]` with exclude everything.
pub fn resolve_field_selection(
    all: &[Value],
    targets: Option<&[PdfActionTargetRef]>,
    exclude: bool,
) -> FieldSelection {
    let Some(targets) = targets else {
        return FieldSelection {
            names: Vec::new(),
            object_numbers: Vec::new(),
            everything: true,
            selected: all.to_vec(),
        };
    };
    let names = targets
        .iter()
        .filter(|t| str_of(t, "kind") == Some("name"))
        .map(|t| string_of(get(t, "name")))
        .collect();
    let object_numbers = targets
        .iter()
        .filter(|t| str_of(t, "kind") == Some("objectNumber"))
        .map(|t| num(t, "objectNumber"))
        .collect();
    let mut selection =
        FieldSelection { names, object_numbers, everything: false, selected: Vec::new() };
    selection.selected = all.iter().filter(|f| selection.listed(f) != exclude).cloned().collect();
    selection
}

/// `buildSubmitEntries(fields, intent, diagnose)`: the submission dataset
/// from a live field list (NoExport veto, buttons and signatures never
/// contribute, an unsupported `/V` is always diagnosed, a valueless field is
/// name-only under IncludeNoValueFields).
pub fn build_submit_entries(
    fields: &[Value],
    intent: &SubmitIntent,
    diagnose: &dyn Fn(ActionDiagnostic),
) -> Vec<FormSubmissionEntry> {
    let selection = resolve_field_selection(fields, intent.fields.as_deref(), intent.exclude);
    let explicit_include = intent.fields.is_some() && !intent.exclude;
    let unsupported = |message: String| {
        diagnose(ActionDiagnostic::new(ActionDiagnosticCode::SubmitEntryUnsupported, message))
    };
    let mut entries = Vec::new();
    for field in &selection.selected {
        let explicit = explicit_include && selection.listed(field);
        let name = string_of(get(field, "name"));
        let family = str_of(field, "family");
        if family == Some("pushbutton") || family == Some("signature") {
            if explicit {
                unsupported(format!(
                    "submit: '{name}' ({}) has no representable submission value in v1 — skipped",
                    family.unwrap_or_default()
                ));
            }
            continue;
        }
        if truthy(get(field, "flags").and_then(|f| get(f, "noExport"))) {
            if explicit {
                unsupported(format!(
                    "submit: '{name}' carries the NoExport flag — the veto is unconditional (ISO 12.7.6.2), skipped"
                ));
            }
            continue;
        }
        let entry = get(field, "valueEntry");
        let kind = entry.and_then(|e| str_of(e, "kind"));
        let member = |key: &str| entry.and_then(|e| get(e, key)).cloned().unwrap_or(Value::Null);
        match kind {
            Some("scalar") => entries.push(FormSubmissionEntry { name, value: member("value") }),
            Some("array") => entries.push(FormSubmissionEntry { name, value: member("values") }),
            Some("none") => {
                if intent.include_no_value_fields {
                    entries.push(FormSubmissionEntry { name, value: Value::Null });
                }
            }
            _ => unsupported(format!(
                "submit: '{name}' has an unsupported /V shape — skipped (would otherwise be silent data loss)"
            )),
        }
    }
    entries
}
