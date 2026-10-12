//! The pieces under the kernel that its vectors reach only indirectly: the
//! runtime's clock and promises, the store's listener lists, signals,
//! scopes, tokens, errors and bytes. Each test names the JavaScript
//! behaviour it pins, since there is no TypeScript file to generate these
//! from: they are the semantics of the platform the TypeScript runs on.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::Action;
use epdf_kernel::action::CoreAction;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, EngineCall};
use epdf_kernel::error::Error;
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::guarded::{GuardedLifetime, guard_handle};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::scope::Scope;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Store;
use epdf_kernel::token::{AnyToken, Token};
use serde_json::{Value, json};

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
fn a_listener_that_removes_itself_mid_pass_keeps_its_next_sibling() {
    // `Set.prototype.delete` of the entry being visited does not shift the
    // ones after it.
    let (store, _) = change_store();
    let seen = log();
    let own: Rc<RefCell<Option<epdf_kernel::Unsubscribe>>> = Rc::default();
    let (s1, s2, s3, me) = (seen.clone(), seen.clone(), seen.clone(), own.clone());
    *own.borrow_mut() = Some(store.subscribe(Rc::new(move || {
        s1.borrow_mut().push("first".into());
        if let Some(off) = me.borrow_mut().take() {
            off.call();
        }
        Ok(())
    })));
    store.subscribe(Rc::new(move || {
        s2.borrow_mut().push("second".into());
        Ok(())
    }));
    store.subscribe(Rc::new(move || {
        s3.borrow_mut().push("third".into());
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["first", "second", "third"]);
}

#[test]
fn a_listener_subscribed_after_destroy_mid_pass_runs_in_that_pass() {
    // `Set.prototype.clear` empties the entries in place, so `forEach` still
    // reaches one added after it.
    let (store, _) = change_store();
    let seen = log();
    let (s, st) = (seen.clone(), store.clone());
    store.subscribe(Rc::new(move || {
        s.borrow_mut().push("first".into());
        st.destroy();
        let s2 = s.clone();
        st.subscribe(Rc::new(move || {
            s2.borrow_mut().push("late".into());
            Ok(())
        }));
        Ok(())
    }));
    bump(&store);
    assert_eq!(*seen.borrow(), ["first", "late"]);
}

#[test]
fn a_nested_action_pass_leaves_the_outer_pass_whole() {
    // A listener that removes itself and dispatches: the nested `forEach`
    // ends while the outer one is still walking, and the outer one must
    // still reach every sibling.
    let (store, _) = change_store();
    let seen = log();
    let own: Rc<RefCell<Option<epdf_kernel::Unsubscribe>>> = Rc::default();
    let (s1, me, st) = (seen.clone(), own.clone(), store.clone());
    *own.borrow_mut() = Some(store.subscribe_action(Rc::new(move |_: &dyn Action| {
        s1.borrow_mut().push("a".into());
        if let Some(off) = me.borrow_mut().take() {
            off.call();
            st.set_core(|_| {}, &CoreAction { kind: "@@nested" });
        }
        Ok(())
    })));
    for name in ["b", "c"] {
        let s = seen.clone();
        store.subscribe_action(Rc::new(move |action: &dyn Action| {
            s.borrow_mut().push(format!("{name}:{}", action.action_type()));
            Ok(())
        }));
    }
    bump(&store);
    assert_eq!(*seen.borrow(), ["a", "b:@@nested", "c:@@nested", "b:@@test", "c:@@test"]);
}

/// An action that counts how often its `type` is read.
struct Counted {
    reads: Rc<Cell<u32>>,
}

impl Action for Counted {
    fn action_type(&self) -> String {
        self.reads.set(self.reads.get() + 1);
        "@@counted".into()
    }
    fn to_json(&self) -> Value {
        json!({ "type": "@@counted" })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn typed_action_listeners_read_the_type_once_per_pass() {
    let (store, _) = change_store();
    let seen = log();
    for name in ["a", "b", "c"] {
        let s = seen.clone();
        store.subscribe_action_of(
            "@@counted",
            Rc::new(move |_: &dyn Action| {
                s.borrow_mut().push(name.into());
                Ok(())
            }),
        );
    }
    let s = seen.clone();
    store.subscribe_action_of(
        "@@other",
        Rc::new(move |_: &dyn Action| {
            s.borrow_mut().push("other".into());
            Ok(())
        }),
    );
    let reads = Rc::new(Cell::new(0));
    store.set_core(|_| {}, &Counted { reads: reads.clone() });
    assert_eq!(*seen.borrow(), ["a", "b", "c"]);
    assert_eq!(reads.get(), 1);
}

#[test]
fn an_action_that_serde_json_refuses_has_no_type_and_does_not_panic() {
    #[derive(serde::Serialize)]
    struct Keyed {
        r#type: &'static str,
        by_pair: std::collections::HashMap<(u8, u8), u8>,
    }
    let action = Keyed { r#type: "@@keyed", by_pair: [((1, 2), 3)].into_iter().collect() };
    assert_eq!(action.action_type(), "");
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
    assert!(store.get_slice("k").is_some());
    assert!(store.get_slice("other").is_none());
    store.remove_slice("k");
    assert!(!store.has_slice("k"));
}

#[test]
fn dispatch_to_commits_through_the_current_lease() {
    let (store, _) = change_store();
    let to_json: epdf_kernel::store::SliceJson =
        Rc::new(|s: &dyn std::any::Any| s.downcast_ref::<Value>().unwrap().clone());
    let lease = store.lease(
        "k",
        Rc::new(|_, action| Some(Rc::new(json!({ "last": action.action_type() })) as _)),
        Rc::new(json!({})),
        to_json,
        None,
    );
    assert_eq!(lease.key(), "k");
    assert_eq!(lease.instance_id(), "k#1");
    assert!(lease.live());
    store.dispatch_to("k", &CoreAction { kind: "ping" });
    assert_eq!(store.slice_json("k"), Some(json!({ "last": "ping" })));
    store.dispatch_to("other", &CoreAction { kind: "lost" });
    lease.revoke();
    assert!(!lease.live());
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
fn signals_compare_by_controller() {
    let signal = AbortSignal::new();
    assert!(signal.same(&signal.clone()));
    assert!(!signal.same(&AbortSignal::new()));
}

#[test]
fn an_event_hook_counts_its_listeners_until_disposed() {
    let hook = EventHookSource::<Value>::new(None);
    assert_eq!(hook.listener_count(), 0);
    hook.on(|_| Ok(()));
    hook.on(|_| Ok(()));
    assert_eq!(hook.listener_count(), 2);
    hook.dispose();
    assert_eq!(hook.listener_count(), 0);
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
    // `name` and `message` are the error's own fields, never members.
    let read = Error::from_spec(&json!({ "name": "E", "message": "m", "k": 1 }));
    assert_eq!((read.member("name"), read.member("message")), (None, None));
    assert_eq!(read.member("k"), Some(&json!(1)));
}

#[test]
fn an_action_downcasts_to_what_it_is() {
    let action: &dyn Action = &CoreAction { kind: "x" };
    assert_eq!(action.downcast::<CoreAction>().map(|a| a.kind), Some("x"));
    assert!(action.downcast::<u8>().is_none());
}

#[test]
fn a_page_ref_prints_as_an_object_number() {
    let value = PageRef::new(7).to_json();
    assert_eq!(value, json!({ "kind": "objectNumber", "pageObjectNumber": 7 }));
    assert_eq!(PageRef::from_json(&value), Some(PageRef::new(7)));
}

// ── the guarded handle's pass-throughs ─────────────────────────────────────────

/// A handle with only the members the guard passes straight through.
struct Passthrough;

impl DocumentHandle for Passthrough {
    fn id(&self) -> String {
        "p".into()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        unimplemented!()
    }
    fn has(&self, member: &str) -> bool {
        member == "render"
    }
    fn call(&self, _: &str, _: Option<u32>, _: Vec<Value>) -> EngineCall<Value> {
        unimplemented!()
    }
    fn page_index(&self, _: u32) -> Result<u32, Error> {
        unimplemented!()
    }
    fn subscribe(&self, _: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
        epdf_kernel::Unsubscribe::noop()
    }
    fn last_server_id(&self) -> Option<String> {
        Some("server-1".into())
    }
}

#[test]
fn the_guard_passes_feature_checks_and_the_server_id_through() {
    // The TypeScript guard is a Proxy, so every member it does not wrap
    // reads through untouched; the Rust guard forwards each one by hand.
    let lifetime = GuardedLifetime { signal: AbortSignal::new(), instance_id: "p#1".into() };
    let guarded = guard_handle(Rc::new(Passthrough), lifetime, "probe");
    assert!(guarded.has("render"));
    assert!(!guarded.has("fonts"));
    assert_eq!(guarded.last_server_id().as_deref(), Some("server-1"));
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

#[test]
fn a_settled_promise_drops_its_abort_and_what_it_captured() {
    // A cached promise used to keep its abort function, and with it the
    // bridge call it would cancel, for as long as the cache kept the promise.
    let held = Rc::new(());
    let probe = Rc::downgrade(&held);
    let (promise, resolver) = Promise::<u32>::pending();
    let promise = promise.with_abort(move |_| {
        let _ = &held;
    });
    assert_eq!(probe.strong_count(), 1);
    resolver.resolve(7);
    assert_eq!(probe.strong_count(), 0, "settling released the abort's captures");
    assert!(promise.is_abortable(), "it is still an AbortablePromise");
    assert!(promise.abort_handle().is_some());
    promise.abort(None);
    assert_eq!(promise.peek().map(|r| r.ok()), Some(Some(7)));
}

// ── abandoned promises: what a collector would reclaim ────────────────────────

/// A JavaScript promise whose resolve and reject functions are unreachable
/// never settles, and once nothing else holds it, neither it nor the async
/// function suspended on it is kept. The Rust task is dropped unrun.
#[test]
fn a_task_awaiting_a_promise_with_no_resolver_left_is_dropped_unrun() {
    rt::reset();
    let held = Rc::new(());
    let resumed = Rc::new(Cell::new(false));
    let (promise, resolver) = Promise::<()>::pending();
    let (h, r) = (held.clone(), resumed.clone());
    rt::spawn(async move {
        let _held = h;
        let _ = promise.await;
        r.set(true);
    });
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&held), 2, "the waiting task holds it");

    drop(resolver);
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&held), 1, "the task went, with what it held");
    assert!(!resumed.get(), "and the code after the await never ran");
}

#[test]
fn a_remaining_resolver_clone_keeps_the_waiter() {
    rt::reset();
    let resumed = Rc::new(Cell::new(false));
    let (promise, resolver) = Promise::<u32>::pending();
    let kept = resolver.clone();
    let r = resumed.clone();
    rt::spawn(async move {
        assert_eq!(promise.await.unwrap(), 7);
        r.set(true);
    });
    drop(resolver);
    rt::run_until_stalled();
    kept.resolve(7);
    rt::run_until_stalled();
    assert!(resumed.get());
}

/// The start task waiting on an abandoned plugin init holds the resolver of
/// `start()`'s promise, and a destroy waits on that: dropping the first
/// frees the whole chain, as a collector would.
#[test]
fn abandonment_cascades_through_the_resolvers_a_dropped_task_held() {
    rt::reset();
    let held = Rc::new(());
    let (init, init_resolver) = Promise::<()>::pending();
    let h = held.clone();
    let start = Promise::spawn(async move {
        let _held = h;
        init.await
    });
    let observed = Rc::new(Cell::new(false));
    let o = observed.clone();
    start.on_settle(move |_| o.set(true));
    let joined = start.map(Ok);
    let h = held.clone();
    let destroy = Promise::spawn(async move {
        let _held = h;
        joined.await
    });
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&held), 3);

    drop(init_resolver);
    rt::run_until_stalled();
    assert_eq!(Rc::strong_count(&held), 1, "both waiting tasks were dropped");
    assert!(!observed.get(), "an observer of a promise that never settles never runs");
    assert!(!start.is_settled() && !destroy.is_settled());
}

#[test]
fn awaiting_an_already_abandoned_promise_drops_the_new_waiter() {
    rt::reset();
    let held = Rc::new(());
    let (promise, resolver) = Promise::<()>::pending();
    drop(resolver);
    let h = held.clone();
    rt::spawn(async move {
        let _held = h;
        let _ = promise.await;
        unreachable!("an abandoned promise never resumes its waiter");
    });
    assert_eq!(Rc::strong_count(&held), 1, "dropped during its first, inline poll");
}

// ── AbortablePromise is a subclass; a promise with an abort member is not ─────

/// The turn (counted by a ticking task spawned beside it) on which `wait`
/// resumes. JavaScript: `await settledPlain` resumes on turn 1, `await
/// settledAbortable` on turn 3 (`PromiseResolve` wraps the subclass, a
/// resolve-thenable job calls its `then`, the wrapper settles, and its
/// reaction resumes the function), and `.then` on either reacts on turn 1.
fn resume_turn(wait: impl std::future::Future<Output = ()> + 'static) -> usize {
    rt::reset();
    let turn = Rc::new(Cell::new(0));
    let resumed = Rc::new(Cell::new(None));
    let r = resumed.clone();
    let t = turn.clone();
    rt::spawn(async move {
        wait.await;
        r.set(Some(t.get()));
    });
    let t = turn.clone();
    rt::spawn(async move {
        // Turn n starts when this sets n; the waiter, queued ahead of it in
        // every turn, reads n while turn n runs.
        for _ in 0..10 {
            t.set(t.get() + 1);
            rt::tick().await;
        }
    });
    rt::run_until_stalled();
    resumed.get().expect("resumed")
}

fn abortable_settled() -> Promise<u32> {
    Promise::resolved(1).with_abort(|_| {})
}

#[test]
fn awaiting_an_abortable_promise_adopts_it_in_three_turns() {
    assert_eq!(
        resume_turn(async {
            let _ = Promise::resolved(1u32).await;
        }),
        1
    );
    assert_eq!(
        resume_turn(async {
            let _ = abortable_settled().await;
        }),
        3
    );
    assert_eq!(
        resume_turn(async {
            let _ = abortable_settled().reaction().await;
        }),
        1,
        "a `.then` reaction does not adopt"
    );
    // Pending at the await, settled on turn 2: the job registered on turn 1,
    // the reaction runs on 3 and the wrapper's on 4.
    assert_eq!(
        resume_turn(async {
            let (p, r) = Promise::<u32>::pending();
            let p = p.with_abort(|_| {});
            rt::spawn(async move {
                rt::tick().await;
                rt::tick().await;
                r.resolve(1);
            });
            let _ = p.await;
        }),
        4
    );
}

#[test]
fn a_promise_with_an_abort_member_is_not_a_subclass() {
    rt::reset();
    let aborted = Rc::new(Cell::new(false));
    let a = aborted.clone();
    let (p, _r) = Promise::<u32>::pending();
    let member = p.with_abort_member(move |_| a.set(true));
    assert!(!member.is_abortable() && member.has_abort());
    assert!(member.abort_handle().is_some());
    member.abort(None);
    assert!(aborted.get());

    // `Promise.race` hands a plain promise straight to `.then`: one turn.
    let race_turn = |input: Promise<u32>| {
        resume_turn(async move {
            let _ = Promise::race(vec![input]).reaction().await;
        })
    };
    assert_eq!(race_turn(Promise::resolved(1).with_abort_member(|_| {})), 2);
    assert_eq!(race_turn(abortable_settled()), 4, "a subclass is adopted first");
}

/// A handle whose every call answers an already-settled `AbortablePromise`.
struct Answers;

impl DocumentHandle for Answers {
    fn id(&self) -> String {
        "a".into()
    }
    fn capabilities(&self) -> Value {
        json!({})
    }
    fn security(&self) -> Rc<dyn DocumentSecurity> {
        unimplemented!()
    }
    fn has(&self, _: &str) -> bool {
        true
    }
    fn call(&self, _: &str, _: Option<u32>, _: Vec<Value>) -> EngineCall<Value> {
        Promise::resolved(json!(1)).with_abort(|_| {})
    }
    fn page_index(&self, _: u32) -> Result<u32, Error> {
        Ok(0)
    }
    fn subscribe(&self, _: Rc<dyn Fn(&Value)>) -> epdf_kernel::Unsubscribe {
        epdf_kernel::Unsubscribe::noop()
    }
}

/// `guardPromise` builds `new Promise(...)` and defines `abort` on it, so
/// the guarded call is a plain promise with an abort, and it settles after
/// the guard's own `Promise.resolve(pending).then(...)` adopts the engine's
/// subclass: turn 3. Awaiting it is then one more turn, not three.
#[test]
fn a_guarded_call_is_a_plain_promise_with_an_abort() {
    let lifetime = GuardedLifetime { signal: AbortSignal::new(), instance_id: "a#1".into() };
    let guarded = guard_handle(Rc::new(Answers), lifetime.clone(), "probe");
    let call = guarded.call("metadata.read", None, Vec::new());
    assert!(!call.is_abortable());
    assert!(call.has_abort());
    assert!(guarded.call_answer("metadata.read", None, Vec::new()).has_abort());
    assert_eq!(
        resume_turn(async move {
            let guarded = guard_handle(Rc::new(Answers), lifetime, "probe");
            let _ = guarded.call("metadata.read", None, Vec::new()).await;
        }),
        4
    );
}
