//! The pieces under the kernel that its vectors reach only indirectly: the
//! runtime's clock and promises, the store's listener lists, signals,
//! scopes, tokens, errors and bytes. Each test names the JavaScript
//! behaviour it pins, since there is no TypeScript file to generate these
//! from: they are the semantics of the platform the TypeScript runs on.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::action::CoreAction;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::error::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::scope::Scope;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Store;
use epdf_kernel::token::{AnyToken, Token};
use serde_json::json;

fn log() -> Rc<RefCell<Vec<String>>> {
    Rc::default()
}

// ── the clock: @sinonjs/fake-timers 15.4.0 ────────────────────────────────────

#[test]
fn delays_are_clamped_and_truncated_like_fake_timers() {
    rt::reset();
    let fired = log();
    for (delay, name) in [
        (f64::NAN, "nan"),
        (-5.0, "negative"),
        (1.7, "fractional"),
        (2_147_483_647.0, "max"),
        (2_147_483_648.0, "over-max"),
    ] {
        let fired = fired.clone();
        rt::set_timeout(delay, move || fired.borrow_mut().push(name.to_string()));
    }
    assert_eq!(rt::pending_timers(), 5);
    rt::advance(0.0);
    // NaN and negative become 0, and 0 outside a tick is 0.
    assert_eq!(*fired.borrow(), ["nan", "negative"]);
    rt::advance(1.0);
    // 1.7 truncates to 1; anything over 2^31-1 becomes 1.
    assert_eq!(*fired.borrow(), ["nan", "negative", "fractional", "over-max"]);
    rt::advance(2_147_483_646.0);
    assert_eq!(fired.borrow().last().map(String::as_str), Some("max"));
    assert_eq!(rt::pending_timers(), 0);
}

#[test]
fn a_sub_millisecond_delay_during_a_tick_is_one() {
    rt::reset();
    let at = Rc::new(Cell::new(-1.0));
    let a = at.clone();
    rt::set_timeout(2.0, move || {
        let a = a.clone();
        rt::set_timeout(0.5, move || a.set(rt::now()));
    });
    rt::advance(10.0);
    assert_eq!(at.get(), 3.0);
    assert_eq!(rt::now(), 10.0);
}

#[test]
fn fractional_advances_floor() {
    rt::reset();
    rt::advance(2.9);
    assert_eq!(rt::now(), 2.0);
}

#[test]
fn intervals_sharing_a_deadline_fire_in_rearm_order() {
    rt::reset();
    let fired = log();
    let (a, b) = (fired.clone(), fired.clone());
    rt::set_interval(2.0, move || a.borrow_mut().push(format!("a@{}", rt::now())));
    rt::set_interval(2.0, move || b.borrow_mut().push(format!("b@{}", rt::now())));
    rt::advance(4.0);
    assert_eq!(*fired.borrow(), ["a@2", "b@2", "a@4", "b@4"]);
}

#[test]
fn a_timer_cleared_by_an_earlier_one_in_the_same_advance_does_not_fire() {
    rt::reset();
    let fired = log();
    let f = fired.clone();
    let later = rt::set_timeout(5.0, move || f.borrow_mut().push("later".into()));
    rt::set_timeout(1.0, move || rt::clear_timer(later));
    rt::advance(10.0);
    assert!(fired.borrow().is_empty());
}

#[test]
fn reset_drops_tasks_timers_and_time() {
    rt::reset();
    rt::advance(7.0);
    rt::set_timeout(1.0, || panic!("dropped by reset"));
    let (pending, _resolver) = Promise::<()>::pending();
    rt::spawn(async move {
        let _ = pending.await;
        panic!("dropped by reset");
    });
    rt::reset();
    assert_eq!(rt::now(), 0.0);
    assert_eq!(rt::pending_timers(), 0);
    rt::advance(5.0);
    rt::run_until_stalled();
}

#[test]
fn sleep_resolves_on_the_clock() {
    rt::reset();
    let done = Rc::new(Cell::new(false));
    let d = done.clone();
    rt::spawn(async move {
        rt::sleep(3.0).await.unwrap();
        d.set(true);
    });
    rt::advance(2.0);
    assert!(!done.get());
    rt::advance(1.0);
    assert!(done.get());
}

// ── promises ──────────────────────────────────────────────────────────────────

