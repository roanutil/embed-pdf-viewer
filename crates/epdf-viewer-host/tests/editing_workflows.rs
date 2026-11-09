mod common;

use common::{NullDelegate, options};
use epdf_ffi::{CancelToken, FormFieldFamily, Point, Rect, ViewerError};
use epdf_plugin_annotation::ffi::{
    AnnotationAttachmentFile, AnnotationStampPlacement, AnnotationStampSource,
};
use epdf_plugin_form::ffi::{
    FormCalculationPosition, FormConfig, FormCreateFieldInput, FormWidgetPlacement,
};
use epdf_viewer_host::{SaveMode, ViewerConfig, ViewerHost};
use futures::executor::block_on;
use serde_json::{Value, json};
use std::sync::Arc;

fn config() -> ViewerConfig {
    ViewerConfig {
        interaction: true,
        annotation: Some(String::new()),
        form: Some(FormConfig::default()),
        ..Default::default()
    }
}

fn host(config: ViewerConfig) -> Arc<ViewerHost> {
    let host = ViewerHost::new(config, Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

fn value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn absent_plugins_and_permission_refusals_are_truthful() {
    let missing = host(ViewerConfig::default());
    let d = block_on(missing.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(
        matches!(block_on(missing.annotation(d.id.clone())),Err(ViewerError::Kernel {code:Some(code),..}) if code == "unsupported")
    );
    assert!(
        matches!(block_on(missing.form(d.id)),Err(ViewerError::Kernel {code:Some(code),..}) if code == "unsupported")
    );
    let host = host(ViewerConfig {
        scope: Some(vec!["doc.open".into(), "doc.forms.read".into()]),
        ..config()
    });
    let d = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let annotations = block_on(host.annotation(d.id.clone())).unwrap();
    let form = block_on(host.form(d.id.clone())).unwrap();
    let page = block_on(host.pages(d.id)).unwrap()[0].page;
    let widget = epdf_ffi::AnnotationRef::ObjectNumber { page, annot_object_number: 42 };
    let denied = |error| {
        assert!(
            matches!(error,ViewerError::Kernel {code:Some(code),..} if code == "permission-denied")
        )
    };
    denied(block_on(annotations.export(None, None, CancelToken::new())).unwrap_err());
    denied(block_on(annotations.import(json!({"format":"embedpdf/annotations","version":1,"pages":[],"items":[],"resources":{}}).to_string(),"{}".into(),CancelToken::new())).unwrap_err());
    denied(
        block_on(form.update_widget(widget.clone(), "{}".into(), CancelToken::new())).unwrap_err(),
    );
    denied(block_on(form.delete_widget(widget, CancelToken::new())).unwrap_err());
    denied(
        block_on(form.reorder_calculations(
            vec![],
            FormCalculationPosition::Start,
            CancelToken::new(),
        ))
        .unwrap_err(),
    );
}

#[test]
fn selected_link_update_commits_the_selection_and_clear_without_crossing_documents() {
    let host = host(config());
    let a = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let b = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let annotation = block_on(host.annotation(a.id.clone())).unwrap();
    let other = block_on(host.annotation(b.id)).unwrap();
    let page = block_on(host.pages(a.id.clone())).unwrap()[0].page;
    let square = block_on(annotation.create(
        page,
        json!({"subtype":"square","box":{"x":30,"y":60,"width":40,"height":20}}).to_string(),
        None,
        None,
        CancelToken::new(),
    ))
    .unwrap();
    block_on(annotation.selection().select(vec![square.clone()], None)).unwrap();
    let result = block_on(annotation.selection().update_link(
        Some(epdf_ffi::PdfLinkTarget::Uri { uri: "https://example.test/selected".into() }),
        CancelToken::new(),
    ))
    .unwrap();
    assert!(result.failed.is_empty());
    assert_eq!(
        value(&block_on(annotation.threads().link(square.clone())).unwrap().unwrap())["uri"],
        "https://example.test/selected"
    );
    assert!(block_on(other.selected()).unwrap().is_empty());
    block_on(annotation.selection().update_link(None, CancelToken::new())).unwrap();
    assert!(block_on(annotation.threads().link(square)).unwrap().is_none());
    block_on(host.close_document(a.id)).unwrap();
    assert!(block_on(annotation.selection().update_link(None, CancelToken::new())).is_err());
}

#[test]
fn annotation_bundles_copy_mixed_resources_and_typed_identities_between_documents() {
    use base64::Engine as _;
    let host = host(config());
    let a = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let b = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let source = block_on(host.annotation(a.id.clone())).unwrap();
    let target = block_on(host.annotation(b.id.clone())).unwrap();
    let page = block_on(host.pages(a.id.clone())).unwrap()[0].page;
    let file_bytes = vec![0, 255, 128, 1, 0, 240];
    let file = block_on(source.drafts().create_attachment(
        page,
        Point { x: 40.0, y: 40.0 },
        AnnotationAttachmentFile {
            bytes: file_bytes.clone(),
            name: "résumé.bin".into(),
            mime_type: None,
            description: None,
        },
        CancelToken::new(),
    ))
    .unwrap();
    let png = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAB4AAAAKCAIAAAAsFXl4AAAAGklEQVR42mP4z8CABzFQIj1q9KjRo0aTIg0AQXcq5OunV0AAAAAASUVORK5CYII=").unwrap();
    let stamp = block_on(source.drafts().place_stamp(
        AnnotationStampSource {
            bytes: png,
            mime_type: Some("image/png".into()),
            name: None,
            subject: None,
        },
        AnnotationStampPlacement { page, center: Point { x: 150.0, y: 100.0 }, rotation: None },
        Some(60.0),
        CancelToken::new(),
    ))
    .unwrap();
    let square = block_on(source.create(
        page,
        json!({"subtype":"square","box":{"x":30,"y":60,"width":40,"height":20}}).to_string(),
        None,
        None,
        CancelToken::new(),
    ))
    .unwrap();
    let empty = value(&block_on(source.export(Some(vec![]), None, CancelToken::new())).unwrap());
    assert_eq!(empty["items"], json!([]));
    let text = block_on(source.export(
        Some(vec![file.clone(), stamp.clone(), square.clone()]),
        None,
        CancelToken::new(),
    ))
    .unwrap();
    assert!(!text.contains("$host"));
    let bundle = value(&text);
    assert_eq!(bundle["items"].as_array().unwrap().len(), 3);
    let resources = bundle["resources"].as_object().unwrap();
    assert_eq!(resources.len(), 2);
    assert!(
        resources
            .values()
            .any(|r| epdf_kernel::bytes::Bytes::from_json(r).unwrap().0 == file_bytes)
    );
    let before = block_on(target.list(None)).unwrap();
    let imported = block_on(target.import(
        text.clone(),
        json!({"pages":"by-position","attribution":"stamp"}).to_string(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(imported.annotations.len(), 3);
    assert_eq!(imported.ref_map.len(), 3);
    assert_eq!(value(&imported.dropped_json), json!([]));
    assert!(imported.ref_map.iter().any(|m| m.from == file));
    for annotation in &imported.annotations {
        assert!(block_on(target.get(annotation.annotation_ref.clone())).unwrap().is_some());
    }
    assert_eq!(block_on(target.list(None)).unwrap().len(), before.len() + 3);
    assert!(block_on(source.get(square)).unwrap().is_some());
    let copied = value(
        &block_on(target.export(
            Some(imported.annotations.iter().map(|a| a.annotation_ref.clone()).collect()),
            None,
            CancelToken::new(),
        ))
        .unwrap(),
    );
    assert_eq!(copied["resources"], bundle["resources"]);
    let saved =
        block_on(host.save(b.id.clone(), SaveMode::Incremental, CancelToken::new())).unwrap();
    let reopened = block_on(host.open_document(saved, options())).unwrap();
    let reopened = block_on(host.annotation(reopened.id)).unwrap();
    let again = value(
        &block_on(reopened.export(
            Some(imported.annotations.iter().map(|a| a.annotation_ref.clone()).collect()),
            None,
            CancelToken::new(),
        ))
        .unwrap(),
    );
    assert_eq!(again["resources"], bundle["resources"]);
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(block_on(source.export(None, None, cancel)).is_err());
    block_on(host.close_document(a.id)).unwrap();
    assert!(
        matches!(block_on(source.export(None, None, CancelToken::new())), Err(ViewerError::Kernel { code:Some(code),..}) if code == "instance-closed")
    );
    assert!(block_on(source.import(text, "{}".into(), CancelToken::new())).is_err());
    assert!(block_on(target.export(Some(vec![]), None, CancelToken::new())).is_ok());
}

#[test]
fn form_widget_changes_and_calculation_order_survive_save_and_reopen() {
    let host = host(config());
    let a = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let b = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let form = block_on(host.form(a.id.clone())).unwrap();
    let other = block_on(host.form(b.id)).unwrap();
    let page = block_on(host.pages(a.id.clone())).unwrap()[0].page;
    block_on(form.refresh(CancelToken::new())).unwrap();
    let make = |name: &str| {
        block_on(form.create(
            FormCreateFieldInput {
                family: FormFieldFamily::Text,
                name: name.into(),
                page,
                bounds: Rect { x: 20.0, y: 30.0, width: 140.0, height: 24.0 },
                options: None,
                export_value: None,
            },
            CancelToken::new(),
        ))
        .unwrap()
    };
    let first = make("first");
    let second = make("second");
    // Existing interchange carries the engine's open action payload.
    let mut bundle = value(
        &block_on(form.export(
            Some(vec![first.field_ref.clone(), second.field_ref.clone()]),
            CancelToken::new(),
        ))
        .unwrap(),
    );
    for (index, field) in bundle["fields"].as_array_mut().unwrap().iter_mut().enumerate() {
        field["data"]["name"] = json!(format!("copy_{index}"));
        field["data"]["actions"] = json!({"calculate":{"root":{"type":"javascript","subtype":"JavaScript","script":"event.value = 1","next":[]},"incomplete":false,"warnings":[]}});
    }
    bundle["calculationOrder"] =
        json!([bundle["fields"][0]["data"]["ref"], bundle["fields"][1]["data"]["ref"]]);
    let imported =
        block_on(form.import(bundle.to_string(), "{}".into(), CancelToken::new())).unwrap();
    assert_eq!(imported.fields.len(), 2, "{imported:?}");
    assert_eq!(
        block_on(form.snapshot()).unwrap().unwrap().calculation_order.len(),
        2,
        "{imported:?}"
    );
    let f = &imported.fields[0];
    let g = &imported.fields[1];
    let widget = f.widgets[0].annotation_ref.clone().unwrap();
    let annotations = block_on(host.annotation(a.id.clone())).unwrap();
    let interaction = block_on(host.interaction(a.id.clone())).unwrap();
    block_on(interaction.activate_tool("form-edit".into())).unwrap();
    block_on(annotations.selection().select(vec![widget.clone()], None)).unwrap();
    assert_eq!(block_on(form.selected_field()).unwrap().unwrap().field_ref, f.field_ref);
    block_on(annotations.selection().clear()).unwrap();
    assert_eq!(block_on(form.field_widgets(f.field_ref.clone())).unwrap().len(), 1);
    let changed = block_on(form.update_widget(
        widget.clone(),
        json!({"rect":{"x":60,"y":90,"width":160,"height":28},"color":"#123456"}).to_string(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(changed.bounds.x, 60.0);
    assert_eq!(block_on(form.widget(widget.clone())).unwrap(), Some(changed));
    let added = block_on(form.add_widget(
        f.field_ref.clone(),
        FormWidgetPlacement {
            page: Some(page),
            bounds: Some(Rect { x: 70.0, y: 130.0, width: 120.0, height: 24.0 }),
            export_value: None,
        },
        CancelToken::new(),
    ))
    .unwrap();
    let extra = epdf_ffi::AnnotationRef::from_json(&value(&added.widget_json)["ref"]).unwrap();
    let remaining =
        block_on(form.delete_widget(extra.clone(), CancelToken::new())).unwrap().unwrap();
    assert_eq!(remaining.widgets.len(), 1);
    assert!(block_on(form.widget(extra)).unwrap().is_none());
    let order = block_on(form.reorder_calculations(
        vec![g.field_ref.clone()],
        FormCalculationPosition::Start,
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(order, vec![Some(g.field_ref.clone()), Some(f.field_ref.clone())]);
    assert_eq!(
        block_on(form.reorder_calculations(
            vec![g.field_ref.clone()],
            FormCalculationPosition::After { field: f.field_ref.clone() },
            CancelToken::new()
        ))
        .unwrap(),
        vec![Some(f.field_ref.clone()), Some(g.field_ref.clone())]
    );
    assert_eq!(
        block_on(form.reorder_calculations(
            vec![g.field_ref.clone()],
            FormCalculationPosition::Before { field: f.field_ref.clone() },
            CancelToken::new()
        ))
        .unwrap(),
        order
    );
    assert_eq!(block_on(form.snapshot()).unwrap().unwrap().calculation_order, order);
    assert!(block_on(other.fields(None)).unwrap().is_empty());
    assert!(block_on(form.selected_field()).unwrap().is_none());
    let unknown = epdf_ffi::AnnotationRef::ObjectNumber { page, annot_object_number: 999_999 };
    assert!(block_on(form.widget(unknown.clone())).unwrap().is_none());
    assert!(block_on(form.update_widget(unknown, "[]".into(), CancelToken::new())).is_err());
    let saved =
        block_on(host.save(a.id.clone(), SaveMode::Incremental, CancelToken::new())).unwrap();
    let reopened = block_on(host.open_document(saved, options())).unwrap();
    let saved = block_on(host.form(reopened.id)).unwrap();
    block_on(saved.refresh(CancelToken::new())).unwrap();
    assert_eq!(block_on(saved.snapshot()).unwrap().unwrap().calculation_order, order);
    assert_eq!(block_on(saved.widget(widget.clone())).unwrap().unwrap().bounds.x, 60.0);
    assert_eq!(block_on(saved.field_widgets(f.field_ref.clone())).unwrap().len(), 1);
    block_on(host.close_document(a.id)).unwrap();
    assert!(block_on(form.widget(widget.clone())).is_err());
    assert!(block_on(form.update_widget(widget.clone(), "{}".into(), CancelToken::new())).is_err());
    assert!(block_on(form.delete_widget(widget, CancelToken::new())).is_err());
    assert!(
        block_on(form.reorder_calculations(
            vec![],
            FormCalculationPosition::End,
            CancelToken::new()
        ))
        .is_err()
    );
    assert!(block_on(form.selected_field()).is_err());
    assert!(block_on(form.field_widgets(f.field_ref.clone())).is_err());
}
