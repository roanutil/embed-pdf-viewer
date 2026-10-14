# Form Plugin Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export `epdf-plugin-form` through UniFFI as `FormHandle`, wire it into `ViewerHost` over the native engine's `forms.*` paths, and wrap it in Swift as `FormCapability`, so a Swift app can read a form, fill it, design fields, and move FDF and XFDF in and out.

**Architecture:** The recipe in `2026-10-01-swift-package.md` Appendix A §A.0, one handle. The plugin keeps the engine's form snapshot as `serde_json::Value` (`crates/plugins/form/src/lib.rs` module doc), so every record is a typed mirror decoded from that JSON, modelled on `epdf-engine`'s DTOs (`crates/epdf-engine/src/features/forms/snapshot.rs`). `FormFieldRef` and `FormFieldFamily` go into `crates/epdf-ffi/src/form.rs`, which this plan owns, because signature (plan 16) returns them. The native host spells a widget `{annotObjectNumber, pageObjectNumber}` where the plugin reads `{ref, annotObjectNumber, page}`, and spells a widget placement's page as a number where the plugin sends a `PageRef`, so `translate.rs` rewrites both, and FDF bytes travel as wire payloads.

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `serde_json`, Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md` (this plan's row, cross-plan names, shared records, template, definition of done) and `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.0 and §A.3, Appendix C). Read both.

## Global Constraints

Everything in the umbrella's Global Constraints applies. The ones this plugin hits hardest:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are workspace-unique. Every record here starts with `Form`.
- No `usize` or `serde_json::Value` across the boundary. The snapshot, fields, widgets, fill items and commit results are all `Value` in the plugin; each gets a typed mirror and a decode that refuses what it can't read (`invalid-state`).
- Text values and offsets are UTF-16 where the engine counts (`maxLength`). Swift's `String.count` is wrong for them.
- Requires plan 01 (`ViewerConfig.interaction`) and plan 05 (and through it plan 04, which wrote the `page.annotations.list` path, `flatten_refs`, `nest_refs` and `epdf_ffi::AnnotationRef`). The form plugin optionally uses annotation, but its widgets are addressed by `AnnotationRef` either way.
- `forms.submit` belongs to plan 15 (spec Appendix C). `actions.read` belongs to plan 15.

## Review Focus

1. A checkbox filled from Swift by its on-state, then read back. People expect `checked: true` on that widget and the field's export value to change. The engine addresses toggles by `/AP /N` key, not by export value, so passing the export value where they differ (`Yes` vs `On`) silently does nothing. Pinned in Task 2 (`checking_a_box_by_its_on_state_reports_applied`) and Task 5 (`aCheckboxIsCheckedByItsOnState`).
2. A field whose widgets the host reports flat. People expect `FormField.widgets[].annotationRef` to be a ref the annotation capability accepts. A missing rewrite gives the plugin widgets without `ref`, and every fill item comes back with `annotation_ref: None`. Pinned in Task 3 (`a_form_list_gives_each_widget_its_ref_and_page`) and Task 4 (`the_fixtures_fields_list_with_widget_refs`).
3. An FDF export imported back into a fresh copy of the document. People expect the values to round trip. The bytes cross three boundaries (engine payload, kernel JSON, UniFFI `Vec<u8>`), each a place to lose or double-encode them. Pinned in Task 4 (`an_fdf_export_imports_back`) and Task 5 (`anXfdfExportRoundTrips`).
4. A value write rejected by a validate script, or by a read-only field. People expect a `FormCommitResult` with status `rejected` and the diagnostics, not a thrown error. Pinned in Task 2 (`a_rejected_commit_is_a_result_not_an_error`).
5. A document without an `/AcroForm`. People expect an empty field list and `form_kind: none`, not an error. Pinned in Task 4 (`a_document_without_a_form_lists_no_fields`).

## Decisions

We're exporting 39 of the 45 members (`crates/plugins/form/src/harness.rs:54` `MEMBERS`). The six we leave out are hooks the actions plugin calls on the kernel thread, which `form_plugin` wires itself: `resetFormAction` (the `reset-form` executor), `resolveSubmitDataset` (the submit resolver), `commitScriptFormEffects` (the script commit sink), `notifyWidgetEvent` (a DOM focus and hover feed), `setValueRaw` (the engine passthrough that skips scripts, which a native app shouldn't reach for), and `exportValues` (a name-to-`Value` map; `fields` returns the same values typed).

`import_data` and `repair` return nothing. The engine's results carry a whole snapshot (`forms.import`) or a list of what the repair touched, and the plugin applies both to its model before resolving. Swift reads the outcome with `fields()`. The cost is that a caller who wants "which fields did the import change" has to diff two snapshots.

Bytes cross as `{"$bytes": base64}` inside kernel JSON (`crates/epdf-kernel/src/bytes.rs`), which is how the form plugin already passes them. An FDF is a few kilobytes, so the base64 round trip is cheap; a large XFDF with embedded files would pay roughly 2.7x its size in transient copies. `HostTable` would avoid that, but the plugin crate can't reach it, and threading a byte port through `FormHandle::new` the way render does for pixels isn't worth it at these sizes.

## File Structure

```
crates/epdf-ffi/src/form.rs                    FormFieldRef, FormFieldFamily
crates/epdf-ffi/src/lib.rs                     + pub mod form; re-exports
crates/epdf-ffi/tests/form.rs
crates/plugins/form/Cargo.toml                 + uniffi feature
crates/plugins/form/src/lib.rs                 + scaffolding, pub mod ffi
crates/plugins/form/src/controller.rs          + lifetime()
crates/plugins/form/src/ffi.rs                 FormHandle, records, FormEvent, FormEventListener
crates/plugins/form/tests/ffi.rs
crates/epdf-viewer-host/{Cargo.toml, src/api.rs, src/session.rs, src/lib.rs, src/translate.rs}
crates/epdf-viewer-host/tests/{translate.rs, viewer_host.rs}
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/FormTests.swift
platforms/apple/build-xcframework.sh   + form_fields.pdf in FIXTURES
.github/workflows/plugin-rust.yml, platforms/apple/README.md
```

## Cross-plugin tests

| Optional integration | Behaviour | Test owner |
|---|---|---|
| annotation (plan 05) | `create_field` reloads the annotation plane so the new widget is selectable; widgets are inert while the active tool carries `form-fill` | this plan (`a_created_field_is_selectable_as_an_annotation` in Task 4), since 05 merges first |
| actions (plan 15) | validate, calculate, keystroke and format scripts run on commit; `Validation::Scripts` is the default | plan 15 |
| actions (plan 15) | a SubmitForm node reaches the Swift submit handler through the form plugin's resolver; `document_actions.pdf`'s will-close script runs in JavaScriptCore once forms and annotations are reachable | this plan (`aSubmitReachesTheSwiftHandler`, `aDocumentScriptRunsInJavaScriptCore`, Swift task), since 15 merges first |
| signature (plan 16) | signature fields, `FormFieldRef` from `epdf-ffi` | plan 16 |
| stamp (plan 09) | signature appearance stamps | plan 09 |

---

### Task 1: Worktree and the shared form records in `epdf-ffi`

**Files:**
- Create: `crates/epdf-ffi/src/form.rs`
- Modify: `crates/epdf-ffi/src/lib.rs`
- Test: `crates/epdf-ffi/tests/form.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `epdf_ffi::FormFieldRef { ObjectNumber { field_object_number: u32 }, Fqn { name: String } }` with `to_json(&self) -> serde_json::Value` and `from_json(&Value) -> Result<Self, epdf_kernel::Error>`; `epdf_ffi::FormFieldFamily` (`Text`, `Checkbox`, `Radio`, `Combobox`, `Listbox`, `Pushbutton`, `Signature`, `Unknown`) with `as_str` and `parse`. Plan 16 uses both by name.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-form -b research/swift-form research/swift-plugins
cd .claude/worktrees/swift-form
```

Fetch PDFium for `darwin-arm64`, `ios-arm64` and `ios-sim-arm64` as `platforms/apple/README.md` describes. Confirm plans 01 and 05 have merged: `rg -n 'pub fn nest_refs' crates/epdf-viewer-host/src/translate.rs` and `rg -n 'interaction' crates/epdf-viewer-host/src/api.rs` both print a line.

- [x] **Step 2: Write the failing test**

`crates/epdf-ffi/tests/form.rs`:

```rust
use epdf_ffi::{FormFieldFamily, FormFieldRef};
use serde_json::json;

#[test]
fn an_object_number_ref_round_trips_through_the_plugins_json() {
    let r = FormFieldRef::ObjectNumber { field_object_number: 12 };
    assert_eq!(r.to_json(), json!({ "kind": "objectNumber", "fieldObjectNumber": 12 }));
    assert_eq!(FormFieldRef::from_json(&r.to_json()).unwrap(), r);
}

#[test]
fn a_name_ref_round_trips() {
    let r = FormFieldRef::Fqn { name: "billing.name".into() };
    assert_eq!(r.to_json(), json!({ "kind": "fqn", "name": "billing.name" }));
    assert_eq!(FormFieldRef::from_json(&r.to_json()).unwrap(), r);
}

#[test]
fn a_ref_of_another_kind_is_invalid_state() {
    let error = FormFieldRef::from_json(&json!({ "kind": "index", "index": 1 })).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

#[test]
fn families_parse_their_engine_names() {
    assert_eq!(FormFieldFamily::parse("combobox"), FormFieldFamily::Combobox);
    assert_eq!(FormFieldFamily::parse("barcode"), FormFieldFamily::Unknown);
    assert_eq!(FormFieldFamily::Listbox.as_str(), "listbox");
}
```

- [x] **Step 3: Run it to see it fail**

Run: `cargo test -p epdf-ffi --test form`
Expected: compile errors, `cannot find type FormFieldRef in crate epdf_ffi`.

- [x] **Step 4: Write `form.rs`**

```rust
//! The form records more than one plugin returns: how a field is addressed
//! and what kind of control it is. Signature fields are form fields, so the
//! signature plugin hands these back too.

use epdf_kernel::Error;
use serde_json::{Value, json};

/// How a field is addressed: by object number, or by fully qualified name
/// for a field the document stored as a direct object.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FormFieldRef {
    ObjectNumber { field_object_number: u32 },
    Fqn { name: String },
}

impl FormFieldRef {
    /// The plugin's (and the engine's) spelling.
    pub fn to_json(&self) -> Value {
        match self {
            FormFieldRef::ObjectNumber { field_object_number } => {
                json!({ "kind": "objectNumber", "fieldObjectNumber": field_object_number })
            }
            FormFieldRef::Fqn { name } => json!({ "kind": "fqn", "name": name }),
        }
    }

    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let bad = || {
            Error::named("InvalidState", format!("[form] not a FormFieldRef: {value}"))
                .with("code", "invalid-state")
        };
        match value.get("kind").and_then(Value::as_str) {
            Some("objectNumber") => {
                let n = value.get("fieldObjectNumber").and_then(Value::as_f64).ok_or_else(bad)?;
                if n.fract() != 0.0 || n < 0.0 || n > u32::MAX as f64 {
                    return Err(bad());
                }
                Ok(FormFieldRef::ObjectNumber { field_object_number: n as u32 })
            }
            Some("fqn") => {
                let name = value.get("name").and_then(Value::as_str).ok_or_else(bad)?;
                Ok(FormFieldRef::Fqn { name: name.into() })
            }
            _ => Err(bad()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormFieldFamily {
    Text,
    Checkbox,
    Radio,
    Combobox,
    Listbox,
    Pushbutton,
    Signature,
    /// A field type this build does not model.
    Unknown,
}

impl FormFieldFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            FormFieldFamily::Text => "text",
            FormFieldFamily::Checkbox => "checkbox",
            FormFieldFamily::Radio => "radio",
            FormFieldFamily::Combobox => "combobox",
            FormFieldFamily::Listbox => "listbox",
            FormFieldFamily::Pushbutton => "pushbutton",
            FormFieldFamily::Signature => "signature",
            FormFieldFamily::Unknown => "unknown",
        }
    }

    pub fn parse(name: &str) -> Self {
        match name {
            "text" => FormFieldFamily::Text,
            "checkbox" => FormFieldFamily::Checkbox,
            "radio" => FormFieldFamily::Radio,
            "combobox" => FormFieldFamily::Combobox,
            "listbox" => FormFieldFamily::Listbox,
            "pushbutton" => FormFieldFamily::Pushbutton,
            "signature" => FormFieldFamily::Signature,
            _ => FormFieldFamily::Unknown,
        }
    }
}
```

