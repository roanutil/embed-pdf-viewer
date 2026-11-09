#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_stamp as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::stamp_plugin(core::StampConfig {
                preview_width: Some(320.0),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| StampHandle::new(k.capability(&core::STAMP).unwrap(), post)))
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
        StampHandle::new(k.kernel.capability(&core::STAMP, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.preview_width, 320.0);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || StampConfig { dynamic: Some(false), ..Default::default() };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert!(!changed.dynamic);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.preview_width, settings.preview_width);
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    assert_eq!(block_on(other.settings()).unwrap(), settings);
    block_on(thread.call(|k| plugin_conformance::test_kernel::settle(&k.kernel.destroy())))
        .unwrap()
        .unwrap();
    let error = block_on(handle.settings()).unwrap_err();
    assert!(
        matches!(error, epdf_ffi::ViewerError::Kernel { code: Some(ref code), .. } if code == "instance-closed")
    );
    assert!(block_on(handle.update_settings(patch())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}

#[test]
fn scalar_patches_and_reset_preserve_the_registered_asset_engine_factory() {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![core::stamp_plugin(core::StampConfig {
                asset_engine: Some(Some(core::StampAssetEngine::Factory(std::rc::Rc::new(|| {
                    Err(epdf_kernel::Error::new("registered asset engine"))
                })))),
                ..Default::default()
            })],
            one_document(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| StampHandle::new(k.capability(&core::STAMP).unwrap(), post)))
            .unwrap();
    for reset in [false, true] {
        if reset {
            block_on(handle.reset_settings()).unwrap();
        } else {
            block_on(
                handle.update_settings(StampConfig {
                    preview_width: Some(256.0),
                    dynamic: Some(false),
                }),
            )
            .unwrap();
        }
        block_on(thread.call(|k| {
            let cap = k.capability(&core::STAMP).unwrap();
            assert_eq!(cap.retained_engines(), 1);
            let core::StampAssetEngine::Factory(factory) = cap.get_settings().asset_engine.unwrap()
            else {
                panic!("registered engine replaced")
            };
            assert_eq!(factory().err().unwrap().message, "registered asset engine");
        }))
        .unwrap();
    }
}
