#![cfg(feature = "uniffi")]

use core::ffi::*;
use epdf_ffi::KernelThread;
use epdf_plugin_render as core;
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, one_document};

#[test]
fn live_settings_merge_notify_and_reset_to_registration() {
    let thread = KernelThread::spawn(|_post| {
        let mut spec = one_document();
        spec["documents"]["b"] = spec["documents"]["a"].clone();
        spec["documents"]["b"]["id"] = serde_json::json!("b");
        TestKernel::open(
            vec![core::render_plugin(core::RenderConfig {
                slow_after_ms: Some(950.0),
                ..Default::default()
            })],
            spec,
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        RenderHandle::new(k.capability(&core::RENDER).unwrap(), post, std::rc::Rc::new(|_| None))
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
        RenderHandle::new(
            k.kernel.capability(&core::RENDER, Some(&id)).unwrap(),
            post,
            std::rc::Rc::new(|_| None),
        )
    }))
    .unwrap();
    let settings = block_on(handle.settings()).unwrap();
    assert_eq!(settings.slow_after_ms, 950.0);
    let revision = block_on(thread.call(|k| k.kernel.state_revision())).unwrap();
    let patch = || RenderSettingsPatch {
        snapshot: Some(RenderSnapshotPatch { scale: Some(3.0), ..Default::default() }),
        ..Default::default()
    };
    block_on(handle.update_settings(patch())).unwrap();
    let changed = block_on(handle.settings()).unwrap();
    assert_eq!(changed.snapshot.scale, 3.0);
    assert_ne!(changed, settings);
    assert_eq!(block_on(other.settings()).unwrap(), changed);
    assert!(block_on(thread.call(|k| k.kernel.state_revision())).unwrap() > revision);
    block_on(handle.update_settings(Default::default())).unwrap();
    assert_eq!(block_on(handle.settings()).unwrap(), changed);
    assert_eq!(changed.slow_after_ms, 950.0);
    assert_eq!(changed.full_page, settings.full_page);
    assert_eq!(changed.tiles, settings.tiles);
    block_on(handle.update_settings(RenderSettingsPatch {
        format: Some(RenderEncodingFormat::Webp),
        quality: Some(0.6),
        tiles: Some(RenderTilesPatch::On {
            options: RenderTilesOptions {
                engage_ratio: Some(1.5),
                prefetch: Some(RenderPrefetchOptions { margin: Some(0.8), ..Default::default() }),
                ..Default::default()
            },
        }),
        snapshot: Some(RenderSnapshotPatch {
            box_look: Some(RenderSnapshotBox {
                fill: Some("red".into()),
                stroke: Some("blue".into()),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    let configured = block_on(other.settings()).unwrap();
    assert_eq!(configured.format, Some(RenderEncodingFormat::Webp));
    assert_eq!(configured.quality, Some(0.6));
    block_on(handle.update_settings(RenderSettingsPatch {
        clear_format: true,
        clear_quality: true,
        tiles: Some(RenderTilesPatch::On {
            options: RenderTilesOptions { clear_engage_ratio: true, ..Default::default() },
        }),
        snapshot: Some(RenderSnapshotPatch {
            box_look: Some(RenderSnapshotBox { fill: Some("green".into()), stroke: None }),
            ..Default::default()
        }),
        ..Default::default()
    }))
    .unwrap();
    let cleared = block_on(other.settings()).unwrap();
    assert_eq!(cleared.format, None);
    assert_eq!(cleared.quality, None);
    assert_eq!(cleared.snapshot.scale, 3.0);
    assert_eq!(cleared.snapshot.box_look.stroke, None);
    assert_eq!(cleared.snapshot.box_look.fill.as_deref(), Some("green"));
    let RenderTilesSetting::On { options } = cleared.tiles else { panic!("expected tiles") };
    assert_eq!(options.engage_ratio, None);
    assert_eq!(options.size, Some(512.0));
    let prefetch = options.prefetch.unwrap();
    assert_eq!(prefetch.margin, Some(0.8));
    assert_eq!(prefetch.velocity_bias, Some(true));
    block_on(handle.update_settings(RenderSettingsPatch {
        tiles: Some(RenderTilesPatch::Off),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(block_on(other.settings()).unwrap().tiles, RenderTilesSetting::Off);
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
