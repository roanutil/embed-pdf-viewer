//! `types.ts`: the JSON contract between a host and its script sandbox, and
//! the constants that document the shipped posture.
//!
//! Engine DTOs the bridge only carries (a field ref, an annotation ref, a form
//! effect, an action tree) stay JSON. The shapes the host builds or reads are
//! typed and serialize to exactly the JSON the TypeScript builds: an optional
//! member the TypeScript spreads in only when present is skipped when `None`.

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// `FormFieldRef`, carried as the engine wrote it.
pub type FormFieldRef = Value;
/// `AnnotationRef`, carried as the engine wrote it.
pub type AnnotationRef = Value;
/// `FormEffect`: one document write a script staged, for the form sink.
pub type FormEffect = Value;
/// `FormSnapshot`, as `doc.forms.list()` answers it.
pub type FormSnapshot = Value;
/// `PdfActionTree`, as the engine extracted it.
pub type PdfActionTree = Value;
/// `PdfActionNode`.
pub type PdfActionNode = Value;
/// `ScriptValue`: `string | number | boolean | null | string[]`.
pub type ScriptValue = Value;

/// An optional member whose `null` is a value, not absence: `None` only when
/// the member is missing.
pub fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptFieldOption {
    pub label: String,
    pub value: String,
}

/// Detached field state supplied to one script transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptFieldInput {
    #[serde(rename = "ref")]
    pub field_ref: FormFieldRef,
    pub name: String,
    pub family: String,
    /// `None` where the TypeScript's value is `undefined` (an entry kind it
    /// does not know): the member is then absent.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub value: Option<ScriptValue>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<ScriptValue>,
    pub display: String,
    pub read_only: bool,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ScriptFieldOption>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptIdentity {
    pub name: String,
    pub login_name: String,
    pub corporation: String,
    pub email: String,
}

/// `Partial<ScriptIdentity>`. A `null` member is absent, as `??` reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialScriptIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corporation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDocumentInput {
    pub id: String,
    pub file_name: String,
    pub page_count: f64,
    /// Zero-based current page, matching Acrobat `this.pageNum`.
    pub page_number: f64,
}

/// Required deterministic environment. `utc_offset_minutes` is local minus UTC.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEnvironment {
    pub now_ms: f64,
    pub utc_offset_minutes: f64,
    pub random_seed: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptEventKind {
    NameTreeBoot,
    WidgetActivate,
    FieldKeystroke,
    FieldValidate,
    FieldCalculate,
    FieldFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEventInput {
    pub kind: ScriptEventKind,
    /// Acrobat `event.type` for action-driven runs (`Field`, `Page`, `Doc`,
    /// `Link`, and the EmbedPDF extension `Annot`).
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub target: Option<FormFieldRef>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub source: Option<FormFieldRef>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub value: Option<ScriptValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sel_start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sel_end: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_commit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,
}

impl ScriptEventInput {
    /// An event with only its kind.
    pub fn of(kind: ScriptEventKind) -> Self {
        ScriptEventInput {
            kind,
            event_type: None,
            name: None,
            target: None,
            source: None,
            value: None,
            change: None,
            sel_start: None,
            sel_end: None,
            will_commit: None,
            modifier: None,
            shift: None,
        }
    }
}

/// An Acrobat color: `['T']`, `['G', g]`, `['RGB', r, g, b]`,
/// `['CMYK', c, m, y, k]`, components 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScriptColorArray {
    T,
    G(f64),
    Rgb(f64, f64, f64),
    Cmyk(f64, f64, f64, f64),
}

impl Serialize for ScriptColorArray {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (space, parts): (&str, Vec<f64>) = match *self {
            ScriptColorArray::T => ("T", vec![]),
            ScriptColorArray::G(g) => ("G", vec![g]),
            ScriptColorArray::Rgb(r, g, b) => ("RGB", vec![r, g, b]),
            ScriptColorArray::Cmyk(c, m, y, k) => ("CMYK", vec![c, m, y, k]),
        };
        let mut seq = s.serialize_seq(Some(parts.len() + 1))?;
        seq.serialize_element(space)?;
        for part in parts {
            seq.serialize_element(&part)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for ScriptColorArray {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ArrayVisitor;
        impl<'de> Visitor<'de> for ArrayVisitor {
            type Value = ScriptColorArray;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an Acrobat color array")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<ScriptColorArray, A::Error> {
                let space: String =
                    seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let mut parts = Vec::new();
                while let Some(part) = seq.next_element::<f64>()? {
                    parts.push(part);
                }
                ScriptColorArray::from_parts(&space, &parts)
                    .ok_or_else(|| de::Error::custom(format!("not an Acrobat color: {space}")))
            }
        }
        d.deserialize_seq(ArrayVisitor)
    }
}

