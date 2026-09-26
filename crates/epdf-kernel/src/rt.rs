//! The runtime: a single-threaded executor, a virtual clock, and promises
//! shaped like JavaScript's.
//!
//! The TypeScript this crate ports is written against the JavaScript event
//! loop: `async` functions that run synchronously up to their first `await`,
//! promises that several parties await, and timers. Three rules reproduce it
//! closely enough for the vectors to compare traces:
//!
//! 1. [`Promise::spawn`] polls its future once, inline, before returning, so
//!    the synchronous prefix of a ported `async` function runs at call time,
//!    as it does in JavaScript.
//! 2. Awaiting a promise always yields at least once. An `await` in
//!    JavaScript is at least one turn of the microtask queue even when the
//!    promise has settled, and a continuation that ran inline instead would
//!    reorder two chains the TypeScript interleaves.
//! 3. The ready queue is FIFO, and a woken task runs in wake order, as
//!    microtask reactions do.
//!
//! The clock follows `@sinonjs/fake-timers` 15.4.0, because that is what the
//! oracle ran under: delays truncate to whole milliseconds, a zero delay
//! scheduled while the clock is advancing becomes one, timers fire in
//! (deadline, insertion) order, and the ready queue drains after every timer.
//!
//! Everything is thread-local. A kernel and its plugins live on one thread,
//! which is the confinement `multiplatform-plan.md` section 4 already asks of
//! the engine; a host drives the runtime from that thread's run loop with
//! [`run_until_stalled`] and [`advance`].

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::error::Error;

type Task = Pin<Box<dyn Future<Output = ()>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

struct Timer {
    callback: Rc<RefCell<Box<dyn FnMut()>>>,
    interval: Option<f64>,
    call_at: f64,
}

#[derive(Default)]
struct Runtime {
    tasks: RefCell<HashMap<usize, Task>>,
    ready: RefCell<VecDeque<usize>>,
    queued: RefCell<HashSet<usize>>,
    next_task: Cell<usize>,
    now: Cell<f64>,
    during_tick: Cell<bool>,
    /// Keyed (deadline bits, order): `f64` deadlines are never negative, so
    /// their bit patterns sort as the numbers do.
    timers: RefCell<BTreeMap<(u64, u64), TimerId>>,
    timer_data: RefCell<HashMap<TimerId, (Timer, (u64, u64))>>,
    next_timer: Cell<u64>,
    next_order: Cell<u64>,
}

thread_local! {
    static RT: Runtime = Runtime::default();
}

// ── wakers: a task id, pushed onto this thread's ready queue ──────────────────

fn wake_id(id: usize) {
    RT.with(|rt| {
        if rt.queued.borrow_mut().insert(id) {
            rt.ready.borrow_mut().push_back(id);
        }
    });
}

unsafe fn waker_clone(data: *const ()) -> RawWaker {
    RawWaker::new(data, &VTABLE)
}
unsafe fn waker_wake(data: *const ()) {
    wake_id(data as usize);
}
unsafe fn waker_wake_by_ref(data: *const ()) {
    wake_id(data as usize);
}
unsafe fn waker_drop(_: *const ()) {}

static VTABLE: RawWakerVTable =
    RawWakerVTable::new(waker_clone, waker_wake, waker_wake_by_ref, waker_drop);

fn waker_for(id: usize) -> Waker {
    // SAFETY: the data pointer is a task id, not a pointer; the vtable never
    // dereferences it. Waking from another thread pushes onto that thread's
    // queue, where no such task exists, and is a no-op there.
    unsafe { Waker::from_raw(RawWaker::new(id as *const (), &VTABLE)) }
}

fn poll_task(id: usize, mut task: Task) {
    let waker = waker_for(id);
    let mut cx = Context::from_waker(&waker);
    if task.as_mut().poll(&mut cx).is_pending() {
        RT.with(|rt| rt.tasks.borrow_mut().insert(id, task));
    }
}

