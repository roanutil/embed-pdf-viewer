//! A harness whose callbacks call a live `LiveFn` instead of replaying a
//! scripted `$fn` spec.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use plugin_conformance::{Answer, Harness, LiveFn};
use serde_json::{Value, json};

type Recorded = Rc<RefCell<Vec<(Value, Vec<Value>)>>>;

#[test]
fn a_callback_forwards_to_the_live_fn() {
    let calls: Recorded = Rc::default();
    let recorded = calls.clone();
    let live: LiveFn = Rc::new(move |spec, args| {
        recorded.borrow_mut().push((spec.clone(), args));
        Answer::Returns(json!(42))
    });
    let harness = Harness::live(Vec::new(), HashMap::new(), live);

    let callback = harness.callback(&json!({ "$fn": "js:7" }));
    let answer = callback.call(vec![json!("a")]);

    match answer {
        Answer::Returns(value) => assert_eq!(value, json!(42)),
        _ => panic!("expected Answer::Returns"),
    }
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0["$fn"], json!("js:7"));
    assert_eq!(calls[0].1, vec![json!("a")]);
}
