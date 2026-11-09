#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_measurement as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![
                epdf_plugin_interaction::interaction_plugin(Default::default()),
                epdf_plugin_annotation::annotation_plugin(None),
                core::measurement_plugin(core::MeasurementConfig {
                    default_scale: Some(core::DefaultScale::Imperial),
                    ..Default::default()
                }),
            ],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| {
            MeasurementHandle::new(k.capability(&core::MEASUREMENT).unwrap(), post)
        }))
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
        MeasurementHandle::new(k.kernel.capability(&core::MEASUREMENT, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.default_scale, MeasurementDefaultScale::Imperial);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || MeasurementConfig { presets: Some(vec![]), ..Default::default() };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.presets.len(), 0);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.default_scale, settings.default_scale);
    block_on(handle.update_settings(MeasurementConfig {
        presets: Some(vec![core::ScalePreset {
            id: "custom".into(),
            label: "1:50".into(),
            paper: 1.0,
            real: 50.0,
            unit: "m".into(),
        }]),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(block_on(other.settings()).unwrap().presets[0].real, 50.0);
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
