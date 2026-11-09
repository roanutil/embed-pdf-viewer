#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_interaction as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::interaction_plugin(core::InteractionConfig {
                default_tool: Some("pan".into()),
                tools: Some(vec![std::rc::Rc::new(core::Tool::new(
                    "registered-tool",
                    "crosshair",
                    ["resource"],
                ))]),
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        InteractionHandle::new(
            k.capability(&core::INTERACTION).unwrap(),
            post,
            std::rc::Rc::new(|_| {}),
        )
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
        InteractionHandle::new(
            k.kernel.capability(&core::INTERACTION, Some(&id)).unwrap(),
            post,
            std::rc::Rc::new(|_| {}),
        )
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.default_tool, "pan");
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || InteractionSettingsPatch { default_tool: Some("pointer".into()) };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert!(block_on(handle.has_tool("registered-tool".into())).unwrap());
    assert_eq!(changed.default_tool, "pointer");
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    block_on(handle.reset_settings()).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), settings);
    assert_eq!(block_on(other.settings()).unwrap(), settings);
    assert!(block_on(other.has_tool("registered-tool".into())).unwrap());

    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let error = block_on(handle.settings()).unwrap_err();
    assert!(
        matches!(error, epdf_ffi::ViewerError::Kernel { code: Some(ref code), .. } if code == "instance-closed")
    );
    assert!(block_on(handle.update_settings(patch())).is_err());
    assert!(block_on(handle.reset_settings()).is_err());
}
