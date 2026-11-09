#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_search as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::search_plugin(core::SearchConfig {
                highlight: Some(core::SearchHighlightConfig {
                    color: Some("red".into()),
                    ..Default::default()
                }),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SearchHandle::new(k.capability(&core::SEARCH).unwrap(), post)),
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
        SearchHandle::new(k.kernel.capability(&core::SEARCH, Some(&id)).unwrap(), post)
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.highlight.color, "red");
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || SearchSettingsPatch {
        highlight: Some(SearchHighlightPatch {
            active_color: Some("blue".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.highlight.active_color, "blue");
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.highlight.color, "red");
    assert_eq!(changed.reveal, settings.reveal);
    block_on(handle.update_settings(SearchSettingsPatch {
        reveal: Some(SearchRevealPatch { clear_anchor: true, ..Default::default() }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.reveal.anchor, None);
    assert_eq!(cleared.reveal.behavior, settings.reveal.behavior);
    assert_eq!(cleared.highlight, changed.highlight);
    block_on(handle.update_settings(SearchSettingsPatch {
        reveal: Some(SearchRevealPatch {
            anchor: Some(SearchRevealAnchor { x: Some(SearchRevealAxis::End), y: None }),
            behavior: Some(SearchScrollBehavior::Instant),
            ..Default::default()
        }),
        highlight: Some(SearchHighlightPatch {
            blend_mode: Some(SearchBlendMode::Screen),
            ..Default::default()
        }),
    }))
    .unwrap();
    let aligned = block_on(handle.settings()).unwrap();
    assert_eq!(aligned.reveal.anchor.unwrap().x, Some(SearchRevealAxis::End));
    assert_eq!(aligned.reveal.behavior, SearchScrollBehavior::Instant);
    assert_eq!(aligned.highlight.blend_mode, SearchBlendMode::Screen);
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
