//! Cursor behavior through the real plugin, without an actions plugin.
use epdf_kernel::rt;
use epdf_plugin_form::{FORM, FormCapability, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec, settle};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};
fn field(n: u32, value: &str) -> Value {
    json!({"ref":{"kind":"objectNumber","objectNumber":n},"name":format!("field{n}"),"family":"text","readOnly":n==3,"value":value,"valueEntry":{"kind":"scalar","value":value},"widgets":[{"objectNumber":n+10,"page":{"kind":"objectNumber","objectNumber":1}}]})
}
fn widget(n: u32) -> Value {
    json!({"kind":"objectNumber","objectNumber":n,"page":{"kind":"objectNumber","objectNumber":1}})
}
fn rig() -> (TestKernel, FormCapability) {
    rig_on(false)
}
fn rig_on(with_stage: bool) -> (TestKernel, FormCapability) {
    rt::reset();
    let page = json!({"ref":{"kind":"objectNumber","objectNumber":1},"index":0,"size":{"width":600,"height":800},"rotation":0,"userUnit":1,"tabOrder":"rows","boxes":{"crop":{"x":0,"y":0,"width":600,"height":800},"media":{"x":0,"y":0,"width":600,"height":800}}});
    let spec=TestSpec::new(json!({"documents":{"a":{"pages":[page],"security":{"allows":"*"}}}})).respond("forms.list", |_| Ok(Reply::value(json!({"formKind":"acroform","needsAppearances":false,"calculationOrder":[],"fields":[field(1,""),field(2,"Ada"),field(3,"")],"widgets":[{"ref":widget(13),"page":{"objectNumber":1},"subtype":"widget","rect":{"x":50,"y":300,"width":200,"height":20}},{"ref":widget(12),"page":{"objectNumber":1},"subtype":"widget","rect":{"x":50,"y":200,"width":200,"height":20}},{"ref":widget(11),"page":{"objectNumber":1},"subtype":"widget","rect":{"x":50,"y":100,"width":200,"height":20}}]}))));
    let mut plugins = vec![interaction_plugin(InteractionConfig::default()), form_plugin(None)];
    if with_stage {
        plugins.push(epdf_plugin_stage::stage_plugin(Default::default()));
    }
    let k = TestKernel::open(plugins, spec).unwrap();
    let form = k.capability(&FORM).unwrap();
    settle(&form.refresh(None)).unwrap();
    (k, form)
}
fn attach(
    form: &FormCapability,
    n: u32,
    heard: &Rc<RefCell<Vec<(u32, bool)>>>,
) -> epdf_kernel::Unsubscribe {
    let heard = heard.clone();
    let target_form = form.clone();
    form.attach_focus_target(
        &widget(n),
        Rc::new(move |select| {
            heard.borrow_mut().push((n, select));
            target_form
                .notify_widget_event(
                    &json!({"kind":"objectNumber","objectNumber":n-10}),
                    &widget(n),
                    epdf_plugin_actions::PdfAnnotationEventKind::Focus,
                )
                .unwrap();
        }),
    )
}
#[test]
fn focus_is_synchronous_and_tracks_the_cursor_without_actions() {
    let (_k, form) = rig();
    let heard = Rc::default();
    let _off = attach(&form, 12, &heard);
    assert!(form.focus(&field(2, "")["ref"], Some(&json!({"select":true}))));
    assert_eq!(*heard.borrow(), vec![(12, true)]);
    assert_eq!(form.get_focused_field().unwrap()["name"], "field2");
    assert!(!form.focus(&field(3, "")["ref"], None));
    assert!(!form.focus(&json!({"kind":"objectNumber","objectNumber":99}), None));
}
#[test]
fn tab_uses_page_rows_skips_readonly_and_leaves_at_the_end() {
    let (_k, form) = rig();
    let heard = Rc::default();
    let _a = attach(&form, 11, &heard);
    let _b = attach(&form, 12, &heard);
    assert!(form.focus_next(None));
    assert!(form.focus_next(None));
    assert!(!form.focus_next(None));
    assert_eq!(*heard.borrow(), vec![(11, true), (12, true)]);
    assert!(form.focus_next(Some(&json!({"wrap":true}))));
    assert_eq!(heard.borrow().last(), Some(&(11, true)));
    assert!(!form.focus_next(Some(&json!({"from":field(1,"")["ref"],"empty":true}))));
}
#[test]
fn blur_and_focus_in_one_turn_emit_no_intermediate_null() {
    let (_k, form) = rig();
    let heard = Rc::default();
    let _a = attach(&form, 11, &heard);
    let _b = attach(&form, 12, &heard);
    let changes = Rc::new(RefCell::new(Vec::new()));
    let log = changes.clone();
    let _event = form.on_focus_changed(move |e| {
        log.borrow_mut().push(e["field"]["name"].clone());
        Ok(())
    });
    form.focus(&field(1, "")["ref"], None);
    form.notify_widget_event(
        &field(1, "")["ref"],
        &widget(11),
        epdf_plugin_actions::PdfAnnotationEventKind::Blur,
    )
    .unwrap();
    form.focus(&field(2, "")["ref"], None);
    rt::advance(0.0);
    assert_eq!(*changes.borrow(), vec![json!("field1"), json!("field2")]);
    form.notify_widget_event(
        &field(2, "")["ref"],
        &widget(12),
        epdf_plugin_actions::PdfAnnotationEventKind::Blur,
    )
    .unwrap();
    rt::advance(0.0);
    assert!(form.get_focused_field().is_none());
    assert_eq!(changes.borrow().last(), Some(&Value::Null));
}
#[test]
fn a_replaced_targets_old_disposer_cannot_remove_the_new_one() {
    let (_k, form) = rig();
    let old = Rc::default();
    let new = Rc::default();
    let off = attach(&form, 11, &old);
    let _new = attach(&form, 11, &new);
    off.call();
    assert!(form.focus(&field(1, "")["ref"], None));
    assert!(old.borrow().is_empty());
    assert_eq!(new.borrow().len(), 1);
    _new.call();
    assert!(!form.focus(&field(1, "")["ref"], None));
}