/// Run a future to completion on this thread. The first poll happens now,
/// inline; later ones when it is woken and the queue reaches it.
pub fn spawn(future: impl Future<Output = ()> + 'static) {
    let id = RT.with(|rt| {
        let id = rt.next_task.get();
        rt.next_task.set(id + 1);
        id
    });
    poll_task(id, Box::pin(future));
}

/// Poll ready tasks until none is ready. Timers do not fire: that is
/// [`advance`]'s job.
pub fn run_until_stalled() {
    loop {
        let next = RT.with(|rt| {
            let id = rt.ready.borrow_mut().pop_front()?;
            rt.queued.borrow_mut().remove(&id);
            Some((id, rt.tasks.borrow_mut().remove(&id)))
        });
        match next {
            None => return,
            Some((_, None)) => continue, // finished, or woken while running
            Some((id, Some(task))) => poll_task(id, task),
        }
    }
}

/// Milliseconds on the virtual clock: `Date.now()` and `performance.now()`.
pub fn now() -> f64 {
    RT.with(|rt| rt.now.get())
}

fn schedule(delay: f64, interval: Option<f64>, callback: Box<dyn FnMut()>) -> TimerId {
    RT.with(|rt| {
        let delay = if delay.is_finite() { delay } else { 0.0 };
        let delay = if delay > 2_147_483_647.0 { 1.0 } else { delay.max(0.0) };
        // `parseInt(String(delay)) || (duringTick ? 1 : 0)`
        let whole = delay.trunc();
        let offset = if whole != 0.0 {
            whole
        } else if rt.during_tick.get() {
            1.0
        } else {
            0.0
        };
        let call_at = rt.now.get() + offset;
        let id = TimerId(rt.next_timer.get());
        rt.next_timer.set(id.0 + 1);
        let order = rt.next_order.get();
        rt.next_order.set(order + 1);
        let key = (call_at.to_bits(), order);
        rt.timers.borrow_mut().insert(key, id);
        let timer = Timer { callback: Rc::new(RefCell::new(callback)), interval, call_at };
        rt.timer_data.borrow_mut().insert(id, (timer, key));
        id
    })
}

/// `setTimeout`.
pub fn set_timeout(delay: f64, callback: impl FnOnce() + 'static) -> TimerId {
    let mut callback = Some(callback);
    schedule(
        delay,
        None,
        Box::new(move || {
            if let Some(callback) = callback.take() {
                callback();
            }
        }),
    )
}

/// `setInterval`.
pub fn set_interval(delay: f64, callback: impl FnMut() + 'static) -> TimerId {
    schedule(delay, Some(delay), Box::new(callback))
}

/// `clearTimeout` and `clearInterval`. Clearing a timer that fired or never
/// existed does nothing.
pub fn clear_timer(id: TimerId) {
    RT.with(|rt| {
        if let Some((_, key)) = rt.timer_data.borrow_mut().remove(&id) {
            rt.timers.borrow_mut().remove(&key);
        }
    });
}

/// Advance the clock by `ms`, firing every timer due on the way in deadline
/// order and draining the ready queue after each, then land exactly `ms` later.
pub fn advance(ms: f64) {
    let tick_to = RT.with(|rt| {
        rt.during_tick.set(true);
        rt.now.get() + ms.floor()
    });
    loop {
        let due = RT.with(|rt| {
            let now = rt.now.get();
            let (&key, &id) = rt.timers.borrow().iter().next()?;
            let call_at = f64::from_bits(key.0);
            if call_at < now || call_at > tick_to {
                return None;
            }
            rt.timers.borrow_mut().remove(&key);
            rt.now.set(call_at);
            let mut data = rt.timer_data.borrow_mut();
            let callback = {
                let (timer, _) = data.get(&id)?;
                timer.callback.clone()
            };
            let rearm = data.get(&id).and_then(|(t, _)| t.interval.map(|i| (i, t.call_at)));
            match rearm {
                Some((interval, call_at)) => {
                    let order = rt.next_order.get();
                    rt.next_order.set(order + 1);
                    let next_at = call_at + interval;
                    let key = (next_at.to_bits(), order);
                    rt.timers.borrow_mut().insert(key, id);
                    if let Some(entry) = data.get_mut(&id) {
                        entry.0.call_at = next_at;
                        entry.1 = key;
                    }
                }
                None => {
                    data.remove(&id);
                }
            }
            Some(callback)
        });
        let Some(callback) = due else { break };
        (callback.borrow_mut())();
        run_until_stalled();
    }
    RT.with(|rt| {
        rt.during_tick.set(false);
        rt.now.set(tick_to);
    });
    run_until_stalled();
}