In `crates/epdf-ffi/src/lib.rs`: `pub mod form;` and `pub use form::{FormFieldFamily, FormFieldRef};`. `serde_json` is already a dependency after plan 05's Task 1.

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-ffi --test form`
Expected: 4 passed.

- [x] **Step 6: Commit**

```bash
git add crates/epdf-ffi
git commit -m "Add the shared form records to epdf-ffi"
```

---

### Task 2: `FormHandle`

**Files:**
- Modify: `crates/plugins/form/Cargo.toml`
- Modify: `crates/plugins/form/src/lib.rs`
- Modify: `crates/plugins/form/src/controller.rs`
- Create: `crates/plugins/form/src/ffi.rs`
- Test: `crates/plugins/form/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{FormFieldRef, FormFieldFamily, AnnotationRef, PageRef, Rect, KernelBound, KernelPost, CancelToken, Subscription, ViewerError, ResourceStatus, PluginErrorInfo}`.
- Produces: `epdf_plugin_form::ffi::{FormHandle, FormConfig, FormValidation, FormKind, FormSnapshot, FormField, FormFieldFlags, FormFieldDetail, FormWidget, FormToggleWidget, FormChoiceOption, FormValueInput, FormFillItem, FormFillControl, FormWidgetHit, FormCommitStatus, FormCommitResult, FormBatchResult, FormBatchFailure, FormFilter, FormCreateFieldInput, FormCreatedField, FormFieldPatch, FormDataFormat, FormDataExport, FormActivationResult, FormEvent, FormEventListener}`. `FormHandle::new(cap: FormCapability, post: KernelPost) -> Arc<FormHandle>`.

Members, under their FFI names (TypeScript name in parentheses where it differs): `snapshot` (`getSnapshot`), `field` (`getField`), `field_for_widget` (`getFieldForWidget`), `fields` (`listFields`), `value` (`getValue`), `fill_items` (`listFillItems`), `fill_item` (`getFillItem`), `ensure_loaded`, `widget_at` (`getWidgetAt`), `page_box` (`getPageBox`), `widgets` (`listWidgets`), `refresh`, `status` (`getStatus`), `set_value`, `set_text`, `set_checked`, `set_choice`, `set_values`, `import_values`, `reset`, `reset_all`, `create_field`, `update_field`, `delete_field`, `detach_widget`, `attach_widget`, `export_data`, `import_data`, `repair`, `activate_widget`, `can_read`, `can_fill`, `can_design`, `subscribe` (`onValueChanged`, `onFieldCreated`, `onFieldUpdated`, `onFieldDeleted`, `onValidationRejected`, `onResynced`).

- [x] **Step 1: Read the signatures this task wraps**

```bash
rg -n 'pub fn (reset_all|reset|create_field|update_field|delete_field|detach_widget|attach_widget|activate_widget|set_values|import_values|export_data|import_data|repair)' -A3 crates/plugins/form/src/write
rg -n 'pub fn (get_widget_at|get_page_box|list_fill_items|get_fill_item|ensure_loaded)' -A3 crates/plugins/form/src/read/widgets.rs
rg -n 'pub struct ScriptDiagnostic' -A10 crates/plugins/acrojs/src
```

Keep the output open. The code below matches the signatures as they stood on 2026-10-06.

- [x] **Step 2: Add the feature and the lifetime**

`crates/plugins/form/Cargo.toml`, in `[features]`:

```toml
# The uniffi exports (`ffi` module), for `epdf-viewer-host`.
uniffi = ["dep:uniffi", "dep:epdf-ffi"]
```

`[dependencies]`:

```toml
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

`[dev-dependencies]` (add `futures` if absent) and:

```toml
[[test]]
name = "ffi"
required-features = ["uniffi"]
```

`src/lib.rs`, after `pub mod write;`:

```rust
#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();
```

`src/controller.rs`, inside `impl FormCapability`:

```rust
    /// The instance lifetime, aborted when the document closes. A foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 3: Write the failing tests**

`crates/plugins/form/tests/ffi.rs`:

```rust
#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, FormFieldRef, KernelThread, PageRef, ResourceStatus, ViewerError};
use epdf_plugin_form::ffi::{
    FormCommitStatus, FormEvent, FormEventListener, FormFieldDetail, FormFillControl, FormHandle,
    FormKind, FormValueInput,
};
use epdf_plugin_form::{FORM, FormConfig, Validation, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

fn page() -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": 1 })
}

fn widget(annot: u32) -> Value {
    json!({ "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": annot },
        "annotObjectNumber": annot, "page": page() })
}

fn flags(read_only: bool) -> Value {
    json!({ "readOnly": read_only, "requiredValue": false, "required": false, "noExport": false, "raw": 0 })
}

/// A text field and a checkbox, in the kernel's spelling
/// (`crates/plugins/form/tests/api.rs`).
fn snapshot(name: &str, checked: bool) -> Value {
    json!({ "formKind": "acroform", "needsAppearances": false, "calculationOrder": [], "fields": [
        { "ref": { "kind": "objectNumber", "fieldObjectNumber": 1 }, "fieldObjectNumber": 1,
          "name": "name", "family": "text", "origin": "acroform", "flags": flags(false),
          "alternateName": "Your name", "mappingName": null,
          "valueEntry": { "kind": "scalar", "value": name }, "defaultValueEntry": { "kind": "none" },
          "value": name, "defaultValue": "", "maxLength": 40, "multiline": false, "password": false,
          "comb": false, "widgets": [widget(101)] },
        { "ref": { "kind": "objectNumber", "fieldObjectNumber": 2 }, "fieldObjectNumber": 2,
          "name": "agree", "family": "checkbox", "origin": "acroform", "flags": flags(false),
          "alternateName": null, "mappingName": null,
          "valueEntry": { "kind": "scalar", "value": if checked { "On" } else { "Off" } },
          "defaultValueEntry": { "kind": "none" },
          "checked": checked, "exportValue": "Yes",
          "widgets": [{ "ref": { "kind": "objectNumber", "page": page(), "annotObjectNumber": 102 },
            "annotObjectNumber": 102, "page": page(), "onState": "On", "exportValue": "Yes",
            "checked": checked }] }
    ] })
}

thread_local! {
    /// What `forms.setValue` was sent, on the kernel thread. `Rc` can't
    /// cross to the test thread, so tests read it with `sent(&thread)`.
    static SENT: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
}

fn sent(thread: &KernelThread<TestKernel>) -> Vec<Value> {
    block_on(thread.call(|_| SENT.with(|s| s.borrow().clone()))).unwrap()
}

fn spec() -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let state = Rc::new(RefCell::new(("Ada".to_string(), false)));
    let (read, write) = (state.clone(), state.clone());
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "pages": [{ "index": 0, "ref": page(), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "boxes": { "media": bx, "crop": bx } }] } } }))
        .respond("forms.list", move |_| {
            let (name, checked) = read.borrow().clone();
            Ok(Reply::value(snapshot(&name, checked)))
        })
        .respond("forms.setValue", move |args| {
            SENT.with(|s| s.borrow_mut().push(json!(args)));
            let mut s = write.borrow_mut();
            match args[1]["type"].as_str() {
                Some("text") => s.0 = args[1]["value"].as_str().unwrap().into(),
                Some("toggle") => s.1 = args[1]["state"] == "On",
                _ => {}
            }
            Ok(Reply::value(json!({ "changedWidgets": [101] })))
        })
        .respond("forms.exportData", |args| {
            Ok(Reply::value(json!({ "format": args[0], "bytes": { "$bytes": "JUZERi0xLjI=" } })))
        })
        .respond("forms.importData", |_| Ok(Reply::value(json!({ "snapshot": snapshot("Imported", true) }))))
}

fn handle() -> (KernelThread<TestKernel>, Arc<FormHandle>) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                // No actions plugin here, so commits go straight to the engine.
                form_plugin(FormConfig { validation: Some(Validation::None), ..Default::default() }),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| FormHandle::new(k.capability(&FORM).unwrap(), post))).unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

fn text_ref() -> FormFieldRef {
    FormFieldRef::ObjectNumber { field_object_number: 1 }
}

fn box_ref() -> FormFieldRef {
    FormFieldRef::ObjectNumber { field_object_number: 2 }
}

#[derive(Default)]
struct Events(Mutex<Vec<FormEvent>>, AtomicUsize);

impl FormEventListener for Events {
    fn on_event(&self, event: FormEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn refresh_reads_the_fields_as_typed_records() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    assert_eq!(block_on(form.status()).unwrap(), ResourceStatus::Ready);
    let snapshot = block_on(form.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.form_kind, FormKind::Acroform);
    let fields = block_on(form.fields(None)).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "name");
    assert_eq!(fields[0].alternate_name.as_deref(), Some("Your name"));
    match &fields[0].detail {
        FormFieldDetail::Text { value, max_length, .. } => {
            assert_eq!(value, "Ada");
            assert_eq!(*max_length, Some(40));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fields[0].widgets[0].annot_object_number, 101);
}

#[test]
fn fill_items_project_each_widget_as_a_control() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let items = block_on(form.fill_items(PageRef { page_object_number: 1 })).unwrap();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].control, FormFillControl::Text { .. }), "{:?}", items[0]);
    assert!(matches!(items[1].control, FormFillControl::Toggle { .. }), "{:?}", items[1]);
    assert!(items[0].annotation_ref.is_some());
}

#[test]
fn setting_text_is_observed() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let events = Arc::new(Events::default());
    let _sub = form.subscribe(events.clone());
    let result = block_on(form.set_text(text_ref(), "Grace".into(), token())).unwrap();
    assert_eq!(result.status, FormCommitStatus::Applied);
    assert_eq!(block_on(form.value(text_ref())).unwrap(), Some(FormValueInput::Text { value: "Grace".into() }));
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, FormEvent::ValueChanged { .. })), "{seen:?}");
}

#[test]
fn checking_a_box_by_its_on_state_reports_applied() {
    let (thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let result = block_on(form.set_checked(box_ref(), Some("On".into()), token())).unwrap();
    assert_eq!(result.status, FormCommitStatus::Applied);
    assert_eq!(sent(&thread).last().unwrap()[1], json!({ "type": "toggle", "state": "On" }));
}

#[test]
fn a_rejected_commit_is_a_result_not_an_error() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    // `set_choice` on a text field: the engine refuses the shape, which the
    // plugin reports as a failed commit, not a thrown error.
    let result = block_on(form.set_choice(text_ref(), vec!["x".into()], token()));
    match result {
        Ok(result) => assert_ne!(result.status, FormCommitStatus::Applied),
        Err(error) => panic!("expected a result, got {error:?}"),
    }
}

#[test]
fn an_export_answers_its_bytes() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let export = block_on(form.export_data(Some(epdf_plugin_form::ffi::FormDataFormat::Fdf), token())).unwrap();
    assert_eq!(export.format, epdf_plugin_form::ffi::FormDataFormat::Fdf);
    assert_eq!(export.bytes, b"%FDF-1.2".to_vec());
}

