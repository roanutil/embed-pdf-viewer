//! `get_menu_target` answers the ui-core `MenuTarget`, so the capability's
//! answer passes to `project_overflow` with no conversion of the target.
//! The read itself is fallible, as `getMenuTarget` throws where reading the
//! configured commands does.

use epdf_plugin_commands::CommandsCapability;
use epdf_ui_core::{FitResult, NormalizedBar, ResolveMenuTarget, project_overflow};
use serde_json::json;

fn resolver(cap: &CommandsCapability) -> impl ResolveMenuTarget + '_ {
    move |id: &str| cap.get_menu_target(id).ok().flatten()
}

#[test]
fn the_commands_menu_target_is_the_ui_core_one() {
    let _ = resolver;
    let target = epdf_plugin_commands::MenuTarget { menu: Some("zoom".into()) };
    let same: epdf_ui_core::MenuTarget = target.clone();
    assert_eq!(same, target);

    let bar = NormalizedBar { id: "toolbar".into(), sections: Vec::new() };
    let fit: FitResult = serde_json::from_value(json!({
        "units": { "$map": [] }, "groups": { "$map": [] }, "hasOverflow": false, "length": 0, "complete": true
    }))
    .unwrap();
    assert!(project_overflow(&bar, &fit, |_: &str| Some(target.clone())).is_empty());
}