/// How many timers are scheduled. For hosts deciding whether to wake up.
pub fn pending_timers() -> usize {
    RT.with(|rt| rt.timers.borrow().len())
}

/// Reset this thread's runtime: drop every task and timer, and put the clock
/// back to zero. For test harnesses running one case after another.
pub fn reset() {
    let (tasks, timers) = RT.with(|rt| {
        rt.ready.borrow_mut().clear();
        rt.queued.borrow_mut().clear();
        rt.timers.borrow_mut().clear();
        rt.now.set(0.0);
        rt.during_tick.set(false);
        (
            std::mem::take(&mut *rt.tasks.borrow_mut()),
            std::mem::take(&mut *rt.timer_data.borrow_mut()),
        )
    });
    // Dropped outside the borrow: a task's destructor may touch the runtime.
    drop(tasks);
    drop(timers);
}

// ── yielding ──────────────────────────────────────────────────────────────────

/// One turn of the queue: `await undefined`.
pub fn tick() -> Tick {
    Tick(false)
}

pub struct Tick(bool);

impl Future for Tick {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// `await asyncFn()`: run a ported `async` function's body inline, then take
/// the turn a JavaScript caller spends resuming after that function's
/// promise settles. Awaiting a nested Rust future directly would skip it,
/// and put this chain one turn ahead of a sibling the TypeScript interleaves
/// with it.
pub async fn returned<F: Future>(body: F) -> F::Output {
    let out = body.await;
    tick().await;
    out
}

/// A future that resolves after `ms` on the virtual clock.
pub fn sleep(ms: f64) -> Promise<()> {
    let (promise, resolver) = Promise::pending();
    set_timeout(ms, move || resolver.resolve(()));
    promise
}

// ── promises ──────────────────────────────────────────────────────────────────

enum State<T> {
    Pending(Vec<Waker>),
    Settled(Result<T, Error>),
}

struct Inner<T> {
    state: RefCell<State<T>>,
    on_settle: RefCell<Vec<Box<dyn FnOnce(&Result<T, Error>)>>>,
    abort: RefCell<Option<Rc<dyn Fn(Option<serde_json::Value>)>>>,
}

/// A JavaScript promise: settles once, any number of parties await it, and
/// awaiting a settled one still costs a turn.
pub struct Promise<T>(Rc<Inner<T>>);

impl<T> Clone for Promise<T> {
    fn clone(&self) -> Self {
        Promise(self.0.clone())
    }
}

/// The settling half of [`Promise::pending`]. Settling twice keeps the first.
pub struct Resolver<T>(Promise<T>);

impl<T> Clone for Resolver<T> {
    fn clone(&self) -> Self {
        Resolver(self.0.clone())
    }
}

impl<T: Clone + 'static> Resolver<T> {
    pub fn resolve(&self, value: T) {
        self.0.settle(Ok(value));
    }
    pub fn reject(&self, error: Error) {
        self.0.settle(Err(error));
    }
    pub fn settle(&self, result: Result<T, Error>) {
        self.0.settle(result);
    }
    pub fn promise(&self) -> Promise<T> {
        self.0.clone()
    }
}

