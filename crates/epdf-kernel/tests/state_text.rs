//! `Store::state_text` is `state_json().to_string()`, byte for byte, through
//! every kind of change, and `revision` moves exactly when it might differ.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::store::{SliceJson, SliceReducer, SliceState, Store};
use serde_json::{Value, json};

#[derive(serde::Serialize)]
struct Add {
    r#type: &'static str,
    by: i64,
}

fn counter() -> (SliceReducer, SliceJson) {
    let reducer: SliceReducer = Rc::new(|state: &SliceState, action| {
        let add = action.as_any().downcast_ref::<Add>()?;
        let n = *state.downcast_ref::<i64>().expect("an i64 slice");
        (add.by != 0).then(|| Rc::new(n + add.by) as SliceState)
    });
    let to_json: SliceJson =
        Rc::new(|state: &dyn Any| json!({ "n": state.downcast_ref::<i64>(), "tag": "a\"b" }));
    (reducer, to_json)
}

fn same(store: &Store) {
    assert_eq!(&*store.state_text(), store.state_json().to_string().as_str());
}

#[test]
fn state_text_matches_state_json_through_every_change() {
    let store = Store::new(Rc::new(|_| {}));
    same(&store);
    let r0 = store.revision();

    let (reducer, to_json) = counter();
    let a = store.lease("zeta", reducer.clone(), Rc::new(0_i64), to_json.clone(), None);
    let r1 = store.revision();
    assert!(r1 > r0, "a new slice moves the revision");
    same(&store);
    store.lease("alpha \u{e9}", reducer, Rc::new(5_i64), to_json, None);
    same(&store);

    let before = store.revision();
    a.commit(&Add { r#type: "add", by: 0 });
    assert_eq!(store.revision(), before, "a reducer that returns None changes nothing");
    assert_eq!(&*store.state_text(), store.state_json().to_string().as_str());

    a.commit(&Add { r#type: "add", by: 3 });
    assert!(store.revision() > before);
    same(&store);
    let parsed: Value = serde_json::from_str(&store.state_text()).unwrap();
    assert_eq!(parsed["plugins"]["zeta"]["n"], 3);

    store.set_core(|core| core.active_id = Some("doc".into()), &Add { r#type: "core", by: 0 });
    same(&store);

    store.remove_slice("zeta");
    same(&store);
    assert!(
        serde_json::from_str::<Value>(&store.state_text()).unwrap()["plugins"]["zeta"].is_null()
    );

    store.destroy();
    same(&store);
}

#[test]
fn an_unchanged_revision_returns_the_same_text() {
    let store = Store::new(Rc::new(|_| {}));
    let (reducer, to_json) = counter();
    store.lease("k", reducer, Rc::new(1_i64), to_json, None);
    let first = store.state_text();
    let second = store.state_text();
    assert!(Rc::ptr_eq(&first, &second), "no rebuild while the revision holds");
}

#[test]
fn a_slice_text_writer_stands_in_for_the_value_and_none_falls_back() {
    use epdf_kernel::store::SliceText;
    let store = Store::new(Rc::new(|_| {}));
    let (reducer, to_json) = counter();
    let written: SliceText = Rc::new(|state: &dyn Any| {
        state.downcast_ref::<i64>().map(|n| format!(r#"{{"n":{n},"tag":"a\"b"}}"#))
    });
    store.lease_with_text(
        "w",
        reducer.clone(),
        Rc::new(4_i64),
        to_json.clone(),
        Some(written),
        None,
    );
    let declined: SliceText = Rc::new(|_: &dyn Any| None);
    store.lease_with_text("d", reducer, Rc::new(6_i64), to_json, Some(declined), None);
    same(&store);
    assert_eq!(&*store.slice_text("w").unwrap(), r#"{"n":4,"tag":"a\"b"}"#);
    assert_eq!(&*store.slice_text("d").unwrap(), r#"{"n":6,"tag":"a\"b"}"#);
}