#[test]
fn a_pending_target_arrives_once_and_expires_after_two_seconds() {
    let (k, form) = rig_on(true);
    let heard = Rc::default();
    assert!(form.focus(&field(1, "")["ref"], Some(&json!({"select":true}))));
    let off = attach(&form, 11, &heard);
    assert_eq!(*heard.borrow(), vec![(11, true)]);
    off.call();
    assert!(form.focus(&field(2, "")["ref"], None));
    rt::advance(2000.0);
    let late = attach(&form, 12, &heard);
    assert_eq!(heard.borrow().len(), 1);
    late.call();
    k.close().unwrap();
}
#[test]
fn a_users_focus_cancels_an_offscreen_request_and_settings_gate_input() {
    let (k, form) = rig_on(true);
    let heard = Rc::default();
    let first = attach(&form, 11, &heard);
    assert!(form.focus(&field(2, "")["ref"], None));
    form.focus(&field(1, "")["ref"], None);
    let late = attach(&form, 12, &heard);
    assert_eq!(*heard.borrow(), vec![(11, false)]);
    form.update_settings(&json!({"fillable":false}));
    assert!(!form.focus_next(None));
    assert!(!form.focus(&field(1, "")["ref"], None));
    form.update_settings(&json!({"fillable":["checkbox"]}));
    assert!(!form.focus(&field(1, "")["ref"], None));
    form.update_settings(&json!({"fillable":true}));
    assert!(form.focus_next(None));
    assert_eq!(heard.borrow().last(), Some(&(12, true)));
    first.call();
    late.call();
    k.close().unwrap();
}

#[test]
fn radios_are_one_stop_at_the_checked_button_and_arrows_wrap_and_pick() {
    rt::reset();
    let plan = json!({"ref":{"kind":"objectNumber","objectNumber":5},"name":"plan","family":"radio","readOnly":false,"value":"yearly","valueEntry":{"kind":"scalar","value":"yearly"},"widgets":[{"objectNumber":15,"page":{"objectNumber":1},"checked":false,"exportValue":"monthly"},{"objectNumber":16,"page":{"objectNumber":1},"checked":true,"exportValue":"yearly"}]});
    let snapshot = json!({"formKind":"acroform","needsAppearances":false,"calculationOrder":[],"fields":[plan],"widgets":[{"ref":widget(15),"page":{"objectNumber":1},"subtype":"widget","rect":{"x":10,"y":100,"width":20,"height":20}},{"ref":widget(16),"page":{"objectNumber":1},"subtype":"widget","rect":{"x":10,"y":200,"width":20,"height":20}}]});
    let applied = Rc::new(RefCell::new(Vec::new()));
    let log = applied.clone();
    let spec=TestSpec::new(json!({"documents":{"a":{"pages":[{"ref":{"kind":"objectNumber","objectNumber":1},"index":0,"size":{"width":600,"height":800},"rotation":0,"userUnit":1,"tabOrder":"rows"}],"security":{"allows":"*"}}}}))
        .respond("forms.list",move |_|Ok(Reply::value(snapshot.clone())))
        .respond("apply",move |args|{log.borrow_mut().push(args[0]["ops"].clone());Ok(Reply::value(json!({"items":[],"meta":{"undoable":true}})))});
    let k = TestKernel::open(
        vec![
            interaction_plugin(Default::default()),
            form_plugin(Some(json!({"validation":"none"}))),
        ],
        spec,
    )
    .unwrap();
    let form = k.capability(&FORM).unwrap();
    settle(&form.refresh(None)).unwrap();
    let heard = Rc::default();
    let first = attach(&form, 15, &heard);
    let second = attach(&form, 16, &heard);
    assert!(form.focus_next(None));
    assert_eq!(*heard.borrow(), vec![(16, true)]);
    assert!(!form.focus_next(None));
    assert!(form.focus(
        &json!({"kind":"objectNumber","objectNumber":5}),
        Some(&json!({"widget":widget(15)}))
    ));
    assert_eq!(heard.borrow().last(), Some(&(15, false)));
    assert!(form.move_in_group(&widget(16), false));
    rt::run_until_stalled();
    assert_eq!(heard.borrow().last(), Some(&(15, false)));
    assert!(applied.borrow().iter().any(|ops| {
        ops.as_array().is_some_and(|ops| ops.iter().any(|op| op["value"]["value"] == "monthly"))
    }));
    first.call();
    second.call();
    k.close().unwrap();
}
