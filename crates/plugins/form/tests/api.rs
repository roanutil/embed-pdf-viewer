//! The typed surface the vectors cannot see: the fill projection hands back
//! the same `Rc` while the model and the fill grant are unchanged, and the
//! key order of the objects `exportValues` and `importValues` take and give
//! (the vectors compare objects without order, and print their arguments in
//! JavaScript's order already).

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_form::{FORM, FormConfig, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::{Map, Value, json};

/// A text field on page 1, object number `n`, widget `n + 100`.
fn text_field(n: u32, name: &str, value: &str) -> Value {
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 1 });
    let widget_ref = json!({ "kind": "objectNumber", "page": page, "annotObjectNumber": n + 100 });
    json!({
        "ref": { "kind": "objectNumber", "fieldObjectNumber": n },
        "fieldObjectNumber": n, "name": name, "family": "text", "origin": "acroform",
        "flags": { "readOnly": false, "required": false, "noExport": false, "raw": 0 },
        "alternateName": null, "mappingName": null,
        "widgets": [{ "ref": widget_ref, "annotObjectNumber": n + 100, "page": page }],
        "valueEntry": { "kind": "scalar", "value": value }, "defaultValueEntry": { "kind": "none" },
        "value": value, "defaultValue": "", "maxLength": null,
        "multiline": false, "password": false, "comb": false
    })
}

fn kernel() -> Kernel {
    kernel_with(vec![text_field(1, "name", "Ada")])
}

fn kernel_with(fields: Vec<Value>) -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 1 });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "security": { "allows": "*" },
            "pages": [{ "index": 0, "ref": page, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ } }],
            "responses": [
                { "path": "forms.list", "value": {
                    "formKind": "acroform", "needsAppearances": false,
                    "fields": fields, "calculationOrder": [] } },
                { "path": "forms.setValue", "value": { "changedWidgets": [] } }
            ]
        } }
    }));
    let kernel = Kernel::new(
        Rc::new(AsEngine(fake)),
        vec![interaction_plugin(InteractionConfig::default()), form_plugin(FormConfig::default())],
        KernelOptions::default(),
    )
    .expect("kernel");
    kernel.start();
    rt::run_until_stalled();
    kernel.documents().open(
        OpenSource::Input(json!({ "kind": "bytes", "id": "a", "bytes": { "$bytes": "" } })),
        OpenOptions::default(),
    );
    rt::run_until_stalled();
    kernel
}

#[test]
fn the_fill_projection_is_reference_stable() {
    let kernel = kernel();
    let form = kernel.capability(&FORM, Some("a")).expect("form");
    let items = form.list_fill_items(PageRef::new(1));
    assert!(Rc::ptr_eq(&items, &form.list_fill_items(PageRef::new(1))));
    assert!(Rc::ptr_eq(&items, &form.list_widgets(PageRef::new(1))));
    let item = form.get_fill_item(101.0).expect("the widget's fill item");
    assert!(Rc::ptr_eq(&item, &form.get_fill_item(101.0).expect("again")));
}

/// Names a JavaScript object orders differently from their insertion order.
const NAMES: [&str; 5] = ["b", "10", "2", "a", "02"];
/// `Object.keys` of an object built in `NAMES` order.
const JS_ORDER: [&str; 5] = ["2", "10", "b", "a", "02"];

#[test]
fn export_values_keys_follow_javascript_object_order() {
    let kernel =
        kernel_with(NAMES.iter().zip(1..).map(|(name, n)| text_field(n, name, name)).collect());
    let form = kernel.capability(&FORM, Some("a")).expect("form");
    let values = form.export_values();
    assert_eq!(values.keys().map(String::as_str).collect::<Vec<_>>(), JS_ORDER);
}

#[test]
fn import_values_writes_in_javascript_object_order() {
    let kernel =
        kernel_with(NAMES.iter().zip(1..).map(|(name, n)| text_field(n, name, "")).collect());
    let form = kernel.capability(&FORM, Some("a")).expect("form");
    let mut values = Map::new();
    for name in NAMES {
        values.insert(name.into(), json!({ "type": "text", "value": name }));
    }
    let result = form.import_values(values);
    rt::run_until_stalled();
    let result = result.peek().expect("settled").expect("resolved");
    assert!(result.failed.is_empty(), "{:?}", result.failed.len());
    let written: Vec<&str> =
        result.applied.iter().map(|r| r["name"].as_str().unwrap_or_default()).collect();
    assert_eq!(written, JS_ORDER);
}
