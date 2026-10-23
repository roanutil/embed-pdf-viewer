//! The package's own tests (`packages/core/ui/test/{layout,layout-validate,
//! suggestions,schema,shortcuts}.test.ts`), as Rust behaviour. Where the
//! TypeScript asserts `toBe`, the same object, these assert `Rc::ptr_eq`:
//! an edit keeps everything it didn't touch as the same allocation.

use std::rc::Rc;

use epdf_ui_core::{
    AddItemOptions, BarEdge, BarPlace, JsValue, Layout, LayoutEnv, LayoutKnownIds, LayoutOption,
    LayoutResult, RemoveItemOptions, ShortcutOptions, TabsVisibility, TextDirection, ViewerSize,
    closest_match, create_layout, did_you_mean, edit_distance, format_shortcut, is_layout,
    layout_warnings_of, match_shortcut, normalize_bar, parse_shortcut, resolve_layout,
    same_layout_env, same_shortcut, size_of, validate_layout,
};
use serde_json::{Value, json};

fn js(value: Value) -> JsValue {
    JsValue::from(value)
}

/// `value.a.b.c`.
fn at(value: &JsValue, path: &[&str]) -> Option<JsValue> {
    path.iter().try_fold(value.clone(), |v, key| v.get(key))
}

fn get(value: &JsValue, path: &[&str]) -> JsValue {
    at(value, path).unwrap_or_else(|| panic!("no {path:?}"))
}

