#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_selection as core;
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
                core::selection_plugin(core::SelectionConfig {
                    drag_threshold_px: Some(9.0),
                    color: Some(Some("red".into())),
                    ..Default::default()
                }),
            ],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SelectionHandle::new(k.capability(&core::SELECTION).unwrap(), post)),
    )
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
        SelectionHandle::new(k.kernel.capability(&core::SELECTION, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.drag_threshold_px, 9.0);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || SelectionSettingsPatch { drag_threshold_px: Some(12.0), ..Default::default() };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.drag_threshold_px, 12.0);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.color, settings.color);
    assert_eq!(changed.handles, settings.handles);
    block_on(handle.update_settings(SelectionSettingsPatch {
        clear_color: true,
        handles: Some(SelectionHandleSettingsPatch {
            color: Some("green".into()),
            shadow: Some("none".into()),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.color, None);
    assert_eq!(cleared.drag_threshold_px, 12.0);
    assert_eq!(cleared.handles.color.as_deref(), Some("green"));
    block_on(handle.update_settings(SelectionSettingsPatch {
        handles: Some(SelectionHandleSettingsPatch { clear_color: true, ..Default::default() }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(handle.settings()).unwrap();
    assert_eq!(cleared.handles.color, None);
    assert_eq!(cleared.handles.shadow, "none");
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
