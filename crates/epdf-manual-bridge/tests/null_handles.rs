//! A null handle answers `invalid-input` through `done` instead of being
//! dereferenced. No host is needed: nothing reaches a kernel thread.

use std::ffi::{CStr, c_void};
use std::sync::Mutex;

use epdf_manual_bridge::*;

/// What each `done` answered: (ctx, kind, code).
static ANSWERS: Mutex<Vec<(usize, i32, Option<String>)>> = Mutex::new(Vec::new());

extern "C" fn done(ctx: *mut c_void, error: EpdfError, _result: *const c_void) {
    let code = (!error.code.is_null())
        .then(|| unsafe { CStr::from_ptr(error.code) }.to_string_lossy().into_owned());
    ANSWERS.lock().unwrap().push((ctx as usize, error.kind, code));
    unsafe { epdf_error_free(error) };
}

extern "C" fn on_event(_ctx: *mut c_void, _kind: i32, _payload: *const c_void) {}

static RELEASED: Mutex<Vec<usize>> = Mutex::new(Vec::new());
extern "C" fn release(ctx: *mut c_void) {
    RELEASED.lock().unwrap().push(ctx as usize);
}

fn answer(tag: usize) -> Option<(i32, Option<String>)> {
    let answers = ANSWERS.lock().unwrap();
    answers.iter().find(|(t, ..)| *t == tag).map(|(_, kind, code)| (*kind, code.clone()))
}

#[test]
fn a_null_handle_answers_invalid_input() {
    let text = c"needle".as_ptr();
    let ctx = |n: usize| n as *mut c_void;
    let null_search = std::ptr::null_mut::<EpdfSearch>();
    unsafe {
        assert_eq!(epdf_render_page(std::ptr::null_mut(), 1, 1.0, ctx(1), done), 0);
        epdf_search_hit_count(null_search, ctx(2), done);
        epdf_search_hits(null_search, ctx(3), done);
        assert_eq!(epdf_search_search(null_search, text, ctx(4), done), 0);
        assert_eq!(epdf_search_find_all(null_search, text, ctx(5), done), 0);
    }
    for tag in 1..=5 {
        let (kind, code) = answer(tag).unwrap_or_else(|| panic!("call {tag} never answered"));
        assert_eq!(kind, EPDF_ERROR_KERNEL, "call {tag}");
        assert_eq!(code.as_deref(), Some("invalid-input"), "call {tag}");
    }
}

#[test]
fn a_null_search_subscribes_nothing_and_releases_at_once() {
    let subscription = unsafe {
        epdf_search_subscribe(std::ptr::null_mut(), 7 as *mut c_void, on_event, Some(release))
    };
    assert!(subscription.is_null());
    assert_eq!(*RELEASED.lock().unwrap(), vec![7]);
    unsafe { epdf_subscription_free(subscription) };
}