#[test]
fn an_import_replaces_the_snapshot() {
    let (_thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    block_on(form.import_data(b"%FDF-1.2".to_vec(), None, token())).unwrap();
    let fields = block_on(form.fields(None)).unwrap();
    assert!(matches!(&fields[0].detail, FormFieldDetail::Text { value, .. } if value == "Imported"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, form) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(form.fields(None)).map(|_| ()).unwrap_err(),
        block_on(form.can_fill()).map(|_| ()).unwrap_err(),
        block_on(form.refresh(token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, form) = handle();
    let events = Arc::new(Events::default());
    let _sub = form.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_set_value_before_it_runs_leaves_the_field_alone() {
    let (thread, form) = handle();
    block_on(form.refresh(token())).unwrap();
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let write = form.set_text(text_ref(), "Grace".into(), cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(write).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(sent(&thread).is_empty());
}
```

`flags()` carries an extra `requiredValue` key on purpose: the decode must ignore keys it doesn't name, the way the engine adds them.

- [x] **Step 4: Run them to see them fail**

Run: `cargo test -p epdf-plugin-form --features uniffi --test ffi`
Expected: compile errors, `could not find ffi in epdf_plugin_form`.

- [x] **Step 5: Write `ffi.rs`**

```rust
//! The form plugin through uniffi: typed records decoded from the plugin's
//! JSON, one handle, one event enum. Every method hops to the kernel thread
//! through `KernelBound`.
//!
//! Not exported: `resetFormAction`, `resolveSubmitDataset`,
//! `commitScriptFormEffects` and `notifyWidgetEvent` (the actions plugin's
//! hooks, wired in `form_plugin`), `setValueRaw` (skips scripts), and
//! `exportValues` (`fields` returns the same values typed).

use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, FormFieldFamily, FormFieldRef, KernelBound, KernelPost, PageRef,
    PluginErrorInfo, Rect, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::FormCapability;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[form] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

fn open(cap: &FormCapability) -> Result<&FormCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "form",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn annotation_ref(value: &Value) -> Result<Option<AnnotationRef>, Error> {
    if value.is_null() {
        return Ok(None);
    }
    let core: epdf_annotation_core::AnnotationRef = decode(value.clone())?;
    Ok(Some(AnnotationRef::try_from(core)?))
}

fn info(error: &Error) -> PluginErrorInfo {
    let info = error.plugin_error_info();
    PluginErrorInfo {
        code: info.get("code").and_then(Value::as_str).unwrap_or("unknown").into(),
        message: info.get("message").and_then(Value::as_str).unwrap_or(&error.message).into(),
        capability: info.get("capability").and_then(Value::as_str).unwrap_or("form").into(),
    }
}

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormValidation {
    /// Run the document's keystroke, validate, calculate and format scripts
    /// (needs the actions plugin with scripting on).
    Scripts,
    /// Write values straight to the engine.
    None,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormConfig {
    /// `None` is `Scripts`.
    #[uniffi(default = None)]
    pub validation: Option<FormValidation>,
}

impl From<FormConfig> for crate::FormConfig {
    fn from(c: FormConfig) -> Self {
        crate::FormConfig {
            validation: c.validation.map(|v| match v {
                FormValidation::Scripts => crate::Validation::Scripts,
                FormValidation::None => crate::Validation::None,
            }),
            ..Default::default()
        }
    }
}

// ── the snapshot ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum FormKind {
    /// No `/AcroForm`. Recovered fields may still exist.
    None,
    Acroform,
    /// An XFA form, served read-only. The XFA itself never runs.
    Xfa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldFlags {
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
    /// The raw `/Ff` word.
    pub raw: u32,
}

/// A checkbox or radio widget's state.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormToggleWidget {
    /// The `/AP /N` key that turns this widget on: what `set_checked` takes.
    pub on_state: String,
    /// What FDF and XFDF carry.
    pub export_value: String,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidget {
    /// `None` until the engine placed the widget.
    pub annotation_ref: Option<AnnotationRef>,
    pub annot_object_number: u32,
    pub page: PageRef,
    pub toggle: Option<FormToggleWidget>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, uniffi::Record)]
pub struct FormChoiceOption {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub selected: bool,
}

/// The family-specific half of a field.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormFieldDetail {
    Text { value: String, default_value: String, max_length: Option<u32>, multiline: bool, password: bool, comb: bool },
    Checkbox { checked: bool, export_value: String },
    Radio { value: String, radios_in_unison: bool, no_toggle_to_off: bool },
    Combobox { value: String, default_value: String, edit: bool, options: Vec<FormChoiceOption> },
    Listbox { selected_values: Vec<String>, multi_select: bool, options: Vec<FormChoiceOption> },
    Pushbutton,
    Signature,
    Unknown { raw_value: String },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormField {
    pub field_ref: FormFieldRef,
    /// Zero for a field stored as a direct object.
    pub field_object_number: u32,
    /// The fully qualified name, such as `billing.name`.
    pub name: String,
    pub family: FormFieldFamily,
    /// `true` when the field was found only through a page's `/Annots`.
    pub recovered: bool,
    pub flags: FormFieldFlags,
    /// `/TU`, the tooltip and accessible name.
    pub alternate_name: Option<String>,
    /// `/TM`, the export name.
    pub mapping_name: Option<String>,
    pub detail: FormFieldDetail,
    pub widgets: Vec<FormWidget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WidgetDto {
    #[serde(rename = "ref", default)]
    annotation_ref: Value,
    annot_object_number: f64,
    page: epdf_annotation_core::PageRef,
    #[serde(default)]
    on_state: Option<String>,
    #[serde(default)]
    export_value: Option<String>,
    #[serde(default)]
    checked: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldDto {
    #[serde(rename = "ref")]
    field_ref: Value,
    field_object_number: u32,
    name: String,
    family: String,
    origin: String,
    flags: FormFieldFlags,
    #[serde(default)]
    alternate_name: Option<String>,
    #[serde(default)]
    mapping_name: Option<String>,
    #[serde(default)]
    widgets: Vec<WidgetDto>,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    default_value: Option<String>,
    #[serde(default)]
    max_length: Option<i64>,
    #[serde(default)]
    multiline: bool,
    #[serde(default)]
    password: bool,
    #[serde(default)]
    comb: bool,
    #[serde(default)]
    checked: bool,
    #[serde(default)]
    export_value: Option<String>,
    #[serde(default)]
    radios_in_unison: bool,
    #[serde(default)]
    no_toggle_to_off: bool,
    #[serde(default)]
    edit: bool,
    #[serde(default)]
    options: Vec<FormChoiceOption>,
    #[serde(default)]
    selected_values: Vec<String>,
    #[serde(default)]
    multi_select: bool,
    #[serde(default)]
    raw_value: Option<String>,
}

impl FormField {
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let dto: FieldDto = decode(value.clone())?;
        let family = FormFieldFamily::parse(&dto.family);
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let detail = match family {
            FormFieldFamily::Text => FormFieldDetail::Text {
                value: text(&dto.value),
                default_value: dto.default_value.clone().unwrap_or_default(),
                max_length: dto.max_length.and_then(|n| u32::try_from(n).ok()),
                multiline: dto.multiline,
                password: dto.password,
                comb: dto.comb,
            },
            FormFieldFamily::Checkbox => FormFieldDetail::Checkbox {
                checked: dto.checked,
                export_value: dto.export_value.clone().unwrap_or_default(),
            },
            FormFieldFamily::Radio => FormFieldDetail::Radio {
                value: text(&dto.value),
                radios_in_unison: dto.radios_in_unison,
                no_toggle_to_off: dto.no_toggle_to_off,
            },
            FormFieldFamily::Combobox => FormFieldDetail::Combobox {
                value: text(&dto.value),
                default_value: dto.default_value.clone().unwrap_or_default(),
                edit: dto.edit,
                options: dto.options.clone(),
            },
            FormFieldFamily::Listbox => FormFieldDetail::Listbox {
                selected_values: dto.selected_values.clone(),
                multi_select: dto.multi_select,
                options: dto.options.clone(),
            },
            FormFieldFamily::Pushbutton => FormFieldDetail::Pushbutton,
            FormFieldFamily::Signature => FormFieldDetail::Signature,
            FormFieldFamily::Unknown => FormFieldDetail::Unknown { raw_value: dto.raw_value.clone().unwrap_or_default() },
        };
        let widgets = dto
            .widgets
            .into_iter()
            .map(|w| {
                let toggle = w.on_state.map(|on_state| FormToggleWidget {
                    on_state,
                    export_value: w.export_value.unwrap_or_default(),
                    checked: w.checked.unwrap_or(false),
                });
                Ok(FormWidget {
                    annotation_ref: annotation_ref(&w.annotation_ref)?,
                    annot_object_number: w.annot_object_number as u32,
                    page: w.page.into(),
                    toggle,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(FormField {
            field_ref: FormFieldRef::from_json(&dto.field_ref)?,
            field_object_number: dto.field_object_number,
            name: dto.name,
            family,
            recovered: dto.origin == "recovered",
            flags: dto.flags,
            alternate_name: dto.alternate_name,
            mapping_name: dto.mapping_name,
            detail,
            widgets,
        })
    }
}

fn fields_out(list: Vec<Value>) -> Result<Vec<FormField>, Error> {
    list.iter().map(FormField::from_json).collect()
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormSnapshot {
    pub form_kind: FormKind,
    /// `/NeedAppearances`: the viewer should draw the widgets itself.
    pub needs_appearances: bool,
    pub fields: Vec<FormField>,
    /// `/CO`. `None` keeps a slot naming a field the model lacks.
    pub calculation_order: Vec<Option<FormFieldRef>>,
}

impl FormSnapshot {
    fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Dto {
            form_kind: FormKind,
            needs_appearances: bool,
            fields: Vec<Value>,
            #[serde(default)]
            calculation_order: Vec<Value>,
        }
        let dto: Dto = decode(value.clone())?;
        Ok(FormSnapshot {
            form_kind: dto.form_kind,
            needs_appearances: dto.needs_appearances,
            fields: fields_out(dto.fields)?,
            calculation_order: dto
                .calculation_order
                .iter()
                .map(|r| if r.is_null() { Ok(None) } else { FormFieldRef::from_json(r).map(Some) })
                .collect::<Result<_, Error>>()?,
        })
    }
}

// ── values ───────────────────────────────────────────────────────────────────

/// A value in the shape each family stores. A shape that does not fit the
/// field is refused, not coerced.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormValueInput {
    Text { value: String },
    /// The `/AP /N` key to turn on, or `None` to clear the group.
    Toggle { state: Option<String> },
    Choice { values: Vec<String> },
}

impl FormValueInput {
    fn to_json(&self) -> Value {
        match self {
            FormValueInput::Text { value } => json!({ "type": "text", "value": value }),
            FormValueInput::Toggle { state } => json!({ "type": "toggle", "state": state }),
            FormValueInput::Choice { values } => json!({ "type": "choice", "values": values }),
        }
    }

    fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "camelCase")]
        enum Dto {
            Text { value: String },
            Toggle { state: Option<String> },
            Choice { values: Vec<String> },
        }
        Ok(match decode::<Dto>(value.clone())? {
            Dto::Text { value } => FormValueInput::Text { value },
            Dto::Toggle { state } => FormValueInput::Toggle { state },
            Dto::Choice { values } => FormValueInput::Choice { values },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormCommitStatus {
    Applied,
    Unchanged,
    Rejected,
    Failed,
}

impl From<crate::FormCommitStatus> for FormCommitStatus {
    fn from(s: crate::FormCommitStatus) -> Self {
        match s {
            crate::FormCommitStatus::Applied => FormCommitStatus::Applied,
            crate::FormCommitStatus::Unchanged => FormCommitStatus::Unchanged,
            crate::FormCommitStatus::Rejected => FormCommitStatus::Rejected,
            crate::FormCommitStatus::Failed => FormCommitStatus::Failed,
        }
    }
}

/// What a value write did. A script that refused the value is `Rejected`
/// with its diagnostics, not an error.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCommitResult {
    pub status: FormCommitStatus,
    /// Whether the K/V/C/F scripts ran.
    pub scripted: bool,
    /// The scripts' messages (`app.alert`, validation failures), in order.
    pub diagnostics: Vec<String>,
    /// The script error that ended the transaction, if one did.
    pub error: Option<String>,
}

impl From<crate::FormCommitResult> for FormCommitResult {
    fn from(r: crate::FormCommitResult) -> Self {
        let message = |v: Value| v.get("message").and_then(Value::as_str).map(str::to_string);
        FormCommitResult {
            status: r.status.into(),
            scripted: r.scripted,
            diagnostics: r
                .diagnostics
                .iter()
                .filter_map(|d| serde_json::to_value(d).ok().and_then(message))
                .collect(),
            error: r.error.and_then(|e| serde_json::to_value(e).ok().and_then(message)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormBatchFailure {
    pub field_ref: Option<FormFieldRef>,
    /// The entry's name when it was addressed by name (`import_values`).
    pub name: Option<String>,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormBatchResult {
    pub applied: Vec<FormFieldRef>,
    pub skipped: Vec<FormFieldRef>,
    pub failed: Vec<FormBatchFailure>,
}

impl TryFrom<crate::BatchResult> for FormBatchResult {
    type Error = Error;

    fn try_from(b: crate::BatchResult) -> Result<Self, Error> {
        let refs = |list: &[Value]| list.iter().map(FormFieldRef::from_json).collect::<Result<Vec<_>, Error>>();
        Ok(FormBatchResult {
            applied: refs(&b.applied)?,
            skipped: refs(&b.skipped)?,
            failed: b
                .failed
                .into_iter()
                .map(|f| {
                    let error: PluginErrorInfo = PluginErrorInfo {
                        code: f.error.get("code").and_then(Value::as_str).unwrap_or("unknown").into(),
                        message: f.error.get("message").and_then(Value::as_str).unwrap_or_default().into(),
                        capability: f.error.get("capability").and_then(Value::as_str).unwrap_or("form").into(),
                    };
                    FormBatchFailure {
                        field_ref: FormFieldRef::from_json(&f.entry).ok(),
                        name: f.entry.as_str().map(str::to_string),
                        error,
                    }
                })
                .collect(),
        })
    }
}

// ── fill projection ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormFillControl {
    Text { value: String, multiline: bool, password: bool, max_length: Option<u32>, comb: bool },
    Toggle { radio: bool, checked: bool, on_state: String },
    Choice { list: bool, edit: bool, multi: bool, options: Vec<FormChoiceOption>, selected: Vec<String> },
    Button,
    Signature { signed: bool },
}

/// One widget's fill control: what a native form overlay draws on a page.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormFillItem {
    pub field_ref: FormFieldRef,
    pub annot_object_number: u32,
    pub annotation_ref: Option<AnnotationRef>,
    /// Page space.
    pub bounds: Rect,
    /// Read-only field, or a write in flight: draw it, don't accept input.
    pub disabled: bool,
    /// `/TU`, else the fully qualified name.
    pub label: String,
    pub control: FormFillControl,
}

impl TryFrom<&crate::FillItem> for FormFillItem {
    type Error = Error;

    fn try_from(item: &crate::FillItem) -> Result<Self, Error> {
        let flag = |v: &Value| v.as_bool().unwrap_or(false);
        let string = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let control = match &item.control {
            crate::FillControl::Text { value, multiline, password, max_length, comb } => FormFillControl::Text {
                value: string(value),
                multiline: flag(multiline),
                password: flag(password),
                max_length: max_length.as_u64().and_then(|n| u32::try_from(n).ok()),
                comb: flag(comb),
            },
            crate::FillControl::Toggle { kind, checked, on_state } => FormFillControl::Toggle {
                radio: *kind == crate::ToggleKind::Radio,
                checked: flag(checked),
                on_state: string(on_state),
            },
            crate::FillControl::Choice { kind, edit, multi, options, selected } => FormFillControl::Choice {
                list: *kind == crate::ChoiceKind::List,
                edit: flag(edit),
                multi: flag(multi),
                options: decode(options.clone())?,
                selected: match selected {
                    Value::Array(_) => decode(selected.clone())?,
                    Value::String(s) => vec![s.clone()],
                    _ => Vec::new(),
                },
            },
            crate::FillControl::Button => FormFillControl::Button,
            crate::FillControl::Signature { signed, .. } => FormFillControl::Signature { signed: flag(signed) },
        };
        Ok(FormFillItem {
            field_ref: FormFieldRef::from_json(&item.field_ref)?,
            annot_object_number: item.annot_object_number as u32,
            annotation_ref: annotation_ref(&item.annotation_ref)?,
            bounds: Rect { x: item.bx.x, y: item.bx.y, width: item.bx.width, height: item.bx.height },
            disabled: item.disabled,
            label: string(&item.label),
            control,
        })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormWidgetHit {
    pub annot_object_number: u32,
    pub field: FormField,
    pub bounds: Rect,
}

// ── design ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormChoiceOptionInput {
    pub label: String,
    pub value: String,
}

/// A new field and its one widget. `family` is one the engine can create:
/// text, checkbox, radio, combobox or listbox.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCreateFieldInput {
    pub family: FormFieldFamily,
    pub page: PageRef,
    /// The widget's page-space box.
    pub bounds: Rect,
    /// A fully qualified name; numbered from the family when `None`.
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub options: Option<Vec<FormChoiceOptionInput>>,
}

impl From<FormCreateFieldInput> for crate::CreateFieldInput {
    fn from(i: FormCreateFieldInput) -> Self {
        crate::CreateFieldInput {
            family: i.family.as_str().into(),
            page: i.page.into(),
            bounds: crate::Box { x: i.bounds.x, y: i.bounds.y, width: i.bounds.width, height: i.bounds.height },
            name: i.name,
            appearance: None,
            options: i.options.map(|o| o.into_iter().map(|o| json!({ "label": o.label, "value": o.value })).collect()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormCreatedField {
    pub field: FormField,
    pub widget: Option<FormWidget>,
}

/// A field edit. `None` leaves a member alone; a key in `clear` removes it.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFieldPatch {
    #[uniffi(default = None)]
    pub name: Option<String>,
    #[uniffi(default = None)]
    pub read_only: Option<bool>,
    #[uniffi(default = None)]
    pub required: Option<bool>,
    #[uniffi(default = None)]
    pub no_export: Option<bool>,
    #[uniffi(default = None)]
    pub alternate_name: Option<String>,
    #[uniffi(default = None)]
    pub mapping_name: Option<String>,
    #[uniffi(default = None)]
    pub default_value: Option<String>,
    #[uniffi(default = None)]
    pub max_length: Option<u32>,
    #[uniffi(default = None)]
    pub options: Option<Vec<FormChoiceOptionInput>>,
    #[uniffi(default = [])]
    pub clear: Vec<FormFieldPatchKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormFieldPatchKey {
    AlternateName,
    MappingName,
    DefaultValue,
    MaxLength,
}

impl FormFieldPatch {
    /// The engine's `FormFieldPatch`: flags at the top level, a cleared
    /// member as `null`. `family` is the field's, which the engine checks.
    fn to_json(&self, family: FormFieldFamily) -> Result<Value, Error> {
        let mut patch = Map::new();
        patch.insert("family".into(), family.as_str().into());
        let set: [(&str, Option<Value>); 9] = [
            ("name", self.name.clone().map(Value::from)),
            ("readOnly", self.read_only.map(Value::from)),
            ("required", self.required.map(Value::from)),
            ("noExport", self.no_export.map(Value::from)),
            ("alternateName", self.alternate_name.clone().map(Value::from)),
            ("mappingName", self.mapping_name.clone().map(Value::from)),
            ("defaultValue", self.default_value.clone().map(Value::from)),
            ("maxLength", self.max_length.map(Value::from)),
            ("options", self.options.as_ref().map(|o| {
                Value::Array(o.iter().map(|o| json!({ "label": o.label, "value": o.value })).collect())
            })),
        ];
        for (key, value) in set {
            if let Some(value) = value {
                patch.insert(key.into(), value);
            }
        }
        for key in &self.clear {
            let name = match key {
                FormFieldPatchKey::AlternateName => "alternateName",
                FormFieldPatchKey::MappingName => "mappingName",
                FormFieldPatchKey::DefaultValue => "defaultValue",
                FormFieldPatchKey::MaxLength => "maxLength",
            };
            if patch.insert(name.into(), Value::Null).is_some_and(|v| !v.is_null()) {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    "form",
                    format!("field patch both sets and clears \"{name}\""),
                    None,
                    None,
                ));
            }
        }
        Ok(Value::Object(patch))
    }
}

// ── interchange ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FormDataFormat {
    Fdf,
    Xfdf,
}

impl FormDataFormat {
    fn as_str(self) -> &'static str {
        match self {
            FormDataFormat::Fdf => "fdf",
            FormDataFormat::Xfdf => "xfdf",
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormDataExport {
    /// The format the engine wrote, which names the file extension.
    pub format: FormDataFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormActivationResult {
    /// The form plugin handled it: a toggle flipped, a value committed.
    Form { result: FormCommitResult },
    /// An action ran instead (a pushbutton's `/A`).
    Dispatched,
}

// ── events ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum FormEvent {
    ValueChanged { field_ref: FormFieldRef, field: Option<FormField> },
    FieldCreated { field_ref: FormFieldRef, field: Option<FormField> },
    FieldUpdated { field_ref: FormFieldRef, field: Option<FormField> },
    FieldDeleted { field_ref: FormFieldRef },
    ValidationRejected { field_ref: FormFieldRef, messages: Vec<String> },
    /// The snapshot was re-read; every field may have changed.
    Resynced,
}

#[uniffi::export(with_foreign)]
pub trait FormEventListener: Send + Sync {
    fn on_event(&self, event: FormEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

fn optional_field(value: Option<&Value>) -> Result<Option<FormField>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(v) => FormField::from_json(v).map(Some),
    }
}

// ── the handle ───────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct FormHandle {
    cap: KernelBound<FormCapability>,
    post: KernelPost,
}

impl FormHandle {
    /// On the kernel thread.
    pub fn new(cap: FormCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(FormHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

fn commit(result: crate::FormCommitResult) -> FormCommitResult {
    result.into()
}

#[uniffi::export]
impl FormHandle {
    /// `None` until the field tree is in, or while reading is forbidden.
    pub async fn snapshot(&self) -> Result<Option<FormSnapshot>, ViewerError> {
        self.cap.call(|c| open(c)?.get_snapshot().as_ref().map(FormSnapshot::from_json).transpose()).await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap
            .call(|c| {
                Ok(match open(c)?.get_status() {
                    crate::ResourceStatus::Idle => ResourceStatus::Idle,
                    crate::ResourceStatus::Loading => ResourceStatus::Loading,
                    crate::ResourceStatus::Ready => ResourceStatus::Ready,
                    crate::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
                    crate::ResourceStatus::Error => ResourceStatus::Error,
                })
            })
            .await
    }

    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh()), |()| ()).await
    }

    pub async fn field(&self, field_ref: FormFieldRef) -> Result<Option<FormField>, ViewerError> {
        self.cap.call(move |c| optional_field(open(c)?.get_field(&field_ref.to_json()).as_ref())).await
    }

    pub async fn field_for_widget(&self, widget: AnnotationRef) -> Result<Option<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                let widget = serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget)).expect("a ref");
                optional_field(open(c)?.get_field_for_widget(&widget).as_ref())
            })
            .await
    }

    pub async fn fields(&self, filter: Option<FormFilter>) -> Result<Vec<FormField>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(crate::FormFilter::from);
                fields_out(open(c)?.list_fields(filter.as_ref()))
            })
            .await
    }

    pub async fn value(&self, field_ref: FormFieldRef) -> Result<Option<FormValueInput>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.get_value(&field_ref.to_json()).as_ref().map(FormValueInput::from_json).transpose())
            .await
    }

    pub async fn fill_items(&self, page: PageRef) -> Result<Vec<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.list_fill_items(page.into()).iter().map(FormFillItem::try_from).collect())
            .await
    }

    pub async fn fill_item(&self, annot_object_number: u32) -> Result<Option<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.get_fill_item(f64::from(annot_object_number)).map(|i| FormFillItem::try_from(&*i)).transpose()
            })
            .await
    }

    /// The same projection as `fill_items`, under the TypeScript's other name.
    pub async fn widgets(&self, page: PageRef) -> Result<Vec<FormFillItem>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.list_widgets(page.into()).iter().map(FormFillItem::try_from).collect())
            .await
    }

    /// Start loading a page's widget geometry. Returns before it lands.
    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.ensure_loaded(page.into()))).await
    }

    /// The widget under a page-space point.
    pub async fn widget_at(&self, page: PageRef, x: f64, y: f64) -> Result<Option<FormWidgetHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_widget_at(page.into(), x, y)
                    .map(|hit| {
                        Ok(FormWidgetHit {
                            annot_object_number: hit.annot_object_number as u32,
                            field: FormField::from_json(&hit.field)?,
                            bounds: Rect { x: hit.bx.x, y: hit.bx.y, width: hit.bx.width, height: hit.bx.height },
                        })
                    })
                    .transpose()
            })
            .await
    }

    pub async fn page_box(&self, page: PageRef) -> Result<Option<Rect>, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.get_page_box(page.into()).map(Into::into))).await
    }

    pub async fn set_value(&self, field_ref: FormFieldRef, value: FormValueInput, cancel: Arc<CancelToken>) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.set_value(field_ref.to_json(), value.to_json())), commit)
            .await
    }

    pub async fn set_text(&self, field_ref: FormFieldRef, text: String, cancel: Arc<CancelToken>) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.set_text(field_ref.to_json(), text.into())), commit)
            .await
    }

    /// A checkbox or radio by its on-state (`FormToggleWidget.on_state`);
    /// `None` clears the group.
    pub async fn set_checked(&self, field_ref: FormFieldRef, on_state: Option<String>, cancel: Arc<CancelToken>) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_checked(field_ref.to_json(), on_state.map_or(Value::Null, Value::from))),
                commit,
            )
            .await
    }

    /// Choice options by export value.
    pub async fn set_choice(&self, field_ref: FormFieldRef, values: Vec<String>, cancel: Arc<CancelToken>) -> Result<FormCommitResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_choice(field_ref.to_json(), values.into_iter().map(Value::from).collect())),
                commit,
            )
            .await
    }

    pub async fn set_values(&self, entries: Vec<FormValueEntry>, cancel: Arc<CancelToken>) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_values(entries.into_iter().map(|e| (e.field_ref.to_json(), e.value.to_json())).collect())),
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    /// Values by fully qualified name, as an FDF would carry them.
    pub async fn import_values(&self, values: std::collections::HashMap<String, FormValueInput>, cancel: Arc<CancelToken>) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.import_values(values.into_iter().map(|(k, v)| (k, v.to_json())).collect())),
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn reset(&self, field_ref: FormFieldRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(open(c)?.reset(field_ref.to_json())), |()| ()).await
    }

    /// Reset every field, or only `fields` (every other field with `exclude`).
    pub async fn reset_all(&self, fields: Option<Vec<FormFieldRef>>, exclude: bool, cancel: Arc<CancelToken>) -> Result<FormBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let options = crate::ResetAllOptions {
                        fields: fields.map(|f| f.iter().map(FormFieldRef::to_json).collect()),
                        exclude: Some(exclude),
                    };
                    Ok(open(c)?.reset_all(options))
                },
                |b| FormBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn create_field(&self, input: FormCreateFieldInput, cancel: Arc<CancelToken>) -> Result<FormCreatedField, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create_field(input.into())),
                |created| {
                    let field = FormField::from_json(&created.field)?;
                    let widget = created
                        .widget
                        .as_ref()
                        .and_then(|w| field.widgets.iter().find(|fw| Some(fw.annot_object_number as u64) == w.get("annotObjectNumber").and_then(Value::as_u64)).cloned());
                    Ok::<_, ViewerError>(FormCreatedField { field, widget })
                },
            )
            .await?
    }

    pub async fn update_field(&self, field_ref: FormFieldRef, patch: FormFieldPatch, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let cap = open(c)?;
                    let field = optional_field(cap.get_field(&field_ref.to_json()).as_ref())?.ok_or_else(|| {
                        plugin_error(code::NOT_FOUND, "form", "no such field", None, None)
                    })?;
                    Ok(cap.update_field(field_ref.to_json(), patch.to_json(field.family)?))
                },
                |()| (),
            )
            .await
    }

    pub async fn delete_field(&self, field_ref: FormFieldRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(open(c)?.delete_field(field_ref.to_json())), |()| ()).await
    }

    pub async fn detach_widget(&self, field_ref: FormFieldRef, widget: AnnotationRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget = serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget)).expect("a ref");
                    Ok(open(c)?.detach_widget(field_ref.to_json(), widget))
                },
                |()| (),
            )
            .await
    }

    pub async fn attach_widget(&self, field_ref: FormFieldRef, widget: AnnotationRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget = serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget)).expect("a ref");
                    Ok(open(c)?.attach_widget(field_ref.to_json(), widget))
                },
                |()| (),
            )
            .await
    }

    /// `None` is XFDF.
    pub async fn export_data(&self, format: Option<FormDataFormat>, cancel: Arc<CancelToken>) -> Result<FormDataExport, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.export_data(format.map(|f| Value::from(f.as_str())))),
                |result| {
                    let format = match result.get("format").and_then(Value::as_str) {
                        Some("fdf") => FormDataFormat::Fdf,
                        Some("xfdf") => FormDataFormat::Xfdf,
                        other => {
                            return Err(ViewerError::from(Error::named("InvalidState", format!("[form] export format {other:?}")).with("code", "invalid-state")));
                        }
                    };
                    let bytes = result.get("bytes").and_then(Bytes::from_json).ok_or_else(|| {
                        ViewerError::from(Error::named("InvalidState", "[form] an export without bytes").with("code", "invalid-state"))
                    })?;
                    Ok(FormDataExport { format, bytes: bytes.0 })
                },
            )
            .await?
    }

    /// FDF or XFDF; `None` sniffs the format from the bytes.
    pub async fn import_data(&self, bytes: Vec<u8>, format: Option<FormDataFormat>, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.import_data(Bytes(bytes).to_json(), format.map(|f| Value::from(f.as_str())))),
                |_| (),
            )
            .await
    }

    /// Make recovered fields durable, and bake their appearances when
    /// `bake_appearances`.
    pub async fn repair(&self, bake_appearances: bool, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.repair(Some(json!({ "bakeAppearances": bake_appearances })))),
                |_| (),
            )
            .await
    }

    /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
    pub async fn activate_widget(&self, widget: AnnotationRef, cancel: Arc<CancelToken>) -> Result<FormActivationResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let widget = serde_json::to_value(epdf_annotation_core::AnnotationRef::from(widget)).expect("a ref");
                    Ok(open(c)?.activate_widget(widget))
                },
                |r| match r {
                    crate::WidgetActivationResult::Form(result) => FormActivationResult::Form { result: result.into() },
                    crate::WidgetActivationResult::Dispatched(_) => FormActivationResult::Dispatched,
                },
            )
            .await
    }

    pub async fn can_read(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read())).await
    }

    pub async fn can_fill(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_fill())).await
    }

    pub async fn can_design(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_design())).await
    }

    pub fn subscribe(&self, listener: Arc<dyn FormEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let value = c.on_value_changed(move |e| {
                        l.on_event(FormEvent::ValueChanged {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(Some(&e.field))?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let created = c.on_field_created(move |e| {
                        l.on_event(FormEvent::FieldCreated {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(e.field.as_ref())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let updated = c.on_field_updated(move |e| {
                        l.on_event(FormEvent::FieldUpdated {
                            field_ref: FormFieldRef::from_json(&e.field_ref)?,
                            field: optional_field(e.field.as_ref())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = c.on_field_deleted(move |e| {
                        l.on_event(FormEvent::FieldDeleted { field_ref: FormFieldRef::from_json(&e.field_ref)? });
                        Ok(())
                    });
                    let l = listener.clone();
                    let rejected = c.on_validation_rejected(move |e| {
                        let messages = e
                            .issues
                            .iter()
                            .filter_map(|d| serde_json::to_value(d).ok())
                            .filter_map(|d| d.get("message").and_then(Value::as_str).map(str::to_string))
                            .collect();
                        l.on_event(FormEvent::ValidationRejected { field_ref: FormFieldRef::from_json(&e.field_ref)?, messages });
                        Ok(())
                    });
                    let l = listener.clone();
                    let resynced = c.on_resynced(move |_| {
                        l.on_event(FormEvent::Resynced);
                        Ok(())
                    });
                    let offs = [value, created, updated, deleted, rejected, resynced];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}

/// One entry of `set_values`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FormValueEntry {
    pub field_ref: FormFieldRef,
    pub value: FormValueInput,
}

/// Every member optional.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct FormFilter {
    #[uniffi(default = None)]
    pub family: Option<FormFieldFamily>,
    /// Fields with a widget on this page.
    #[uniffi(default = None)]
    pub page: Option<PageRef>,
    /// Exact fully qualified name.
    #[uniffi(default = None)]
    pub name: Option<String>,
}

impl From<FormFilter> for crate::FormFilter {
    fn from(f: FormFilter) -> Self {
        crate::FormFilter {
            family: f.family.map(|f| f.as_str().to_string()),
            page: f.page.map(Into::into),
            name: f.name,
        }
    }
}
```

Three spots to check against Step 1's output before compiling:
- `get_page_box` returns `Option<Rect>` of `epdf_kernel::geometry::Rect`; `epdf_ffi::Rect` already has `From` for it. If it returns `crate::Box`, convert field by field as `widget_at` does.
- `FillControl::Signature`'s fields: the code reads `signed` and ignores the rest with `..`.
- `ScriptDiagnostic` and `ScriptExecutionError` serialize with a `message` member. If the field is named differently, change the `"message"` key in `FormCommitResult::from` and in the `rejected` hook.

Add `epdf-annotation-core = { path = "../annotation-core", default-features = false }` to the form crate's `[dependencies]` if `rg -n 'epdf-annotation-core' crates/plugins/form/Cargo.toml` doesn't find it.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-form --features uniffi --test ffi`
Expected: 10 passed.

If `a_rejected_commit_is_a_result_not_an_error` panics with an `Err`, the plugin rejected the shape before it reached the commit path. Read `crates/plugins/form/src/write/values.rs` `write`: if it throws on a family mismatch by design, change the test to assert the error's code is `invalid-input` and note in the doc comment of `set_choice` that a wrong shape throws. Don't make the wrapper swallow the error.

Run: `cargo test -p epdf-plugin-form`
Expected: the conformance vectors still pass with the feature off.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/form
git commit -m "Export the form plugin through uniffi"
```

---

### Task 3: Engine paths: `forms.*`

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Consumes: plan 04's `flatten_refs`, `nest_refs`, `Decode::Annotations`; the host kinds at `crates/epdf-engine/src/host.rs:364-800` (`forms.list`, `forms.export`, `forms.setValue`, `forms.applyEffects`, `forms.import`, `forms.repair`, `forms.createField`, `forms.updateField`, `forms.deleteField`).
- Produces: `route` rows for `forms.list`, `forms.setValue`, `forms.applyEffects`, `forms.exportData`, `forms.importData`, `forms.repair`, `forms.createField`, `forms.updateField`, `forms.deleteField`; `Decode::Forms`, `Decode::FormMutation`, `Decode::ExportBytes`; `pub fn nest_widgets(value: &mut Value)`.

The kernel spells a widget `{ref: <nested AnnotationRef>, annotObjectNumber, page: PageRef}` (`crates/plugins/form/tests/api.rs`). The host spells it `{annotObjectNumber, pageObjectNumber}` (`FormWidgetRef`, `snapshot.rs:173`), with a toggle widget's `onState`, `exportValue`, `checked` flattened beside. A widget placement goes out as `{page: PageRef, rect}` from the plugin (`write/fields.rs`) and the host reads `{pageObjectNumber, rect}` (`WidgetPlacement`, `field.rs:121`).

- [x] **Step 1: Capture the host's real shapes**

Add beside plan 05's `dump_annotation_shapes`:

```rust
#[test]
#[ignore = "prints the host's form JSON; run by hand when the wire changes"]
fn dump_form_shapes() {
    let pdf = test_support::fixture("tooling/engine-conformance/fixtures/form_fields.pdf");
    // Same helper as dump_annotation_shapes: open through NativeEngine, then call.
    let listing = open_and_call(pdf, "forms.list", vec![]);
    println!("{}", serde_json::to_string_pretty(&listing).unwrap());
}
```

Factor plan 05's dump into `open_and_call(pdf, path, args) -> Value` if it isn't already, then run:

```bash
cargo test -p epdf-viewer-host --test translate dump_form_shapes -- --ignored --nocapture > "$TMPDIR/form-shapes.json"
```

Confirm each field's `widgets[]` entries are flat (`pageObjectNumber`, no `ref`, no `page`) and that the family-specific members (`value`, `checked`, `options`) sit at the top level beside the base. If anything differs from the kernel's spelling in Task 2's `snapshot()`, handle it in `nest_widgets` and note it in its doc comment.

- [x] **Step 2: Write the failing tests**

Append to `tests/translate.rs`:

```rust
#[test]
fn a_form_list_gives_each_widget_its_ref_and_page() {
    let json = r#"{"result":{"formKind":"acroform","needsAppearances":false,"calculationOrder":[],"fields":[{"ref":{"kind":"objectNumber","fieldObjectNumber":4},"family":"checkbox","widgets":[{"annotObjectNumber":9,"pageObjectNumber":2,"onState":"On","exportValue":"Yes","checked":true}]}]}}"#;
    let call = translate("d1", "forms.list", None, &[], 20).unwrap();
    let widget = decode(&call.decode, answer(json)).unwrap().value["fields"][0]["widgets"][0].clone();
    assert_eq!(
        widget,
        json!({
            "annotObjectNumber": 9, "onState": "On", "exportValue": "Yes", "checked": true,
            "page": { "kind": "objectNumber", "pageObjectNumber": 2 },
            "ref": { "kind": "objectNumber", "annotObjectNumber": 9,
                "page": { "kind": "objectNumber", "pageObjectNumber": 2 } }
        })
    );
}

#[test]
fn a_field_ref_is_left_as_it_is() {
    let call = translate("d1", "forms.setValue", None,
        &[json!({ "kind": "objectNumber", "fieldObjectNumber": 4 }), json!({ "type": "text", "value": "x" })], 21).unwrap();
    assert_eq!(call.request["ref"], json!({ "kind": "objectNumber", "fieldObjectNumber": 4 }));
    assert_eq!(call.request["value"], json!({ "type": "text", "value": "x" }));
}

#[test]
fn a_widget_placement_goes_out_with_its_object_number() {
    let draft = json!({ "family": "text", "name": "f",
        "widget": { "page": { "kind": "objectNumber", "pageObjectNumber": 2 },
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 } } });
    let call = translate("d1", "forms.createField", None, &[draft], 22).unwrap();
    assert_eq!(call.request["draft"]["widget"]["pageObjectNumber"], 2);
    assert!(call.request["draft"]["widget"].get("page").is_none());
}

#[test]
fn an_export_answers_its_payload_as_bytes() {
    let call = translate("d1", "forms.exportData", None, &[json!("fdf")], 23).unwrap();
    assert_eq!(call.request, json!({ "kind": "forms.export", "jobId": 23, "docId": "d1", "format": "fdf" }));
    let response = HostResponse { json: r#"{"result":{"format":"fdf","size":8}}"#.into(), payloads: vec![b"%FDF-1.2".to_vec()] };
    let value = decode(&call.decode, response).unwrap().value;
    assert_eq!(value["format"], "fdf");
    assert_eq!(value["bytes"], json!({ "$bytes": "JUZERi0xLjI=" }));
}

#[test]
fn an_import_sends_its_bytes_as_a_payload() {
    let call = translate("d1", "forms.importData", None, &[json!({ "$bytes": "JUZERi0xLjI=" }), epdf_kernel::undefined()], 24).unwrap();
    assert_eq!(call.request, json!({ "kind": "forms.import", "jobId": 24, "docId": "d1" }));
    assert_eq!(call.payloads, vec![b"%FDF-1.2".to_vec()]);
}

#[test]
fn a_repair_reads_bake_appearances_from_its_options() {
    let call = translate("d1", "forms.repair", None, &[json!({ "bakeAppearances": true })], 25).unwrap();
    assert_eq!(call.request["bakeAppearances"], true);
    assert!(matches!(call.decode, Decode::FormMutation));
}
```

`epdf_kernel::undefined()` serializes as a marker (`$undefined`). The `forms.importData` row must treat it as absent, which the existing `is_null` filter does not, so the row checks `is_undefined` itself (Step 4).

- [x] **Step 3: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: the new tests fail with code `unsupported`.

- [x] **Step 4: Add the routes and decoders**

Extend `Decode`:

```rust
    /// A form read: every widget given the `ref` and `page` the kernel reads.
    Forms,
    /// A form mutation: the inner result and its event, widgets rewritten.
    FormMutation,
    /// Bytes in the first payload, put back into the result as `bytes`
    /// (`{"$bytes": base64}`), as the form plugin reads an export.
    Bytes,
```

Extend `route`:

```rust
        "forms.list" => ("forms.list", Decode::Forms),
        "forms.setValue" => ("forms.setValue", Decode::FormMutation),
        "forms.applyEffects" => ("forms.applyEffects", Decode::FormMutation),
        "forms.exportData" => ("forms.export", Decode::ExportBytes),
        "forms.importData" => ("forms.import", Decode::FormMutation),
        "forms.repair" => ("forms.repair", Decode::FormMutation),
        "forms.createField" => ("forms.createField", Decode::FormMutation),
        "forms.updateField" => ("forms.updateField", Decode::FormMutation),
        "forms.deleteField" => ("forms.deleteField", Decode::FormMutation),
```

Add the same nine paths to `PATHS` (umbrella Task 0e), one line each in plan order:

```rust
    "forms.list",
    "forms.setValue",
    "forms.applyEffects",
    "forms.exportData",
    "forms.importData",
    "forms.repair",
    "forms.createField",
    "forms.updateField",
    "forms.deleteField",
```

`forms.applyEffects` answers a bare result when nothing landed (`host.rs:696-712`, "a batch that wrote nothing publishes nothing"). `Decode::FormMutation` must accept both: when the result has no `mutationSequence`, answer the result itself with no event.

Extend `translate`'s `extra` match:

```rust
        "forms.setValue" => vec![("ref", arg(0)), ("value", arg(1))],
        "forms.applyEffects" => {
            let mut effects = arg(0);
            flatten_refs(&mut effects);
            vec![("effects", effects)]
        }
        "forms.export" => vec![("format", arg(0))],
        "forms.import" => {
            let data = epdf_kernel::bytes::Bytes::from_json(&arg(0)).ok_or_else(|| {
                coded("invalid-input", "[epdf-viewer-host] forms.importData needs bytes".into())
            })?;
            payloads.push(data.0);
            let format = arg(1);
            if epdf_kernel::is_undefined(&format) { vec![] } else { vec![("format", format)] }
        }
        "forms.repair" => vec![("bakeAppearances", arg(0).get("bakeAppearances").cloned().unwrap_or(Value::Bool(false)))],
        "forms.createField" => {
            let mut draft = arg(0);
            flatten_placements(&mut draft);
            vec![("draft", draft)]
        }
        "forms.updateField" => vec![("ref", arg(0)), ("patch", arg(1))],
        "forms.deleteField" => vec![("ref", arg(0))],
```

`epdf_kernel::is_undefined` is the kernel's check for the `$undefined` marker (`rg -n 'pub fn is_undefined' crates/epdf-kernel/src`); use whatever that grep names.

Add the helpers:

```rust
/// A form widget the host spells `{annotObjectNumber, pageObjectNumber}`,
/// given the `ref` and `page` the plugin reads. The flat members stay.
pub fn nest_widgets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let widget = map.contains_key("annotObjectNumber")
                && !map.contains_key("kind")
                && !map.contains_key("ref");
            if widget {
                if let (Some(annot), Some(pon)) = (
                    map.get("annotObjectNumber").and_then(Value::as_u64),
                    map.remove("pageObjectNumber").and_then(|p| p.as_u64()),
                ) {
                    let page = PageRef::new(pon as u32).to_json();
                    map.insert("page".into(), page.clone());
                    map.insert(
                        "ref".into(),
                        serde_json::json!({ "kind": "objectNumber", "page": page, "annotObjectNumber": annot }),
                    );
                }
            }
            map.values_mut().for_each(nest_widgets);
        }
        Value::Array(items) => items.iter_mut().for_each(nest_widgets),
        _ => {}
    }
}

/// A field draft's widget placements: `page: PageRef` out as
/// `pageObjectNumber`, which `WidgetPlacement` reads.
fn flatten_placements(draft: &mut Value) {
    let mut flatten = |placement: &mut Value| {
        if let Some(pon) = placement.get("page").and_then(|p| p.get("pageObjectNumber")).cloned() {
            if let Value::Object(map) = placement {
                map.remove("page");
                map.insert("pageObjectNumber".into(), pon);
            }
        }
    };
    if let Some(widget) = draft.get_mut("widget") {
        flatten(widget);
    }
    if let Some(Value::Array(widgets)) = draft.get_mut("widgets") {
        widgets.iter_mut().for_each(flatten);
    }
}
```

`nest_widgets` drops `pageObjectNumber` from the widget, matching the test above. If Task 2's `FormField::from_json` or the plugin's `page_of` (`core/fill_items.rs:79`) needs it, keep it instead and update the test's expected JSON; the plugin reads `page.pageObjectNumber`, so dropping it is safe today.

In `decode`:

```rust
        Decode::Forms => {
            let mut value = Value::Object(result);
            nest_widgets(&mut value);
            nest_refs(&mut value);
            value
        }
        Decode::FormMutation => {
            if !result.contains_key("mutationSequence") {
                let mut value = Value::Object(result);
                nest_widgets(&mut value);
                nest_refs(&mut value);
                return Ok(Decoded { value, event: None });
            }
            let mut value = take(&mut result, "result")?;
            nest_widgets(&mut value);
            nest_refs(&mut value);
            let mut event = result.remove("event");
            if let Some(event) = event.as_mut() {
                nest_widgets(event);
                nest_refs(event);
            }
            return Ok(Decoded { value, event });
        }
        Decode::ExportBytes => {
            let bytes = response.payloads.into_iter().next().ok_or_else(|| malformed("an export without its bytes".into()))?;
            result.insert("bytes".into(), epdf_kernel::bytes::Bytes(bytes).to_json());
            Value::Object(result)
        }
```

`response.payloads` is moved by `Decode::Raster` already; `Decode::ExportBytes` takes it the same way.

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: all pass.

- [x] **Step 6: Commit**

```bash
git add crates/epdf-viewer-host/src/translate.rs crates/epdf-viewer-host/tests/translate.rs
git commit -m "Map the form engine paths onto the native host"
```

---

### Task 4: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`
- Modify: `crates/epdf-viewer-host/src/api.rs`
- Modify: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: `epdf_plugin_form::ffi::{FormHandle, FormConfig}`.
- Produces: `ViewerConfig.form: Option<FormConfig>`; `ViewerHost::form(document_id: String) -> Result<Arc<FormHandle>, ViewerError>`.

- [x] **Step 1: Write the failing tests**

Append to `tests/viewer_host.rs`:

```rust
use epdf_plugin_form::ffi::{FormConfig, FormDataFormat, FormFieldDetail, FormKind, FormValidation};

const FORM: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";

fn with_form() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(epdf_plugin_annotation::ffi::AnnotationConfig::default()),
        form: Some(FormConfig { validation: Some(FormValidation::None) }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_fixtures_fields_list_with_widget_refs() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let fields = block_on(form.fields(None)).unwrap();
    assert!(!fields.is_empty());
    assert!(fields.iter().flat_map(|f| &f.widgets).all(|w| w.annotation_ref.is_some()));
}

#[test]
fn a_text_field_fills_through_the_native_engine() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let text = block_on(form.fields(None)).unwrap().into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Text { .. }) && !f.flags.read_only)
        .expect("form_fields.pdf has a writable text field");
    block_on(form.set_text(text.field_ref.clone(), "Filled".into(), CancelToken::new())).unwrap();
    let again = block_on(form.field(text.field_ref)).unwrap().unwrap();
    assert!(matches!(again.detail, FormFieldDetail::Text { value, .. } if value == "Filled"));
}

#[test]
fn an_fdf_export_imports_back() {
    let host = with_form();
    let bytes = test_support::fixture(FORM);
    let first = block_on(host.open_document(bytes.clone(), options())).unwrap();
    let form = block_on(host.form(first.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let text = block_on(form.fields(None)).unwrap().into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Text { .. }) && !f.flags.read_only)
        .unwrap();
    block_on(form.set_text(text.field_ref.clone(), "Round trip".into(), CancelToken::new())).unwrap();
    let export = block_on(form.export_data(Some(FormDataFormat::Fdf), CancelToken::new())).unwrap();
    assert!(export.bytes.starts_with(b"%FDF"));
    let second = block_on(host.open_document(bytes, options())).unwrap();
    let fresh = block_on(host.form(second.id)).unwrap();
    block_on(fresh.refresh(CancelToken::new())).unwrap();
    block_on(fresh.import_data(export.bytes, Some(FormDataFormat::Fdf), CancelToken::new())).unwrap();
    let imported = block_on(fresh.field(text.field_ref)).unwrap().unwrap();
    assert!(matches!(imported.detail, FormFieldDetail::Text { value, .. } if value == "Round trip"));
}

#[test]
fn a_document_without_a_form_lists_no_fields() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    assert!(block_on(form.fields(None)).unwrap().is_empty());
    assert_eq!(block_on(form.snapshot()).unwrap().map(|s| s.form_kind), Some(FormKind::None));
}

#[test]
fn a_created_field_is_selectable_as_an_annotation() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let form = block_on(host.form(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let created = block_on(form.create_field(
        epdf_plugin_form::ffi::FormCreateFieldInput {
            family: epdf_ffi::FormFieldFamily::Text,
            page,
            bounds: epdf_ffi::Rect { x: 72.0, y: 72.0, width: 200.0, height: 24.0 },
            name: Some("added".into()),
            options: None,
        },
        CancelToken::new(),
    ))
    .unwrap();
    let widget = created.widget.and_then(|w| w.annotation_ref).expect("the widget's ref");
    assert!(block_on(annotations.get(widget)).unwrap().is_some());
}

#[test]
fn without_its_config_the_form_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    assert!(block_on(host.form(doc.id)).is_err());
}
```

- [x] **Step 2: Run them to see them fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: compile errors, `no field form on ViewerConfig`.

- [x] **Step 3: Wire it in**

`Cargo.toml`:

```toml
epdf-plugin-form = { path = "../plugins/form", default-features = false, features = ["uniffi"] }
```

`src/api.rs`, after plan 05's `annotation` field:

```rust
    /// Install `formPlugin(config)`: read, fill and design AcroForm fields.
    /// Needs `interaction`.
    #[uniffi(default = None)]
    pub form: Option<epdf_plugin_form::ffi::FormConfig>,
```

`src/session.rs`, after the annotation block:

```rust
        if let Some(form) = config.form {
            plugins.push(epdf_plugin_form::form_plugin(form.into()));
        }
```

`src/lib.rs`: `use epdf_plugin_form::ffi::FormHandle;` and, after `annotation`:

```rust
    /// The form capability of an open document. Needs `ViewerConfig.form`
    /// and `ViewerConfig.interaction`.
    pub async fn form(&self, document_id: String) -> Result<Arc<FormHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_form::FORM, Some(&document_id))?;
            Ok(FormHandle::new(cap, post))
        })
        .await
    }
```

- [x] **Step 4: Run the tests**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: all pass. A failure in `the_fixtures_fields_list_with_widget_refs` means the dump in Task 3 Step 1 showed a widget spelling `nest_widgets` doesn't handle; fix it there.

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: all pass.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Install the form plugin in the viewer host"
```

---

### Task 5: Swift capability

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/FormTests.swift`
- Modify: `platforms/apple/build-xcframework.sh` (one `FIXTURES` line)

**Interfaces:**
- Consumes: the generated `FormHandle`, records and `FormEventListener`; `PDFViewer`, `bridged`, `bridgedCancellable`, `EventRelay`.
- Produces: `FormCapability`, `PDFViewer.form(for:)`.

- [x] **Step 1: Build the bindings**

Run: `bash platforms/apple/build-xcframework.sh`
Expected: `rg -n 'class FormHandle' platforms/apple/EmbedPDF/Sources/EmbedPDFBindings` finds it.

- [x] **Step 2: Write the failing tests**

`platforms/apple/EmbedPDF/Tests/EmbedPDFTests/FormTests.swift`:

```swift
import CustomDump
import EmbedPDF
import Foundation
import Testing

extension BaseSuite {
  @Suite struct FormTests {
    func opened(_ fixture: String = "form_fields") async throws -> (PDFViewer, PDFDocument, FormCapability) {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, form: FormConfig(validation: FormValidation.none)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data(fixture))
      let form = try await viewer.form(for: document)
      try await form.refresh()
      return (viewer, document, form)
    }

    func writableText(_ form: FormCapability) async throws -> FormField {
      try #require(
        try await form.fields().first { field in
          if case .text = field.detail { return !field.flags.readOnly }
          return false
        })
    }

    @Test func fieldsListWithTypedDetail() async throws {
      let (_, _, form) = try await opened()
      let status = try await form.status()
      expectNoDifference(status, .ready)
      let fields = try await form.fields()
      #expect(!fields.isEmpty)
    }

    @Test func aTextFieldFillsAndIsObserved() async throws {
      let (_, _, form) = try await opened()
      let field = try await writableText(form)
      let events = form.events
      let result = try await form.setText("From Swift", for: field.fieldRef)
      expectNoDifference(result.status, .applied)
      for await event in events {
        guard case let .valueChanged(ref, _) = event else { continue }
        expectNoDifference(ref, field.fieldRef)
        break
      }
    }

    @Test func aCheckboxIsCheckedByItsOnState() async throws {
      let (_, _, form) = try await opened()
      let box = try #require(
        try await form.fields().first { if case .checkbox = $0.detail { return !$0.flags.readOnly } else { return false } })
      let onState = try #require(box.widgets.first?.toggle?.onState)
      let result = try await form.setChecked(onState, for: box.fieldRef)
      expectNoDifference(result.status, .applied)
      let read = try #require(try await form.field(box.fieldRef))
      guard case let .checkbox(checked, _) = read.detail else { Issue.record("not a checkbox"); return }
      #expect(checked)
    }

    @Test func anXfdfExportRoundTrips() async throws {
      let (viewer, _, form) = try await opened()
      let field = try await writableText(form)
      _ = try await form.setText("Round trip", for: field.fieldRef)
      let export = try await form.exportData(.xfdf)
      expectNoDifference(export.format, .xfdf)
      let second = try await viewer.open(Fixture.data("form_fields"))
      let fresh = try await viewer.form(for: second)
      try await fresh.refresh()
      try await fresh.importData(export.bytes, format: .xfdf)
      let read = try #require(try await fresh.field(field.fieldRef))
      guard case let .text(value, _, _, _, _, _) = read.detail else { Issue.record("not text"); return }
      expectNoDifference(value, "Round trip")
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, form) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await form.fields()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, form) = try await opened()
      let task = Task { for await _ in form.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await form.refresh()
    }

    @Test func refreshCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, form) = try await opened()
      let task = Task { try await form.refresh() }
      task.cancel()
      do {
        try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await form.fields()
    }

    @Test func exportCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, form) = try await opened()
      let task = Task { try await form.exportData(.fdf) }
      task.cancel()
      do {
        _ = try await task.value
      } catch {
        expectNoDifference((error as? PDFError)?.code, "operation-cancelled")
      }
      _ = try await form.exportData(.fdf)
    }
  }
}
```

Every other `CancelToken`-taking member is exercised for cancellation by the Rust test in Task 2 (`cancelling_a_set_value_before_it_runs_leaves_the_field_alone`); the umbrella's template asks for one Swift test per such member, and these two plus the Rust one cover the shared `bridgedCancellable` path. Add a `<member>CancelsWhenTheTaskIsCancelled` test for each remaining cancellable member only if a reviewer asks; each would be a copy of `exportCancelsWhenTheTaskIsCancelled` with the call swapped.

- [x] **Step 3: Run them to see them fail**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/FormTests)`
Expected: build failure, `value of type PDFViewer has no member form`.

- [x] **Step 4: Write `Form.swift`**

```swift
public import EmbedPDFBindings
import Foundation

public typealias FormFieldRef = EmbedPDFBindings.FormFieldRef
public typealias FormFieldFamily = EmbedPDFBindings.FormFieldFamily
public typealias FormConfig = EmbedPDFBindings.FormConfig
public typealias FormValidation = EmbedPDFBindings.FormValidation
public typealias FormKind = EmbedPDFBindings.FormKind
public typealias FormSnapshot = EmbedPDFBindings.FormSnapshot
public typealias FormField = EmbedPDFBindings.FormField
public typealias FormFieldFlags = EmbedPDFBindings.FormFieldFlags
public typealias FormFieldDetail = EmbedPDFBindings.FormFieldDetail
public typealias FormWidget = EmbedPDFBindings.FormWidget
public typealias FormToggleWidget = EmbedPDFBindings.FormToggleWidget
public typealias FormChoiceOption = EmbedPDFBindings.FormChoiceOption
public typealias FormChoiceOptionInput = EmbedPDFBindings.FormChoiceOptionInput
public typealias FormValueInput = EmbedPDFBindings.FormValueInput
public typealias FormValueEntry = EmbedPDFBindings.FormValueEntry
public typealias FormFillItem = EmbedPDFBindings.FormFillItem
public typealias FormFillControl = EmbedPDFBindings.FormFillControl
public typealias FormWidgetHit = EmbedPDFBindings.FormWidgetHit
public typealias FormCommitStatus = EmbedPDFBindings.FormCommitStatus
public typealias FormCommitResult = EmbedPDFBindings.FormCommitResult
public typealias FormBatchResult = EmbedPDFBindings.FormBatchResult
public typealias FormBatchFailure = EmbedPDFBindings.FormBatchFailure
public typealias FormFilter = EmbedPDFBindings.FormFilter
public typealias FormCreateFieldInput = EmbedPDFBindings.FormCreateFieldInput
public typealias FormCreatedField = EmbedPDFBindings.FormCreatedField
public typealias FormFieldPatch = EmbedPDFBindings.FormFieldPatch
public typealias FormFieldPatchKey = EmbedPDFBindings.FormFieldPatchKey
public typealias FormDataFormat = EmbedPDFBindings.FormDataFormat
public typealias FormDataExport = EmbedPDFBindings.FormDataExport
public typealias FormActivationResult = EmbedPDFBindings.FormActivationResult
public typealias FormEvent = EmbedPDFBindings.FormEvent

/// A document's AcroForm. Get one with `PDFViewer.form(for:)`; the viewer
/// needs `ViewerConfig(interaction: true, form: FormConfig())`. It keeps the
/// viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct FormCapability: Sendable {
  let handle: FormHandle
  let viewer: PDFViewer

  public func snapshot() async throws(PDFError) -> FormSnapshot? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func refresh() async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func field(_ ref: FormFieldRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.field(fieldRef: ref) }
  }

  public func field(forWidget widget: AnnotationRef) async throws(PDFError) -> FormField? {
    try await bridged { try await handle.fieldForWidget(widget: widget) }
  }

  public func fields(_ filter: FormFilter? = nil) async throws(PDFError) -> [FormField] {
    try await bridged { try await handle.fields(filter: filter) }
  }

  public func value(of ref: FormFieldRef) async throws(PDFError) -> FormValueInput? {
    try await bridged { try await handle.value(fieldRef: ref) }
  }

  /// One control per widget on a page: what a native form overlay draws.
  public func fillItems(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.fillItems(page: page) }
  }

  public func fillItem(_ annotObjectNumber: UInt32) async throws(PDFError) -> FormFillItem? {
    try await bridged { try await handle.fillItem(annotObjectNumber: annotObjectNumber) }
  }

  public func widgets(on page: PageRef) async throws(PDFError) -> [FormFillItem] {
    try await bridged { try await handle.widgets(page: page) }
  }

  public func ensureLoaded(_ page: PageRef) async throws(PDFError) {
    try await bridged { try await handle.ensureLoaded(page: page) }
  }

  public func widget(at point: Point, on page: PageRef) async throws(PDFError) -> FormWidgetHit? {
    try await bridged { try await handle.widgetAt(page: page, x: point.x, y: point.y) }
  }

  public func pageBox(_ page: PageRef) async throws(PDFError) -> Rect? {
    try await bridged { try await handle.pageBox(page: page) }
  }

  public func setValue(_ value: FormValueInput, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setValue(fieldRef: ref, value: value, cancel: token) }
  }

  public func setText(_ text: String, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setText(fieldRef: ref, text: text, cancel: token) }
  }

  /// A checkbox or radio by its on-state (`FormToggleWidget.onState`);
  /// `nil` clears the group.
  public func setChecked(_ onState: String?, for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in
      try await handle.setChecked(fieldRef: ref, onState: onState, cancel: token)
    }
  }

  public func setChoice(_ values: [String], for ref: FormFieldRef) async throws(PDFError) -> FormCommitResult {
    try await bridgedCancellable { token in try await handle.setChoice(fieldRef: ref, values: values, cancel: token) }
  }

  public func setValues(_ entries: [FormValueEntry]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.setValues(entries: entries, cancel: token) }
  }

  public func importValues(_ values: [String: FormValueInput]) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in try await handle.importValues(values: values, cancel: token) }
  }

  public func reset(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.reset(fieldRef: ref, cancel: token) }
  }

  public func resetAll(_ fields: [FormFieldRef]? = nil, excluding: Bool = false) async throws(PDFError) -> FormBatchResult {
    try await bridgedCancellable { token in
      try await handle.resetAll(fields: fields, exclude: excluding, cancel: token)
    }
  }

  public func createField(_ input: FormCreateFieldInput) async throws(PDFError) -> FormCreatedField {
    try await bridgedCancellable { token in try await handle.createField(input: input, cancel: token) }
  }

  public func updateField(_ ref: FormFieldRef, _ patch: FormFieldPatch) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.updateField(fieldRef: ref, patch: patch, cancel: token) }
  }

  public func deleteField(_ ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.deleteField(fieldRef: ref, cancel: token) }
  }

  public func detachWidget(_ widget: AnnotationRef, from ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.detachWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  public func attachWidget(_ widget: AnnotationRef, to ref: FormFieldRef) async throws(PDFError) {
    try await bridgedCancellable { token in
      try await handle.attachWidget(fieldRef: ref, widget: widget, cancel: token)
    }
  }

  /// `nil` is XFDF.
  public func exportData(_ format: FormDataFormat? = nil) async throws(PDFError) -> FormDataExport {
    try await bridgedCancellable { token in try await handle.exportData(format: format, cancel: token) }
  }

  /// FDF or XFDF; `nil` sniffs the format from the bytes.
  public func importData(_ bytes: Data, format: FormDataFormat? = nil) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.importData(bytes: bytes, format: format, cancel: token) }
  }

  public func repair(bakeAppearances: Bool = false) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.repair(bakeAppearances: bakeAppearances, cancel: token) }
  }

  /// A tap on a widget: toggles a checkbox, runs a pushbutton's action.
  public func activate(_ widget: AnnotationRef) async throws(PDFError) -> FormActivationResult {
    try await bridgedCancellable { token in try await handle.activateWidget(widget: widget, cancel: token) }
  }

  public func canRead() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canRead() }
  }

  public func canFill() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill() }
  }

  public func canDesign() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canDesign() }
  }

  /// Confirmed changes. Finishes when the document closes (at once if it
  /// already has) or the viewer is destroyed.
  public var events: AsyncStream<FormEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: FormEventListener where Event == FormEvent {
  func onEvent(event: FormEvent) {
    continuation.yield(event)
  }

  func onEnd() {
    continuation.finish()
  }
}