impl<T: Clone + 'static> Promise<T> {
    fn new_pending() -> Self {
        Promise(Rc::new(Inner {
            state: RefCell::new(State::Pending(Vec::new())),
            on_settle: RefCell::new(Vec::new()),
            abort: RefCell::new(None),
        }))
    }

    pub fn pending() -> (Self, Resolver<T>) {
        let promise = Self::new_pending();
        (promise.clone(), Resolver(promise))
    }

    pub fn resolved(value: T) -> Self {
        let promise = Self::new_pending();
        promise.settle(Ok(value));
        promise
    }

    pub fn rejected(error: Error) -> Self {
        let promise = Self::new_pending();
        promise.settle(Err(error));
        promise
    }

    /// An `async` function call: runs `future` now up to its first pending
    /// await, and settles with what it returns.
    pub fn spawn(future: impl Future<Output = Result<T, Error>> + 'static) -> Self {
        let (promise, resolver) = Self::pending();
        spawn(async move { resolver.settle(future.await) });
        promise
    }

    fn settle(&self, result: Result<T, Error>) {
        let wakers = {
            let mut state = self.0.state.borrow_mut();
            if matches!(*state, State::Settled(_)) {
                return;
            }
            match std::mem::replace(&mut *state, State::Settled(result.clone())) {
                State::Pending(wakers) => wakers,
                State::Settled(_) => unreachable!(),
            }
        };
        let observers = std::mem::take(&mut *self.0.on_settle.borrow_mut());
        for observer in observers {
            observer(&result);
        }
        for waker in wakers {
            waker.wake();
        }
    }

    /// The settled result, if any, without waiting.
    pub fn peek(&self) -> Option<Result<T, Error>> {
        match &*self.0.state.borrow() {
            State::Pending(_) => None,
            State::Settled(result) => Some(result.clone()),
        }
    }

    /// `a === b`.
    pub fn same(&self, other: &Promise<T>) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn is_settled(&self) -> bool {
        matches!(*self.0.state.borrow(), State::Settled(_))
    }

    /// Observe settlement synchronously, at the moment it happens (or now,
    /// if it already has). For harnesses; plugin code awaits.
    pub fn on_settle(&self, observer: impl FnOnce(&Result<T, Error>) + 'static) {
        let settled = self.peek();
        match settled {
            Some(result) => observer(&result),
            None => self.0.on_settle.borrow_mut().push(Box::new(observer)),
        }
    }

    /// Make this promise an `AbortablePromise`: `abort` runs the given
    /// function, which is expected to reject it.
    pub fn with_abort(self, abort: impl Fn(Option<serde_json::Value>) + 'static) -> Self {
        *self.0.abort.borrow_mut() = Some(Rc::new(abort));
        self
    }

    pub fn is_abortable(&self) -> bool {
        self.0.abort.borrow().is_some()
    }

    /// `AbortablePromise.abort(reason)`. A promise without an abort, or one
    /// already settled, ignores it.
    pub fn abort(&self, reason: Option<serde_json::Value>) {
        if self.is_settled() {
            return;
        }
        let abort = self.0.abort.borrow().clone();
        if let Some(abort) = abort {
            abort(reason);
        }
    }

    /// Carry this promise's abort over to a derived one.
    pub fn abort_handle(&self) -> Option<Rc<dyn Fn(Option<serde_json::Value>)>> {
        self.0.abort.borrow().clone()
    }

    /// `.then(f)` for a synchronous `f`: a new promise settled one turn
    /// after this one, with `f` applied to a success.
    pub fn map<U: Clone + 'static>(
        &self,
        f: impl FnOnce(T) -> Result<U, Error> + 'static,
    ) -> Promise<U> {
        let source = self.clone();
        Promise::spawn(async move { f(source.await?) })
    }

    /// `promise.catch(() => {})`: await and discard.
    pub fn ignore(&self) {
        let source = self.clone();
        spawn(async move {
            let _ = source.await;
        });
    }

    /// `Promise.race`: settles as the first of `promises` settles.
    ///
    /// `Promise.race` reads each input through `Promise.resolve(input).then`.
    /// A plain promise is returned as it is and reacts one turn after it
    /// settles. An `AbortablePromise` is a subclass, so `Promise.resolve`
    /// adopts it as a thenable: one turn for the adoption job, one for its
    /// reaction, then the race's own reaction.
    pub fn race(promises: Vec<Promise<T>>) -> Promise<T> {
        let (race, resolver) = Promise::pending();
        for promise in promises {
            let resolver = resolver.clone();
            if promise.is_abortable() {
                spawn(async move {
                    tick().await;
                    let result = promise.await;
                    tick().await;
                    resolver.settle(result);
                });
                continue;
            }
            promise.on_settle(move |result| {
                // `.then(resolve, reject)` runs one turn after settlement.
                let result = result.clone();
                spawn(async move {
                    tick().await;
                    resolver.settle(result);
                });
            });
        }
        race
    }
}

