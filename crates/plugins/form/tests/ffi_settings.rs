#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_form as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(vec![epdf_plugin_interaction::interaction_plugin(Default::default()), core::form_plugin(Some(serde_json::json!({"validation":"none","focus":{"color":"red"},"fillable":["text"]})))], spec) }).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| FormHandle::new(k.capability(&core::FORM).unwrap(), post)))
            .unwrap();
    let post = thread.post();
    let other = block_on(thread.call(move |k| {
        let opened = plugin_conformance::test_kernel::settle(&k.kernel.documents().open(
            epdf_kernel::OpenSource::Input(
                serde_json::json!({"kind":"bytes","id":"b","bytes":{"$bytes":""}}),
            ),
            epdf_kernel::OpenOptions::default(),
        ))
        .unwrap();
        let id = opened["document"]["id"].as_str().unwrap().to_string();
        FormHandle::new(k.kernel.capability(&core::FORM, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.validation, FormValidation::None);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || FormSettingsPatch {
        fields: Some(FormFieldSettingsPatch {
            background: Some("blue".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.fields.background, "blue");
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.validation, FormValidation::None);
    assert_eq!(changed.focus, settings.focus);
    assert_eq!(
        changed.fillable,
        FormFillable::Kinds { families: vec![epdf_ffi::FormFieldFamily::Text] }
    );
    block_on(handle.update_settings(FormSettingsPatch {
        focus: Some(FormFocusSettingsPatch { clear_color: true, ..Default::default() }),
        fields: Some(FormFieldSettingsPatch { border: Some("red".into()), ..Default::default() }),
        fillable: Some(FormFillable::None),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(block_on(other.settings()).unwrap().focus.color, None);
    block_on(handle.update_settings(FormSettingsPatch {
        fields: Some(FormFieldSettingsPatch { clear_border: true, ..Default::default() }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(handle.settings()).unwrap();
    assert_eq!(cleared.fields.border, None);
    assert_eq!(cleared.fields.background, "blue");
    assert_eq!(cleared.fillable, FormFillable::None);
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    assert_eq!(block_on(other.settings()).unwrap(), settings);

    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let error = block_on(handle.settings()).unwrap_err();
    assert!(
        matches!(error, epdf_ffi::ViewerError::Kernel { code: Some(ref code), .. } if code == "instance-closed")
    );
    assert!(block_on(handle.update_settings(patch())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}