impl ScriptColorArray {
    pub fn from_parts(space: &str, parts: &[f64]) -> Option<Self> {
        Some(match (space, parts) {
            ("T", []) => ScriptColorArray::T,
            ("G", [g]) => ScriptColorArray::G(*g),
            ("RGB", [r, g, b]) => ScriptColorArray::Rgb(*r, *g, *b),
            ("CMYK", [c, m, y, k]) => ScriptColorArray::Cmyk(*c, *m, *y, *k),
            _ => return None,
        })
    }
}

/// One script-addressable annotation in the prefetched world: every subtype
/// except link and widget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptAnnotInput {
    #[serde(rename = "ref")]
    pub annot_ref: AnnotationRef,
    /// `/NM`, the `getAnnot(nPage, name)` key; empty when the PDF has none.
    pub name: String,
    pub subtype: String,
    pub page: f64,
    /// PDF-space `[llx, lly, urx, ury]`.
    pub rect: [f64; 4],
    pub contents: String,
    pub author: String,
    pub subject: String,
    pub stroke_color: ScriptColorArray,
    pub fill_color: ScriptColorArray,
    pub opacity: f64,
    pub width: f64,
    /// `'S'` solid or `'D'` dashed.
    pub border_style: String,
    pub dash: Vec<f64>,
    pub hidden: bool,
    pub print: bool,
    pub read_only: bool,
    pub locked: bool,
    pub no_view: bool,
    pub toggle_no_view: bool,
    pub opaque_body: bool,
}

/// The curated write patch one script staged for one annotation. The patch
/// is carried to the annotation sink as the sandbox wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptAnnotEffect {
    #[serde(rename = "ref")]
    pub annot_ref: AnnotationRef,
    pub patch: Value,
}

/// The writable-property matrix, keyed by engine subtype. The prelude inlines
/// an identical copy.
pub const ANNOT_WRITABLE_KEYS: &[(&str, &[&str])] = &[
    ("square", &["strokeColor", "fillColor", "opacity", "width", "borderStyle", "dash", "rect"]),
    ("circle", &["strokeColor", "fillColor", "opacity", "width", "borderStyle", "dash", "rect"]),
    ("polygon", &["strokeColor", "fillColor", "opacity", "width", "borderStyle", "dash", "rect"]),
    ("polyline", &["strokeColor", "fillColor", "opacity", "width", "borderStyle", "dash", "rect"]),
    ("line", &["strokeColor", "fillColor", "opacity", "width", "borderStyle", "dash", "rect"]),
    ("ink", &["strokeColor", "opacity", "width", "rect"]),
    ("free-text", &["strokeColor", "fillColor", "opacity", "rect"]),
    ("highlight", &["strokeColor", "opacity"]),
    ("underline", &["strokeColor", "opacity"]),
    ("squiggly", &["strokeColor", "opacity"]),
    ("strikeout", &["strokeColor", "opacity"]),
    ("caret", &["strokeColor", "opacity"]),
    ("text", &["strokeColor", "opacity", "rect"]),
    ("stamp", &["rect"]),
    ("file-attachment", &["strokeColor", "opacity"]),
];

/// `ANNOT_WRITABLE_KEYS[subtype]`.
pub fn annot_writable_keys(subtype: &str) -> Option<&'static [&'static str]> {
    ANNOT_WRITABLE_KEYS.iter().find(|(s, _)| *s == subtype).map(|(_, keys)| *keys)
}

