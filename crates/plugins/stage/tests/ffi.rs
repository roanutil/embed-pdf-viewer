#![cfg(feature = "uniffi")]

use epdf_ffi::{StageAlign, StageAlignValue, ZoomMode, ZoomSpec};
use epdf_plugin_stage::ffi::{
    StageArrivalAlignValue, StageGap, StageGridColumns, StageRevealOptions, StageRevealZoom,
    StageSettings, StageSettingsPatch, StageZoomTarget,
};
use epdf_plugin_stage::{PartialSettings, RevealOptions, ZoomTarget, default_settings};

#[test]
fn the_default_settings_round_trip() {
    let settings = default_settings();
    let ffi = StageSettings::try_from(settings.clone()).unwrap();
    assert_eq!(epdf_plugin_stage::StageSettings::try_from(ffi).unwrap(), settings);
}

#[test]
fn a_patch_sets_only_what_it_names() {
    let patch = StageSettingsPatch {
        padding: Some(8.0),
        gap: Some(StageGap::Px { px: 4.0 }),
        columns: Some(StageGridColumns::Count { count: 3 }),
        zoom: Some(ZoomSpec::Mode { mode: ZoomMode::FitPage }),
        arrival_align: None,
        ..Default::default()
    };
    let partial = PartialSettings::try_from(patch).unwrap();
    assert_eq!(partial.padding, Some(8.0));
    assert_eq!(partial.gap, Some(epdf_plugin_stage::Gap::Px { px: 4.0 }));
    assert_eq!(partial.columns, Some(epdf_plugin_stage::GridColumns::Count(3.0)));
    assert!(partial.flow.is_none() && partial.arrival_align.is_none());
}

#[test]
fn a_fractional_column_count_is_invalid_state_not_a_round() {
    let mut settings = default_settings();
    settings.columns = epdf_plugin_stage::GridColumns::Count(2.5);
    let error = StageSettings::try_from(settings).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

#[test]
fn keep_survives_the_arrival_alignment() {
    let keep = StageArrivalAlignValue::Keep;
    let core: epdf_plugin_stage::ArrivalAlignValue = keep.into();
    assert_eq!(core, epdf_plugin_stage::ArrivalAlignValue::KEEP);
    let named =
        StageArrivalAlignValue::Align { value: StageAlignValue::Named { align: StageAlign::End } };
    assert_eq!(
        StageArrivalAlignValue::from(epdf_plugin_stage::ArrivalAlignValue::from(named)),
        named
    );
}

#[test]
fn reveal_options_and_zoom_targets_convert() {
    let options = StageRevealOptions {
        behavior: None,
        rect: Some(epdf_ffi::Rect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }),
        zoom: Some(StageRevealZoom::Level { level: 2.0 }),
        anchor: None,
    };
    let core = RevealOptions::from(options.clone());
    assert_eq!(core.zoom, Some(epdf_plugin_stage::RevealZoom::Level { level: 2.0 }));
    assert_eq!(StageRevealOptions::from(core), options);
    assert_eq!(ZoomTarget::from(StageZoomTarget::Level { level: 1.25 }), ZoomTarget::Level(1.25));
}