pub struct PromiseFuture<T> {
    promise: Promise<T>,
    yielded: bool,
}

impl<T: Clone + 'static> Future for PromiseFuture<T> {
    type Output = Result<T, Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let settled = {
            let mut state = self.promise.0.state.borrow_mut();
            match &mut *state {
                State::Settled(result) => Some(result.clone()),
                State::Pending(wakers) => {
                    wakers.push(cx.waker().clone());
                    None
                }
            }
        };
        match settled {
            Some(result) if self.yielded => Poll::Ready(result),
            Some(_) => {
                self.yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            None => {
                // Woken by settlement, which already queued us: that wake IS
                // the turn a JavaScript reaction job takes.
                self.yielded = true;
                Poll::Pending
            }
        }
    }
}

impl<T: Clone + 'static> std::future::IntoFuture for Promise<T> {
    type Output = Result<T, Error>;
    type IntoFuture = PromiseFuture<T>;
    fn into_future(self) -> PromiseFuture<T> {
        PromiseFuture { promise: self, yielded: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_runs_the_synchronous_prefix_inline() {
        reset();
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        let p = Promise::spawn(async move {
            l.borrow_mut().push("sync");
            tick().await;
            l.borrow_mut().push("after");
            Ok(1)
        });
        assert_eq!(*log.borrow(), ["sync"]);
        run_until_stalled();
        assert_eq!(*log.borrow(), ["sync", "after"]);
        assert_eq!(p.peek().unwrap().unwrap(), 1);
    }

    #[test]
    fn awaiting_a_settled_promise_yields_a_turn() {
        reset();
        let log = Rc::new(RefCell::new(Vec::new()));
        let settled = Promise::resolved(());
        let (l1, l2) = (log.clone(), log.clone());
        let s = settled.clone();
        spawn(async move {
            s.await.unwrap();
            l1.borrow_mut().push("a");
        });
        spawn(async move {
            l2.borrow_mut().push("b");
        });
        assert_eq!(*log.borrow(), ["b"]);
        run_until_stalled();
        assert_eq!(*log.borrow(), ["b", "a"]);
    }

    #[test]
    fn timers_fire_in_deadline_then_insertion_order() {
        reset();
        let log = Rc::new(RefCell::new(Vec::new()));
        for (delay, name) in [(10.0, "ten"), (5.0, "five"), (10.0, "ten-b"), (0.0, "zero")] {
            let l = log.clone();
            set_timeout(delay, move || l.borrow_mut().push(name));
        }
        let cleared = set_timeout(1.0, || panic!("cleared"));
        clear_timer(cleared);
        advance(9.0);
        assert_eq!(*log.borrow(), ["zero", "five"]);
        assert_eq!(now(), 9.0);
        advance(1.0);
        assert_eq!(*log.borrow(), ["zero", "five", "ten", "ten-b"]);
    }

    #[test]
    fn a_zero_delay_during_a_tick_is_one() {
        reset();
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = log.clone();
        set_timeout(5.0, move || {
            let l2 = l.clone();
            set_timeout(0.0, move || l2.borrow_mut().push(now()));
        });
        advance(10.0);
        assert_eq!(*log.borrow(), [6.0]);
    }

    #[test]
    fn intervals_rearm() {
        reset();
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let id = set_interval(3.0, move || c.set(c.get() + 1));
        advance(10.0);
        assert_eq!(count.get(), 3);
        clear_timer(id);
        advance(10.0);
        assert_eq!(count.get(), 3);
    }
}