/// Complete input for one deterministic run. No ambient host values are read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptInput {
    pub document: ScriptDocumentInput,
    pub identity: ScriptIdentity,
    pub environment: ScriptEnvironment,
    pub fields: Vec<ScriptFieldInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annots: Option<Vec<ScriptAnnotInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annot_pages: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annots_cover_document: Option<bool>,
    pub event: ScriptEventInput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScriptUiEffect {
    Alert {
        message: String,
        icon: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    Print,
    GotoPage {
        page: f64,
    },
    /// `doc.submitForm(...)`: a submit intent, resolved and sink-routed
    /// outside the VM. `field_names` are include-mode; `None` is the whole
    /// eligible form.
    #[serde(rename_all = "camelCase")]
    SubmitForm {
        url: Option<String>,
        field_names: Option<Vec<String>>,
        include_empty: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method: Option<String>,
    },
}

impl ScriptUiEffect {
    /// `effect.kind`.
    pub fn kind(&self) -> &'static str {
        match self {
            ScriptUiEffect::Alert { .. } => "alert",
            ScriptUiEffect::Print => "print",
            ScriptUiEffect::GotoPage { .. } => "gotoPage",
            ScriptUiEffect::SubmitForm { .. } => "submitForm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptDiagnosticCode {
    BlockedNetwork,
    UnsupportedApi,
    InvalidFieldValue,
    ScriptError,
    UiEffectSuppressed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptDiagnostic {
    pub code: ScriptDiagnosticCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptExecutionErrorKind {
    Exception,
    Budget,
    InvalidOutput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptExecutionError {
    pub kind: ScriptExecutionErrorKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEventOutput {
    pub rc: bool,
    pub value: ScriptValue,
    pub change: String,
    pub sel_start: f64,
    pub sel_end: f64,
}

/// What one boot or run produced. Effects are committed by the originating
/// client only, form effects first, then annotation effects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptOutput {
    pub event: ScriptEventOutput,
    pub form_effects: Vec<FormEffect>,
    pub annot_effects: Vec<ScriptAnnotEffect>,
    pub ui_effects: Vec<ScriptUiEffect>,
    pub diagnostics: Vec<ScriptDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptExecutionError>,
}

impl ScriptOutput {
    /// An output with nothing in it: `rc` true, no effects, no diagnostics.
    pub fn empty() -> Self {
        ScriptOutput {
            event: ScriptEventOutput {
                rc: true,
                value: Value::Null,
                change: String::new(),
                sel_start: 0.0,
                sel_end: 0.0,
            },
            form_effects: Vec::new(),
            annot_effects: Vec::new(),
            ui_effects: Vec::new(),
            diagnostics: Vec::new(),
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptBudget {
    pub max_execution_ms: f64,
    pub max_memory_bytes: f64,
    pub max_stack_bytes: f64,
    pub max_effects: f64,
    pub max_output_bytes: f64,
}

pub const DEFAULT_SCRIPT_BUDGET: ScriptBudget = ScriptBudget {
    max_execution_ms: 50.0,
    max_memory_bytes: 16.0 * 1024.0 * 1024.0,
    max_stack_bytes: 512.0 * 1024.0,
    max_effects: 256.0,
    max_output_bytes: 1024.0 * 1024.0,
};

/// The shipped security posture. These constants document it; the actions
/// plugin's policy enforces it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSecurityPolicy {
    pub enabled: bool,
    pub execution_owner: &'static str,
    pub name_tree_boot: &'static str,
    pub submit_form: &'static str,
    pub open_action: &'static str,
    pub page_actions: &'static str,
    pub catalog_lifecycle_actions: &'static str,
    pub annotation_actions: &'static str,
}

pub const DEFAULT_SCRIPT_SECURITY_POLICY: ScriptSecurityPolicy = ScriptSecurityPolicy {
    enabled: false,
    execution_owner: "originating-client-only",
    name_tree_boot: "lazy-first-transaction",
    submit_form: "sink-chain",
    open_action: "execute-lifecycle",
    page_actions: "execute-lifecycle",
    catalog_lifecycle_actions: "execute-on-verb",
    annotation_actions: "execute-full-matrix",
};

/// `SCRIPT_EVENT_MATRIX`: the shipped dispatch matrix.
pub fn script_event_matrix() -> Value {
    json!({
        "nameTree": "lazy-first-originating-field-transaction",
        "field": {
            "keystroke": "execute-on-commit",
            "validate": "execute",
            "calculate": "execute-in-calculation-order",
            "format": "execute-after-value-calculation",
        },
        "openAction": "execute-lifecycle",
        "page": "execute-lifecycle",
        "catalogLifecycle": "execute-on-verb",
        "annotation": {
            "widgetActivate": "execute-on-originating-client",
            "other": "execute-full-matrix",
        },
        "submitForm": "sink-chain",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_writable_keys_by_subtype() {
        assert_eq!(annot_writable_keys("stamp"), Some(&["rect"][..]));
        assert_eq!(annot_writable_keys("ink").map(<[_]>::len), Some(4));
        assert_eq!(annot_writable_keys("link"), None);
    }

    #[test]
    fn names_each_ui_effect_kind() {
        let submit = ScriptUiEffect::SubmitForm {
            url: None,
            field_names: None,
            include_empty: false,
            format: None,
            method: None,
        };
        for effect in [
            ScriptUiEffect::Alert { message: String::new(), icon: 0.0, title: None },
            ScriptUiEffect::Print,
            ScriptUiEffect::GotoPage { page: 0.0 },
            submit,
        ] {
            assert_eq!(serde_json::to_value(&effect).unwrap()["kind"], effect.kind());
        }
    }
}