/// The TypeScript's `toBe` for an object or an array: the same allocation.
fn ptr_eq(left: &JsValue, right: &JsValue) -> bool {
    match (left, right) {
        (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
        (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn base() -> JsValue {
    js(json!({
        "bars": {
            "main": {
                "place": "top",
                "start": [
                    { "id": "document", "items": ["menu:document"], "importance": 5 },
                    { "id": "zoom", "items": [{ "custom": "zoom", "command": "menu:zoom" }] },
                    { "id": "tools", "items": ["tool:pointer", "tool:pan"] }
                ],
                "center": [{ "id": "modes", "items": [], "collapse": "select" }],
                "end": [{ "id": "panels", "items": ["panel:search", "panel:comments"] }]
            },
            "annotate": {
                "place": { "with": "main" },
                "mode": "annotate",
                "titleKey": "commands.mode.annotate",
                "center": [["tool:highlight", "tool:underline"], "history:undo"]
            },
            "selection": { "place": { "at": "selection" }, "center": [["selection:copy"]] }
        },
        "menus": {
            "document": [["document:download", "document:print"], ["view:fullscreen"]],
            "view": [{ "titleKey": "commands.scroll.group", "items": ["view:scroll-vertical"] }]
        },
        "panels": { "comments": { "side": "end" }, "thumbnails": { "side": "start" } }
    }))
}

fn to(id: &str) -> AddItemOptions {
    AddItemOptions { to: id.into(), ..Default::default() }
}

fn from(id: &str) -> RemoveItemOptions {
    RemoveItemOptions { from: id.into() }
}

const ENV: LayoutEnv =
    LayoutEnv { size: ViewerSize::Expanded, width: 1280.0, touch: false, dir: TextDirection::Ltr };

// ── createLayout ──────────────────────────────────────────────────────────────

#[test]
fn a_layout_gives_back_its_data_unchanged() {
    let base = base();
    let layout = create_layout(base.clone());
    assert!(ptr_eq(&layout.to_json(), &base));
    assert!(is_layout(&layout));
    assert!(!is_layout(&base));
}

#[test]
fn every_edit_returns_a_new_layout_and_leaves_the_old_one() {
    let layout = create_layout(base());
    let moved = layout.place("main", "bottom");
    assert!(!Layout::same(&moved, &layout));
    assert_eq!(get(&layout.to_json(), &["bars", "main", "place"]), JsValue::from("top"));
    assert_eq!(get(&moved.to_json(), &["bars", "main", "place"]), JsValue::from("bottom"));
}

// ── place ─────────────────────────────────────────────────────────────────────

#[test]
fn place_moves_a_bar_and_keeps_its_sections_and_everything_else() {
    let base = base();
    let place = BarPlace::OnEdge { edge: BarEdge::Top, before: None, after: Some("review".into()) };
    let next = create_layout(base.clone()).place("main", place).to_json();
    assert_eq!(
        get(&next, &["bars", "main", "place"]),
        js(json!({ "edge": "top", "after": "review" }))
    );
    assert!(ptr_eq(
        &get(&next, &["bars", "main", "start"]),
        &get(&base, &["bars", "main", "start"])
    ));
    // Unaffected structures are the same allocation, not copies.
    assert!(ptr_eq(&get(&next, &["bars", "annotate"]), &get(&base, &["bars", "annotate"])));
    assert!(ptr_eq(&get(&next, &["menus"]), &get(&base, &["menus"])));
    assert!(ptr_eq(&get(&next, &["panels"]), &get(&base, &["panels"])));
    assert!(!ptr_eq(&get(&next, &["bars"]), &get(&base, &["bars"])));
}

#[test]
fn place_warns_about_an_unknown_bar_with_the_closest_id_and_changes_nothing() {
    let base = base();
    let layout = create_layout(base.clone()).place("mian", "bottom");
    assert!(ptr_eq(&layout.to_json(), &base));
    assert_eq!(layout_warnings_of(&layout), ["place: no bar \"mian\" (did you mean \"main\"?)"]);
}

// ── addBar and removeBar ──────────────────────────────────────────────────────

#[test]
fn add_bar_adds_and_replaces() {
    let review = js(json!({ "place": "bottom", "center": [["acme:approve"]] }));
    let added = create_layout(base()).add_bar("review", review.clone()).to_json();
    assert!(ptr_eq(&get(&added, &["bars", "review"]), &review));
    assert_eq!(get(&added, &["bars"]).keys(), ["main", "annotate", "selection", "review"]);
    let replaced = create_layout(base()).add_bar("main", review.clone()).to_json();
    assert!(ptr_eq(&get(&replaced, &["bars", "main"]), &review));
    assert_eq!(get(&replaced, &["bars"]).keys(), ["main", "annotate", "selection"]);
}

#[test]
fn remove_bar_removes_and_warns_about_one_it_does_not_have() {
    let layout = create_layout(base()).remove_bar("anotate").remove_bar("annotate");
    assert_eq!(get(&layout.to_json(), &["bars"]).keys(), ["main", "selection"]);
    assert_eq!(
        layout_warnings_of(&layout),
        ["removeBar: no bar \"anotate\" (did you mean \"annotate\"?)"]
    );
}

// ── add ───────────────────────────────────────────────────────────────────────

#[test]
fn add_puts_an_item_in_a_new_group_at_the_end_of_the_end_section_by_default() {
    let end = get(
        &create_layout(base()).add("document:print", &to("main")).to_json(),
        &["bars", "main", "end"],
    );
    assert_eq!(
        end,
        js(
            json!([{ "id": "panels", "items": ["panel:search", "panel:comments"] }, ["document:print"]])
        )
    );
}

#[test]
fn add_puts_an_item_at_the_end_of_a_section() {
    let options = AddItemOptions { section: Some("start".into()), ..to("main") };
    let start = get(
        &create_layout(base()).add("document:print", &options).to_json(),
        &["bars", "main", "start"],
    );
    assert_eq!(start.as_array().unwrap().last(), Some(&js(json!(["document:print"]))));
}

#[test]
fn add_joins_a_group_by_id_and_keeps_the_other_groups() {
    let base = base();
    let options = AddItemOptions { group: Some("tools".into()), ..to("main") };
    let next = create_layout(base.clone()).add("tool:ink", &options).to_json();
    assert_eq!(
        get(&next, &["bars", "main", "start", "2"]),
        js(json!({ "id": "tools", "items": ["tool:pointer", "tool:pan", "tool:ink"] }))
    );
    assert!(ptr_eq(
        &get(&next, &["bars", "main", "start", "0"]),
        &get(&base, &["bars", "main", "start", "0"])
    ));
    assert!(!ptr_eq(
        &get(&next, &["bars", "main", "center"]),
        &get(&base, &["bars", "main", "center"])
    ));
    assert!(ptr_eq(
        &get(&next, &["bars", "main", "center", "0"]),
        &get(&base, &["bars", "main", "center", "0"])
    ));
}

#[test]
fn add_starts_a_named_group_when_the_group_id_is_new() {
    let options =
        AddItemOptions { section: Some("end".into()), group: Some("acme".into()), ..to("main") };
    let end =
        get(&create_layout(base()).add("acme:send", &options).to_json(), &["bars", "main", "end"]);
    assert_eq!(
        end.as_array().unwrap().last(),
        Some(&js(json!({ "id": "acme", "items": ["acme:send"] })))
    );
}

#[test]
fn add_puts_an_item_before_or_after_another_in_that_items_group() {
    let layout = create_layout(base());
    let after = AddItemOptions { after: Some("panel:search".into()), ..to("main") };
    assert_eq!(
        get(&layout.add("document:print", &after).to_json(), &["bars", "main", "end", "0"]),
        js(
            json!({ "id": "panels", "items": ["panel:search", "document:print", "panel:comments"] })
        )
    );
    let before = AddItemOptions { before: Some("tool:pointer".into()), ..to("main") };
    assert_eq!(
        get(&layout.add("tool:ink", &before).to_json(), &["bars", "main", "start", "2"]),
        js(json!({ "id": "tools", "items": ["tool:ink", "tool:pointer", "tool:pan"] }))
    );
}

#[test]
fn add_turns_a_lone_item_into_a_group_when_something_goes_next_to_it() {
    let options = AddItemOptions { after: Some("history:undo".into()), ..to("annotate") };
    let center = get(
        &create_layout(base()).add("history:redo", &options).to_json(),
        &["bars", "annotate", "center"],
    );
    assert_eq!(
        center,
        js(json!([["tool:highlight", "tool:underline"], ["history:undo", "history:redo"]]))
    );
}

#[test]
fn add_finds_your_own_component_by_its_id() {
    let options = AddItemOptions { after: Some("zoom".into()), ..to("main") };
    let zoom = get(
        &create_layout(base()).add("zoom:in", &options).to_json(),
        &["bars", "main", "start", "1"],
    );
    assert_eq!(
        zoom,
        js(
            json!({ "id": "zoom", "items": [{ "custom": "zoom", "command": "menu:zoom" }, "zoom:in"] })
        )
    );
}

#[test]
fn add_still_adds_the_item_when_the_item_to_go_next_to_is_missing_and_warns() {
    let options = AddItemOptions { after: Some("panel:serch".into()), ..to("main") };
    let layout = create_layout(base()).add("document:print", &options);
    let end = get(&layout.to_json(), &["bars", "main", "end"]);
    assert_eq!(end.as_array().unwrap().last(), Some(&js(json!(["document:print"]))));
    assert_eq!(
        layout_warnings_of(&layout),
        ["add: \"panel:serch\" is not in \"main\" (did you mean \"panel:search\"?)"]
    );
}

#[test]
fn add_does_not_put_an_item_in_a_group_twice() {
    let base = base();
    let options = AddItemOptions { group: Some("tools".into()), ..to("main") };
    let layout = create_layout(base.clone()).add("tool:pan", &options);
    assert!(ptr_eq(&layout.to_json(), &base));
    assert_eq!(
        layout_warnings_of(&layout),
        ["add: \"tool:pan\" is already in that group of \"main\""]
    );
}

#[test]
fn add_to_a_menu_in_a_new_section_or_next_to_an_item() {
    let layout = create_layout(base());
    assert_eq!(
        get(&layout.add("acme:export", &to("document")).to_json(), &["menus", "document"]),
        js(json!([["document:download", "document:print"], ["view:fullscreen"], ["acme:export"]]))
    );
    let after = AddItemOptions { after: Some("document:print".into()), ..to("document") };
    assert_eq!(
        get(&layout.add("acme:export", &after).to_json(), &["menus", "document"]),
        js(json!([["document:download", "document:print", "acme:export"], ["view:fullscreen"]]))
    );
    let after = AddItemOptions { after: Some("view:scroll-vertical".into()), ..to("view") };
    assert_eq!(
        get(&layout.add("view:scroll-horizontal", &after).to_json(), &["menus", "view"]),
        js(
            json!([{ "titleKey": "commands.scroll.group", "items": ["view:scroll-vertical", "view:scroll-horizontal"] }])
        )
    );
}

#[test]
fn add_takes_a_command_object_into_a_menu_and_refuses_your_own_component() {
    let layout = create_layout(base())
        .add(js(json!({ "command": "acme:export", "importance": 5 })), &to("document"))
        .add(js(json!({ "custom": "versions" })), &to("document"));
    let document = get(&layout.to_json(), &["menus", "document"]);
    assert_eq!(document.as_array().unwrap().last(), Some(&js(json!(["acme:export"]))));
    assert_eq!(
        layout_warnings_of(&layout),
        ["add: \"document\" is a menu, which holds commands, not your own components"]
    );
}

#[test]
fn add_prefers_a_bar_to_a_menu_with_the_same_id() {
    let base = base();
    let layout = create_layout(base.clone())
        .add_bar("document", js(json!({ "place": "bottom" })))
        .add("acme:x", &to("document"));
    assert_eq!(get(&layout.to_json(), &["bars", "document", "end"]), js(json!([["acme:x"]])));
    assert!(ptr_eq(
        &get(&layout.to_json(), &["menus", "document"]),
        &get(&base, &["menus", "document"])
    ));
}

#[test]
fn add_warns_about_an_unknown_place_and_a_missing_target() {
    let base = base();
    let layout = create_layout(base.clone())
        .add("x:y", &to("documnet"))
        .add("x:y", &AddItemOptions::default());
    assert!(ptr_eq(&layout.to_json(), &base));
    assert_eq!(
        layout_warnings_of(&layout),
        [
            "add: no bar or menu \"documnet\" (did you mean \"document\"?)",
            "add: \"x:y\" needs a bar or menu to go to ({ to })",
        ]
    );
}

#[test]
fn add_warns_about_something_that_is_not_an_item() {
    let base = base();
    let layout = create_layout(base.clone()).add(js(json!({ "nope": true })), &to("main"));
    assert!(ptr_eq(&layout.to_json(), &base));
    assert_eq!(layout_warnings_of(&layout), ["add: {\"nope\":true} is not an item"]);
}

#[test]
fn add_warns_about_a_section_that_is_not_one_and_uses_end() {
    let options = AddItemOptions { section: Some("middle".into()), ..to("selection") };
    let layout = create_layout(base()).add("x", &options);
    assert_eq!(get(&layout.to_json(), &["bars", "selection", "end"]), js(json!([["x"]])));
    assert_eq!(get(&layout.to_json(), &["bars", "selection"]).keys(), ["place", "center", "end"]);
    assert_eq!(
        layout_warnings_of(&layout),
        ["add: \"middle\" is not a section (start, center or end); using \"end\""]
    );
}

// ── remove ────────────────────────────────────────────────────────────────────

#[test]
fn remove_takes_items_out_of_one_bar_only() {
    let next = create_layout(base())
        .add_bar("rail", js(json!({ "place": "start", "start": [["tool:pan"]] })))
        .remove(&["tool:pan", "panel:comments"], &from("main"))
        .to_json();
    assert_eq!(
        get(&next, &["bars", "main", "start", "2"]),
        js(json!({ "id": "tools", "items": ["tool:pointer"] }))
    );
    assert_eq!(
        get(&next, &["bars", "main", "end"]),
        js(json!([{ "id": "panels", "items": ["panel:search"] }]))
    );
    assert_eq!(get(&next, &["bars", "rail", "start"]), js(json!([["tool:pan"]])));
}

#[test]
fn edits_keep_the_modes_group_whose_tabs_are_derived() {
    let modes = js(json!([{ "id": "modes", "items": [], "collapse": "select" }]));
    let removed = create_layout(base()).remove(&["tool:pan"], &from("main")).to_json();
    assert_eq!(get(&removed, &["bars", "main", "center"]), modes);
    let replaced = create_layout(base()).replace("tool:pan", "acme:pan").to_json();
    assert_eq!(get(&replaced, &["bars", "main", "center"]), modes);
    let panel_gone = create_layout(base()).remove_panel("comments").to_json();
    assert_eq!(get(&panel_gone, &["bars", "main", "center"]), modes);
}

#[test]
fn remove_drops_a_group_with_nothing_left_and_a_section_with_nothing_left() {
    let next = create_layout(base())
        .remove(&["panel:search", "panel:comments"], &from("main"))
        .remove(&["selection:copy"], &from("selection"))
        .to_json();
    assert_eq!(at(&next, &["bars", "main", "end"]), None);
    assert_eq!(get(&next, &["bars", "selection"]), js(json!({ "place": { "at": "selection" } })));
}

#[test]
fn remove_takes_your_own_component_by_its_id_and_a_lone_item() {
    let next = create_layout(base())
        .remove(&["zoom"], &from("main"))
        .remove(&["history:undo"], &from("annotate"))
        .to_json();
    let ids: Vec<JsValue> = get(&next, &["bars", "main", "start"])
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.get("id").unwrap())
        .collect();
    assert_eq!(ids, [JsValue::from("document"), JsValue::from("tools")]);
    assert_eq!(
        get(&next, &["bars", "annotate", "center"]),
        js(json!([["tool:highlight", "tool:underline"]]))
    );
}

#[test]
fn remove_takes_items_out_of_a_menu_dropping_an_emptied_section_and_keeping_titles() {
    let next = create_layout(base())
        .remove(&["view:fullscreen"], &from("document"))
        .add_menu("view", js(json!([{ "title": "Scroll", "items": ["view:scroll-vertical", "view:scroll-horizontal"] }])))
        .remove(&["view:scroll-vertical"], &from("view"))
        .to_json();
    assert_eq!(
        get(&next, &["menus", "document"]),
        js(json!([["document:download", "document:print"]]))
    );
    assert_eq!(
        get(&next, &["menus", "view"]),
        js(json!([{ "title": "Scroll", "items": ["view:scroll-horizontal"] }]))
    );
}

#[test]
fn remove_warns_for_each_id_that_is_not_in_that_place() {
    let layout = create_layout(base()).remove(&["tool:pann", "tool:pointer"], &from("main"));
    assert_eq!(
        get(&layout.to_json(), &["bars", "main", "start", "2"]),
        js(json!({ "id": "tools", "items": ["tool:pan"] }))
    );
    assert_eq!(
        layout_warnings_of(&layout),
        ["remove: \"tool:pann\" is not in \"main\" (did you mean \"tool:pan\"?)"]
    );
}

#[test]
fn remove_always_names_one_place() {
    let base = base();
    let layout = create_layout(base.clone()).remove(&["tool:pan"], &RemoveItemOptions::default());
    assert!(ptr_eq(&layout.to_json(), &base));
    let warning = &layout_warnings_of(&layout)[0];
    assert!(warning.contains("needs a bar or menu") && warning.contains("disable"), "{warning}");
}

// ── replace ───────────────────────────────────────────────────────────────────

#[test]
fn replace_puts_another_item_in_its_place_in_every_bar_and_menu() {
    let next = create_layout(base())
        .add_bar("rail", js(json!({ "place": "start", "start": [["document:print", "tool:pan"]] })))
        .replace("document:print", "acme:print")
        .to_json();
    assert_eq!(get(&next, &["bars", "rail", "start"]), js(json!([["acme:print", "tool:pan"]])));
    assert_eq!(
        get(&next, &["menus", "document", "0"]),
        js(json!(["document:download", "acme:print"]))
    );
}

#[test]
fn replace_takes_a_command_object_which_a_menu_shows_by_its_command() {
    let next = create_layout(base())
        .replace("panel:comments", js(json!({ "command": "acme:chat", "importance": 5 })))
        .replace("view:fullscreen", js(json!({ "command": "acme:full" })))
        .to_json();
    assert_eq!(
        get(&next, &["bars", "main", "end", "0"]),
        js(
            json!({ "id": "panels", "items": ["panel:search", { "command": "acme:chat", "importance": 5 }] })
        )
    );
    assert_eq!(get(&next, &["menus", "document", "1"]), js(json!(["acme:full"])));
}

#[test]
fn replace_leaves_menus_alone_when_the_replacement_is_your_own_component() {
    let next = create_layout(base())
        .add_bar("rail", js(json!({ "place": "start", "start": [["document:print"]] })))
        .replace("document:print", js(json!({ "custom": "printer" })))
        .to_json();
    assert_eq!(get(&next, &["bars", "rail", "start"]), js(json!([[{ "custom": "printer" }]])));
    assert_eq!(
        get(&next, &["menus", "document", "0"]),
        js(json!(["document:download", "document:print"]))
    );
}

#[test]
fn replace_warns_about_an_id_that_is_nowhere() {
    let layout = create_layout(base()).replace("document:downlaod", "acme:save");
    assert_eq!(
        layout_warnings_of(&layout),
        [
            "replace: \"document:downlaod\" is in no bar or menu (did you mean \"document:download\"?)"
        ]
    );
}

#[test]
fn replace_keeps_bars_it_did_not_touch_as_the_same_allocation() {
    let base = base();
    let next = create_layout(base.clone()).replace("tool:highlight", "tool:ink").to_json();
    assert!(ptr_eq(&get(&next, &["bars", "main"]), &get(&base, &["bars", "main"])));
    assert!(ptr_eq(&get(&next, &["bars", "selection"]), &get(&base, &["bars", "selection"])));
    assert!(ptr_eq(&get(&next, &["menus", "document"]), &get(&base, &["menus", "document"])));
    assert!(!ptr_eq(&get(&next, &["bars", "annotate"]), &get(&base, &["bars", "annotate"])));
}

// ── menus, panels and tabs ───────────────────────────────────────────────────

#[test]
fn add_menu_with_titled_sections() {
    let sections =
        js(json!([{ "title": "Export as", "items": ["acme:word"] }, ["document:download"]]));
    let next = create_layout(base()).add_menu("export", sections.clone()).to_json();
    assert!(ptr_eq(&get(&next, &["menus", "export"]), &sections));
}

#[test]
fn panels_are_added_changed_and_removed() {
    let layout = create_layout(base())
        .add_panel("notes", js(json!({ "side": "end", "title": "Notes", "icon": "book" })))
        .panel("thumbnails", js(json!({ "side": "end", "width": 320 })))
        .panel("thumbnail", js(json!({ "side": "start" })));
    assert_eq!(
        get(&layout.to_json(), &["panels"]),
        js(json!({
            "comments": { "side": "end" },
            "thumbnails": { "side": "end", "width": 320 },
            "notes": { "side": "end", "title": "Notes", "icon": "book" }
        }))
    );
    assert_eq!(
        layout_warnings_of(&layout),
        ["panel: no panel \"thumbnail\" (did you mean \"thumbnails\"?)"]
    );
}

#[test]
fn panels_changes_every_panel_the_layout_has_so_far() {
    let layout = create_layout(base())
        .panels(js(json!({ "present": { "medium": "dock" }, "resizable": false })))
        .panel("thumbnails", js(json!({ "present": { "compact": "overlay" } })))
        .add_panel("notes", js(json!({ "side": "end" })));
    assert_eq!(
        get(&layout.to_json(), &["panels"]),
        js(json!({
            "comments": { "side": "end", "present": { "medium": "dock" }, "resizable": false },
            "thumbnails": { "side": "start", "present": { "medium": "dock", "compact": "overlay" }, "resizable": false },
            "notes": { "side": "end" }
        }))
    );
    assert!(layout_warnings_of(&layout).is_empty());
}

#[test]
fn a_present_map_changes_only_the_sizes_it_names() {
    let layout = create_layout(base())
        .panel("comments", js(json!({ "present": "overlay" })))
        .panel("comments", js(json!({ "present": { "expanded": "dock" } })))
        .panel("thumbnails", js(json!({ "present": { "compact": "overlay" } })))
        .panel("thumbnails", js(json!({ "present": "dock" })));
    assert_eq!(
        get(&layout.to_json(), &["panels"]),
        js(json!({
            "comments": { "side": "end", "present": { "compact": "overlay", "medium": "overlay", "expanded": "dock" } },
            "thumbnails": { "side": "start", "present": "dock" }
        }))
    );
}

#[test]
fn a_width_keeps_its_limits_when_only_where_it_starts_changes() {
    let layout = create_layout(base())
        .panel("comments", js(json!({ "width": { "initial": 360, "min": 280, "max": 520 } })))
        .panel("comments", js(json!({ "width": 400 })))
        .panel("thumbnails", js(json!({ "width": 300 })))
        .panel("thumbnails", js(json!({ "width": { "max": 480 } })));
    assert_eq!(
        get(&layout.to_json(), &["panels"]),
        js(json!({
            "comments": { "side": "end", "width": { "initial": 400, "min": 280, "max": 520 } },
            "thumbnails": { "side": "start", "width": { "initial": 300, "max": 480 } }
        }))
    );
    let twice = create_layout(base())
        .panel("comments", js(json!({ "width": 300 })))
        .panel("comments", js(json!({ "width": 320 })))
        .to_json();
    assert_eq!(get(&twice, &["panels", "comments"]), js(json!({ "side": "end", "width": 320 })));
}

#[test]
fn remove_panel_takes_its_button_out_of_every_bar_and_menu() {
    let next = create_layout(base())
        .add_menu("extra", js(json!([["panel:comments", "acme:x"]])))
        .remove_panel("comments")
        .to_json();
    assert_eq!(get(&next, &["panels"]), js(json!({ "thumbnails": { "side": "start" } })));
    assert_eq!(
        get(&next, &["bars", "main", "end"]),
        js(json!([{ "id": "panels", "items": ["panel:search"] }]))
    );
    assert_eq!(get(&next, &["menus", "extra"]), js(json!([["acme:x"]])));
}

#[test]
fn tabs_sets_when_the_document_tabs_show() {
    let next = create_layout(base()).tabs(TabsVisibility::Multiple).to_json();
    assert_eq!(get(&next, &["tabs"]), JsValue::from("multiple"));
    assert_eq!(next.keys(), ["bars", "menus", "panels", "tabs"]);
}

// ── the env ───────────────────────────────────────────────────────────────────

#[test]
fn the_viewer_is_sized_by_its_own_width() {
    assert_eq!(size_of(0.0), ViewerSize::Compact);
    assert_eq!(size_of(599.0), ViewerSize::Compact);
    assert_eq!(size_of(600.0), ViewerSize::Medium);
    assert_eq!(size_of(1023.0), ViewerSize::Medium);
    assert_eq!(size_of(1024.0), ViewerSize::Expanded);
}

#[test]
fn envs_compare_every_field() {
    assert!(same_layout_env(&ENV, &ENV.clone()));
    assert!(!same_layout_env(&ENV, &LayoutEnv { width: 1281.0, ..ENV }));
    assert!(!same_layout_env(&ENV, &LayoutEnv { touch: true, ..ENV }));
    assert!(!same_layout_env(&ENV, &LayoutEnv { dir: TextDirection::Rtl, ..ENV }));
}

// ── resolveLayout ─────────────────────────────────────────────────────────────

#[test]
fn resolve_layout_gives_the_default_without_an_option() {
    let defaults = create_layout(base());
    assert!(Layout::same(&resolve_layout(None, &defaults, &ENV), &defaults));
}

#[test]
fn resolve_layout_calls_a_function_with_the_default_and_the_env() {
    let defaults = create_layout(base());
    let option = LayoutOption::Function(Rc::new(|layout: &Layout, env: &LayoutEnv| {
        Ok(LayoutResult::Layout(if env.size == ViewerSize::Compact {
            layout.place("main", "bottom")
        } else {
            layout.clone()
        }))
    }));
    let compact = LayoutEnv { size: ViewerSize::Compact, width: 400.0, ..ENV };
    let resolved = resolve_layout(Some(&option), &defaults, &compact);
    assert_eq!(get(&resolved.to_json(), &["bars", "main", "place"]), JsValue::from("bottom"));
}

#[test]
fn resolve_layout_takes_a_layout_plain_data_or_a_function_that_returns_plain_data() {
    let defaults = create_layout(base());
    let data = js(json!({ "bars": { "main": { "place": "top" } } }));
    let resolved =
        resolve_layout(Some(&LayoutOption::Layout(defaults.clone())), &Layout::default(), &ENV);
    assert!(Layout::same(&resolved, &defaults));
    let resolved = resolve_layout(Some(&LayoutOption::Data(data.clone())), &defaults, &ENV);
    assert!(ptr_eq(&resolved.to_json(), &data));
    let returned = data.clone();
    let option = LayoutOption::Function(Rc::new(move |_: &Layout, _: &LayoutEnv| {
        Ok(LayoutResult::Data(returned.clone()))
    }));
    assert!(ptr_eq(&resolve_layout(Some(&option), &defaults, &ENV).to_json(), &data));
}

#[test]
fn resolve_layout_falls_back_to_the_default_with_a_warning() {
    let base = base();
    let defaults = create_layout(base.clone());
    let threw =
        LayoutOption::Function(Rc::new(|_: &Layout, _: &LayoutEnv| Err("boom".to_string())));
    let threw = resolve_layout(Some(&threw), &defaults, &ENV);
    assert!(ptr_eq(&threw.to_json(), &base));
    assert_eq!(
        layout_warnings_of(&threw),
        ["the layout function threw (boom); showing the default layout"]
    );
    let forgot =
        LayoutOption::Function(Rc::new(|_: &Layout, _: &LayoutEnv| Ok(LayoutResult::Nothing)));
    let forgot = resolve_layout(Some(&forgot), &defaults, &ENV);
    assert!(ptr_eq(&forgot.to_json(), &base));
    assert_eq!(
        layout_warnings_of(&forgot),
        ["the layout function returned nothing, not a layout; showing the default layout"]
    );
    let list = resolve_layout(Some(&LayoutOption::Data(js(json!([])))), &defaults, &ENV);
    assert_eq!(
        layout_warnings_of(&list),
        ["layout is object, not a layout; showing the default layout"]
    );
}

// ── edits of data of the wrong shape ─────────────────────────────────────────

#[test]
fn edits_skip_what_is_not_an_item_a_section_or_a_menu() {
    let broken = create_layout(js(json!({
        "bars": { "main": { "place": "top", "start": [[42, "tool:pan"]], "end": "tool:pan" } },
        "menus": { "document": "document:print" }
    })));
    let edited = broken
        .remove(&["tool:pan"], &from("main"))
        .add("tool:ink", &AddItemOptions { after: Some("tool:pan".into()), ..to("main") })
        .replace("tool:ink", "tool:note")
        .add(
            "document:download",
            &AddItemOptions { after: Some("document:print".into()), ..to("document") },
        )
        .remove(&["document:download"], &from("document"));
    assert_eq!(get(&edited.to_json(), &["bars", "main", "end"]), js(json!([["tool:note"]])));
    assert_eq!(get(&edited.to_json(), &["menus", "document"]), js(json!([])));
}

// ── validateLayout ────────────────────────────────────────────────────────────

const COMMANDS: [&str; 9] = [
    "menu:document",
    "tool:pan",
    "tool:pointer",
    "tool:highlight",
    "panel:search",
    "document:download",
    "document:print",
    "view:fullscreen",
    "menu:zoom",
];

fn known() -> LayoutKnownIds {
    LayoutKnownIds { commands: COMMANDS.iter().map(|c| c.to_string()).collect(), has_command: None }
}

fn valid() -> JsValue {
    js(json!({
        "bars": {
            "main": {
                "place": "top",
                "start": [["menu:document"], [{ "custom": "zoom", "command": "menu:zoom" }]],
                "center": [{ "id": "modes", "items": [] }],
                "end": [["panel:search", "panel:comments"]]
            },
            "annotate": {
                "place": { "with": "main" },
                "mode": "annotate",
                "titleKey": "commands.mode.annotate",
                "center": [["tool:highlight"]]
            },
            "review": { "place": { "edge": "top", "after": "main" }, "center": [["mode:view", "mode:annotate"]] },
            "pager": { "place": { "float": "bottom" }, "center": [[{ "custom": "page-number" }]] },
            "selection": { "place": { "at": "selection" } }
        },
        "menus": { "document": [["document:download"], { "title": "More", "items": ["view:fullscreen"] }] },
        "panels": { "comments": { "side": "end", "present": "overlay", "width": 320 } },
        "tabs": "multiple"
    }))
}

#[test]
fn validate_finds_nothing_wrong_with_a_layout_whose_ids_all_exist() {
    // mode:view, mode:annotate and panel:comments are the layout's own.
    assert!(validate_layout(&valid(), &known()).is_empty());
    assert!(validate_layout(&create_layout(valid()), &known()).is_empty());
}

#[test]
fn validate_names_an_unknown_command_with_the_closest_one_that_exists() {
    let typo = js(json!({
        "bars": { "main": { "place": "top", "start": [["tool:pann", "acme:mine"]] } },
        "menus": { "document": [["document:downlaod"]] }
    }));
    assert_eq!(
        validate_layout(&typo, &known()),
        [
            "bar \"main\": no command \"tool:pann\" (did you mean \"tool:pan\"?)",
            "bar \"main\": no command \"acme:mine\"",
            "menu \"document\": no command \"document:downlaod\" (did you mean \"document:download\"?)",
        ]
    );
}

#[test]
fn validate_asks_has_command_about_ids_a_list_can_not_hold() {
    let family = js(json!({ "menus": { "zoom": [["zoom:125", "zoom:nope"]] } }));
    let ids = LayoutKnownIds {
        has_command: Some(Box::new(|id: &str| {
            id.strip_prefix("zoom:")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })),
        ..known()
    };
    assert_eq!(validate_layout(&family, &ids), ["menu \"zoom\": no command \"zoom:nope\""]);
}

#[test]
fn validate_checks_the_command_your_own_component_stands_for() {
    let custom = js(json!({
        "bars": { "main": { "place": "top", "start": [[{ "custom": "versions", "command": "acme:versions" }]] } }
    }));
    assert_eq!(validate_layout(&custom, &known()), ["bar \"main\": no command \"acme:versions\""]);
}

#[test]
fn validate_puts_what_the_edits_could_not_do_first() {
    let layout = create_layout(valid()).remove(&["tool:pann"], &from("main"));
    assert_eq!(validate_layout(&layout, &known()), ["remove: \"tool:pann\" is not in \"main\""]);
}

#[test]
fn validate_checks_every_kind_of_place() {
    let places = js(json!({
        "bars": {
            "main": { "place": "top" },
            "rail": { "place": "start" },
            "none": {},
            "sideways": { "place": "sideways" },
            "odd": { "place": { "edge": "middle" } },
            "lost": { "place": { "edge": "top", "after": "mian" } },
            "wrongEdge": { "place": { "edge": "top", "before": "rail" } },
            "floating": { "place": { "float": "bottom-middle" } },
            "self": { "place": { "with": "self" } },
            "orphan": { "place": { "with": "mian" } },
            "loopA": { "place": { "with": "loopB" } },
            "loopB": { "place": { "with": "loopA" } },
            "selected": { "place": { "at": "everything" } },
            "unknown": { "place": { "near": "main" } }
        }
    }));
    assert_eq!(
        validate_layout(&places, &known()),
        [
            "bar \"none\" has no place",
            "bar \"sideways\": \"sideways\" is not a place (an edge, { edge, before | after }, { float }, { with } or { at })",
            "bar \"odd\": \"middle\" is not an edge",
            "bar \"lost\" goes after \"mian\": no such bar (did you mean \"main\"?)",
            "bar \"wrongEdge\" goes before \"rail\", which is not on the top edge",
            "bar \"floating\": \"bottom-middle\" is not a floating position",
            "bar \"self\" rides along itself",
            "bar \"orphan\" rides along \"mian\": no such bar (did you mean \"main\"?)",
            "bar \"loopA\" rides along a bar that rides along it",
            "bar \"loopB\" rides along a bar that rides along it",
            "bar \"selected\": \"everything\" is not selection or annotation",
            "bar \"unknown\": {\"near\":\"main\"} is not a place (an edge, { edge, before | after }, { float }, { with } or { at })",
        ]
    );
}

#[test]
fn validate_warns_about_a_mode_bar_with_no_title_and_no_command_of_its_own() {
    let untitled = js(json!({
        "bars": { "review": { "place": { "with": "main" }, "mode": "review" }, "main": { "place": "top" } }
    }));
    assert_eq!(
        validate_layout(&untitled, &known()),
        ["bar \"review\": its mode \"review\" has no title, so its tab reads \"review\""]
    );
    let mut ids = known();
    ids.commands.push("mode:review".into());
    assert!(validate_layout(&untitled, &ids).is_empty());
}

#[test]
fn validate_warns_about_repeats() {
    let repeats = js(json!({
        "bars": {
            "main": {
                "place": "top",
                "start": [{ "id": "tools", "items": ["tool:pan", "tool:pan"] }, { "id": "tools", "items": ["tool:pointer"] }]
            }
        },
        "menus": { "document": [["document:print"], ["document:print"]] }
    }));
    assert_eq!(
        validate_layout(&repeats, &known()),
        [
            "bar \"main\": \"tool:pan\" is twice in one group",
            "bar \"main\": two groups are called \"tools\"",
            "menu \"document\": \"document:print\" is in it twice",
        ]
    );
}

#[test]
fn validate_warns_about_what_is_not_an_item_a_section_or_a_menu() {
    let shapes = js(json!({
        "bars": { "main": { "place": "top", "start": [[42]], "end": "tool:pan" } },
        "menus": { "document": "document:print", "view": [[7]] }
    }));
    assert_eq!(
        validate_layout(&shapes, &known()),
        [
            "bar \"main\": 42 is not an item",
            "bar \"main\": its end section is not a list of groups",
            "menu \"document\" is not a list of sections",
            "menu \"view\": 7 is not a command id",
        ]
    );
}

#[test]
fn validate_checks_panels_and_tabs() {
    let wrong = js(json!({
        "panels": { "notes": { "side": "left", "present": "float", "width": -1 }, "fine": { "side": "start" } },
        "tabs": "sometimes"
    }));
    assert_eq!(
        validate_layout(&wrong, &known()),
        [
            "panel \"notes\": side \"left\" is not start or end",
            "panel \"notes\": present \"float\" is not dock, overlay or sheet",
            "panel \"notes\": width -1 is not a width in px",
            "tabs: \"sometimes\" is not one of always, multiple or never",
        ]
    );
}

#[test]
fn validate_checks_a_panels_present_per_size_its_width_limits_and_resizable() {
    let wrong = js(json!({
        "panels": {
            "notes": {
                "present": { "compact": "overlay", "medum": "dock", "expanded": "float" },
                "width": { "initial": 300, "min": 400, "max": 320, "widest": 9 },
                "resizable": "no"
            },
            "fine": { "present": { "compact": "overlay" }, "width": { "initial": 320, "min": 240, "max": 480 } }
        }
    }));
    assert_eq!(
        validate_layout(&wrong, &known()),
        [
            "panel \"notes\": present has \"medum\", not a size (did you mean \"medium\"?)",
            "panel \"notes\": present.expanded \"float\" is not dock, overlay or sheet",
            "panel \"notes\": width has \"widest\", not initial, min or max",
            "panel \"notes\": width.min 400 is wider than width.max 320",
            "panel \"notes\": resizable \"no\" is not true or false",
        ]
    );
}

#[test]
fn validate_says_so_when_the_layout_is_not_an_object() {
    assert_eq!(validate_layout(&JsValue::Null, &known()), ["the layout is not an object"]);
}

#[test]
fn validate_names_a_dependent_that_is_not_a_boolean_and_a_bar_of_dependent_groups_only() {
    let problems = validate_layout(
        &js(json!({
            "bars": {
                "main": { "place": "top", "start": [["tool:pan"], { "id": "odd", "items": ["tool:pointer"], "dependent": "yes" }] },
                "alone": { "place": "bottom", "center": [{ "id": "history", "items": ["tool:highlight"], "dependent": true }] }
            }
        })),
        &known(),
    );
    assert_eq!(
        problems,
        [
            "bar \"main\": group \"odd\" has dependent \"yes\", which is not true or false",
            "bar \"alone\": every group in it is dependent, so it never shows",
        ]
    );
}

// ── suggestions ───────────────────────────────────────────────────────────────

#[test]
fn edit_distance_counts_inserts_deletes_substitutions_and_swapped_neighbours() {
    assert_eq!(edit_distance("tool:pan", "tool:pan"), 0);
    assert_eq!(edit_distance("tool:pann", "tool:pan"), 1);
    assert_eq!(edit_distance("tool:pen", "tool:pan"), 1);
    assert_eq!(edit_distance("mian", "main"), 1);
    assert_eq!(edit_distance("", "abc"), 3);
    assert_eq!(edit_distance("kitten", "sitting"), 3);
    // UTF-16 units, as the TypeScript indexes strings.
    assert_eq!(edit_distance("😀", "a"), 2);
}

#[test]
fn closest_match_suggests_only_within_a_typos_reach_and_never_the_id_itself() {
    assert_eq!(closest_match("tool:pann", ["tool:pan", "tool:pointer"]), Some("tool:pan"));
    assert_eq!(closest_match("mian", ["main", "menu"]), Some("main"));
    assert_eq!(closest_match("pan:toggle", ["tool:pan"]), None);
    assert_eq!(closest_match("acme:mine", ["tool:pan"]), None);
    assert_eq!(closest_match("x", []), None);
    assert_eq!(closest_match("main", ["main"]), None);
    assert_eq!(did_you_mean("tool:pann", ["tool:pan"]), " (did you mean \"tool:pan\"?)");
    assert_eq!(did_you_mean("acme:mine", ["tool:pan"]), "");
}

// ── schema and shortcuts refusals ─────────────────────────────────────────────

#[test]
fn normalize_bar_refuses_a_duplicate_unit_group_or_an_empty_ladder() {
    let refuse =
        |bar: Value| normalize_bar(&serde_json::from_value(bar).unwrap()).unwrap_err().message;
    assert_eq!(
        refuse(
            json!({ "id": "b", "sections": { "start": [{ "id": "g", "items": ["a", { "command": "a" }] }] } })
        ),
        "[ui-core] duplicate unit in bar: g:a"
    );
    assert_eq!(
        refuse(
            json!({ "id": "b", "sections": { "start": [{ "id": "g", "items": ["a"] }], "end": [{ "id": "g", "items": ["b"] }] } })
        ),
        "[ui-core] duplicate group in bar: g"
    );
    assert_eq!(
        refuse(
            json!({ "id": "b", "sections": { "start": [{ "id": "g", "items": [{ "slot": "s", "variants": [] }] }] } })
        ),
        "[ui-core] empty variant ladder: g:s"
    );
}

#[test]
fn a_custom_item_without_a_command_pins_itself_and_its_group() {
    let bar = normalize_bar(
        &serde_json::from_value(json!({
            "id": "b",
            "sections": { "start": [{ "id": "g", "importance": 1, "items": [{ "slot": "s", "importance": 2 }, "a"] }] }
        }))
        .unwrap(),
    )
    .unwrap();
    let group = &bar.sections[0].groups[0];
    assert_eq!(group.importance, 5);
    assert_eq!(group.units[0].importance(), 5);
    assert_eq!(serde_json::to_value(&group.units[0]).unwrap()["terminal"], Value::Null);
}

#[test]
fn parse_shortcut_refuses_an_unknown_modifier() {
    assert_eq!(
        parse_shortcut("Hyper+K").unwrap_err().message,
        "[ui-core] unknown modifier \"Hyper\" in \"Hyper+K\""
    );
    assert!(parse_shortcut("Ctrl+Shift++").is_ok());
}

#[test]
fn shortcuts_match_compare_and_show_as_each_platform_does() {
    let mac = ShortcutOptions { is_mac: true };
    let other = ShortcutOptions { is_mac: false };
    let stroke = |key: &str, shift: bool| epdf_ui_core::KeyStroke {
        key: key.into(),
        code: None,
        ctrl_key: false,
        meta_key: false,
        alt_key: false,
        shift_key: shift,
    };
    // An undeclared shift on a letter is another shortcut.
    assert!(!match_shortcut(&parse_shortcut("k").unwrap(), &stroke("K", true), other));
    assert!(match_shortcut(&parse_shortcut("+").unwrap(), &stroke("+", true), other));
    assert!(same_shortcut("Mod+S", "Ctrl+s", other).unwrap());
    assert!(!same_shortcut("Mod+S", "Ctrl+s", mac).unwrap());
    assert_eq!(format_shortcut("Mod+Escape", mac).unwrap(), "⌘Esc");
    assert_eq!(format_shortcut("Shift+Backspace", other).unwrap(), "Shift+Backspace");
    assert_eq!(format_shortcut("Ctrl+ArrowLeft", mac).unwrap(), "⌃←");
}

// ── Rust-only divergences (README, "Not ported") ──────────────────────────────

/// `Object.prototype` members are not layout ids: the TypeScript finds
/// `bars['constructor']` (a function) and treats it as a bar.
#[test]
fn an_object_prototype_name_is_no_bar_menu_or_key_name() {
    let layout = create_layout(base()).add("x", &to("constructor"));
    assert_eq!(layout_warnings_of(&layout), ["add: no bar or menu \"constructor\""]);
    assert_eq!(
        format_shortcut("Ctrl+constructor", ShortcutOptions { is_mac: false }).unwrap(),
        "Ctrl+Constructor"
    );
}

/// The TypeScript throws a `TypeError` reading a member of a `null` bar or
/// panel; Rust reads nothing there and goes on.
#[test]
fn a_null_bar_or_panel_is_skipped_where_the_typescript_throws() {
    let data = js(json!({
        "bars": { "gone": null, "main": { "place": { "edge": "top", "after": "x" }, "start": [["a"]] } },
        "panels": { "gone": null, "notes": { "side": "end" } }
    }));
    assert_eq!(
        validate_layout(&data, &LayoutKnownIds { commands: vec!["a".into()], has_command: None }),
        [
            "bar \"gone\" is not a bar",
            "bar \"main\" goes after \"x\": no such bar",
            "panel \"gone\" is not a panel",
        ]
    );
    let replaced = create_layout(data.clone()).replace("a", "b").to_json();
    assert_eq!(get(&replaced, &["bars", "main", "start"]), js(json!([["b"]])));
    let panels = create_layout(data.clone()).panels(js(json!({ "side": "start" }))).to_json();
    assert_eq!(get(&panels, &["panels", "gone"]), js(json!({ "side": "start" })));
}