#[test]
fn a_promise_settles_once_and_reports_it() {
    rt::reset();
    let (promise, resolver) = Promise::<i32>::pending();
    assert!(!promise.is_settled());
    assert!(promise.peek().is_none());
    resolver.resolve(1);
    resolver.resolve(2);
    resolver.reject(Error::new("late"));
    assert!(promise.is_settled());
    assert_eq!(promise.peek().unwrap().unwrap(), 1);
    assert!(promise.same(&promise.clone()));
    assert!(!promise.same(&Promise::resolved(1)));
}

#[test]
fn on_settle_observes_synchronously_or_immediately() {
    rt::reset();
    let seen = log();
    let (promise, resolver) = Promise::<i32>::pending();
    let s = seen.clone();
    promise.on_settle(move |r| s.borrow_mut().push(format!("{:?}", r.as_ref().ok())));
    assert!(seen.borrow().is_empty());
    resolver.resolve(4);
    assert_eq!(*seen.borrow(), ["Some(4)"]);
    let s = seen.clone();
    promise.on_settle(move |_| s.borrow_mut().push("late".into()));
    assert_eq!(seen.borrow().len(), 2);
}

#[test]
fn abort_runs_only_on_an_abortable_pending_promise() {
    rt::reset();
    let calls = Rc::new(Cell::new(0));
    let (plain, _r) = Promise::<()>::pending();
    assert!(!plain.is_abortable());
    plain.abort(None);
    let (raw, resolver) = Promise::<()>::pending();
    let c = calls.clone();
    let r = resolver.clone();
    let abortable = raw.with_abort(move |reason| {
        c.set(c.get() + 1);
        r.reject(Error::abort(reason));
    });
    assert!(abortable.is_abortable());
    abortable.abort(Some(json!("stop")));
    assert_eq!(calls.get(), 1);
    let error = abortable.peek().unwrap().unwrap_err();
    assert_eq!((error.name.as_str(), error.message.as_str()), ("AbortError", "stop"));
    abortable.abort(None);
    assert_eq!(calls.get(), 1, "a settled promise ignores abort");
    assert!(abortable.abort_handle().is_some());
}

#[test]
fn ignore_awaits_and_discards() {
    rt::reset();
    let (promise, resolver) = Promise::<()>::pending();
    promise.ignore();
    resolver.reject(Error::new("nobody cares"));
    rt::run_until_stalled();
}

#[test]
fn race_settles_one_turn_after_the_first() {
    rt::reset();
    let (a, ra) = Promise::<i32>::pending();
    let (b, rb) = Promise::<i32>::pending();
    let race = Promise::race(vec![a, b]);
    rb.resolve(2);
    assert!(!race.is_settled());
    rt::run_until_stalled();
    ra.resolve(1);
    rt::run_until_stalled();
    assert_eq!(race.peek().unwrap().unwrap(), 2);
}

#[test]
fn map_is_then() {
    rt::reset();
    let mapped = Promise::resolved(2).map(|v| Ok(v * 10));
    let failed = Promise::<i32>::rejected(Error::new("no")).map(|v| Ok(v * 10));
    rt::run_until_stalled();
    assert_eq!(mapped.peek().unwrap().unwrap(), 20);
    assert_eq!(failed.peek().unwrap().unwrap_err().message, "no");
}

#[test]
fn returned_costs_exactly_one_turn() {
    rt::reset();
    let order = log();
    let (o1, o2) = (order.clone(), order.clone());
    rt::spawn(async move {
        rt::returned(async {}).await;
        o1.borrow_mut().push("returned".into());
    });
    rt::spawn(async move {
        rt::tick().await;
        o2.borrow_mut().push("tick".into());
    });
    rt::run_until_stalled();
    assert_eq!(*order.borrow(), ["returned", "tick"]);
}

// ── the store's listener lists: JavaScript Set iteration ───────────────────────

fn change_store() -> (Rc<Store>, Rc<RefCell<Vec<Error>>>) {
    let reported: Rc<RefCell<Vec<Error>>> = Rc::default();
    let r = reported.clone();
    (Store::new(Rc::new(move |e: &Error| r.borrow_mut().push(e.clone()))), reported)
}

fn bump(store: &Store) {
    store.set_core(|c| c.order.push("x".into()), &CoreAction { kind: "@@test" });
}