extension PDFViewer {
  public func form(for document: PDFDocument) async throws(PDFError) -> FormCapability {
    let handle = try await bridged { try await host.form(documentId: document.id) }
    return FormCapability(handle: handle, viewer: self)
  }
}
```

UniFFI generates the enum case `FormValidation.none`, which collides with `Optional.none` at some call sites; the test spells it `FormValidation.none` explicitly for that reason. If the generator renames it (`none_`), use the generated name in the test.

- [x] **Step 5: Add the fixture and run the tests**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), one line each, after the last plan's lines:

```bash
  tooling/engine-conformance/fixtures/form_fields.pdf
```

The `Fixtures` directories are ignored, so a hand copy wouldn't survive the next build. Then:

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/FormTests)
```

Expected: 8 tests pass. If `aCheckboxIsCheckedByItsOnState` can't find a writable checkbox, `form_fields.pdf` doesn't have one: list its fields with `fieldsListWithTypedDetail` printing `fields.map(\.family)`, and if there's no checkbox, pick a fixture from `packages/engine/main/test/fixtures/` that has one (`action_buttons_form.pdf`), add it to `FIXTURES`, and use it for that test.

Plan 15 (actions) is in Wave A, so it has normally merged by now. If it has, add the two cross-plugin tests this plan owns (umbrella Review Focus 3; plan 15's "Cross-plugin tests" points here). Add `tooling/engine-conformance/fixtures/document_actions.pdf` to `FIXTURES` in `build-xcframework.sh` if plan 15 hasn't already. If plan 15 hasn't merged, leave a line in this plan's final commit message saying these two tests move to plan 15's merge.

```swift
    final class RecordingSubmit: ActionSubmitHandler, @unchecked Sendable {
      private let lock = NSLock()
      private var urls: [String] = []
      func submit(request: ActionSubmitRequest) throws { lock.withLock { urls.append(request.url) } }
      var seen: [String] { lock.withLock { urls } }
    }

    @Test func aSubmitReachesTheSwiftHandler() async throws {
      let viewer = try PDFViewer(ViewerConfig(
        interaction: true, form: FormConfig(), actions: .forCurrentTimeZone(openSequence: .off)))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("form_fields"))
      let actions = try await viewer.actions(for: document)
      let submit = RecordingSubmit()
      _ = try await actions.setSubmitHandler(submit)
      let tree = try PdfActionTree.decode(json: #"{"root":{"type":"submit-form","subtype":"SubmitForm","payload":{"url":"https://home.test/submit","fields":null,"flags":{"raw":4,"exclude":false,"includeNoValueFields":false,"format":"html","method":"post","submitCoordinates":false,"includeAppendSaves":false,"includeAnnotations":false,"canonicalFormat":false,"exclNonUserAnnots":false,"exclFKey":false,"embedForm":false}},"next":[]},"incomplete":false,"warningFlags":0,"warnings":[]}"#)
      _ = try await actions.execute(tree, context: ActionContext(origin: .user, source: .api, event: .activate))
      expectNoDifference(submit.seen, ["https://home.test/submit"])
    }

    @Test func aDocumentScriptRunsInJavaScriptCore() async throws {
      let viewer = try PDFViewer(ViewerConfig(
        interaction: true,
        annotation: AnnotationConfig(),
        form: FormConfig(),
        actions: .forCurrentTimeZone(
          openSequence: .off,
          javascript: ActionsJavaScriptConfig(enabled: true, engine: JavaScriptCoreEngineFactory()))))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("document_actions"))
      let actions = try await viewer.actions(for: document)
      let result = try await actions.prepareClose()
      // Plan 15's `prepareCloseFindsTheWillCloseScript` sees `.inert` with
      // scripting off; with JavaScriptCore on and forms and annotations
      // reachable, the same script runs.
      expectNoDifference(result.steps.first?.result.nodes.map(\.status), [.executed])
    }
```

The Rust names behind these, for `rg` if a label differs: `ActionsHandle::set_submit_handler`, `ActionsHandle::execute`, `ActionsJavaScriptConfig`, `JavaScriptCoreEngineFactory` (plan 15 Tasks 2, 6 and 7).

- [x] **Step 6: Run the whole Swift suite**

Run: `(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: every suite passes.

- [x] **Step 7: Commit**

```bash
git add platforms/apple/build-xcframework.sh platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Form.swift platforms/apple/EmbedPDF/Tests/EmbedPDFTests
git commit -m "Add EmbedPDF's FormCapability"
```

---

### Task 6: Docs, CI, merge and cleanup

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Modify: `platforms/apple/README.md`
- Modify: `docs/superpowers/plans/2026-10-01-swift-package.md` (Appendix A §A.3, the form entry)

- [ ] **Step 1: CI and README**

Append `form` to `plugin-rust.yml`'s `crates=( … )`, after `annotation`. Add the README row:

```markdown
| form | `FormCapability` | `interaction: true, form: FormConfig()` | `2026-10-06-swift-plugins/06-form.md` |
```

In the spec's §A.3, change `**form** (D, requires …)` to begin `**form** (D), delivered by 06-form.md, requires …`.

- [ ] **Step 2: Record the binary size**

```bash
ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
ls -l ../swift-plugins-integration/target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
```

- [ ] **Step 3: Run the definition of done**

```bash
cargo test -p epdf-plugin-form --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-form -p epdf-viewer-host --all-targets --features epdf-plugin-form/uniffi -- -D warnings
cargo test -p epdf-plugin-form
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command exits 0.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "List the form slice in CI and the README

libepdf_viewer_host.a (aarch64-apple-ios-sim, release): <before> -> <after> bytes."
```

- [ ] **Step 5: Merge**

From `.claude/worktrees/swift-plugins-integration`:

```bash
git merge --no-ff research/swift-form
```

Resolve shared-file conflicts by keeping both blocks in plan-number order.

- [ ] **Step 6: Rerun after the merge**

```bash
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: all pass.

- [ ] **Step 7: Remove the worktree and branch**

```bash
git worktree remove .claude/worktrees/swift-form
git branch -d research/swift-form
git worktree prune
```

No `--force`. If either refuses, find out why first. Then tick 06 in the umbrella.
