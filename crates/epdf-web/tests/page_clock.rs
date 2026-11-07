//! `PageClock`: the kernel's frames from the host clock's `nextFrame`.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::clock::HostClock;
use epdf_web::clock::PageClock;
use epdf_web::runtime::reset;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// A `nextFrame` that queues each request in `host.frames` and counts its
/// cancels in `host.cancelled`.
fn host() -> (JsValue, js_sys::Function) {
    let host = js_sys::Object::new();
    let make = js_sys::Function::new_with_args(
        "host",
        "host.frames = []; host.cancelled = 0;
         return (run) => { host.frames.push(run); return () => { host.cancelled += 1; }; };",
    );
    let next_frame = make.call1(&JsValue::NULL, &host).unwrap().into();
    (host.into(), next_frame)
}

fn get(host: &JsValue, member: &str) -> JsValue {
    js_sys::Reflect::get(host, &member.into()).unwrap()
}

/// Run the host's oldest frame request at `time`.
fn fire(host: &JsValue, time: f64) {
    let frames: js_sys::Array = get(host, "frames").into();
    let run: js_sys::Function = frames.shift().into();
    run.call1(&JsValue::NULL, &time.into()).unwrap();
}

#[wasm_bindgen_test]
fn without_next_frame_it_has_no_frames() {
    reset();
    assert!(!PageClock::new(None).has_frames());
}

#[wasm_bindgen_test]
fn every_frame_asked_before_the_host_paints_runs_in_one_request() {
    reset();
    let (host, next_frame) = host();
    let clock = PageClock::new(Some(next_frame));
    assert!(clock.has_frames());
    let seen = Rc::new(RefCell::new(Vec::new()));
    for tag in ["a", "b"] {
        let seen = seen.clone();
        let _cancel = clock.next_frame(Box::new(move |time| seen.borrow_mut().push((tag, time))));
    }
    assert_eq!(js_sys::Array::from(&get(&host, "frames")).length(), 1);
    fire(&host, 16.0);
    assert_eq!(*seen.borrow(), vec![("a", 16.0), ("b", 16.0)]);

    // A run that asks again gets the next frame.
    let again = seen.clone();
    let _cancel = clock.next_frame(Box::new(move |time| again.borrow_mut().push(("c", time))));
    fire(&host, 32.0);
    assert_eq!(seen.borrow().last(), Some(&("c", 32.0)));
}

#[wasm_bindgen_test]
fn a_frame_nothing_waits_for_is_cancelled() {
    reset();
    let (host, next_frame) = host();
    let clock = PageClock::new(Some(next_frame));
    let ran = Rc::new(RefCell::new(0));
    let (a, b) = (ran.clone(), ran.clone());
    let first = clock.next_frame(Box::new(move |_| *a.borrow_mut() += 1));
    let second = clock.next_frame(Box::new(move |_| *b.borrow_mut() += 1));
    first.call();
    assert_eq!(get(&host, "cancelled").as_f64(), Some(0.0), "the second still waits");
    second.call();
    assert_eq!(get(&host, "cancelled").as_f64(), Some(1.0));
    assert_eq!(*ran.borrow(), 0);
}