#[test]
fn a_listener_added_mid_pass_runs_in_that_pass() {
    let (store, _) = change_store();
    let seen = log();
    let (s, st) = (seen.clone(), store.clone());
    let added = Rc::new(Cell::new(false));
    store.subscribe(Rc::new(move || {
        s.borrow_mut().push("first".into());
        if !added.replace(true) {
            let s2 = s.clone();
            st.subscribe(Rc::new(move || {
                s2.borrow_mut().push("added".into());
                Ok(())
            }));
        }
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["first", "added"]);
}

#[test]
fn a_listener_removed_mid_pass_is_skipped_and_its_siblings_are_not() {
    let (store, _) = change_store();
    let seen = log();
    let second: Rc<RefCell<Option<epdf_kernel::Unsubscribe>>> = Rc::default();
    let (s1, s2, s3, sec) = (seen.clone(), seen.clone(), seen.clone(), second.clone());
    store.subscribe(Rc::new(move || {
        s1.borrow_mut().push("first".into());
        if let Some(off) = sec.borrow_mut().take() {
            off.call();
        }
        Ok(())
    }));
    *second.borrow_mut() = Some(store.subscribe(Rc::new(move || {
        s2.borrow_mut().push("second".into());
        Ok(())
    })));
    store.subscribe(Rc::new(move || {
        s3.borrow_mut().push("third".into());
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["first", "third"]);
    seen.borrow_mut().clear();
    bump(&store);
    assert_eq!(*seen.borrow(), ["first", "third"]);
}

#[test]
fn unsubscribing_one_listener_leaves_every_other() {
    let (store, _) = change_store();
    let seen = log();
    let mut offs = Vec::new();
    for name in ["a", "b", "c", "d"] {
        let s = seen.clone();
        offs.push(store.subscribe(Rc::new(move || {
            s.borrow_mut().push(name.into());
            Ok(())
        })));
    }
    offs[1].call();
    offs[1].call();
    bump(&store);
    assert_eq!(*seen.borrow(), ["a", "c", "d"]);
    offs[3].call();
    bump(&store);
    assert_eq!(seen.borrow()[3..], ["a", "c"]);
}

#[test]
fn a_listener_that_fails_is_reported_and_the_pass_continues() {
    let (store, reported) = change_store();
    let seen = log();
    store.subscribe(Rc::new(|| Err(Error::new("listener broke"))));
    let s = seen.clone();
    store.subscribe(Rc::new(move || {
        s.borrow_mut().push("after".into());
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["after"]);
    assert_eq!(reported.borrow()[0].message, "listener broke");
    store.isolate(|| Err(Error::new("isolated")));
    store.report(&Error::new("direct"));
    let messages: Vec<String> = reported.borrow().iter().map(|e| e.message.clone()).collect();
    assert_eq!(messages, ["listener broke", "isolated", "direct"]);
}

#[test]
fn destroy_mid_pass_stops_the_rest() {
    let (store, _) = change_store();
    let seen = log();
    let (s1, s2, st) = (seen.clone(), seen.clone(), store.clone());
    store.subscribe(Rc::new(move || {
        s1.borrow_mut().push("first".into());
        st.destroy();
        Ok(())
    }));
    store.subscribe(Rc::new(move || {
        s2.borrow_mut().push("second".into());
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["first"]);
    bump(&store);
    assert_eq!(*seen.borrow(), ["first"]);
}

#[test]
fn slices_register_read_and_remove() {
    let (store, _) = change_store();
    let initial: Rc<dyn std::any::Any> = Rc::new(json!({ "n": 1 }));
    store.register_slice(
        "k",
        Rc::new(|_, _| None),
        initial,
        Rc::new(|s: &dyn std::any::Any| s.downcast_ref::<serde_json::Value>().unwrap().clone()),
    );
    assert!(store.has_slice("k"));
    assert!(!store.has_slice("other"));
    assert_eq!(store.slice_json("k"), Some(json!({ "n": 1 })));
    assert_eq!(store.slice_json("other"), None);
    store.remove_slice("k");
    assert!(!store.has_slice("k"));
}

// ── signals and scopes ────────────────────────────────────────────────────────

#[test]
fn a_signal_aborts_once_and_forgets_removed_listeners() {
    let signal = AbortSignal::new();
    let seen = log();
    let (s1, s2, s3) = (seen.clone(), seen.clone(), seen.clone());
    signal.on_abort(Rc::new(move |e| s1.borrow_mut().push(format!("a:{}", e.message))));
    let b = signal.on_abort(Rc::new(move |_| s2.borrow_mut().push("b".into())));
    signal.on_abort(Rc::new(move |_| s3.borrow_mut().push("c".into())));
    signal.remove(b);
    assert!(!signal.aborted());
    assert!(signal.reason().is_none());
    signal.abort(Error::new("why"));
    signal.abort(Error::new("again"));
    assert_eq!(*seen.borrow(), ["a:why", "c"]);
    assert_eq!(signal.reason().unwrap().message, "why");
}

#[test]
fn a_scope_reports_disposal_and_runs_late_teardowns() {
    rt::reset();
    let scope = Scope::new(Rc::new(|_| {}));
    let seen = log();
    let s = seen.clone();
    scope.defer_fn(move || s.borrow_mut().push("early".into()));
    assert!(!scope.disposed());
    let disposal = scope.dispose();
    assert!(scope.disposed());
    assert!(disposal.same(&scope.dispose()));
    let s = seen.clone();
    scope.defer_fn(move || s.borrow_mut().push("late".into()));
    rt::run_until_stalled();
    assert_eq!(*seen.borrow(), ["early", "late"]);
}

// ── tokens, errors, bytes ─────────────────────────────────────────────────────

#[test]
fn tokens_are_identities_not_names() {
    let a = AnyToken::new("same", None);
    let b = AnyToken::new("same", Some("fix".into()));
    assert_ne!(a, b);
    assert_eq!(a, a.clone());
    let set: HashSet<AnyToken> = [a.clone(), a.clone(), b.clone()].into_iter().collect();
    assert_eq!(set.len(), 2);
    assert_eq!(format!("{a:?}"), "Token(same)");
    assert_eq!(b.hint(), Some("fix"));
    let typed = Token::<u8>::with_hint("typed", "remedy");
    assert_eq!(typed.name(), "typed");
    assert_eq!(typed.any().hint(), Some("remedy"));
    assert_eq!(format!("{typed:?}"), "Token(typed)");
    assert_eq!(Token::<u8>::from_any(a.clone()).any(), &a);
}

#[test]
fn error_shapes_match_their_javascript_classes() {
    let engine = Error::engine("NotFound", "no page 3");
    assert_eq!(engine.code(), Some("NotFound"));
    assert_eq!(engine.member("code"), Some(&json!("NotFound")));
    assert_eq!(engine.member("absent"), None);
    assert_eq!(engine.to_string(), "EngineError: no page 3");
    assert_eq!(Error::new("x").code(), None);

    let denied = Error::permission_denied("doc.render", Some("local"));
    assert_eq!(denied.message, "permission denied (local): doc.render");
    assert_eq!(
        Error::permission_denied("doc.render", None).message,
        "permission denied: doc.render"
    );

    // `AbortError`'s message rules (`AbortError.ts`).
    assert_eq!(Error::abort(None).message, "aborted");
    assert_eq!(Error::abort(Some(json!(null))).message, "aborted");
    assert_eq!(Error::abort(Some(json!("stop"))).message, "stop");
    assert_eq!(Error::abort(Some(Error::new("inner").to_canonical())).message, "inner");
    assert_eq!(Error::abort(Some(Error::new("").to_canonical())).message, "aborted");
    assert_eq!(Error::abort(Some(json!({ "k": 1 }))).message, "{\"k\":1}");
    assert!(Error::abort(None).member("reason").is_none());
    assert!(Error::abort(None).is_abort());

    let spec = json!({ "name": "PermissionDenied", "message": "m", "required": "r" });
    let parsed = Error::from_spec(&spec);
    assert_eq!(parsed.to_canonical(), json!({ "$error": spec }));
    assert_eq!(Error::from_spec(&parsed.to_canonical()), parsed);
    assert_eq!(Error::from_spec(&json!("not an object")).name, "Error");
}

#[test]
fn bytes_travel_as_base64() {
    let bytes = Bytes(vec![0, 1, 254, 255]);
    let value = serde_json::to_value(&bytes).unwrap();
    assert_eq!(value, json!({ "$bytes": "AAH+/w==" }));
    assert_eq!(serde_json::from_value::<Bytes>(value).unwrap(), bytes);
    assert!(Bytes::from_json(&json!({ "$bytes": "!!" })).is_none());
    assert!(serde_json::from_value::<Bytes>(json!("plain")).is_err());
    assert_eq!(Bytes::from_json(&json!({ "$bytes": "" })), Some(Bytes(Vec::new())));
}
