//! The typed surface the vectors cannot see: the fill projection hands back
//! the same `Rc` while the model and the fill grant are unchanged.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use epdf_kernel::kernel::{OpenOptions, OpenSource};
use epdf_kernel::{Kernel, KernelOptions, rt};
use epdf_plugin_form::{FORM, FormConfig, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::fake_engine::{AsEngine, FakeEngine};
use serde_json::json;

fn kernel() -> Kernel {
    rt::reset();
    let box_ = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let page = json!({ "kind": "objectNumber", "pageObjectNumber": 1 });
    let widget_ref = json!({ "kind": "objectNumber", "page": page, "annotObjectNumber": 101 });
    let field = json!({
        "ref": { "kind": "objectNumber", "fieldObjectNumber": 1 },
        "fieldObjectNumber": 1, "name": "name", "family": "text", "origin": "acroform",
        "flags": { "readOnly": false, "required": false, "noExport": false, "raw": 0 },
        "alternateName": null, "mappingName": null,
        "widgets": [{ "ref": widget_ref, "annotObjectNumber": 101, "page": page }],
        "valueEntry": { "kind": "scalar", "value": "Ada" }, "defaultValueEntry": { "kind": "none" },
        "value": "Ada", "defaultValue": "", "maxLength": null,
        "multiline": false, "password": false, "comb": false
    });
    let fake = FakeEngine::new(json!({
        "documents": { "a": {
            "security": { "allows": "*" },
            "pages": [{ "index": 0, "ref": page, "label": null,
                "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                "boxes": { "media": box_, "crop": box_ } }],
            "responses": [{ "path": "forms.list", "value": {
                "formKind": "acroform", "needsAppearances": false,
                "fields": [field], "calculationOrder": [] } }]
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
