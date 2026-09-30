//! `Store::track_begin`/`track_end` name each part a call read, with its
//! revision as the call ended, and a nested call's reads count for the call
//! around it.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::store::{SliceJson, SliceReducer, SliceState, Store};
use serde_json::{Value, json};

fn counter() -> (SliceReducer, SliceJson) {
    let reducer: SliceReducer = Rc::new(|_: &SliceState, _| None);
    let to_json: SliceJson = Rc::new(|state: &dyn Any| json!({ "n": state.downcast_ref::<i64>() }));
    (reducer, to_json)
}

fn reads(text: String) -> Value {
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_call_names_what_it_read() {
    let store = Store::new(Rc::new(|_| {}));
    let (reducer, to_json) = counter();
    let a = store.lease("a", reducer.clone(), Rc::new(0_i64), to_json.clone(), None);
    store.lease("b", reducer, Rc::new(0_i64), to_json, None);
    let revisions = reads(store.revisions_json());

    store.track_begin();
    a.read();
    let only_a = reads(store.track_end());
    assert_eq!(only_a, json!({ "core": null, "plugins": { "a": revisions["plugins"]["a"] } }));

    store.track_begin();
    store.core();
    store.track_begin();
    store.get_slice("b");
    store.get_slice("missing");
    let inner = reads(store.track_end());
    let outer = reads(store.track_end());
    assert_eq!(inner, json!({ "core": null, "plugins": { "b": revisions["plugins"]["b"], "missing": null } }));
    assert_eq!(
        outer,
        json!({
            "core": revisions["core"],
            "plugins": { "b": revisions["plugins"]["b"], "missing": null }
        })
    );

    // A read of the whole tree depends on everything.
    store.track_begin();
    store.state_json();
    assert_eq!(reads(store.track_end()), json!({ "all": true }));

    // Nothing is recorded outside a tracked call.
    a.read();
    store.track_begin();
    assert_eq!(reads(store.track_end()), json!({ "core": null, "plugins": {} }));
}
