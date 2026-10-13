# EmbedPDF Swift Package Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Swift package, `platforms/apple/EmbedPDF`, that runs the Rust kernel and plugin crates over the Rust PDFium engine on iOS, with typed UniFFI exports per plugin and a thin Swift facade on top.

**Architecture:** Each plugin crate gets an `ffi` module behind a `uniffi` cargo feature. That module exports a typed handle object, typed records, an event enum with a listener callback interface, and `async fn`s for the plugin's promises. A new support crate, `crates/epdf-ffi`, owns the kernel's owner thread and the `KernelBound<C>` handle that lets a `Send + Sync` UniFFI object call an `Rc` capability on that thread. A new host crate, `crates/epdf-viewer-host`, is the P6 crate named in `multiplatform-plugins-plan.md` §4. It builds the kernel from a typed `ViewerConfig`, runs the kernel's `Engine` over `epdf_engine::host::Host` on the engine's own thread, and exports `ViewerHost`. The Swift package has three layers. `EmbedPDFBindings` holds the generated code. `EmbedPDF` re-exports the generated records and wraps the objects in Swift-shaped capabilities. `EmbedPDFUI` holds `@MainActor @Observable` models and SwiftUI views. The same Rust exports generate the Kotlin binding with no further Rust work.

**Tech Stack:** Rust 2024 edition, `uniffi =0.32.0` (proc-macros, library mode), `futures` 0.3 (oneshot channels behind async exports), Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Xcode 27.0, iOS 17 simulator.

**Spec:** No standalone spec exists. The plan argues from three documents: `multiplatform-plugins-plan.md` (§2.2 engine contract, §4 P6, §5 facades), `multiplatform-plan.md` §8 (native API rules), and `platforms/apple/README.md`. Executors read those alongside this plan.

## Global Constraints

- `uniffi` is pinned at `=0.32.0` in every crate that uses it. That pin is already in the root `Cargo.toml` comment and in `crates/epdf-uniffi/Cargo.toml`.
- iOS deployment floor is 17.0. It comes from the fork's `ios_deployment_target`, noted in the `platforms/apple/build-xcframework.sh` heredoc comment.
- Slices are `aarch64-apple-ios` and `aarch64-apple-ios-sim` only. There's no macOS slice: darwin PDFium ships as a dylib (`crates/build-support/src/lib.rs:122`), and a static XCFramework can't carry one.
- PDFium never crosses threads. One engine owns one OS thread (`multiplatform-plan.md` §4, constraint 5).
- The kernel is `!Send`. It's built from `Rc` and `RefCell`, with a `thread_local!` runtime (`crates/epdf-kernel/src/rt.rs:64`). Every kernel touch happens on the kernel thread, through `epdf_ffi::KernelBound` or a kernel job.
- `[profile.release]` keeps unwinding (no `panic = "abort"`). UniFFI catches panics at the boundary.
- UniFFI type names are unique across the whole workspace. All generated Swift compiles into one module, `EmbedPDFBindings`, so two crates that each export a `Status` collide. Types shared between plugins (`PageRef`, `Rect`, `Point`, `Size`, `ViewerError`, `Subscription`) live in `epdf-ffi` and nowhere else.
- UniFFI has no `usize`, `isize`, `Rc`, or `serde_json::Value`. A plugin type with any of those gets an FFI mirror record in its crate's `ffi` module, with a `From` impl. A type without them derives `uniffi::Record` or `uniffi::Enum` in place, behind `cfg_attr(feature = "uniffi", ...)`.
- Error fields are named `detail`, not `message`. Teardown methods are named `shutdown`, not `close`. Both avoid Kotlin generator collisions that already bit `epdf-uniffi`.
- Facades don't decide PDF semantics (`multiplatform-plan.md` §5.1, the facades row). Swift renames and reshapes; behaviour stays in Rust.
- Text offsets are UTF-16 code units. Swift's `String.count` counts grapheme clusters and is wrong for them.
- A returned `CGImage` owns its pixels (remix §8).
- Swift test targets never link what the target under test already provides (pfw-testing). Value assertions use `expectNoDifference`.
- `Package.swift` argument order is `name`, `platforms`, `products`, `dependencies`, `targets` (pfw-spm).
- One changeset file per directly affected npm package (`AGENTS.md`). This plan touches no npm package, so it adds none.

## Review Focus

Happy-path tests don't exercise these five inputs. Each one has a test in the task that owns the code.

1. A Swift `Task` cancelled while awaiting a plugin promise. People expect the Rust operation to abort, not to run on and settle into nothing. Pinned in Task 7 (`searchCancelsWhenTheTaskIsCancelled`) and in Task 1 for the Rust half (`dropping_a_promise_future_aborts_the_promise`).
2. An event stream the consumer stops iterating. People expect the Rust listener to unsubscribe, not leak. Pinned in Task 1 (`cancelling_a_subscription_unsubscribes_on_the_kernel_thread`) and Task 6 (`droppingTheStreamUnsubscribes`).
3. A document closed while Swift still holds a capability for it. People expect a typed error with code `instance-closed` on the next call, not a crash. Pinned in Task 6 (`aClosedDocumentsCapabilityThrows`).
4. `ViewerHost` dropped without `destroy()`. People expect both threads to exit and PDFium to be released. Pinned in Task 3 (`dropping_the_host_joins_both_threads`).
5. An encrypted PDF opened without a password. People expect a typed error they can prompt on (`DocPasswordRequired`), as the existing engine smoke test shows. Pinned in Task 3 (`a_locked_document_reports_that_it_needs_a_password`) and Task 5.

---

## Decisions

We're exporting typed UniFFI objects per plugin and not a JSON dispatch. An earlier draft of this plan mirrored the web's `WasmKernel` (`crates/epdf-web/src/exports.rs:270`): one `invoke(ref, argsJson)` entry point over the conformance harness adapters. That's the right shape for wasm, where every value crosses as JS anyway. On mobile it's the wrong trade. Every DTO would be hand-written a second time as Swift `Codable` and a third time for Kotlin, a Rust rename would fail at runtime instead of compile time, and every call would pay a JSON encode and parse. Typed exports generate the Swift and Kotlin types from Rust. The hand-written Swift layer only renames, and the compiler checks it against the generated signatures.

Typed doesn't mean direct. The kernel is single-threaded, so every call still posts a job to the kernel thread and waits for the reply. That hop costs more than the serialization either approach adds. UniFFI also copies arguments into its own binary buffer format. The win is type safety, generated bindings and a smaller per-call cost. It isn't a zero-cost call.

The cost is Rust code. About 600 members across 19 plugins each need a one-line-to-ten-line wrapper in an `ffi` module, plus mirror records wherever a DTO uses `usize`, `isize`, `Rc<Vec<_>>` or `serde_json::Value`. The binary grows by the scaffolding for every export. This plan writes three plugins in full (metadata, search, render) and fixes the recipe for the other 16 in Appendix A. Until a plugin's `ffi` module lands, Swift can't reach it at all. That's the price of dropping the dynamic path, and we think it's worth paying.

The wrappers live in each plugin crate, behind `uniffi`, rather than all inside `epdf-viewer-host`. A plugin's FFI surface then changes in the same commit as the plugin, and a mirror record sits next to the type it mirrors. That needs a small shared crate the plugin crates can depend on without a dependency cycle: `epdf-ffi`.

Events use one enum and one listener interface per plugin (`SearchEvent`, `SearchEventListener`), not one callback interface per event. Search alone has seven events, and the whole plugin set has about 150. `subscribe` is synchronous. It posts the registration job and returns a `Subscription` at once, so a Swift `AsyncStream` can be built without a race, and any call made after `subscribe` returns sees the listener registered.

The engine runs on its own thread behind `OwnerThread` (`crates/epdf-engine/src/dispatch.rs:58`), so a render never blocks the kernel thread. A PDFium call can't be interrupted part-way. A kernel abort rejects the promise at once and discards the late reply.

We don't actually know how much UniFFI costs compared with a hand-written C ABI for this kernel, so Task 11 measures it. It hand-bridges the search plugin and render's `render_page` through `extern "C"` functions, `#[repr(C)]` structs and C callbacks, then runs the same workloads through both bridges. The manual bridge is experimental and ships in no product. If it wins clearly on a workload, the likely outcome is a hybrid: UniFFI for most of the API, plus hand-written entry points for the hot paths it wins on (raster bytes, pointer samples). Task 11's results doc records that decision.

`multiplatform-plan.md` §8 says UniFFI's output isn't the public API, and `crates/epdf-uniffi/src/lib.rs` repeats it. With typed exports we're relaxing that rule for records only. `EmbedPDF` re-exports generated records under their own names, which is the only way to avoid duplicating them. Objects, errors and events stay wrapped. The cost: a UniFFI upgrade that changes how records are generated changes our public API. We'll pin and review on each bump.

## File Structure

```
crates/epdf-ffi/
  Cargo.toml
  src/lib.rs                    uniffi scaffolding; re-exports
  src/kernel_thread.rs          KernelThread<S>, KernelPost
  src/bound.rs                  KernelBound<C>: call, call_promise, slab on the kernel thread
  src/subscription.rs           Subscription object
  src/error.rs                  ViewerError
  src/geometry.rs               PageRef, Point, Size, Rect records + From impls
  tests/kernel_thread.rs
  tests/bound.rs
crates/epdf-viewer-host/
  Cargo.toml
  build.rs
  src/lib.rs                    ViewerHost, ViewerConfig, ViewerDelegate, DocumentInfo, PageInfo
  src/engine_worker.rs          Host on an OwnerThread
  src/native_engine.rs          epdf_kernel::Engine over EngineWorker
  src/translate.rs              kernel path -> wire request, wire response -> kernel value
  src/host_table.rs             bytes kept out of JSON
  src/session.rs                Session: Kernel + plugin list, built on the kernel thread
  tests/translate.rs
  tests/viewer_host.rs
crates/plugins/metadata/src/ffi.rs     MetadataHandle, MetadataEvent, MetadataEventListener
crates/plugins/search/src/ffi.rs       SearchHandle, SearchEvent, mirrors (SearchHit, SearchConfig, ...)
crates/plugins/render/src/ffi.rs       RenderHandle, PageImage, RenderEvent
crates/plugins/{metadata,search,render}/tests/ffi.rs
platforms/apple/
  build-xcframework.sh          builds epdf-viewer-host, merges every crate's modulemap
  .gitignore
  README.md
  EmbedPDF/Package.swift        tracked from now on
  EmbedPDF/Sources/EmbedPDFBindings/          generated (ignored)
  EmbedPDF/Sources/EmbedPDF/Exports.swift     typealiases re-exporting generated records
  EmbedPDF/Sources/EmbedPDF/PDFError.swift
  EmbedPDF/Sources/EmbedPDF/EventRelay.swift
  EmbedPDF/Sources/EmbedPDF/PDFViewer.swift
  EmbedPDF/Sources/EmbedPDF/Plugins/Metadata.swift
  EmbedPDF/Sources/EmbedPDF/Plugins/Search.swift
  EmbedPDF/Sources/EmbedPDF/Plugins/Render.swift
  EmbedPDF/Sources/EmbedPDFUI/SearchModel.swift
  EmbedPDF/Sources/EmbedPDFUI/PageImageView.swift
  EmbedPDF/Tests/EmbedPDFBindingsSmokeTests/SmokeTests.swift   moved from EmbedPDFSmokeTests
  EmbedPDF/Tests/EmbedPDFTests/{BaseSuite,ViewerTests,MetadataTests,SearchTests,RenderTests}.swift
  EmbedPDF/Tests/EmbedPDFUITests/SearchModelTests.swift
.github/workflows/apple-swift.yml
crates/epdf-manual-bridge/                  experimental (Task 11): extern "C" search + render
  Cargo.toml
  include/epdf_manual.h
  src/lib.rs
  src/search.rs
  src/render.rs
  src/bench.rs                  noop and hop-only entry points
  tests/abi.rs
crates/epdf-viewer-host/src/experimental.rs  kernel registry the manual bridge looks up (feature manual-bridge)
platforms/apple/EmbedPDF/Sources/EmbedPDFManualBridge/ManualSearch.swift, ManualRender.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFBridgeBenchmarks/BridgeBenchmarks.swift
docs/research/swift-bridge-benchmarks.md    results
```

Every Rust task runs from the repo root with the darwin PDFium fetched. Do this once before Task 1:

```bash
bash packages/engine/runtime/scripts/fetch-libpdfium.sh darwin-arm64
```

iOS slices also need the local fork artifacts that `platforms/apple/README.md` describes: run `scripts/runtime-use-local.sh`, then run `fetch-libpdfium.sh` for `ios-arm64` and for `ios-sim-arm64`, with `PDF_RUNTIME_BUILD_FILE=$PWD/engine-runtime-build.local.json`.

---

### Task 1: `epdf-ffi`, the kernel thread and the handle that reaches it

**Files:**
- Create: `crates/epdf-ffi/Cargo.toml`, `src/lib.rs`, `src/kernel_thread.rs`, `src/bound.rs`, `src/subscription.rs`, `src/error.rs`, `src/geometry.rs`
- Test: `crates/epdf-ffi/tests/kernel_thread.rs`, `crates/epdf-ffi/tests/bound.rs`
- Modify: root `Cargo.toml` (add `"crates/epdf-ffi"` to `members`, after `"crates/epdf-uniffi"`)

**Interfaces:**
- Consumes: `epdf_kernel::rt::{advance, now, next_deadline, run_until_stalled, set_timeout}` (`rt.rs:121,198,255`). `epdf_kernel::Promise::{on_settle, abort}` (`rt.rs:447,471`). `epdf_kernel::store::Unsubscribe`. `epdf_kernel::Error::{code, name, message}` (`error.rs:31,76`). `epdf_kernel::geometry::{PageRef, Point, Rect}` (`geometry.rs:17,57,64`).
- Produces:
  ```rust
  // kernel_thread.rs
  pub type Job<S> = Box<dyn FnOnce(&mut S) + Send>;
  pub type KernelPost = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> bool + Send + Sync>;
  pub struct KernelThread<S: 'static>;
  impl<S: 'static> KernelThread<S> {
      pub fn spawn<B>(build: B) -> Result<Self, epdf_kernel::Error>
      where B: FnOnce(KernelPost) -> Result<S, epdf_kernel::Error> + Send + 'static;
      pub fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut S) -> T + Send + 'static)
          -> futures::channel::oneshot::Receiver<T>;
      pub fn post(&self) -> KernelPost;
      pub fn shutdown(&mut self);                       // idempotent; joins
  }
  // bound.rs
  pub struct KernelBound<C: 'static>;                  // Send + Sync
  impl<C: 'static> KernelBound<C> {
      pub fn new(value: C, post: KernelPost) -> Self;  // call on the kernel thread
      pub fn call<T: Send + 'static>(&self, f: impl FnOnce(&C) -> Result<T, epdf_kernel::Error> + Send + 'static)
          -> impl Future<Output = Result<T, ViewerError>> + Send;
      pub fn call_promise<U: Clone + 'static, T: Send + 'static>(
          &self,
          f: impl FnOnce(&C) -> Result<epdf_kernel::Promise<U>, epdf_kernel::Error> + Send + 'static,
          map: impl FnOnce(U) -> T + Send + 'static,
      ) -> impl Future<Output = Result<T, ViewerError>> + Send;     // drop = abort
      pub fn fire(&self, f: impl FnOnce(&C) + Send + 'static); // no reply; hot paths
  }
  // subscription.rs
  #[derive(uniffi::Object)] pub struct Subscription;
  impl Subscription { pub fn pending(post: KernelPost) -> (Arc<Subscription>, SubscriptionSlot); }
  pub struct SubscriptionSlot;  // Send; fill(Unsubscribe) on the kernel thread
  #[uniffi::export] impl Subscription { pub fn cancel(&self); }   // idempotent; Drop cancels
  // error.rs
  #[derive(Debug, thiserror::Error, uniffi::Error)]
  pub enum ViewerError {
      #[error("{name}: {detail}")] Kernel { name: String, code: Option<String>, detail: String },
      #[error("closed")] Closed,
  }
  impl From<epdf_kernel::Error> for ViewerError;
  // geometry.rs
  #[derive(uniffi::Record)] pub struct PageRef { pub page_object_number: u32 }
  #[derive(uniffi::Record)] pub struct Point { pub x: f64, pub y: f64 }
  #[derive(uniffi::Record)] pub struct Size { pub width: f64, pub height: f64 }
  #[derive(uniffi::Record)] pub struct Rect { pub origin: Point, pub size: Size }
  ```

Check `epdf_kernel::geometry::Rect`'s fields at `geometry.rs:64` and make the record match them. The `origin`/`size` layout above is the TypeScript `Rect` and may not be the Rust one.

- [ ] **Step 1: Write the manifest and `lib.rs`**

`crates/epdf-ffi/Cargo.toml`:
```toml
[package]
name = "epdf-ffi"
version.workspace = true
edition.workspace = true
publish.workspace = true

[dependencies]
epdf-kernel = { path = "../epdf-kernel" }
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
thiserror = "2"
uniffi = "=0.32.0"
```

`crates/epdf-ffi/src/lib.rs`:
```rust
//! What every crate that exports the kernel or a plugin through uniffi
//! shares: the kernel's owner thread, the handle that reaches it, the error
//! every export returns, and the records more than one plugin uses.

pub mod bound;
pub mod error;
pub mod geometry;
pub mod kernel_thread;
pub mod subscription;

pub use bound::KernelBound;
pub use error::ViewerError;
pub use geometry::{PageRef, Point, Rect, Size};
pub use kernel_thread::{Job, KernelPost, KernelThread};
pub use subscription::{Subscription, SubscriptionSlot};

uniffi::setup_scaffolding!();
```

- [ ] **Step 2: Write the failing tests**

`crates/epdf-ffi/tests/kernel_thread.rs`:
```rust
use std::sync::mpsc;
use std::time::{Duration, Instant};

use epdf_ffi::KernelThread;
use futures::executor::block_on;

#[test]
fn a_job_runs_on_the_kernel_thread_and_answers() {
    let thread = KernelThread::spawn(|_| Ok(41u32)).unwrap();
    let answer = block_on(thread.call(|n: &mut u32| {
        *n += 1;
        *n
    }))
    .unwrap();
    assert_eq!(answer, 42);
}

#[test]
fn a_virtual_timer_fires_on_wall_time() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    (thread.post())(Box::new(move || {
        epdf_kernel::rt::set_timeout(50.0, move || tx.send(Instant::now()).unwrap());
    }));
    let fired = rx.recv_timeout(Duration::from_secs(2)).expect("timer never fired");
    assert!(fired.duration_since(started) >= Duration::from_millis(50));
}

#[test]
fn shutdown_is_idempotent_and_refuses_later_jobs() {
    let mut thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    thread.shutdown();
    thread.shutdown();
    assert!(!post(Box::new(|| {})));
}

#[test]
fn a_build_error_reaches_the_caller() {
    let result = KernelThread::<()>::spawn(|_| Err(epdf_kernel::Error::new("no")));
    assert_eq!(result.err().map(|e| e.message), Some("no".into()));
}
```

`crates/epdf-ffi/tests/bound.rs`:
```rust
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use epdf_ffi::{KernelBound, KernelThread, Subscription, ViewerError};
use epdf_kernel::Promise;
use epdf_kernel::store::Unsubscribe;
use futures::executor::block_on;

/// Something `!Send`, as every capability is.
#[derive(Clone)]
struct Counter(Rc<Cell<u32>>);

fn bound() -> (KernelThread<()>, KernelBound<Counter>) {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let post = thread.post();
    let bound = block_on(thread.call(move |_| KernelBound::new(Counter(Rc::new(Cell::new(0))), post))).unwrap();
    (thread, bound)
}

#[test]
fn a_call_reaches_a_non_send_value() {
    let (_thread, bound) = bound();
    let n = block_on(bound.call(|c| {
        c.0.set(c.0.get() + 1);
        Ok(c.0.get())
    }))
    .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn a_kernel_error_keeps_its_code() {
    let (_thread, bound) = bound();
    let error = block_on(bound.call(|_| -> Result<(), _> {
        Err(epdf_kernel::Error::engine("not-found", "gone"))
    }))
    .unwrap_err();
    assert!(matches!(error, ViewerError::Kernel { code: Some(ref c), .. } if c == "not-found"));
}

#[test]
fn a_promise_resolves_through_the_future() {
    let (_thread, bound) = bound();
    let n = block_on(bound.call_promise(|_| Ok(Promise::resolved(7u32)), |n| n * 6)).unwrap();
    assert_eq!(n, 42);
}

#[test]
fn dropping_a_promise_future_aborts_the_promise() {
    let (thread, bound) = bound();
    let aborted = Arc::new(AtomicBool::new(false));
    let flag = aborted.clone();
    let future = bound.call_promise(
        move |_| {
            let (promise, _resolver) = Promise::<u32>::pending();
            Ok(promise.with_abort(move |_| flag.store(true, Ordering::SeqCst)))
        },
        |n| n,
    );
    // Let the job run so the promise exists, then drop the waiting future.
    let mut future = Box::pin(future);
    let _ = futures::executor::block_on(futures::future::select(future.as_mut(), futures::future::ready(())));
    drop(future);
    block_on(thread.call(|_| ())).unwrap(); // drain the abort job
    assert!(aborted.load(Ordering::SeqCst));
}

#[test]
fn a_call_after_the_thread_stops_is_closed() {
    let (mut thread, bound) = bound();
    thread.shutdown();
    assert!(matches!(block_on(bound.call(|_| Ok(()))), Err(ViewerError::Closed)));
}

#[test]
fn cancelling_a_subscription_unsubscribes_on_the_kernel_thread() {
    let thread = KernelThread::spawn(|_| Ok(())).unwrap();
    let unsubscribed = Arc::new(AtomicBool::new(false));
    let flag = unsubscribed.clone();
    let (subscription, slot) = Subscription::pending(thread.post());
    block_on(thread.call(move |_| {
        slot.fill(Unsubscribe::new(move || flag.store(true, Ordering::SeqCst)));
    }))
    .unwrap();
    subscription.cancel();
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert!(unsubscribed.load(Ordering::SeqCst));
}
```

Check how `Unsubscribe` is constructed at `crates/epdf-kernel/src/store.rs:519`, and spell `Unsubscribe::new` the way it's actually built.

In `dropping_a_promise_future_aborts_the_promise`, `select` with `ready(())` polls the future once and returns. One poll isn't guaranteed to come after the job has run on the kernel thread. Make the test deterministic: before dropping the future, run `block_on(thread.call(|_| ()))` once, so the job that created the promise has run.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-ffi`
Expected: FAIL to compile, with unresolved imports from `epdf_ffi`.

- [ ] **Step 4: Implement `kernel_thread.rs`**

```rust
//! The thread the kernel lives on.
//!
//! The kernel is `Rc` all the way down and its runtime is thread-local, so
//! it is built here and never leaves. Everything else reaches it by posting
//! a job. Between jobs the loop sleeps until the runtime's next virtual
//! deadline, then catches the virtual clock up to wall time, as
//! `crates/epdf-web/src/runtime.rs` `after_entry` does with `setTimeout`.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use epdf_kernel::{Error, rt};
use futures::channel::oneshot;

pub type Job<S> = Box<dyn FnOnce(&mut S) + Send>;
pub type KernelPost = Arc<dyn Fn(Box<dyn FnOnce() + Send>) -> bool + Send + Sync>;

enum Msg<S> {
    Job(Job<S>),
    Stop,
}

pub struct KernelThread<S: 'static> {
    tx: Arc<Mutex<Option<Sender<Msg<S>>>>>,
    handle: Option<JoinHandle<()>>,
}

impl<S: 'static> KernelThread<S> {
    pub fn spawn<B>(build: B) -> Result<Self, Error>
    where
        B: FnOnce(KernelPost) -> Result<S, Error> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Msg<S>>();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let post = post_for(&tx);
        let (built_tx, built_rx) = mpsc::channel::<Result<(), Error>>();
        let handle = std::thread::Builder::new()
            .name("epdf-kernel".into())
            .spawn(move || {
                let origin = Instant::now();
                let mut state = match build(post) {
                    Ok(state) => {
                        let _ = built_tx.send(Ok(()));
                        state
                    }
                    Err(error) => {
                        let _ = built_tx.send(Err(error));
                        return;
                    }
                };
                drain(origin);
                loop {
                    let msg = match rt::next_deadline() {
                        Some(deadline) => {
                            let wait = (deadline - rt::now()).max(0.0);
                            rx.recv_timeout(Duration::from_secs_f64(wait / 1000.0))
                        }
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    };
                    match msg {
                        Ok(Msg::Job(job)) => job(&mut state),
                        Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    drain(origin);
                }
                drop(state);
                drain(origin);
            })
            .map_err(|e| Error::new(format!("[epdf-ffi] no kernel thread: {e}")))?;
        built_rx
            .recv()
            .map_err(|_| Error::new("[epdf-ffi] the kernel thread died while building"))??;
        Ok(KernelThread { tx, handle: Some(handle) })
    }

    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut S) -> T + Send + 'static,
    ) -> oneshot::Receiver<T> {
        let (reply, rx) = oneshot::channel();
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(Msg::Job(Box::new(move |s| {
                let _ = reply.send(f(s));
            })));
        }
        rx
    }

    pub fn post(&self) -> KernelPost {
        post_for(&self.tx)
    }

    pub fn shutdown(&mut self) {
        let Some(handle) = self.handle.take() else { return };
        if let Some(tx) = self.tx.lock().unwrap().take() {
            let _ = tx.send(Msg::Stop);
        }
        let _ = handle.join();
    }
}

impl<S: 'static> Drop for KernelThread<S> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn post_for<S: 'static>(tx: &Arc<Mutex<Option<Sender<Msg<S>>>>>) -> KernelPost {
    let tx = tx.clone();
    Arc::new(move |job: Box<dyn FnOnce() + Send>| match tx.lock().unwrap().as_ref() {
        Some(tx) => tx.send(Msg::Job(Box::new(move |_| job()))).is_ok(),
        None => false,
    })
}

/// Catch the virtual clock up to wall time, then run everything runnable.
/// `rt::advance` fires each due timer and drains after it.
fn drain(origin: Instant) {
    let wall = origin.elapsed().as_secs_f64() * 1000.0;
    let behind = wall - rt::now();
    if behind > 0.0 {
        rt::advance(behind);
    }
    rt::run_until_stalled();
}
```

`KernelPost` holds the `Sender` through the shared `Option`. Once `shutdown` takes the sender, every outstanding `KernelPost` returns `false`. That's how a handle Swift still holds learns the viewer is gone.

- [ ] **Step 5: Implement `bound.rs`, `subscription.rs`, `error.rs` and `geometry.rs`**

`bound.rs`:
```rust
//! A `Send + Sync` handle to a `!Send` value that lives on the kernel thread.
//!
//! The value sits in a kernel-thread-local slab; the handle is its id plus a
//! way to post jobs. Every uniffi object that wraps a capability holds one.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use epdf_kernel::{Error, Promise};
use futures::channel::oneshot;

use crate::{KernelPost, ViewerError};

thread_local! {
    static SLAB: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
}
static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct KernelBound<C: 'static> {
    id: u64,
    post: KernelPost,
    _value: PhantomData<fn() -> C>,
}

impl<C: 'static> KernelBound<C> {
    pub fn new(value: C, post: KernelPost) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        SLAB.with(|s| s.borrow_mut().insert(id, Box::new(value)));
        KernelBound { id, post, _value: PhantomData }
    }

    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&C) -> Result<T, Error> + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (tx, rx) = oneshot::channel();
        let id = self.id;
        (self.post)(Box::new(move || {
            let _ = tx.send(with(id, |c: &C| f(c).map_err(ViewerError::from)));
        }));
        async move { rx.await.unwrap_or(Err(ViewerError::Closed)) }
    }

    pub fn call_promise<U: Clone + 'static, T: Send + 'static>(
        &self,
        f: impl FnOnce(&C) -> Result<Promise<U>, Error> + Send + 'static,
        map: impl FnOnce(U) -> T + Send + 'static,
    ) -> impl Future<Output = Result<T, ViewerError>> + Send {
        let (tx, rx) = oneshot::channel::<Result<T, ViewerError>>();
        let pending = Arc::new(AtomicU64::new(0));
        let slot = pending.clone();
        let id = self.id;
        (self.post)(Box::new(move || {
            let started = with(id, |c: &C| f(c).map_err(ViewerError::from));
            match started {
                Err(error) => {
                    let _ = tx.send(Err(error));
                }
                Ok(promise) => {
                    let pid = NEXT.fetch_add(1, Ordering::Relaxed);
                    SLAB.with(|s| s.borrow_mut().insert(pid, Box::new(promise.clone())));
                    slot.store(pid, Ordering::SeqCst);
                    promise.on_settle(move |result| {
                        SLAB.with(|s| s.borrow_mut().remove(&pid));
                        let _ = tx.send(match result {
                            Ok(value) => Ok(map(value.clone())),
                            Err(error) => Err(ViewerError::from(error.clone())),
                        });
                    });
                }
            }
        }));
        let guard = AbortOnDrop::<U> { pending, post: self.post.clone(), _value: PhantomData };
        async move {
            let result = rx.await.unwrap_or(Err(ViewerError::Closed));
            guard.disarm();
            result
        }
    }

    pub fn fire(&self, f: impl FnOnce(&C) + Send + 'static) {
        let id = self.id;
        (self.post)(Box::new(move || {
            let _ = with(id, |c: &C| {
                f(c);
                Ok(())
            });
        }));
    }
}

impl<C: 'static> Drop for KernelBound<C> {
    fn drop(&mut self) {
        let id = self.id;
        (self.post)(Box::new(move || {
            SLAB.with(|s| s.borrow_mut().remove(&id));
        }));
    }
}

fn with<C: 'static, T>(id: u64, f: impl FnOnce(&C) -> Result<T, ViewerError>) -> Result<T, ViewerError> {
    let value = SLAB.with(|s| s.borrow().get(&id).and_then(|v| v.downcast_ref::<C>().map(|c| c as *const C)));
    match value {
        // The slab entry outlives this call: only a later job on this same
        // thread can remove it.
        Some(c) => f(unsafe { &*c }),
        None => Err(ViewerError::Closed),
    }
}

struct AbortOnDrop<U: 'static> {
    pending: Arc<AtomicU64>,
    post: KernelPost,
    _value: PhantomData<fn() -> U>,
}

impl<U: 'static> AbortOnDrop<U> {
    fn disarm(self) {
        self.pending.store(0, Ordering::SeqCst);
    }
}

impl<U: 'static> Drop for AbortOnDrop<U> {
    fn drop(&mut self) {
        let pid = self.pending.swap(0, Ordering::SeqCst);
        if pid == 0 {
            return;
        }
        (self.post)(Box::new(move || {
            let promise = SLAB.with(|s| s.borrow_mut().remove(&pid));
            if let Some(promise) = promise.and_then(|p| p.downcast::<Promise<U>>().ok()) {
                promise.abort(None);
            }
        }));
    }
}
```

The `unsafe` in `with` avoids holding the slab's `RefCell` borrow while `f` runs. Holding it would panic if `f` re-entered the slab, for example by creating a `KernelBound` for a capability it returns. Don't replace it with a `borrow()` held across `f`. If the reviewer won't accept the `unsafe`, store `Rc<dyn Any>` in the slab and clone the `Rc` out before calling `f`. That works too, and costs one refcount bump per call.

`subscription.rs`:
```rust
use std::sync::{Arc, Mutex};

use epdf_kernel::store::Unsubscribe;

use crate::{KernelBound, KernelPost};

#[derive(uniffi::Object)]
pub struct Subscription {
    inner: Mutex<Option<Arc<Mutex<Option<KernelBound<Unsubscribe>>>>>>,
}

pub struct SubscriptionSlot {
    cell: Arc<Mutex<Option<KernelBound<Unsubscribe>>>>,
    post: KernelPost,
}

impl Subscription {
    pub fn pending(post: KernelPost) -> (Arc<Subscription>, SubscriptionSlot) {
        let cell = Arc::new(Mutex::new(None));
        let subscription = Arc::new(Subscription { inner: Mutex::new(Some(cell.clone())) });
        (subscription, SubscriptionSlot { cell, post })
    }
}

impl SubscriptionSlot {
    /// On the kernel thread, once the listener is registered.
    pub fn fill(self, unsubscribe: Unsubscribe) {
        *self.cell.lock().unwrap() = Some(KernelBound::new(unsubscribe, self.post));
    }
}

#[uniffi::export]
impl Subscription {
    pub fn cancel(&self) {
        let Some(cell) = self.inner.lock().unwrap().take() else { return };
        if let Some(bound) = cell.lock().unwrap().take() {
            bound.fire(|unsubscribe| unsubscribe.call());
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}
```

There's a race here. If `cancel` runs after `Subscription::pending` but before the kernel thread has called `fill`, the cell is still empty, so nothing gets unsubscribed. Fix it in `fill`: when the subscription has already been cancelled, call `unsubscribe` immediately. Keep a shared `AtomicBool cancelled` between `Subscription` and `SubscriptionSlot` for this. Add a test for it: `cancel()` before the fill job runs, then check that the listener was removed.

`error.rs`:
```rust
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ViewerError {
    #[error("{name}: {detail}")]
    Kernel { name: String, code: Option<String>, detail: String },
    #[error("closed")]
    Closed,
}

impl From<epdf_kernel::Error> for ViewerError {
    fn from(error: epdf_kernel::Error) -> Self {
        ViewerError::Kernel {
            code: error.code().map(String::from),
            name: error.name,
            detail: error.message,
        }
    }
}
```

`geometry.rs` defines the four records, plus `From` impls in both directions with `epdf_kernel::geometry::{PageRef, Point, Rect}`. Plugins that use `epdf_geometry`'s types instead (`crates/plugins/geometry/src/lib.rs:31,37,46`) get their `From` impls in the same file.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p epdf-ffi`
Expected: PASS, 11 tests (4 in `kernel_thread`, 7 in `bound`, counting the early-cancel test the Step 5 note adds).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/epdf-ffi
git commit -m "Add epdf-ffi: the kernel thread and handles that reach it"
```

---

### Task 2: `NativeEngine`, the kernel's engine over PDFium

**Files:**
- Create: `crates/epdf-viewer-host/Cargo.toml`, `build.rs`, `src/lib.rs`, `src/engine_worker.rs`, `src/host_table.rs`, `src/translate.rs`, `src/native_engine.rs`
- Test: `crates/epdf-viewer-host/tests/translate.rs`
- Modify: root `Cargo.toml` (add `"crates/epdf-viewer-host"` to `members`)

**Interfaces:**
- Consumes: `epdf_ffi::{KernelThread, KernelPost}` (Task 1). `epdf_engine::dispatch::OwnerThread::{start, run, close}` (`dispatch.rs:58,82,107`). `epdf_engine::host::{Host, HostResponse}` (`host.rs:154,210`). `epdf_kernel::engine::{Engine, DocumentHandle, DocumentSecurity, EngineCall}` (`engine.rs:25-90`). `epdf_kernel::Promise::{pending, with_abort}` and `Resolver::{resolve, reject}` (`rt.rs:340,358,361,457`).
- Produces:
  ```rust
  pub struct HostTable;   // kernel-thread local
  impl HostTable { pub fn put(Vec<u8>) -> u64; pub fn get(u64) -> Option<Arc<Vec<u8>>>; pub fn take(u64) -> Option<Vec<u8>>; pub fn release(u64); }
  pub struct WireCall { pub request: serde_json::Value, pub payloads: Vec<Vec<u8>>, pub decode: Decode }
  pub enum Decode { Field(&'static str), Raster, Unit }
  pub fn translate(doc: &str, path: &str, pon: Option<u32>, args: &[Value], job: u64) -> Result<WireCall, epdf_kernel::Error>;
  pub fn decode(decode: &Decode, response: HostResponse) -> Result<Value, epdf_kernel::Error>;
  #[derive(Clone)] pub struct EngineWorker;   // Send + Sync
  impl EngineWorker { pub fn spawn() -> Self; pub fn submit(&self, String, Vec<Vec<u8>>, Box<dyn FnOnce(HostResponse) + Send>); pub fn shutdown(&self); }
  pub struct NativeEngine;   // impl epdf_kernel::engine::Engine; kernel thread only
  impl NativeEngine { pub fn new(worker: EngineWorker, post: KernelPost) -> Self; }
  ```

The kernel's engine vocabulary is not the wire's. Plugins call member chains like `page.render.image` and `metadata.read`. The wire speaks kinds like `pages.render` and `metadata.read`. On the web, `packages/core-wasm/src/engine-bridge.ts` turns the chain back into a call on `@embedpdf/engine`'s handle, and that handle packs the wire request (`packages/engine/core/src/wire/worker-protocol.ts`). `translate.rs` does both hops in one table. This task maps only what the three plugins in this plan need. Every other path rejects with code `unsupported`, so a feature that needs one fails loudly. Appendix C lists every path and the follow-on plan that maps it.

| Kernel path | `pon` | Wire kind | Fields beyond `kind`, `jobId`, `docId` | Result |
|---|---|---|---|---|
| `Engine::open` | – | `open.fatMem` | `password`, payload 0 = the bytes | opened document |
| `close` | – | `close` | – | `Unit` |
| `pages.list` | – | `pages.list` | – | field `snapshot` |
| `metadata.read` | – | `metadata.read` | – | field `metadata` |
| `metadata.update` | – | `metadata.update` | `patch: args[0]` | field `metadata` |
| `page.text.read` | yes | `pages.text` | `page` | field `snapshot` |
| `page.geometry.read` | yes | `pages.geometry` | `page` | field `snapshot` |
| `page.render.image` | yes | `pages.render` | `page`, `options: args[0]` | `Raster` |
| `search.query` | – | `search.query` | `request: args[0]` | field `slice` |

`page` is `{"kind": "objectNumber", "pageObjectNumber": pon}`, the shape `epdf_kernel::geometry::PageRef::from_json` reads (`geometry.rs:27`). The response tags are at `worker-protocol.ts:1217-1453`.

`Raster` decoding replaces the pixel payload with a host-table id, the same move `engine-bridge.ts` makes with `$host`. The result value becomes `{ width, height, stride, format, "$host": id }`. Render's raster store reads only the data members. Render's FFI wrapper (Task 8) copies the bytes out for Swift, and `RenderHost::release_image` frees the entry.

- [ ] **Step 1: Write the manifest, `build.rs` and `lib.rs`**

`crates/epdf-viewer-host/Cargo.toml`:
```toml
[package]
name = "epdf-viewer-host"
version.workspace = true
edition.workspace = true
publish.workspace = true

[lib]
name = "epdf_viewer_host"
crate-type = ["cdylib", "staticlib", "rlib"]

[dependencies]
epdf-engine = { path = "../epdf-engine" }
epdf-ffi = { path = "../epdf-ffi" }
epdf-kernel = { path = "../epdf-kernel" }
epdf-uniffi = { path = "../epdf-uniffi" }
base64 = "0.22"
futures = { version = "0.3", default-features = false, features = ["std", "executor"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uniffi = "=0.32.0"

[build-dependencies]
build-support = { path = "../build-support" }

[dev-dependencies]
test-support = { path = "../test-support" }
```

`epdf-uniffi` is a dependency so the one static archive carries the engine-level exports too, and the existing smoke tests keep working. Plugin crates are added one at a time in Tasks 6 to 8, each with `features = ["uniffi"]`.

`build.rs`:
```rust
fn main() {
    build_support::emit_rpath_only();
}
```

`src/lib.rs`, for now:
```rust
//! The kernel and plugins over `epdf-engine`, exported through uniffi: the
//! P6 crate `multiplatform-plugins-plan.md` §4 names.

pub mod engine_worker;
pub mod host_table;
pub mod native_engine;
pub mod translate;

uniffi::setup_scaffolding!();
```

- [ ] **Step 2: Write the failing translation tests**

These don't load PDFium. They check that a kernel call becomes the wire request the TypeScript worker protocol defines.

`crates/epdf-viewer-host/tests/translate.rs`:
```rust
use epdf_viewer_host::translate::{Decode, translate};
use serde_json::json;

#[test]
fn metadata_read_becomes_its_wire_kind() {
    let call = translate("d1", "metadata.read", None, &[], 7).unwrap();
    assert_eq!(call.request, json!({"kind": "metadata.read", "jobId": 7, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Field("metadata")));
}

#[test]
fn a_page_chain_carries_its_page() {
    let call = translate("d1", "page.render.image", Some(12), &[json!({"scale": 2})], 8).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.render", "jobId": 8, "docId": "d1",
            "page": {"kind": "objectNumber", "pageObjectNumber": 12},
            "options": {"scale": 2}
        })
    );
    assert!(matches!(call.decode, Decode::Raster));
}

#[test]
fn a_page_chain_without_a_page_is_invalid_input() {
    let error = translate("d1", "page.text.read", None, &[], 9).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_unmapped_path_is_unsupported_not_a_panic() {
    let error = translate("d1", "signatures.prepare", None, &[], 10).err().unwrap();
    assert_eq!(error.code(), Some("unsupported"));
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL to compile with "unresolved import `epdf_viewer_host::translate`".

- [ ] **Step 4: Implement `translate.rs`**

```rust
//! Kernel engine calls as host wire requests, and back.
//!
//! The kernel speaks member chains (`crates/epdf-kernel/src/engine.rs`); the
//! host speaks wire kinds (`crates/epdf-engine/src/host.rs`). On the web the
//! two hops are `engine-bridge.ts` and `@embedpdf/engine`'s worker protocol;
//! here they are this one table.

use epdf_engine::host::HostResponse;
use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::host_table::HostTable;

pub struct WireCall {
    pub request: Value,
    pub payloads: Vec<Vec<u8>>,
    pub decode: Decode,
}

pub enum Decode {
    Field(&'static str),
    Raster,
    Unit,
}

fn coded(code: &str, message: String) -> Error {
    Error::new(message).with("code", code)
}

pub fn translate(
    doc: &str,
    path: &str,
    pon: Option<u32>,
    args: &[Value],
    job: u64,
) -> Result<WireCall, Error> {
    let page = || {
        pon.map(|n| json!({ "kind": "objectNumber", "pageObjectNumber": n }))
            .ok_or_else(|| coded("invalid-input", format!("[epdf-viewer-host] {path} needs a page")))
    };
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let (kind, extra, decode): (&str, Vec<(&str, Value)>, Decode) = match path {
        "close" => ("close", vec![], Decode::Unit),
        "pages.list" => ("pages.list", vec![], Decode::Field("snapshot")),
        "metadata.read" => ("metadata.read", vec![], Decode::Field("metadata")),
        "metadata.update" => ("metadata.update", vec![("patch", arg(0))], Decode::Field("metadata")),
        "page.text.read" => ("pages.text", vec![("page", page()?)], Decode::Field("snapshot")),
        "page.geometry.read" => ("pages.geometry", vec![("page", page()?)], Decode::Field("snapshot")),
        "page.render.image" => {
            ("pages.render", vec![("page", page()?), ("options", arg(0))], Decode::Raster)
        }
        "search.query" => ("search.query", vec![("request", arg(0))], Decode::Field("slice")),
        other => {
            return Err(coded(
                "unsupported",
                format!("[epdf-viewer-host] the native engine has no \"{other}\" yet"),
            ));
        }
    };
    let mut request = Map::new();
    request.insert("kind".into(), kind.into());
    request.insert("jobId".into(), job.into());
    request.insert("docId".into(), doc.into());
    for (key, value) in extra {
        if !value.is_null() {
            request.insert(key.into(), value);
        }
    }
    Ok(WireCall { request: Value::Object(request), payloads: Vec::new(), decode })
}

pub fn decode(decode: &Decode, response: HostResponse) -> Result<Value, Error> {
    let envelope: Value = serde_json::from_str(&response.json)
        .map_err(|e| Error::new(format!("[epdf-viewer-host] unreadable response: {e}")))?;
    if let Some(error) = envelope.get("error") {
        let code = error.get("code").and_then(Value::as_str).unwrap_or("Unknown");
        let message = error.get("message").and_then(Value::as_str).unwrap_or_default();
        return Err(Error::engine(code, format!("{code}: {message}")));
    }
    let result = envelope.get("result").cloned().unwrap_or(Value::Null);
    match decode {
        Decode::Unit => Ok(epdf_kernel::undefined()),
        Decode::Field(name) => Ok(result.get(*name).cloned().unwrap_or(result)),
        Decode::Raster => {
            let mut raster = result.get("raster").cloned().unwrap_or(result);
            let bytes = response.payloads.into_iter().next().unwrap_or_default();
            let id = HostTable::put(bytes);
            if let Value::Object(map) = &mut raster {
                map.insert("$host".into(), id.into());
            }
            Ok(raster)
        }
    }
}
```

- [ ] **Step 5: Run the translation tests**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: PASS, 4 tests.

- [ ] **Step 6: Implement `host_table.rs`, `engine_worker.rs` and `native_engine.rs`**

`host_table.rs`:
```rust
//! Bytes that must not cross as base64: a raster's pixels, a PDF being
//! opened. Kernel-thread local, like the runtime.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    static TABLE: RefCell<(u64, HashMap<u64, Arc<Vec<u8>>>)> = RefCell::new((0, HashMap::new()));
}

pub struct HostTable;

impl HostTable {
    pub fn put(bytes: Vec<u8>) -> u64 {
        TABLE.with(|t| {
            let mut t = t.borrow_mut();
            t.0 += 1;
            let id = t.0;
            t.1.insert(id, Arc::new(bytes));
            id
        })
    }

    pub fn get(id: u64) -> Option<Arc<Vec<u8>>> {
        TABLE.with(|t| t.borrow().1.get(&id).cloned())
    }

    pub fn take(id: u64) -> Option<Vec<u8>> {
        TABLE
            .with(|t| t.borrow_mut().1.remove(&id))
            .map(|a| Arc::try_unwrap(a).unwrap_or_else(|a| (*a).clone()))
    }

    pub fn release(id: u64) {
        TABLE.with(|t| t.borrow_mut().1.remove(&id));
    }
}
```

`engine_worker.rs`:
```rust
//! The engine's own thread. PDFium never leaves `OwnerThread`; this client
//! thread queues calls to it so the kernel thread never blocks on a render.
//! A PDFium call cannot be interrupted, so a kernel abort only drops the
//! reply.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use epdf_engine::dispatch::OwnerThread;
use epdf_engine::host::{Host, HostResponse};

type Reply = Box<dyn FnOnce(HostResponse) + Send>;

struct Request {
    json: String,
    payloads: Vec<Vec<u8>>,
    reply: Reply,
}

#[derive(Clone)]
pub struct EngineWorker {
    tx: Arc<Mutex<Option<Sender<Request>>>>,
}

impl EngineWorker {
    pub fn spawn() -> EngineWorker {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("epdf-engine-client".into())
            .spawn(move || {
                let mut owner = OwnerThread::start(Host::new);
                for Request { json, payloads, reply } in rx {
                    let response = owner
                        .run(move |host: &mut Host| host.call(&json, payloads))
                        .unwrap_or_else(|error| HostResponse {
                            json: serde_json::json!({ "error": error }).to_string(),
                            payloads: Vec::new(),
                        });
                    reply(response);
                }
                owner.close();
            })
            .expect("the engine client thread");
        EngineWorker { tx: Arc::new(Mutex::new(Some(tx))) }
    }

    pub fn submit(&self, json: String, payloads: Vec<Vec<u8>>, reply: Reply) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(Request { json, payloads, reply });
        }
    }

    pub fn shutdown(&self) {
        self.tx.lock().unwrap().take();
    }
}
```

Match `OwnerThread::run`'s closure signature at `dispatch.rs:82` exactly. `EngineError` must be `Serialize` for the error arm; `host.rs` `Envelope` already serializes it.

`native_engine.rs`:
```rust
//! The kernel's `Engine` over `EngineWorker`. Lives on the kernel thread.
//! An engine call becomes a pending `Promise`; the worker's reply comes back
//! as a kernel job that settles it from `PENDING`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_ffi::KernelPost;
use epdf_kernel::rt::Resolver;
use epdf_kernel::{Error, Promise};
use serde_json::Value;

use crate::engine_worker::EngineWorker;
use crate::translate::{self, Decode, WireCall};

thread_local! {
    static PENDING: RefCell<(u64, HashMap<u64, (Decode, Resolver<Value>)>)> =
        RefCell::new((0, HashMap::new()));
}

pub struct NativeEngine {
    inner: Rc<Inner>,
}

struct Inner {
    worker: EngineWorker,
    post: KernelPost,
}

impl NativeEngine {
    pub fn new(worker: EngineWorker, post: KernelPost) -> NativeEngine {
        NativeEngine { inner: Rc::new(Inner { worker, post }) }
    }
}

impl Inner {
    fn send(&self, call: WireCall) -> Promise<Value> {
        let (promise, resolver) = Promise::pending();
        let id = PENDING.with(|p| {
            let mut p = p.borrow_mut();
            p.0 += 1;
            let id = p.0;
            p.1.insert(id, (call.decode, resolver));
            id
        });
        let post = self.post.clone();
        self.worker.submit(
            call.request.to_string(),
            call.payloads,
            Box::new(move |response| {
                post(Box::new(move || settle(id, response)));
            }),
        );
        promise.with_abort(move |_| {
            PENDING.with(|p| p.borrow_mut().1.remove(&id));
        })
    }
}

fn settle(id: u64, response: epdf_engine::host::HostResponse) {
    let Some((decode, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) else {
        return; // aborted; the late reply is dropped
    };
    match translate::decode(&decode, response) {
        Ok(value) => resolver.resolve(value),
        Err(error) => resolver.reject(error),
    }
}
```

The `job` argument to `translate` reuses the `PENDING` id. Reserve it before calling `translate`, or have `send` patch `jobId` into the request.

`Engine::open(input, options)` reads the bytes as `input["$host"]` (Task 3's `open_document` puts them in the table). It takes them out with `HostTable::take` and sends `open.fatMem` with payload 0 = bytes and `password` from `options["password"]`. The document id is a fresh `doc-<n>` chosen here and sent as `docId`. Before the open promise resolves, it loads `pages.list` once and caches the snapshot on the document. Then it resolves to an `Rc<NativeDocument>`.

`NativeDocument` implements `DocumentHandle`:
- `id()` returns the id.
- `call(path, pon, args)` runs `translate`, then `send`.
- `page_index(pon)` reads the cached `pages.list` snapshot.
- `capabilities()` returns `{}`.
- `has(member)` answers whether `translate` maps it.
- `subscribe` returns an inert `Unsubscribe`. The native engine emits no events yet.
- `security()` returns a `NativeSecurity` that allows everything and has an empty identity. Enforcing document permissions is follow-on plan B4. The README says so.

`Engine::destroy()` resolves after `worker.shutdown()`.

- [ ] **Step 7: Add an integration test that opens a real PDF through the engine trait**

Append to `tests/translate.rs`:
```rust
use epdf_ffi::KernelThread;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::host_table::HostTable;
use epdf_viewer_host::native_engine::NativeEngine;

#[test]
fn the_native_engine_opens_a_pdf_and_lists_its_pages() {
    let bytes = test_support::fixture("report");
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(EngineWorker::spawn(), post))).unwrap();
    let pages = futures::executor::block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = futures::channel::oneshot::channel();
        let id = HostTable::put(bytes);
        engine.open(serde_json::json!({ "$host": id }), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.call("pages.list", None, vec![]).on_settle(move |list| {
                let _ = tx.send(list.as_ref().unwrap().clone());
            });
        });
        rx
    }))
    .unwrap();
    let list = futures::executor::block_on(pages).unwrap();
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
}
```

`report` has 14 pages (`SmokeTests.swift`). Look up the actual name of `test-support`'s fixture loader and the `PageListSnapshot` field name (`worker-protocol.ts`) and use those in place of `fixture` and `"pages"`.

- [ ] **Step 8: Run all crate tests and commit**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, 5 tests.

```bash
git add Cargo.toml crates/epdf-viewer-host
git commit -m "Run the kernel's engine over PDFium in epdf-viewer-host"
```

---

### Task 3: `ViewerHost`, documents and lifecycle

**Files:**
- Create: `crates/epdf-viewer-host/src/session.rs`
- Modify: `crates/epdf-viewer-host/src/lib.rs`
- Test: `crates/epdf-viewer-host/tests/viewer_host.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2. `epdf_kernel::{Kernel, KernelOptions, Documents, OpenSource, OpenOptions}` (`kernel.rs:113,1123,1445`).
- Produces:
  ```rust
  #[derive(uniffi::Record, Default)]
  pub struct ViewerConfig {}                               // plugin fields arrive in Tasks 6-8
  #[derive(uniffi::Record)] pub struct OpenDocumentOptions { #[uniffi(default = None)] pub name: Option<String>, #[uniffi(default = None)] pub password: Option<String>, #[uniffi(default = true)] pub activate: bool }
  #[derive(uniffi::Record)] pub struct DocumentInfo { pub id: String, pub name: Option<String>, pub page_count: u32 }
  #[derive(uniffi::Record)] pub struct PageInfo { pub page: epdf_ffi::PageRef, pub index: u32, pub width: f64, pub height: f64, pub rotation: u32 }
  #[derive(uniffi::Enum)] pub enum DocumentEvent { Opened { document: DocumentInfo }, OpenFailed { document_id: String, error: String }, Locked { document_id: String }, Closed { document_id: String }, ActiveChanged { document_id: Option<String> }, PagesChanged { document_id: String } }
  #[uniffi::export(with_foreign)] pub trait DocumentEventListener: Send + Sync { fn on_event(&self, event: DocumentEvent); }
  #[uniffi::export(with_foreign)] pub trait ViewerDelegate: Send + Sync { fn state_changed(&self, revision: u64); fn report(&self, error: epdf_ffi::ViewerError); }
  #[derive(uniffi::Object)] pub struct ViewerHost;
  #[uniffi::export] impl ViewerHost {
      #[uniffi::constructor] pub fn new(config: ViewerConfig, delegate: Arc<dyn ViewerDelegate>) -> Result<Arc<Self>, ViewerError>;
      pub async fn start(&self) -> Result<(), ViewerError>;
      pub async fn destroy(&self) -> Result<(), ViewerError>;
      pub async fn open_document(&self, bytes: Vec<u8>, options: OpenDocumentOptions) -> Result<DocumentInfo, ViewerError>;
      pub async fn unlock_document(&self, document_id: String, password: String) -> Result<(), ViewerError>;
      pub async fn close_document(&self, document_id: String) -> Result<(), ViewerError>;
      pub async fn documents(&self) -> Result<Vec<DocumentInfo>, ViewerError>;
      pub async fn pages(&self, document_id: String) -> Result<Vec<PageInfo>, ViewerError>;
      pub fn subscribe_documents(&self, listener: Arc<dyn DocumentEventListener>) -> Arc<Subscription>;
      pub fn state_revision(&self) -> u64;
      pub fn shutdown(&self);
  }
  ```

`ViewerConfig` gets one field per plugin as each plugin lands. Plugins with no config are a `bool` (`#[uniffi(default = false)]`). Plugins with config are an `Option<XConfig>` (`#[uniffi(default = None)]`). Swift sees an initializer with every argument defaulted.

`Session` is built on the kernel thread. It holds the `Kernel`, an `EngineWorker` clone (its `Drop` calls `worker.shutdown()`), the delegate, and the kernel subscription that calls `delegate.state_changed(kernel.state_revision())` on every store change. `ViewerHost` holds `Mutex<Option<KernelThread<Session>>>`, an `AtomicU64` mirror of the state revision (so `state_revision()` never posts a job), and a `KernelPost`. `KernelOptions.report` forwards to `delegate.report`.

The `DocumentEvent` payloads come from `LifecycleEvents` (`kernel.rs:222`), which emits `Value`. `subscribe_documents` decodes each event's fields on the kernel thread. Check the field names a lifecycle event carries in `Documents::open` and the matching `emit` calls, and make the decoding match them.

- [ ] **Step 1: Write the failing tests**

`crates/epdf-viewer-host/tests/viewer_host.rs`:
```rust
use std::sync::{Arc, Mutex};

use epdf_ffi::ViewerError;
use epdf_viewer_host::{
    DocumentEvent, DocumentEventListener, OpenDocumentOptions, ViewerConfig, ViewerDelegate, ViewerHost,
};
use futures::executor::block_on;

#[derive(Default)]
struct Delegate;
impl ViewerDelegate for Delegate {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, _error: ViewerError) {}
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl DocumentEventListener for Events {
    fn on_event(&self, event: DocumentEvent) {
        self.0.lock().unwrap().push(format!("{event:?}"));
    }
}

fn options() -> OpenDocumentOptions {
    OpenDocumentOptions { name: None, password: None, activate: true }
}

fn started() -> Arc<ViewerHost> {
    let host = ViewerHost::new(ViewerConfig::default(), Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_document_opens_and_lists_its_pages() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture("report"), options())).unwrap();
    assert_eq!(doc.page_count, 14);
    assert_eq!(block_on(host.pages(doc.id)).unwrap().len(), 14);
}

#[test]
fn a_locked_document_reports_that_it_needs_a_password() {
    let host = started();
    let error = block_on(host.open_document(test_support::fixture("encrypted-hello"), options())).unwrap_err();
    assert!(format!("{error}").contains("DocPasswordRequired"), "{error}");
}

#[test]
fn opening_and_closing_are_observed() {
    let host = started();
    let events = Arc::new(Events::default());
    let _subscription = host.subscribe_documents(events.clone());
    let doc = block_on(host.open_document(test_support::fixture("report"), options())).unwrap();
    block_on(host.close_document(doc.id)).unwrap();
    let seen = events.0.lock().unwrap().join("\n");
    assert!(seen.contains("Opened"), "{seen}");
    assert!(seen.contains("Closed"), "{seen}");
}

#[test]
fn a_call_after_shutdown_is_closed() {
    let host = started();
    host.shutdown();
    assert!(matches!(block_on(host.documents()), Err(ViewerError::Closed)));
}

#[test]
fn dropping_the_host_joins_both_threads() {
    let host = started();
    let weak = Arc::downgrade(&host);
    drop(host);
    assert!(weak.upgrade().is_none());
    // KernelThread::drop joins the kernel thread; Session::drop shuts the
    // EngineWorker down, which ends the client thread and closes OwnerThread.
    // A hang here is the failure.
}
```

The locked-document test assumes the kernel's open rejects with the engine's code. It might instead resolve and emit `Locked`. Check `Documents::open` (`kernel.rs:1445` onward). If the kernel parks a locked document and doesn't reject, `open_document` should still return an error carrying code `DocPasswordRequired` and the document id, so Swift can call `unlock_document`. Make the Rust side do that, and keep the test as written.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host`
Expected: FAIL to compile, with unresolved imports from `epdf_viewer_host`.

- [ ] **Step 3: Implement `session.rs` and the exports**

Every async export follows one pattern:
```rust
pub async fn pages(&self, document_id: String) -> Result<Vec<PageInfo>, ViewerError> {
    self.call_promise(
        move |s: &mut Session| s.kernel.documents().list_pages(&document_id),
        |pages: serde_json::Value| decode_pages(&pages),
    )
    .await?
}
```

`ViewerHost::call` and `call_promise` do for `&mut Session` what `KernelBound::call` and `call_promise` do for `&C`, including abort-on-drop. Write them on top of `KernelThread::call`. Check `list_pages`'s exact signature and return type in `Documents` (`kernel.rs:1445-1745`).

`open_document(bytes, options)` puts `bytes` into `HostTable` on the kernel thread. It then calls `kernel.documents().open(OpenSource::Input(json!({"$host": id})), OpenOptions { activate, name, engine })`, where `engine` carries `{"password": password}`. Check that `OpenOptions.engine` reaches `Engine::open`'s `options` unchanged, and adjust Task 2's `NativeEngine::open` to read the password wherever the kernel actually puts it.

`subscribe_documents(listener)` creates a `Subscription::pending(post)`. It posts a job that registers one handler per `LifecycleEvents` hook, combines the six `Unsubscribe`s into one, and calls `slot.fill`. It returns the `Subscription` immediately.

`impl Drop for ViewerHost` calls `shutdown()`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS, 10 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host
git commit -m "Export ViewerHost: documents and lifecycle"
```

---

### Task 4: Package the XCFramework and track `Package.swift`

**Files:**
- Modify: `platforms/apple/build-xcframework.sh`, `platforms/apple/.gitignore`, `platforms/apple/README.md`
- Create: `platforms/apple/EmbedPDF/Package.swift`
- Move: `Tests/EmbedPDFSmokeTests/SmokeTests.swift` to `Tests/EmbedPDFBindingsSmokeTests/SmokeTests.swift`
- Create: `Sources/EmbedPDF/EmbedPDF.swift`, `Sources/EmbedPDFUI/EmbedPDFUI.swift` (empty), `Tests/EmbedPDFTests/BaseSuite.swift`, `Tests/EmbedPDFUITests/SearchModelTests.swift`

(Paths in this and later Swift tasks are relative to `platforms/apple/EmbedPDF/`, except the script.)

**Interfaces:**
- Consumes: `libepdf_viewer_host.a`, which carries the `epdf_uniffi`, `epdf_ffi` and `epdf_viewer_host` exports now, and every plugin's `ffi` module as Tasks 6 to 8 add them.
- Produces: Swift module `EmbedPDFBindings`, generated and public, containing `ViewerHost`, `ViewerConfig`, `ViewerError`, `Subscription`, `PageRef`, `DocumentInfo`, `PdfEngine`, and so on. Also empty `EmbedPDF` and `EmbedPDFUI` modules.

- [ ] **Step 1: Change the script**

In `platforms/apple/build-xcframework.sh`:

1. Build `cargo build -p epdf-viewer-host` instead of `-p epdf-uniffi`. In the `libtool` line, merge `libepdf_viewer_host.a` with `libembedpdf.a`.
2. Run bindgen with `--library target/aarch64-apple-ios-sim/$PROFILE/libepdf_viewer_host.a`. UniFFI 0.32's library mode reads metadata from a static archive and emits Swift for every crate in it that has scaffolding. This removes the dylib fallback that only worked when a stale host dylib happened to exist.
3. Library mode writes one `*FFI.h` and one `*FFI.modulemap` per crate. The old `cp *.modulemap headers/module.modulemap` keeps only one. Replace it with:
   ```bash
   cat "$BUILD/generated"/*.modulemap > "$BUILD/headers/module.modulemap"
   cp "$BUILD/generated"/*.h "$BUILD/headers/"
   ```
4. Write generated Swift to `EmbedPDF/Sources/EmbedPDFBindings/`, after deleting the `*.swift` already there.
5. Copy fixtures into `Tests/EmbedPDFBindingsSmokeTests/Fixtures/`, `Tests/EmbedPDFTests/Fixtures/` and `Tests/EmbedPDFUITests/Fixtures/`.
6. Delete the `Package.swift` heredoc. The file is tracked from now on.

- [ ] **Step 2: Update `.gitignore`**

```
.build/
EmbedPDF/EmbedPDFFFI.xcframework/
EmbedPDF/Sources/EmbedPDFBindings/
EmbedPDF/Tests/*/Fixtures/
```

- [ ] **Step 3: Write `Package.swift`**

```swift
// swift-tools-version: 6.2

import PackageDescription

let package = Package(
  name: "EmbedPDF",
  platforms: [
    .iOS(.v17),
  ],
  products: [
    .library(name: "EmbedPDF", targets: ["EmbedPDF"]),
    .library(name: "EmbedPDFUI", targets: ["EmbedPDFUI"]),
  ],
  dependencies: [
    .package(url: "https://github.com/pointfreeco/swift-custom-dump", from: "1.3.0"),
  ],
  targets: [
    .binaryTarget(name: "EmbedPDFFFI", path: "EmbedPDFFFI.xcframework"),
    .target(
      name: "EmbedPDFBindings",
      dependencies: [
        "EmbedPDFFFI",
      ],
      swiftSettings: [
        .swiftLanguageMode(.v5),
      ],
      linkerSettings: [
        .linkedLibrary("c++"),
        .linkedFramework("CoreFoundation"),
        .linkedFramework("CoreGraphics"),
      ]
    ),
    .target(
      name: "EmbedPDF",
      dependencies: [
        "EmbedPDFBindings",
      ]
    ),
    .target(
      name: "EmbedPDFUI",
      dependencies: [
        "EmbedPDF",
      ]
    ),
    .testTarget(
      name: "EmbedPDFBindingsSmokeTests",
      dependencies: [
        "EmbedPDFBindings",
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
    .testTarget(
      name: "EmbedPDFTests",
      dependencies: [
        "EmbedPDF",
        .product(name: "CustomDump", package: "swift-custom-dump"),
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
    .testTarget(
      name: "EmbedPDFUITests",
      dependencies: [
        "EmbedPDFUI",
        .product(name: "CustomDump", package: "swift-custom-dump"),
      ],
      resources: [
        .copy("Fixtures"),
      ]
    ),
  ]
)
```

`EmbedPDFBindings` compiles in Swift 5 mode. Generated code isn't ours to make strict-concurrency clean. The handwritten targets use Swift 6 mode, the default for this tools version. The two `linkedFramework` lines are the ones `build-support` emits for iOS cargo links (`apple_frameworks`, `build-support/src/lib.rs:293`). The old heredoc left them out and got away with it, because the bare engine path never called CoreGraphics at link time.

- [ ] **Step 4: Move the smoke tests, add the base suite**

Move `SmokeTests.swift` and change its import to `@testable import EmbedPDFBindings`. Nothing else in it changes.

`Tests/EmbedPDFTests/BaseSuite.swift`:
```swift
import Foundation
import Testing

@Suite(.serialized, .timeLimit(.minutes(1)))
struct BaseSuite {}

enum Fixture {
  static func data(_ name: String) throws -> Data {
    let url = try #require(Bundle.module.url(forResource: name, withExtension: "pdf", subdirectory: "Fixtures"))
    return try Data(contentsOf: url)
  }
}
```

The suite is serialized because each test starts a viewer, and every viewer holds two threads and a PDFium instance. Running them one at a time means a failure points at one viewer.

`Tests/EmbedPDFUITests/SearchModelTests.swift`:
```swift
import Testing

@Suite struct SearchModelTests {}
```

- [ ] **Step 5: Check the generated records' conformances**

Open `Sources/EmbedPDFBindings/epdf_viewer_host.swift`. Records need `Sendable`, `Equatable` and `Hashable` so the facade can re-export them. Check how UniFFI 0.32 declares `DocumentInfo` and `PageRef`. If `Sendable` is missing, the facade adds `extension DocumentInfo: @retroactive @unchecked Sendable {}`, one line per record, in `Exports.swift`. Also check that cancelling a Swift `Task` drops the Rust future. Look for `rust_future_cancel` inside a `withTaskCancellationHandler` in the generated `uniffiRustCallAsync`. Record what you find in `platforms/apple/README.md`. Task 7's cancellation test depends on it, and its fallback is written there.

- [ ] **Step 6: Build and run the smoke tests**

Run:
```bash
bash platforms/apple/build-xcframework.sh
cd platforms/apple/EmbedPDF
xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFBindingsSmokeTests
```
Expected: `** TEST SUCCEEDED **`, 8 tests, 0 failures. That's the count `platforms/apple/README.md` recorded on 2026-09-23.

- [ ] **Step 7: Update the README and commit**

In `platforms/apple/README.md`, replace the build and test commands with the ones above, name the three products, record the Step 5 findings, and correct `Engine(code:message:)` to `Engine(code:detail:)`.

```bash
git add platforms/apple
git commit -m "Package epdf-viewer-host as the Swift package's binary"
```

---

### Task 5: The `EmbedPDF` core

**Files:**
- Delete: `Sources/EmbedPDF/EmbedPDF.swift`
- Create: `Sources/EmbedPDF/Exports.swift`, `PDFError.swift`, `EventRelay.swift`, `PDFViewer.swift`
- Test: `Tests/EmbedPDFTests/ViewerTests.swift`

**Interfaces:**
- Consumes: `EmbedPDFBindings.ViewerHost`, `ViewerConfig`, `ViewerDelegate`, `ViewerError`, `DocumentInfo`, `PageInfo`, `PageRef`, `DocumentEvent`, `DocumentEventListener`, `Subscription` (Task 3), as UniFFI 0.32 spells them: `ViewerHost(config:delegate:)`, `openDocument(bytes:options:)`, `subscribeDocuments(listener:)`.
- Produces:
  ```swift
  public typealias ViewerConfig = EmbedPDFBindings.ViewerConfig
  public typealias PDFDocument = EmbedPDFBindings.DocumentInfo
  public typealias PageInfo = EmbedPDFBindings.PageInfo
  public typealias PageRef = EmbedPDFBindings.PageRef
  public typealias DocumentEvent = EmbedPDFBindings.DocumentEvent
  public struct PDFError: Error, Sendable, Equatable { public let name: String; public let code: String?; public let detail: String }
  final class EventRelay<Event: Sendable>: @unchecked Sendable { init(_ continuation: AsyncStream<Event>.Continuation) }
  public final class PDFViewer: Sendable {
    public init(_ config: ViewerConfig) throws(PDFError)
    public func start() async throws(PDFError)
    public func destroy() async throws(PDFError)
    public func open(_ data: Data, name: String? = nil, password: String? = nil) async throws(PDFError) -> PDFDocument
    public func unlock(_ document: PDFDocument, password: String) async throws(PDFError)
    public func close(_ document: PDFDocument) async throws(PDFError)
    public func documents() async throws(PDFError) -> [PDFDocument]
    public func pages(of document: PDFDocument) async throws(PDFError) -> [PageInfo]
    public var documentEvents: AsyncStream<DocumentEvent> { get }
    public var stateRevisions: AsyncStream<UInt64> { get }
  }
  ```

Swift has no selective re-export that exposes some of a module's types and hides the rest. `Exports.swift` uses one `public typealias` per record, and every other file uses `internal import EmbedPDFBindings`, so the generated objects stay out of the facade's API.

`EventRelay` is the one class every plugin's listener protocol is conformed onto. Each plugin adds a single line in its own file, for example `extension EventRelay: SearchEventListener where Event == SearchEvent { func onEvent(event: SearchEvent) { continuation.yield(event) } }`. Listener calls arrive on the kernel thread. A relay only yields into an `AsyncStream.Continuation`, which never blocks, so the kernel thread never waits on the main actor.

- [ ] **Step 1: Write the failing tests**

`Tests/EmbedPDFTests/ViewerTests.swift`:
```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct ViewerTests {
    func started() async throws -> PDFViewer {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      return viewer
    }

    @Test func aDocumentOpensWithItsPages() async throws {
      let viewer = try await started()
      let document = try await viewer.open(Fixture.data("report"), name: "report.pdf")
      expectNoDifference(document.pageCount, 14)
      expectNoDifference(document.name, "report.pdf")
      expectNoDifference(try await viewer.pages(of: document).count, 14)
    }

    @Test func aLockedDocumentReportsThatItNeedsAPassword() async throws {
      let viewer = try await started()
      await #expect {
        _ = try await viewer.open(Fixture.data("encrypted-hello"))
      } throws: { error in
        (error as? PDFError)?.detail.contains("DocPasswordRequired") == true
      }
    }

    @Test func openingAndClosingAreObserved() async throws {
      let viewer = try await started()
      let events = viewer.documentEvents
      let document = try await viewer.open(Fixture.data("report"))
      try await viewer.close(document)
      var seen: [String] = []
      for await event in events {
        switch event {
        case .opened: seen.append("opened")
        case .closed: seen.append("closed")
        default: continue
        }
        if seen.count == 2 { break }
      }
      expectNoDifference(seen, ["opened", "closed"])
    }

    @Test func aCallAfterDestroyIsClosed() async throws {
      let viewer = try await started()
      try await viewer.destroy()
      await #expect {
        _ = try await viewer.documents()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests`
Expected: FAIL to compile with "cannot find 'PDFViewer' in scope".

- [ ] **Step 3: Implement**

`Sources/EmbedPDF/Exports.swift`:
```swift
internal import EmbedPDFBindings

public typealias ViewerConfig = EmbedPDFBindings.ViewerConfig
public typealias PDFDocument = EmbedPDFBindings.DocumentInfo
public typealias PageInfo = EmbedPDFBindings.PageInfo
public typealias PageRef = EmbedPDFBindings.PageRef
public typealias Point = EmbedPDFBindings.Point
public typealias Size = EmbedPDFBindings.Size
public typealias Rect = EmbedPDFBindings.Rect
public typealias DocumentEvent = EmbedPDFBindings.DocumentEvent
```

A `public typealias` to a type from an `internal import` doesn't compile: Swift requires the aliased type's module to be imported publicly. If that error shows up, change `Exports.swift` to `public import EmbedPDFBindings` and keep `internal import` in every other file. Generated objects (`ViewerHost`, `SearchHandle`) then become visible to clients that look for them. We accept that and document it in the README: the supported API is `PDFViewer` and the capabilities.

`Sources/EmbedPDF/PDFError.swift`:
```swift
internal import EmbedPDFBindings

public struct PDFError: Error, Sendable, Equatable {
  public let name: String
  public let code: String?
  public let detail: String

  init(_ error: ViewerError) {
    switch error {
    case let .Kernel(name, code, detail):
      self.name = name
      self.code = code
      self.detail = detail
    case .Closed:
      name = "Closed"
      code = "instance-closed"
      detail = "The viewer has shut down."
    }
  }

  init(_ error: any Error) {
    if let error = error as? ViewerError {
      self.init(error)
    } else if let error = error as? PDFError {
      self = error
    } else {
      name = "Error"
      code = nil
      detail = "\(error)"
    }
  }
}

func bridged<T>(_ body: () async throws -> T) async throws(PDFError) -> T {
  do {
    return try await body()
  } catch {
    throw PDFError(error)
  }
}
```

`Sources/EmbedPDF/EventRelay.swift`:
```swift
internal import EmbedPDFBindings

final class EventRelay<Event: Sendable>: @unchecked Sendable {
  let continuation: AsyncStream<Event>.Continuation

  init(_ continuation: AsyncStream<Event>.Continuation) {
    self.continuation = continuation
  }

  static func stream(subscribe: (EventRelay<Event>) -> Subscription) -> AsyncStream<Event> {
    let (stream, continuation) = AsyncStream<Event>.makeStream()
    let subscription = subscribe(EventRelay(continuation))
    continuation.onTermination = { _ in subscription.cancel() }
    return stream
  }
}

extension EventRelay: DocumentEventListener where Event == DocumentEvent {
  func onEvent(event: DocumentEvent) {
    continuation.yield(event)
  }
}

final class StateRelay: ViewerDelegate, @unchecked Sendable {
  let continuation: AsyncStream<UInt64>.Continuation

  init(_ continuation: AsyncStream<UInt64>.Continuation) {
    self.continuation = continuation
  }

  func stateChanged(revision: UInt64) {
    continuation.yield(revision)
  }

  func report(error: ViewerError) {}
}
```

`report` is a no-op here; forwarding kernel reports to `os.Logger(subsystem: "EmbedPDF", category: "kernel")` is a one-line follow-up.

`Sources/EmbedPDF/PDFViewer.swift`:
```swift
internal import EmbedPDFBindings
import Foundation

public final class PDFViewer: Sendable {
  let host: ViewerHost
  public let stateRevisions: AsyncStream<UInt64>

  public init(_ config: ViewerConfig) throws(PDFError) {
    let (stream, continuation) = AsyncStream<UInt64>.makeStream(bufferingPolicy: .bufferingNewest(1))
    do {
      host = try ViewerHost(config: config, delegate: StateRelay(continuation))
    } catch {
      throw PDFError(error)
    }
    stateRevisions = stream
  }

  public func start() async throws(PDFError) {
    try await bridged { try await host.start() }
  }

  public func destroy() async throws(PDFError) {
    try await bridged { try await host.destroy() }
  }

  public func open(_ data: Data, name: String? = nil, password: String? = nil) async throws(PDFError) -> PDFDocument {
    try await bridged {
      try await host.openDocument(bytes: data, options: OpenDocumentOptions(name: name, password: password, activate: true))
    }
  }

  public func unlock(_ document: PDFDocument, password: String) async throws(PDFError) {
    try await bridged { try await host.unlockDocument(documentId: document.id, password: password) }
  }

  public func close(_ document: PDFDocument) async throws(PDFError) {
    try await bridged { try await host.closeDocument(documentId: document.id) }
  }

  public func documents() async throws(PDFError) -> [PDFDocument] {
    try await bridged { try await host.documents() }
  }

  public func pages(of document: PDFDocument) async throws(PDFError) -> [PageInfo] {
    try await bridged { try await host.pages(documentId: document.id) }
  }

  public var documentEvents: AsyncStream<DocumentEvent> {
    EventRelay.stream { host.subscribeDocuments(listener: $0) }
  }

  deinit {
    host.shutdown()
  }
}
```

UniFFI 0.32 maps `Vec<u8>` to `Data` in Swift. If the generated `openDocument` takes `[UInt8]` instead, pass `Array(data)`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests`
Expected: `** TEST SUCCEEDED **`, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add platforms/apple/EmbedPDF
git commit -m "Add EmbedPDF's viewer facade"
```

---

### Task 6: Plugin slice, `metadata` (the recipe)

This is the first plugin slice. It fixes the recipe every later plugin follows. Appendix A §A.0 repeats the recipe for implementers of the follow-on plans.

**Files:**
- Modify: `crates/plugins/metadata/Cargo.toml` (feature `uniffi`)
- Modify: `crates/plugins/metadata/src/lib.rs` (`#[cfg(feature = "uniffi")] pub mod ffi;`, plus `cfg_attr` derives on compatible types)
- Create: `crates/plugins/metadata/src/ffi.rs`
- Create: `crates/plugins/metadata/tests/ffi.rs`
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/session.rs`, `src/lib.rs` (config field, accessor)
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Metadata.swift`
- Test: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/MetadataTests.swift`

**Interfaces:**
- Consumes: `epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError}` (Task 1). `MetadataCapability` methods `get_snapshot`, `get_status`, `can_edit`, `update`, `refresh`, `on_updated` (`crates/plugins/metadata/src/controller.rs:39` onward, as `harness.rs` calls them).
- Produces (Rust, crate `epdf-plugin-metadata`, feature `uniffi`):
  ```rust
  #[derive(uniffi::Record)] pub struct DocumentMetadata { pub title: Option<String>, pub author: Option<String>, pub subject: Option<String>, pub keywords: Option<String>, pub creator: Option<String>, pub producer: Option<String>, pub creation_date: Option<String>, pub modification_date: Option<String> }
  #[derive(uniffi::Record)] pub struct MetadataPatch { /* same fields, all #[uniffi(default = None)] */ }
  #[derive(uniffi::Enum)] pub enum ResourceStatus { Idle, Loading, Ready, Error }
  #[derive(uniffi::Record)] pub struct MetadataUpdatedEvent { pub metadata: DocumentMetadata, pub previous: Option<DocumentMetadata>, pub changed_keys: Vec<String> }
  #[derive(uniffi::Enum)] pub enum MetadataEvent { Updated { event: MetadataUpdatedEvent } }
  #[uniffi::export(with_foreign)] pub trait MetadataEventListener: Send + Sync { fn on_event(&self, event: MetadataEvent); }
  #[derive(uniffi::Object)] pub struct MetadataHandle { cap: KernelBound<MetadataCapability>, post: KernelPost }
  impl MetadataHandle { pub fn new(cap: MetadataCapability, post: KernelPost) -> Arc<Self>; }   // kernel thread
  #[uniffi::export] impl MetadataHandle {
      pub async fn snapshot(&self) -> Result<Option<DocumentMetadata>, ViewerError>;
      pub async fn status(&self) -> Result<ResourceStatus, ViewerError>;
      pub async fn can_edit(&self) -> Result<bool, ViewerError>;
      pub async fn update(&self, patch: MetadataPatch) -> Result<DocumentMetadata, ViewerError>;
      pub async fn refresh(&self) -> Result<(), ViewerError>;
      pub fn subscribe(&self, listener: Arc<dyn MetadataEventListener>) -> Arc<Subscription>;
  }
  ```
- Produces (`epdf-viewer-host`): `ViewerConfig.metadata: bool` (`#[uniffi(default = false)]`) and `ViewerHost::metadata(&self, document_id: String) -> Result<Arc<MetadataHandle>, ViewerError>` (async).
- Produces (Swift):
  ```swift
  public typealias DocumentMetadata = EmbedPDFBindings.DocumentMetadata
  public typealias MetadataPatch = EmbedPDFBindings.MetadataPatch
  public typealias ResourceStatus = EmbedPDFBindings.ResourceStatus
  public typealias MetadataEvent = EmbedPDFBindings.MetadataEvent
  public struct MetadataCapability: Sendable {
    public func snapshot() async throws(PDFError) -> DocumentMetadata?
    public func status() async throws(PDFError) -> ResourceStatus
    public func canEdit() async throws(PDFError) -> Bool
    public func update(_ patch: MetadataPatch) async throws(PDFError) -> DocumentMetadata
    public func refresh() async throws(PDFError)
    public var events: AsyncStream<MetadataEvent> { get }
  }
  extension PDFViewer { public func metadata(for document: PDFDocument) async throws(PDFError) -> MetadataCapability }
  ```

Copy `DocumentMetadata`'s real fields from `crates/plugins/metadata/src/lib.rs` before writing the record. The list above is the TypeScript type and may differ by a field. If the Rust `DocumentMetadata` holds only `String`, `Option<String>` and `Vec<String>`, derive `uniffi::Record` on it in place with `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` and don't write a mirror. Do the same for `ResourceStatus` (`lib.rs:41`). `MetadataUpdatedEvent.origin` is a `Value` (`lib.rs:57`), so the event needs a mirror that drops `origin`. No Swift caller can act on it.

- [ ] **Step 1: Add the feature**

`crates/plugins/metadata/Cargo.toml`:
```toml
[features]
default = ["harness"]
harness = [...existing...]
uniffi = ["dep:uniffi", "dep:epdf-ffi"]

[dependencies]
epdf-ffi = { path = "../../epdf-ffi", optional = true }
uniffi = { version = "=0.32.0", optional = true }
```

Keep `default` exactly as it is now and add only the `uniffi` line. In `src/lib.rs`, add `#[cfg(feature = "uniffi")] uniffi::setup_scaffolding!();` and `#[cfg(feature = "uniffi")] pub mod ffi;`.

- [ ] **Step 2: Write the failing Rust tests**

`crates/plugins/metadata/tests/ffi.rs` drives `MetadataHandle` through the conformance `FakeEngine`, without PDFium. That's the reason the wrappers can live in the plugin crate:
```rust
#![cfg(feature = "uniffi")]

use std::sync::{Arc, Mutex};

use epdf_ffi::KernelThread;
use epdf_plugin_metadata::ffi::{MetadataEvent, MetadataEventListener, MetadataHandle, MetadataPatch, ResourceStatus};
use futures::executor::block_on;

struct Events(Mutex<Vec<MetadataEvent>>);
impl MetadataEventListener for Events {
    fn on_event(&self, event: MetadataEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn handle() -> (KernelThread<plugin_conformance::test_kernel::TestKernel>, Arc<MetadataHandle>) {
    let thread = KernelThread::spawn(|_| plugin_conformance::test_kernel::TestKernel::open(
        vec![epdf_plugin_metadata::metadata_plugin()],
        plugin_conformance::test_kernel::spec_with_metadata(serde_json::json!({ "title": "Report" })),
    )).unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        MetadataHandle::new(k.capability(&epdf_plugin_metadata::METADATA).unwrap(), post)
    })).unwrap();
    (thread, handle)
}

#[test]
fn refresh_then_snapshot_reads_the_title() {
    let (_thread, metadata) = handle();
    block_on(metadata.refresh()).unwrap();
    assert!(matches!(block_on(metadata.status()).unwrap(), ResourceStatus::Ready));
    assert_eq!(block_on(metadata.snapshot()).unwrap().and_then(|m| m.title), Some("Report".into()));
}

#[test]
fn an_update_is_observed() {
    let (_thread, metadata) = handle();
    let events = Arc::new(Events(Mutex::new(vec![])));
    let _sub = metadata.subscribe(events.clone());
    block_on(metadata.refresh()).unwrap();
    let patch = MetadataPatch { title: Some("Renamed".into()), ..Default::default() };
    assert_eq!(block_on(metadata.update(patch)).unwrap().title, Some("Renamed".into()));
    let seen = events.0.lock().unwrap();
    assert!(matches!(seen.last(), Some(MetadataEvent::Updated { event }) if event.changed_keys == ["title"]));
}
```

`plugin_conformance::test_kernel` doesn't exist yet. Add it to `crates/plugin-conformance` in this step, as a `pub mod`. It holds a `TestKernel { kernel: Kernel }` built over `FakeEngine::new(spec)` (`fake_engine.rs:304`) with one open document, plus `capability<C: Clone>(&Token<C>) -> Result<C, Error>` and a small spec builder for metadata. Every later plugin slice reuses it, so keep it generic. Add `plugin-conformance` as a dev-dependency of the plugin crate.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-metadata --features uniffi --test ffi`
Expected: FAIL to compile with "unresolved import `epdf_plugin_metadata::ffi`".

- [ ] **Step 4: Implement `ffi.rs`**

```rust
//! The metadata plugin through uniffi: typed records, one handle, one event
//! enum. Every method hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_ffi::{KernelBound, KernelPost, Subscription, ViewerError};

use crate::MetadataCapability;

// Records: derived in place where the type allowed it, mirrored here
// where it did not (see the module docs of `crate::model`).

#[derive(uniffi::Enum)]
pub enum MetadataEvent {
    Updated { event: MetadataUpdatedEvent },
}

#[uniffi::export(with_foreign)]
pub trait MetadataEventListener: Send + Sync {
    fn on_event(&self, event: MetadataEvent);
}

#[derive(uniffi::Object)]
pub struct MetadataHandle {
    cap: KernelBound<MetadataCapability>,
    post: KernelPost,
}

impl MetadataHandle {
    pub fn new(cap: MetadataCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(MetadataHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl MetadataHandle {
    pub async fn snapshot(&self) -> Result<Option<DocumentMetadata>, ViewerError> {
        self.cap.call(|c| Ok(c.get_snapshot().and_then(|v| serde_json::from_value(v).ok()))).await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|c| Ok(c.get_status().into())).await
    }

    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| c.can_edit()).await
    }

    pub async fn update(&self, patch: MetadataPatch) -> Result<DocumentMetadata, ViewerError> {
        let patch = serde_json::to_value(patch).unwrap_or_default();
        self.cap
            .call_promise(move |c| Ok(c.update(patch, None)), |v| serde_json::from_value(v).unwrap_or_default())
            .await
    }

    pub async fn refresh(&self) -> Result<(), ViewerError> {
        self.cap.call_promise(|c| Ok(c.refresh()), |_| ()).await
    }

    pub fn subscribe(&self, listener: Arc<dyn MetadataEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let off = c.on_updated(move |event| {
                listener.on_event(MetadataEvent::Updated { event: event.into() });
                Ok(())
            });
            slot.fill(off);
        });
        subscription
    }
}
```

This sketch converts through `serde_json` because `get_snapshot()` returns `Option<Value>` (`harness.rs` calls `.unwrap_or(Value::Null)` on it). Where a capability method returns a typed struct, convert with `From` instead. Decoding JSON inside Rust is a leftover from the controller's shape, not part of the FFI. When the records derive `serde::Deserialize` too, which they do once `uniffi` is derived on the same structs, `from_value` is exact. If it fails, the record doesn't match the JSON the controller produces. That's a bug, so return `Err(ViewerError::Kernel { code: Some("invalid-state"), .. })`, not a default.

- [ ] **Step 5: Run the Rust tests**

Run: `cargo test -p epdf-plugin-metadata --features uniffi`
Expected: PASS, including the existing conformance tests (unchanged).

- [ ] **Step 6: Wire the host and the Swift facade, with a failing Swift test first**

In `epdf-viewer-host`:
- Add `epdf-plugin-metadata = { path = "../plugins/metadata", features = ["uniffi"] }`.
- Add `#[uniffi(default = false)] pub metadata: bool` to `ViewerConfig`.
- In `Session::build`, push `metadata_plugin()` when it's set.
- Add the async accessor:

```rust
pub async fn metadata(&self, document_id: String) -> Result<Arc<MetadataHandle>, ViewerError> {
    let post = self.post.clone();
    self.call(move |s| {
        let cap = s.kernel.capability(&epdf_plugin_metadata::METADATA, Some(&document_id))?;
        Ok(MetadataHandle::new(cap, post))
    })
    .await
}
```

`Tests/EmbedPDFTests/MetadataTests.swift`:
```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct MetadataTests {
    func opened() async throws -> (PDFViewer, PDFDocument, MetadataCapability) {
      let viewer = try PDFViewer(ViewerConfig(metadata: true))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return (viewer, document, try await viewer.metadata(for: document))
    }

    @Test func refreshThenUpdateIsObserved() async throws {
      let (_, _, metadata) = try await opened()
      try await metadata.refresh()
      expectNoDifference(try await metadata.status(), .ready)
      let events = metadata.events
      let updated = try await metadata.update(MetadataPatch(title: "Renamed"))
      expectNoDifference(updated.title, "Renamed")
      for await event in events {
        guard case let .updated(event) = event else { continue }
        expectNoDifference(event.changedKeys, ["title"])
        break
      }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, metadata) = try await opened()
      try await viewer.close(document)
      await #expect {
        _ = try await metadata.snapshot()
      } throws: { error in
        (error as? PDFError)?.code == "instance-closed"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, metadata) = try await opened()
      let task = Task { for await _ in metadata.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      try await metadata.refresh()
    }
  }
}
```

`droppingTheStreamUnsubscribes` only proves nothing crashes after cancellation. The real leak check lives in Rust (Task 1's `cancelling_a_subscription_unsubscribes_on_the_kernel_thread`). Keep this one as the end-to-end check.

`aClosedDocumentsCapabilityThrows` assumes the kernel refuses calls on a closed document's capability with code `instance-closed` (`error.rs:164` lists the code). If the capability keeps answering from cached state, `MetadataHandle` has to check that the document is still open before each call. Add that check in `ffi.rs`, using `kernel.documents().has(id)`, rather than weakening the test.

- [ ] **Step 7: Run it to verify it fails, then implement the facade**

Run: `bash platforms/apple/build-xcframework.sh && cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/MetadataTests`
Expected: FAIL to compile with "value of type 'PDFViewer' has no member 'metadata'".

`Sources/EmbedPDF/Plugins/Metadata.swift`:
```swift
internal import EmbedPDFBindings

public typealias DocumentMetadata = EmbedPDFBindings.DocumentMetadata
public typealias MetadataPatch = EmbedPDFBindings.MetadataPatch
public typealias ResourceStatus = EmbedPDFBindings.ResourceStatus
public typealias MetadataEvent = EmbedPDFBindings.MetadataEvent

public struct MetadataCapability: Sendable {
  let handle: MetadataHandle

  public func snapshot() async throws(PDFError) -> DocumentMetadata? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> ResourceStatus {
    try await bridged { try await handle.status() }
  }

  public func canEdit() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canEdit() }
  }

  public func update(_ patch: MetadataPatch) async throws(PDFError) -> DocumentMetadata {
    try await bridged { try await handle.update(patch: patch) }
  }

  public func refresh() async throws(PDFError) {
    try await bridged { try await handle.refresh() }
  }

  public var events: AsyncStream<MetadataEvent> {
    EventRelay.stream { handle.subscribe(listener: $0) }
  }
}

extension EventRelay: MetadataEventListener where Event == MetadataEvent {
  func onEvent(event: MetadataEvent) {
    continuation.yield(event)
  }
}

extension PDFViewer {
  public func metadata(for document: PDFDocument) async throws(PDFError) -> MetadataCapability {
    MetadataCapability(handle: try await bridged { try await host.metadata(documentId: document.id) })
  }
}
```

- [ ] **Step 8: Run all Swift tests and commit**

Run: `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests`
Expected: `** TEST SUCCEEDED **`, 7 tests.

```bash
git add crates/plugins/metadata crates/plugin-conformance crates/epdf-viewer-host platforms/apple/EmbedPDF
git commit -m "Export the metadata plugin through uniffi"
```

---

### Task 7: Plugin slice, `search`

**Files:**
- Modify: `crates/plugins/search/Cargo.toml`, `src/lib.rs`, `src/contract.rs` (`cfg_attr` derives where compatible)
- Create: `crates/plugins/search/src/ffi.rs`, `crates/plugins/search/tests/ffi.rs`
- Modify: `crates/epdf-viewer-host` (dependency, `ViewerConfig.search`, `ViewerHost::search`)
- Create: `Sources/EmbedPDF/Plugins/Search.swift`
- Test: `Tests/EmbedPDFTests/SearchTests.swift`

**Interfaces:**
- Consumes: Task 1, Task 6's `TestKernel`. `SearchCapability` (`search/src/controller.rs:95`, methods at `:369-744`).
- Produces (Rust, feature `uniffi`). Mirrors are needed because of `usize` (`page_index`, `hit_count`), `isize` (active index), `Rc<Vec<_>>` (`list_hits`), and `AbortSignal` in the options structs:
  ```rust
  #[derive(uniffi::Record)] pub struct SearchQuery { pub text: String, #[uniffi(default = None)] pub regex: Option<bool>, #[uniffi(default = None)] pub match_case: Option<bool>, #[uniffi(default = None)] pub whole_word: Option<bool>, #[uniffi(default = None)] pub match_diacritics: Option<bool> }
  #[derive(uniffi::Record)] pub struct SearchHit { pub page: PageRef, pub page_index: u32, pub char_start: f64, pub char_count: f64, pub segments: Vec<TextSegment>, pub bounds: Option<Rect> }
  #[derive(uniffi::Record)] pub struct TextSegment { /* copy from contract.rs */ }
  #[derive(uniffi::Enum)] pub enum SearchStatus { Idle, Searching, Complete, Cancelled, Error }
  #[derive(uniffi::Enum)] pub enum SearchMode { /* copy from contract.rs */ }
  #[derive(uniffi::Record)] pub struct SearchProgress { pub scanned: f64, /* ... */ }
  #[derive(uniffi::Record)] pub struct SearchResult { pub status: SearchResultStatus, pub hit_count: u32 }
  #[derive(uniffi::Record)] pub struct SearchRevealOptions { /* copy */ }
  #[derive(uniffi::Record, Default)] pub struct SearchConfig { #[uniffi(default = None)] pub reveal: Option<SearchRevealOptions> }
  #[derive(uniffi::Enum)] pub enum SearchEvent { Started { event: SearchStartedEvent }, Progress { event: SearchProgressEvent }, Completed { event: SearchCompletedEvent }, Cancelled { event: SearchCancelledEvent }, Failed { event: SearchFailedEvent }, ActiveHitChanged { event: SearchActiveHitChangedEvent }, Cleared { event: SearchClearedEvent } }
  #[uniffi::export(with_foreign)] pub trait SearchEventListener: Send + Sync { fn on_event(&self, event: SearchEvent); }
  #[derive(uniffi::Object)] pub struct SearchHandle;
  #[uniffi::export] impl SearchHandle {
      pub async fn can_search(&self, mode: Option<SearchMode>) -> Result<bool, ViewerError>;
      pub async fn search(&self, query: SearchQuery, start_page: Option<PageRef>) -> Result<SearchResult, ViewerError>;
      pub async fn refresh(&self) -> Result<SearchResult, ViewerError>;
      pub async fn cancel(&self) -> Result<(), ViewerError>;
      pub async fn clear(&self) -> Result<(), ViewerError>;
      pub async fn next_hit(&self, reveal: Option<SearchRevealOptions>) -> Result<Option<SearchHit>, ViewerError>;
      pub async fn previous_hit(&self, reveal: Option<SearchRevealOptions>) -> Result<Option<SearchHit>, ViewerError>;
      pub async fn go_to_hit(&self, index: u32, reveal: Option<SearchRevealOptions>) -> Result<Option<SearchHit>, ViewerError>;
      pub async fn reveal_active_hit(&self) -> Result<(), ViewerError>;
      pub async fn query(&self) -> Result<Option<SearchQuery>, ViewerError>;
      pub async fn status(&self) -> Result<SearchStatus, ViewerError>;
      pub async fn hits(&self, page: Option<PageRef>) -> Result<Vec<SearchHit>, ViewerError>;
      pub async fn hit_count(&self) -> Result<u32, ViewerError>;
      pub async fn pages_with_hits(&self) -> Result<Vec<u32>, ViewerError>;
      pub async fn active_hit_index(&self) -> Result<Option<u32>, ViewerError>;   // -1 -> None
      pub async fn active_hit(&self) -> Result<Option<SearchHit>, ViewerError>;
      pub async fn progress(&self) -> Result<SearchProgress, ViewerError>;
      pub async fn error(&self) -> Result<Option<ViewerError>, ViewerError>;
      pub async fn find_all(&self, query: SearchQuery) -> Result<Vec<SearchHit>, ViewerError>;
      pub fn subscribe(&self, listener: Arc<dyn SearchEventListener>) -> Arc<Subscription>;
  }
  ```
- Produces (host): `ViewerConfig.search: Option<SearchConfig>`, `ViewerHost::search(document_id) -> Arc<SearchHandle>`.
- Produces (Swift): typealiases for every record above, `SearchCapability` with one method per handle method under the same name (`hits(on:)` for `hits(page:)`), and `events: AsyncStream<SearchEvent>`. Also `extension PDFViewer { func search(in:) }`.

Before writing the mirrors, copy each record's fields from `crates/plugins/search/src/contract.rs` (`SearchQuery` `:22`, `SearchHit` `:140`, `SearchStatus` `:158`, `SearchProgress` `:167`, `SearchResult` `:247`, events `:251-314`). `SearchQuery` has no incompatible fields, so derive in place. `SearchHit` has `usize`, so mirror it. Its `page` field converts through `epdf_ffi::PageRef`'s `From` impl. `char_start` and `char_count` are UTF-16 offsets, and the Rust doc comments say so; UniFFI carries those comments into the Swift doc comments.

`search` and `find_all` build the Rust `SearchOptions` and `SearchFindAllOptions` with `signal: None`. Cancellation reaches the promise through `KernelBound::call_promise`'s abort-on-drop instead.

- [ ] **Step 1: Write the failing Rust tests**

`crates/plugins/search/tests/ffi.rs`, over `TestKernel` with a `FakeEngine` spec whose `search.query` answers a fixed slice:
```rust
#![cfg(feature = "uniffi")]

use epdf_plugin_search::ffi::{SearchQuery, SearchStatus};
use futures::executor::block_on;

mod support;   // tests/support/mod.rs: handle() as in metadata's test, for search

#[test]
fn a_query_finds_hits_and_navigates_them() {
    let (_thread, search) = support::handle();
    let result = block_on(search.search(SearchQuery { text: "alpha".into(), ..Default::default() }, None)).unwrap();
    assert!(result.hit_count > 0);
    assert!(matches!(block_on(search.status()).unwrap(), SearchStatus::Complete));
    let first = block_on(search.next_hit(None)).unwrap();
    assert_eq!(block_on(search.active_hit_index()).unwrap(), Some(0));
    assert_eq!(block_on(search.active_hit()).unwrap().map(|h| h.char_start), first.map(|h| h.char_start));
}

#[test]
fn no_active_hit_is_none_not_minus_one() {
    let (_thread, search) = support::handle();
    assert_eq!(block_on(search.active_hit_index()).unwrap(), None);
}

#[test]
fn dropping_find_all_aborts_it() {
    let (thread, search) = support::handle_with_deferred_engine();
    let future = Box::pin(search.find_all(SearchQuery { text: "alpha".into(), ..Default::default() }));
    block_on(thread.call(|_| ())).unwrap();
    drop(future);
    block_on(thread.call(|_| ())).unwrap();
    assert!(matches!(block_on(search.status()).unwrap(), SearchStatus::Cancelled | SearchStatus::Idle));
}
```

`handle_with_deferred_engine` uses `Harness::defer` (`harness.rs:441`) so the fake's `search.query` stays pending until the test resolves it. Check which statuses `find_all` leaves behind after an abort; the TypeScript `findAll` doesn't touch session state. If `status()` stays `Idle`, assert that the aborted promise rejected instead: have `support` record engine aborts and check that list.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-search --features uniffi --test ffi`
Expected: FAIL to compile with "unresolved import `epdf_plugin_search::ffi`".

- [ ] **Step 3: Implement `ffi.rs`**

Follow Task 6's file shape. Each handle method is one `self.cap.call(...)` or `self.cap.call_promise(...)` with a conversion. `subscribe` registers all seven `on_*` hooks inside one `fire` job and combines their seven `Unsubscribe`s into one before `slot.fill`. Check whether `Unsubscribe` has a combinator; if not, `Unsubscribe::new(move || { for u in all { u.call() } })`.

- [ ] **Step 4: Run the Rust tests**

Run: `cargo test -p epdf-plugin-search --features uniffi`
Expected: PASS, with the existing conformance tests unchanged.

- [ ] **Step 5: Wire the host, then write the failing Swift tests**

`Tests/EmbedPDFTests/SearchTests.swift`:
```swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SearchTests {
    func searchable() async throws -> SearchCapability {
      let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("report"))
      return try await viewer.search(in: document)
    }

    @Test func aQueryFindsHitsAndNavigatesThem() async throws {
      let search = try await searchable()
      let result = try await search.search(SearchQuery(text: "the"))
      #expect(result.hitCount > 0)
      expectNoDifference(try await search.status(), .complete)
      let first = try await search.nextHit()
      expectNoDifference(try await search.activeHitIndex(), 0)
      expectNoDifference(try await search.activeHit(), first)
    }

    @Test func completionIsObserved() async throws {
      let search = try await searchable()
      let events = search.events
      _ = try await search.search(SearchQuery(text: "the"))
      for await event in events {
        if case .completed = event { break }
      }
    }

    @Test func searchCancelsWhenTheTaskIsCancelled() async throws {
      let search = try await searchable()
      let task = Task { try await search.findAll(SearchQuery(text: "e")) }
      task.cancel()
      await #expect {
        _ = try await task.value
      } throws: { error in
        (error as? PDFError)?.code == "operation-cancelled"
      }
    }
  }
}
```

`searchCancelsWhenTheTaskIsCancelled` depends on what Task 4 Step 5 found:
- If UniFFI's generated Swift drops the Rust future on cancellation, abort-on-drop does the rest. The rejection's code then comes from the kernel's `Cancelled` class.
- If the generated Swift doesn't drop the future, `SearchCapability.findAll` wraps the call in `withTaskCancellationHandler` and calls `handle.cancel()` on cancel. That isn't general, because it cancels the session search, not `findAll`. In that case, add a general fix to `epdf-ffi` before going further: `#[derive(uniffi::Object)] pub struct CancelToken`, passed as the last argument of every promise-returning export, whose `cancel()` aborts the call it was passed to. Swift's `bridged` then cancels it from `withTaskCancellationHandler`. This adds one parameter to about 200 exports, which is why it's only the fallback.

The race note from the earlier draft still applies. `"e"` over 14 pages almost always outruns a one-line cancel. If it flakes, use a larger fixture.

- [ ] **Step 6: Implement `Search.swift`, run, commit**

`Sources/EmbedPDF/Plugins/Search.swift` follows `Metadata.swift`'s shape exactly:
- Typealiases for `SearchQuery`, `SearchHit`, `TextSegment`, `SearchStatus`, `SearchMode`, `SearchProgress`, `SearchResult`, `SearchResultStatus`, `SearchRevealOptions`, `SearchConfig`, `SearchEvent` and the seven event records.
- `SearchCapability` with one `bridged` line per method.
- The `EventRelay: SearchEventListener` one-liner.
- `extension PDFViewer { public func search(in document: PDFDocument) async throws(PDFError) -> SearchCapability }`.

Run: `bash platforms/apple/build-xcframework.sh && cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests`
Expected: `** TEST SUCCEEDED **`, 10 tests.

```bash
git add crates/plugins/search crates/epdf-viewer-host platforms/apple/EmbedPDF
git commit -m "Export the search plugin through uniffi"
```

---

### Task 8: Plugin slice, `render`, returning `CGImage`

**Files:**
- Modify: `crates/plugins/render/Cargo.toml`, `src/lib.rs`
- Create: `crates/plugins/render/src/ffi.rs`, `crates/plugins/render/tests/ffi.rs`
- Modify: `crates/epdf-viewer-host` (dependency, `ViewerConfig.render`, `ViewerHost::render`, `RenderHost.release_image` wired to `HostTable`)
- Create: `Sources/EmbedPDF/Plugins/Render.swift`
- Test: `Tests/EmbedPDFTests/RenderTests.swift`

**Interfaces:**
- Consumes: Task 1, Task 2's `HostTable`. `render_plugin_with_host(RenderConfig, RenderHost)` (`render/src/lib.rs:78`), `RenderHost` (`host_contract.rs:74`).
- Produces (Rust):
  ```rust
  #[derive(uniffi::Enum)] pub enum PixelFormat { Bgra8Premultiplied, Rgba8Premultiplied }
  #[derive(uniffi::Record)] pub struct PageImage { pub width: u32, pub height: u32, pub stride: u32, pub scale: f64, pub format: PixelFormat, pub pixels: Vec<u8> }
  #[derive(uniffi::Record)] pub struct RenderPageOptions { #[uniffi(default = None)] pub scale: Option<f64>, #[uniffi(default = None)] pub rotation: Option<u32>, /* copy the rest */ }
  #[derive(uniffi::Record, Default)] pub struct RenderConfig { /* copy from render config, minus host */ }
  #[derive(uniffi::Enum)] pub enum RenderEvent { Invalidated { .. }, Completed { .. }, Failed { .. } }
  #[uniffi::export(with_foreign)] pub trait RenderEventListener: Send + Sync { fn on_event(&self, event: RenderEvent); }
  #[derive(uniffi::Object)] pub struct RenderHandle;
  #[uniffi::export] impl RenderHandle {
      pub async fn can_render(&self) -> Result<bool, ViewerError>;
      pub async fn render_page(&self, page: PageRef, options: RenderPageOptions) -> Result<PageImage, ViewerError>;
      pub async fn render_thumbnail(&self, page: PageRef, max_size: f64) -> Result<PageImage, ViewerError>;
      pub async fn render_pages(&self, pages: Vec<PageRef>, options: RenderPageOptions) -> Result<Vec<PageImage>, ViewerError>;
      pub async fn render_epoch(&self) -> Result<u64, ViewerError>;
      pub async fn invalidate(&self, pages: Option<Vec<PageRef>>) -> Result<(), ViewerError>;
      pub fn subscribe(&self, listener: Arc<dyn RenderEventListener>) -> Arc<Subscription>;
  }
  ```
- Produces (Swift): `RenderCapability` with `canRender()`, `renderPage(_:options:) -> RenderedPage`, `renderThumbnail(_:maxSize:)`, `renderPages(_:options:)`, `renderEpoch()`, `invalidate(_:)`, `events`. `public struct RenderedPage: Sendable { width, height, scale, image: CGImage }`.

`render_policy`, `render_source`, `source_key`, `conform_viewport`, `paint_settings` and `create_view_demand` drive a tiled viewport that the stage plugin owns. They land with stage in follow-on plan B1 (Appendix A §A.2).

Render's FFI wrapper needs `HostTable`, which lives in `epdf-viewer-host`. Render can't depend on the host, because that would be a cycle. So the wrapper receives the pixel source as a closure. `RenderHandle::new(cap, post, pixels: Rc<dyn Fn(u64) -> Option<Arc<Vec<u8>>>>)` is called on the kernel thread, and the host passes `HostTable::get`. The closure is `Rc` and stays on the kernel thread, so it lives in the `KernelBound` alongside the capability, as `KernelBound<(RenderCapability, PixelSource)>`.

- [ ] **Step 1: Write the failing Rust test**

`crates/plugins/render/tests/ffi.rs`, over `TestKernel` whose fake `page.render.image` answers `{width: 2, height: 1, stride: 8, format: "bgra", "$host": 1}`, with a pixel source answering 8 bytes for id 1:
```rust
#![cfg(feature = "uniffi")]

use epdf_ffi::PageRef;
use epdf_plugin_render::ffi::{PixelFormat, RenderPageOptions};
use futures::executor::block_on;

mod support;

#[test]
fn a_page_renders_to_owned_pixels() {
    let (_thread, render) = support::handle();
    let image = block_on(render.render_page(PageRef { page_object_number: 3 }, RenderPageOptions { scale: Some(1.0), ..Default::default() })).unwrap();
    assert_eq!((image.width, image.height, image.stride), (2, 1, 8));
    assert_eq!(image.pixels.len(), 8);
    assert!(matches!(image.format, PixelFormat::Bgra8Premultiplied));
}

#[test]
fn a_bad_scale_is_refused() {
    let (_thread, render) = support::handle();
    assert!(block_on(render.render_page(PageRef { page_object_number: 3 }, RenderPageOptions { scale: Some(0.0), ..Default::default() })).is_err());
}
```

Copy the wire raster's `format` spelling from `worker-protocol.ts` (`PageRaster`) and map it onto `PixelFormat`. `PageView::render_bgra` produces BGRA. `epdf-uniffi`'s `render_page` swaps to RGBA, but the wire path doesn't go through that swap.

- [ ] **Step 2: Run, implement, run again**

Run: `cargo test -p epdf-plugin-render --features uniffi --test ffi`
Expected: FAIL to compile.

Implement `ffi.rs` as in Task 6. `render_page` calls `c.render_page(...)` and maps the resolved image handle. It reads the handle's value, takes `$host`, copies the bytes out through the pixel source, and builds `PageImage`. The handle drops when the closure returns, and `RenderHost::release_image` frees the table entry.

Run: `cargo test -p epdf-plugin-render --features uniffi`
Expected: PASS.

- [ ] **Step 3: Wire the host**

Add the dependency. Add `#[uniffi(default = None)] pub render: Option<RenderConfig>` to `ViewerConfig`. In `Session::build`, push `render_plugin_with_host(config.into(), RenderHost { release_image: Some(Rc::new(|v| if let Some(id) = v["$host"].as_u64() { HostTable::release(id) })), ..RenderHost::default() })`. Check `ReleaseImage`'s type alias for the closure's exact signature. `request_frame` keeps its 16 ms `rt::set_timeout` default (`host_contract.rs:84`). The kernel thread drives that timer, so no display link is involved.

- [ ] **Step 4: Write the failing Swift tests, then the facade**

`Tests/EmbedPDFTests/RenderTests.swift`:
```swift
import CoreGraphics
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct RenderTests {
    @Test func aPageRendersToACGImageThatOutlivesTheViewer() async throws {
      var image: CGImage?
      do {
        let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
        try await viewer.start()
        let document = try await viewer.open(Fixture.data("colors"))
        let render = try await viewer.render(in: document)
        let page = try await viewer.pages(of: document)[0].page
        let rendered = try await render.renderPage(page, options: RenderPageOptions(scale: 1))
        #expect(rendered.width > 0 && rendered.height > 0)
        image = rendered.image
        try await viewer.destroy()
      }
      let survivor = try #require(image)
      #expect(survivor.width > 0)
      #expect(survivor.dataProvider?.data != nil)
    }

    @Test func aBadScaleIsRefused() async throws {
      let viewer = try PDFViewer(ViewerConfig(render: RenderConfig()))
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("colors"))
      let render = try await viewer.render(in: document)
      let page = try await viewer.pages(of: document)[0].page
      await #expect(throws: PDFError.self) {
        _ = try await render.renderPage(page, options: RenderPageOptions(scale: 0))
      }
    }
  }
}
```

Run it and expect a compile failure: "value of type 'PDFViewer' has no member 'render'".

`Sources/EmbedPDF/Plugins/Render.swift` builds the `CGImage` from `PageImage`:
```swift
func makeImage(_ page: EmbedPDFBindings.PageImage) -> CGImage? {
  let info: UInt32 = switch page.format {
  case .bgra8Premultiplied: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
  case .rgba8Premultiplied: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
  }
  guard let provider = CGDataProvider(data: page.pixels as CFData) else { return nil }
  return CGImage(
    width: Int(page.width), height: Int(page.height), bitsPerComponent: 8, bitsPerPixel: 32,
    bytesPerRow: Int(page.stride), space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGBitmapInfo(rawValue: info), provider: provider, decode: nil,
    shouldInterpolate: true, intent: .defaultIntent)
}
```

`page.pixels` is `Data` (UniFFI's `Vec<u8>`), and `CGDataProvider(data:)` retains that buffer. The `CGImage` owns its pixels and doesn't depend on Rust. A render costs two copies: Rust table to UniFFI buffer, then UniFFI buffer to `Data`. `multiplatform-plan.md` Phase 2 asks for the pixel delivery path to be measured before it is chosen. That benchmark is follow-on plan B5. The `colors` fixture has a red region and a blue region, as the smoke tests check. If the first run shows the channels swapped, `PixelFormat` mapping is wrong, so add that pixel assertion here.

The rest of `Render.swift` follows `Metadata.swift`'s shape: typealiases, `RenderCapability`, the relay one-liner, and `extension PDFViewer { func render(in:) }`.

- [ ] **Step 5: Run and commit**

Run: `bash platforms/apple/build-xcframework.sh && cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests`
Expected: `** TEST SUCCEEDED **`, 12 tests.

```bash
git add crates/plugins/render crates/epdf-viewer-host platforms/apple/EmbedPDF
git commit -m "Export the render plugin through uniffi with CGImage output"
```

---

### Task 9: `EmbedPDFUI`, an observable search model and a page view

**Files:**
- Delete: `Sources/EmbedPDFUI/EmbedPDFUI.swift`
- Create: `Sources/EmbedPDFUI/SearchModel.swift`, `Sources/EmbedPDFUI/PageImageView.swift`
- Modify: `Tests/EmbedPDFUITests/SearchModelTests.swift`

**Interfaces:**
- Consumes: `PDFViewer`, `PDFDocument`, `SearchCapability`, `RenderCapability`, `RenderedPage` (Tasks 5 to 8).
- Produces:
  ```swift
  @MainActor @Observable public final class SearchModel {
    public var text: String
    public private(set) var hits: [SearchHit]
    public private(set) var activeIndex: UInt32?
    public private(set) var status: SearchStatus
    public private(set) var error: PDFError?
    public init(search: SearchCapability)
    public func task() async
    public func searchSubmitted() async
    public func nextButtonTapped() async
    public func previousButtonTapped() async
    public func clearButtonTapped() async
  }
  public struct PageImageView: View { public init(render: RenderCapability, page: PageRef, scale: Double) }
  ```

Methods are named after what the user did (pfw-observable-models). `task()` consumes `search.events` for the view's lifetime. The view calls it from `.task`.

- [ ] **Step 1: Write the failing tests**

`Tests/EmbedPDFUITests/SearchModelTests.swift`:
```swift
import CustomDump
import EmbedPDF
import EmbedPDFUI
import Foundation
import Testing

@MainActor
@Suite(.serialized) struct SearchModelTests {
  func model() async throws -> SearchModel {
    let url = try #require(Bundle.module.url(forResource: "report", withExtension: "pdf", subdirectory: "Fixtures"))
    let viewer = try PDFViewer(ViewerConfig(search: SearchConfig()))
    try await viewer.start()
    let document = try await viewer.open(try Data(contentsOf: url))
    return SearchModel(search: try await viewer.search(in: document))
  }

  @Test func submittingASearchFillsHitsAndNextMovesTheActiveHit() async throws {
    let model = try await model()
    model.text = "the"
    await model.searchSubmitted()
    expectNoDifference(model.status, .complete)
    #expect(!model.hits.isEmpty)
    await model.nextButtonTapped()
    expectNoDifference(model.activeIndex, 0)
    await model.nextButtonTapped()
    expectNoDifference(model.activeIndex, 1)
  }

  @Test func clearingEmptiesTheModel() async throws {
    let model = try await model()
    model.text = "the"
    await model.searchSubmitted()
    await model.clearButtonTapped()
    expectNoDifference(model.hits, [])
    expectNoDifference(model.activeIndex, nil)
    expectNoDifference(model.status, .idle)
  }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFUITests`
Expected: FAIL to compile with "cannot find 'SearchModel' in scope".

- [ ] **Step 3: Implement**

`Sources/EmbedPDFUI/SearchModel.swift`:
```swift
import EmbedPDF
import Observation

@MainActor
@Observable
public final class SearchModel {
  public var text = ""
  public private(set) var hits: [SearchHit] = []
  public private(set) var activeIndex: UInt32?
  public private(set) var status: SearchStatus = .idle
  public private(set) var error: PDFError?
  @ObservationIgnored let search: SearchCapability

  public init(search: SearchCapability) {
    self.search = search
  }

  public func task() async {
    for await event in search.events {
      if case let .activeHitChanged(event) = event {
        activeIndex = event.index
      }
    }
  }

  public func searchSubmitted() async {
    do {
      _ = try await search.search(SearchQuery(text: text))
      hits = try await search.hits()
      status = try await search.status()
      activeIndex = try await search.activeHitIndex()
      error = nil
    } catch {
      self.error = error
      status = .error
    }
  }

  public func nextButtonTapped() async {
    do {
      _ = try await search.nextHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func previousButtonTapped() async {
    do {
      _ = try await search.previousHit()
      activeIndex = try await search.activeHitIndex()
    } catch {
      self.error = error
    }
  }

  public func clearButtonTapped() async {
    do {
      try await search.clear()
      hits = []
      activeIndex = nil
      status = try await search.status()
    } catch {
      self.error = error
    }
  }
}
```

Use the real index field name of `SearchActiveHitChangedEvent` in `task()`.

`Sources/EmbedPDFUI/PageImageView.swift`:
```swift
import EmbedPDF
import SwiftUI

public struct PageImageView: View {
  let render: RenderCapability
  let page: PageRef
  let scale: Double
  @State private var rendered: RenderedPage?
  @State private var error: PDFError?

  public init(render: RenderCapability, page: PageRef, scale: Double) {
    self.render = render
    self.page = page
    self.scale = scale
  }

  public var body: some View {
    Group {
      if let rendered {
        Image(decorative: rendered.image, scale: rendered.scale)
          .resizable()
          .aspectRatio(contentMode: .fit)
      } else if let error {
        ContentUnavailableView("Page unavailable", systemImage: "doc", description: Text(error.detail))
      } else {
        ProgressView()
      }
    }
    .task(id: scale) { await task() }
  }

  private func task() async {
    do {
      rendered = try await render.renderPage(page, options: RenderPageOptions(scale: scale))
    } catch {
      self.error = error
    }
  }
}
```

- [ ] **Step 4: Run every Swift test and commit**

Run: `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17'`
Expected: `** TEST SUCCEEDED **`. That's 8 smoke tests, 12 in `EmbedPDFTests` and 2 in `EmbedPDFUITests`: 22 total.

```bash
git add platforms/apple/EmbedPDF
git commit -m "Add EmbedPDFUI's search model and page image view"
```

---

### Task 10: CI

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`
- Create: `.github/workflows/apple-swift.yml`

`engine-rust.yml` runs `cargo test --workspace` on `ubuntu-latest`. It picks up `epdf-ffi` and `epdf-viewer-host` without an edit, but it doesn't turn on any plugin's `uniffi` feature. Add a step to `plugin-rust.yml`:

```yaml
      - run: cargo test -p epdf-ffi -p epdf-plugin-metadata -p epdf-plugin-search -p epdf-plugin-render --features epdf-plugin-metadata/uniffi,epdf-plugin-search/uniffi,epdf-plugin-render/uniffi
```

Each follow-on plan adds its plugins to that line.

The iOS job can't run on every push yet. The fork release pins no `ios-*` artifacts: `engine-runtime-build.json` covers darwin, linux, linuxmusl, wasm32 and win32 only. `apple-swift.yml` is `workflow_dispatch` only. It takes a runtime build file URL as input. When `ios-arm64` and `ios-sim-arm64` land in `engine-runtime-build.json`, switch it to `pull_request` with `paths: [crates/**, platforms/apple/**]`.

- [ ] **Step 1: Write `apple-swift.yml`**

```yaml
name: apple-swift
on:
  workflow_dispatch:
    inputs:
      runtime_build_file:
        description: "URL of an engine-runtime-build JSON that pins ios-arm64 and ios-sim-arm64"
        required: true
jobs:
  test:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-ios,aarch64-apple-ios-sim
      - name: Fetch PDFium
        run: |
          curl -fsSL "${{ inputs.runtime_build_file }}" -o engine-runtime-build.ci.json
          for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
            PDF_RUNTIME_BUILD_FILE=$PWD/engine-runtime-build.ci.json bash packages/engine/runtime/scripts/fetch-libpdfium.sh $t
          done
      - run: cargo test -p epdf-ffi -p epdf-viewer-host
      - run: bash platforms/apple/build-xcframework.sh
      - working-directory: platforms/apple/EmbedPDF
        run: xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 16'
```

`macos-15` runners ship iPhone 16 simulators. The local commands keep iPhone 17.

- [ ] **Step 2: Commit**

```bash
git add .github/workflows
git commit -m "Test the uniffi plugin exports in CI and add a manual Apple job"
```

---

### Task 11: Experimental manual bridge and the UniFFI comparison

This task builds a second bridge for the same plugin and times both on the same workloads. It's experimental: no product links it, and it never goes in the shipping XCFramework. Its deliverable is `docs/research/swift-bridge-benchmarks.md` with numbers and a recommendation.

**Representative plugin.** Search covers most of the call shapes: synchronous reads (`hit_count`), promise-returning calls (`search`, `find_all`), a large array of nested records (`hits`), and events (`subscribe`, with progress events arriving during a scan). Render's `render_page` is added as the bulk-bytes case. On a raster the two bridges differ in kind, not just in degree: UniFFI copies the pixels into a `RustBuffer` and again into `Data`, while the manual bridge can hand Swift a pointer into the Rust `Vec` and free it from `CGDataProvider`'s release callback.

**Isolating the costs.** A UniFFI call here does three things: it hops to the kernel thread, runs the plugin, and serializes. The benchmark separates those with two control entry points per bridge. `noop()` returns at once on the calling thread, which is pure FFI overhead. `noop_hop()` posts an empty job to the kernel thread and waits, which is FFI plus the hop. Anything above `noop_hop` in a plugin call is plugin work plus serialization.

**Files:**
- Create: `crates/epdf-manual-bridge/Cargo.toml`, `include/epdf_manual.h`, `src/lib.rs`, `src/search.rs`, `src/render.rs`, `src/bench.rs`, `tests/abi.rs`
- Create: `crates/epdf-viewer-host/src/experimental.rs`. Modify `crates/epdf-viewer-host/Cargo.toml` (feature `manual-bridge`) and `src/lib.rs`.
- Modify: `crates/epdf-ffi/src/lib.rs` (UniFFI `noop` and `noop_hop` exports, behind feature `bench`)
- Modify: `platforms/apple/build-xcframework.sh` (`--experimental-manual-bridge` flag)
- Modify: `platforms/apple/EmbedPDF/Package.swift` (one target and one test target, neither in `products`)
- Create: `Sources/EmbedPDFManualBridge/ManualSearch.swift`, `ManualRender.swift`
- Create: `Tests/EmbedPDFBridgeBenchmarks/BridgeBenchmarks.swift`
- Create: `docs/research/swift-bridge-benchmarks.md`

**Interfaces:**
- Consumes: `epdf_ffi::{KernelBound, KernelPost}` (Task 1). `ViewerHost` (Task 3). The search and render plugins' Rust capabilities and their FFI conversions (Tasks 7 and 8). The conversions get reused so both bridges do the same plugin-side work.
- Produces (C ABI, `include/epdf_manual.h`):
  ```c
  typedef struct EpdfError { int32_t kind; char *code; char *detail; } EpdfError;   // kind 0 = ok, 1 = kernel, 2 = closed
  typedef void (*EpdfComplete)(void *ctx, EpdfError error, const void *result);

  typedef struct EpdfRect { double x, y, width, height; } EpdfRect;
  typedef struct EpdfSegment { uint32_t page_object_number; EpdfRect rect; } EpdfSegment;   // match TextSegment
  typedef struct EpdfSearchHit {
    uint32_t page_object_number, page_index;
    double char_start, char_count;
    uint8_t has_bounds; EpdfRect bounds;
    uint32_t segment_offset, segment_count;      // into EpdfHits.segments
  } EpdfSearchHit;
  typedef struct EpdfHits { const EpdfSearchHit *hits; size_t hit_count; const EpdfSegment *segments; size_t segment_count; void *owner; } EpdfHits;

  typedef struct EpdfPixels {
    const uint8_t *data; size_t len; uint32_t width, height, stride, format;   // format 0 = BGRA premul, 1 = RGBA premul
    void *owner;                                                               // pass to epdf_pixels_free
  } EpdfPixels;

  typedef struct EpdfSearch EpdfSearch;           // opaque, holds a KernelBound<SearchCapability>
  typedef struct EpdfSubscription EpdfSubscription;
  typedef void (*EpdfSearchEvent)(void *ctx, int32_t kind, const void *payload);   // kind = SearchEvent discriminant

  uint64_t epdf_manual_kernel_token(void);        // set by ViewerHost when built with manual-bridge
  void     epdf_search_open(uint64_t kernel, const char *document_id, void *ctx, EpdfComplete done);   // result: EpdfSearch*
  void     epdf_search_free(EpdfSearch *s);
  void     epdf_search_hit_count(EpdfSearch *s, void *ctx, EpdfComplete done);                      // result: const uint32_t*
  uint64_t epdf_search_search(EpdfSearch *s, const char *text, void *ctx, EpdfComplete done);       // returns call id; result: const uint32_t* hit count
  uint64_t epdf_search_find_all(EpdfSearch *s, const char *text, void *ctx, EpdfComplete done);     // result: const EpdfHits*
  void     epdf_search_hits(EpdfSearch *s, void *ctx, EpdfComplete done);                           // result: const EpdfHits*
  void     epdf_hits_free(void *owner);
  void     epdf_cancel(uint64_t call_id);
  EpdfSubscription *epdf_search_subscribe(EpdfSearch *s, void *ctx, EpdfSearchEvent on_event);
  void     epdf_subscription_free(EpdfSubscription *sub);   // cancels

  typedef struct EpdfRender EpdfRender;
  void     epdf_render_open(uint64_t kernel, const char *document_id, void *ctx, EpdfComplete done);
  void     epdf_render_free(EpdfRender *r);
  uint64_t epdf_render_page(EpdfRender *r, uint32_t page_object_number, double scale, void *ctx, EpdfComplete done);   // result: const EpdfPixels*
  void     epdf_pixels_free(void *owner);

  void     epdf_bench_noop(void);
  void     epdf_bench_noop_hop(uint64_t kernel, void *ctx, EpdfComplete done);
  void     epdf_error_free(EpdfError error);
  ```
- Produces (UniFFI, behind `epdf-ffi/bench` and `epdf-viewer-host/manual-bridge`): `fn bench_noop()` and `ViewerHost::bench_noop_hop(&self) -> Result<(), ViewerError>` (async). Also `ViewerHost::experimental_kernel_token(&self) -> u64`.
- Produces (Swift, target `EmbedPDFManualBridge`): `ManualSearch` with the same async method names as `SearchCapability` for the measured members, and `ManualRender.renderPage(_:scale:) -> CGImage`.

**Ownership rules for the C ABI.** Every `result` pointer passed to `EpdfComplete` is valid only for the duration of the callback, with two exceptions. `EpdfHits` and `EpdfPixels` carry an `owner` that Swift must free with `epdf_hits_free` or `epdf_pixels_free`. That's what lets a hit array or a raster cross without a copy. Callbacks run on the kernel thread and must not block. `ctx` is an `Unmanaged` retained box that Swift releases inside the callback.

- [ ] **Step 1: Registry and control exports**

`crates/epdf-viewer-host/src/experimental.rs`, behind `#[cfg(feature = "manual-bridge")]`:
```rust
//! Lets the experimental manual bridge reach a ViewerHost's kernel thread
//! without going through uniffi. Not for shipping.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use epdf_ffi::KernelPost;

pub struct Registered {
    pub post: KernelPost,
    pub open_search: Box<dyn Fn(&str) -> Result<epdf_plugin_search::SearchCapability, epdf_kernel::Error> + Send + Sync>,
    pub open_render: Box<dyn Fn(&str) -> Result<epdf_plugin_render::RenderCapability, epdf_kernel::Error> + Send + Sync>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);
fn table() -> &'static Mutex<HashMap<u64, std::sync::Arc<Registered>>> {
    static TABLE: OnceLock<Mutex<HashMap<u64, std::sync::Arc<Registered>>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

pub fn register(entry: Registered) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    table().lock().unwrap().insert(id, std::sync::Arc::new(entry));
    id
}

pub fn lookup(id: u64) -> Option<std::sync::Arc<Registered>> {
    table().lock().unwrap().get(&id).cloned()
}

pub fn unregister(id: u64) {
    table().lock().unwrap().remove(&id);
}
```

The `open_*` closures run only on the kernel thread. They reach the session through a kernel-thread-local that `Session::build` sets, because a `Send + Sync` closure can't capture the `Rc` kernel. `ViewerHost::new` registers when the feature is on, `shutdown` unregisters, and `experimental_kernel_token()` returns the id.

Add `bench_noop` (a no-op `#[uniffi::export] fn`) to `epdf-ffi` behind `bench`. Add `ViewerHost::bench_noop_hop`, which is `self.call(|_| Ok(())).await`, behind `manual-bridge`.

- [ ] **Step 2: Write the failing ABI tests**

`crates/epdf-manual-bridge/tests/abi.rs` calls the `extern "C"` functions from Rust the way Swift will, with `extern "C" fn` callbacks writing into a `std::sync::mpsc` channel through `ctx`:
```rust
use std::ffi::{CString, c_void};
use std::sync::mpsc;

use epdf_manual_bridge::*;

extern "C" fn done(ctx: *mut c_void, error: EpdfError, result: *const c_void) {
    let tx = unsafe { Box::from_raw(ctx as *mut mpsc::Sender<(i32, usize)>) };
    let _ = tx.send((error.kind, result as usize));
    unsafe { epdf_error_free(error) };
}

fn wait<T>(f: impl FnOnce(*mut c_void, EpdfComplete) -> T) -> (i32, usize) {
    let (tx, rx) = mpsc::channel();
    f(Box::into_raw(Box::new(tx)) as *mut c_void, done);
    rx.recv().unwrap()
}

#[test]
fn search_through_the_c_abi_matches_uniffi() {
    let (host, doc) = support::host_with_report();       // ViewerHost with search + render, built with manual-bridge
    let kernel = host.experimental_kernel_token();
    let id = CString::new(doc.id.clone()).unwrap();
    let (kind, search) = wait(|ctx, cb| unsafe { epdf_search_open(kernel, id.as_ptr(), ctx, cb) });
    assert_eq!(kind, 0);
    let search = search as *mut EpdfSearch;
    let text = CString::new("the").unwrap();
    let (kind, _) = wait(|ctx, cb| unsafe { epdf_search_search(search, text.as_ptr(), ctx, cb) });
    assert_eq!(kind, 0);
    // The hits pointer is read inside the callback in the real bridge; the
    // test's callback copies hit_count out before returning instead.
    let manual = support::hit_count_via_c(search);
    let typed = futures::executor::block_on(support::uniffi_search(&host, &doc).hit_count()).unwrap();
    assert_eq!(manual, typed);
    unsafe { epdf_search_free(search) };
}

#[test]
fn pixels_cross_without_a_copy_and_free_once() {
    let (host, doc) = support::host_with_colors();
    let render = support::open_render_via_c(&host, &doc);
    let pixels = support::render_page_via_c(render, 1.0);  // copies EpdfPixels out of the callback
    assert_eq!(pixels.len, (pixels.stride * pixels.height) as usize);
    unsafe { epdf_pixels_free(pixels.owner) };
    unsafe { epdf_render_free(render) };
}

#[test]
fn cancelling_find_all_rejects_it() {
    let (host, doc) = support::host_with_report();
    let search = support::open_search_via_c(&host, &doc);
    let text = CString::new("e").unwrap();
    let (tx, rx) = mpsc::channel();
    let call = unsafe { epdf_search_find_all(search, text.as_ptr(), Box::into_raw(Box::new(tx)) as *mut c_void, done) };
    unsafe { epdf_cancel(call) };
    let (kind, _) = rx.recv().unwrap();
    assert_eq!(kind, 1);   // kernel error, code operation-cancelled
    unsafe { epdf_search_free(search) };
}
```

Write `tests/support/mod.rs` with the helpers named above. They're thin, and each one is a `wait(...)` over a single C call.

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-manual-bridge`
Expected: FAIL to compile with "unresolved import `epdf_manual_bridge`".

- [ ] **Step 4: Implement the crate**

`Cargo.toml` depends on `epdf-ffi`, `epdf-kernel`, `epdf-viewer-host` (features `manual-bridge`), `epdf-plugin-search` and `epdf-plugin-render` (features `uniffi`, for the shared conversions), plus `cbindgen` as a build-dependency that writes `include/epdf_manual.h`. Commit the generated header so a reviewer can read the ABI without building.

Every async entry point has the same shape:
```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_hit_count(s: *mut EpdfSearch, ctx: *mut c_void, done: EpdfComplete) {
    let ctx = SendPtr(ctx);
    let s = unsafe { &*s };
    s.bound.fire(move |cap| {
        let ctx = ctx;
        match cap.get_hit_count() {
            Ok(n) => {
                let n = n as u32;
                done(ctx.0, EpdfError::ok(), &n as *const u32 as *const c_void);
            }
            Err(e) => done(ctx.0, EpdfError::from(e), std::ptr::null()),
        }
    });
}
```

`SendPtr` is a `#[repr(transparent)]` wrapper with `unsafe impl Send`. `EpdfSearch` owns a `KernelBound<SearchCapability>`. Promise-returning calls register the promise in a global `call id -> (KernelPost, slab id)` map. `epdf_cancel` posts an abort, exactly as `KernelBound::call_promise`'s drop guard does. `epdf_search_hits` builds the flat `EpdfSearchHit` and `EpdfSegment` vectors from the capability's hits, boxes both into an owner, and passes pointers into them. `epdf_render_page` takes the bytes out of `HostTable` (a move, not a copy, when the render plugin's handle is the only holder) and boxes the `Vec` as the owner.

`epdf_search_subscribe` registers the seven `on_*` hooks and calls `on_event(ctx, kind, &payload)` with a `#[repr(C)]` payload per event kind. The progress event is the one the benchmark counts.

- [ ] **Step 5: Run the ABI tests**

Run: `cargo test -p epdf-manual-bridge`
Expected: PASS, 3 tests.

- [ ] **Step 6: Package it behind a flag, and write the Swift side**

`build-xcframework.sh --experimental-manual-bridge` builds `epdf-viewer-host` with features `manual-bridge` and `epdf-ffi/bench`, plus `epdf-manual-bridge`. It merges `libepdf_manual_bridge.a` into the same archive, copies `epdf_manual.h` into `headers/`, and appends this to `module.modulemap`:
```
module EpdfManualFFI {
  header "epdf_manual.h"
  export *
}
```
Without the flag, none of this happens and the shipping package is unchanged.

`Package.swift` gains a target and a test target. Neither goes in `products`. Both exist only when the environment variable `EMBEDPDF_EXPERIMENTAL_BRIDGE=1` is set at manifest evaluation, so a normal checkout never sees them:
```swift
.target(
  name: "EmbedPDFManualBridge",
  dependencies: [
    "EmbedPDF",
  ]
),
.testTarget(
  name: "EmbedPDFBridgeBenchmarks",
  dependencies: [
    "EmbedPDFManualBridge",
  ],
  resources: [
    .copy("Fixtures"),
  ]
),
```
Append these with `if Context.environment["EMBEDPDF_EXPERIMENTAL_BRIDGE"] == "1" { package.targets += [...] }` after the `Package` initializer. `EmbedPDFManualBridge` imports `EpdfManualFFI` (the module from the merged modulemap) and `EmbedPDF`. It reaches the kernel token through a `package` accessor on `PDFViewer` that calls `host.experimentalKernelToken()`.

`ManualSearch.swift` bridges each call with a continuation:
```swift
import EmbedPDF
import EpdfManualFFI

public final class ManualSearch: @unchecked Sendable {
  let raw: OpaquePointer

  public func hitCount() async throws -> UInt32 {
    try await withCheckedThrowingContinuation { continuation in
      let box = Unmanaged.passRetained(Box(continuation)).toOpaque()
      epdf_search_hit_count(raw, box) { ctx, error, result in
        let box = Unmanaged<Box<UInt32>>.fromOpaque(ctx!).takeRetainedValue()
        if error.kind == 0 {
          box.continuation.resume(returning: result!.load(as: UInt32.self))
        } else {
          box.continuation.resume(throwing: ManualError(error))
        }
        epdf_error_free(error)
      }
    }
  }

  deinit { epdf_search_free(raw) }
}
```

`hits()` copies nothing in the callback. It wraps the owner and pointers in a `ManualHits: RandomAccessCollection` whose `deinit` calls `epdf_hits_free`, and whose elements are read lazily from the C structs. That's the manual bridge's advantage on large arrays. The benchmark also times an eager `Array(hits.map(SearchHit.init))` conversion, because a real app would usually want Swift values, and that conversion is where the advantage might disappear.

`ManualRender.renderPage(_:scale:)` builds the `CGImage` with `CGDataProvider(dataInfo:data:size:releaseData:)`. `releaseData` calls `epdf_pixels_free(owner)`, so the raster is never copied.

- [ ] **Step 7: Write the benchmarks**

`Tests/EmbedPDFBridgeBenchmarks/BridgeBenchmarks.swift` uses Swift Testing with `ContinuousClock`, not XCTest's `measure`, so medians and p95 can be reported directly. Every workload runs through both bridges against the same `PDFViewer`. Each one does 200 warm-up iterations, then 5 runs of N iterations, and records the median, p95 and per-run spread.

| Workload | UniFFI call | Manual call | N | What it isolates |
|---|---|---|---|---|
| `noop` | `benchNoop()` | `epdf_bench_noop()` | 100,000 | pure FFI entry cost |
| `noop_hop` | `viewer.benchNoopHop()` | `epdf_bench_noop_hop` | 10,000 | FFI + kernel thread hop |
| `hit_count` | `search.hitCount()` | `ManualSearch.hitCount()` | 10,000 | small sync plugin call |
| `hits` lazy | `search.hits()` | `ManualSearch.hits()` | 200 | large nested record array; `report` with query `"e"` (thousands of hits) |
| `hits` eager | same | `Array(ManualSearch.hits().map(SearchHit.init))` | 200 | same, converted to Swift values |
| `search` | `search.search(_:)` | `ManualSearch.search(_:)` | 50 | promise + engine work (expect equal: engine-bound) |
| progress events | `search.events` | `epdf_search_subscribe` | 20 scans | events/s and first-event latency during `findAll("e")` |
| raster @1x, 2x, 3x | `render.renderPage` | `ManualRender.renderPage` | 50 each | copy vs zero-copy; also record peak RSS via `task_info` |

Each test asserts only that both bridges returned equal results: the same hit count, the same first hit, and the same image dimensions. Timing never fails a test. Results are printed as one JSON line per workload, prefixed `BENCH `, so a script can collect them from the `xcodebuild` log.

- [ ] **Step 8: Run on the perf machine**

Timed runs go to the dedicated perf machine, not the dev laptop: `user@remote-host` over SSH. That's the convention the viewer perf comparison already uses. Build Release for both Rust (`PROFILE=release`, the script default) and Swift:
```bash
ssh user@remote-host 'cd ~/repos/embed-pdf-viewer && git fetch && git checkout <branch> \
  && bash platforms/apple/build-xcframework.sh --experimental-manual-bridge \
  && cd platforms/apple/EmbedPDF \
  && EMBEDPDF_EXPERIMENTAL_BRIDGE=1 xcodebuild test -scheme EmbedPDF-Package -configuration Release \
       -destination "platform=iOS Simulator,name=iPhone 17" -only-testing:EmbedPDFBridgeBenchmarks 2>&1 | grep "^BENCH "' \
  > bench-simulator.jsonl
```

Simulator numbers are a proxy. The simulator runs arm64 natively on Apple Silicon, but with the Mac's cores, memory and thermals. A device run needs signing, which the repo doesn't have yet (`platforms/apple/README.md`, and B5's signed device run). Record the simulator results now. Add device numbers to the same doc when B5 lands, and don't make the final call on raster zero-copy until they exist.

- [ ] **Step 9: Write the results doc and commit**

`docs/research/swift-bridge-benchmarks.md` records the machine, macOS, Xcode, simulator and git SHA. It has one table per workload with UniFFI median/p95, manual median/p95 and the ratio, and a verdict per workload. It also records the code-size cost: lines of hand-written Rust, C and Swift for the manual search and render bridges, against the same members in Tasks 7 and 8, and the archive size with and without the manual bridge. A speedup has to be weighed against that maintenance cost. The recommendation names which entry points, if any, should get a hand-written path in the shipping package. That decision goes into B1 (pointer samples) and B5 (rasters).

Write the doc in the house voice (the `writing-style` skill, essay register for the analysis section).

```bash
git add crates/epdf-manual-bridge crates/epdf-viewer-host crates/epdf-ffi platforms/apple docs/research/swift-bridge-benchmarks.md
git commit -m "Measure a hand-written C bridge against uniffi for search and render"
```

---

## Appendix A: The per-plugin recipe and the API of every plugin crate

### A.0 Recipe for a plugin slice

Every follow-on plan repeats these steps for each plugin. Task 6 is the worked example.

1. **Feature.** Add `uniffi = ["dep:uniffi", "dep:epdf-ffi"]` to the crate, plus `#[cfg(feature = "uniffi")] uniffi::setup_scaffolding!();` and `#[cfg(feature = "uniffi")] pub mod ffi;` in `lib.rs`. Leave `default` alone.
2. **Records.** For each DTO the capability takes or returns, derive `uniffi::Record`/`uniffi::Enum` in place under `cfg_attr` if every field is UniFFI-compatible. Otherwise write a mirror in `ffi.rs` with `From` impls both ways. A mirror converts `usize` to `u32` (`u64` for byte counts), an `isize` `-1` sentinel to `Option<u32>`, `Rc<Vec<T>>` to `Vec<T>`, and `Value` to a typed record or a dropped field. Options drop `signal: Option<AbortSignal>`.
3. **Names.** A record's UniFFI name is unique across the workspace. Prefix with the plugin when the bare name is generic (`SearchStatus`, not `Status`). Use shared geometry from `epdf_ffi`.
4. **Handle.** `#[derive(uniffi::Object)] pub struct <Plugin>Handle { cap: KernelBound<<Plugin>Capability>, post: KernelPost }`, built with `new(cap, post) -> Arc<Self>` on the kernel thread.
5. **Methods.** One `pub async fn` per capability member, named in Swift style, snake_case in Rust (UniFFI camel-cases it): drop `get_` and `list_`, and keep `can_`, `is_` and `has_`. A synchronous member uses `cap.call`. A `Promise` member uses `cap.call_promise`. A hot-path member whose result nobody waits on (pointer samples) is a plain `pub fn` using `cap.fire`.
6. **Events.** `pub enum <Plugin>Event` with one variant per `on_*` hook, `{ event: <Record> }`. `#[uniffi::export(with_foreign)] pub trait <Plugin>EventListener: Send + Sync { fn on_event(&self, event: <Plugin>Event); }`. One `pub fn subscribe(&self, listener) -> Arc<Subscription>` registers every hook in one `fire` job.
7. **Ports.** A host port (a Rust trait or closure in the plugin's config) becomes a `with_foreign` trait in `ffi.rs` when Swift must supply it. Its adapter, which implements the Rust trait by calling the foreign one, lives in `ffi.rs` too. A port with a sensible Rust default (timers, logging) keeps the default.
8. **Tests.** `tests/ffi.rs` drives the handle over `plugin_conformance::test_kernel::TestKernel` and `FakeEngine`, without PDFium.
9. **Host.** Add the crate with `features = ["uniffi"]`. Add a `ViewerConfig` field (`bool` or `Option<<Plugin>Config>`), a `Session::build` branch, and an async `ViewerHost::<plugin>(document_id)` accessor (no id for workspace plugins).
10. **Swift.** Typealiases for the records. A `<Plugin>Capability` struct with one `bridged` line per method. The `EventRelay` one-liner. An `extension PDFViewer` accessor. Swift Testing tests against a real fixture.
11. **Engine paths.** Map any engine path the plugin calls that `translate.rs` doesn't map yet (Appendix C), with a translation test.

Below, each plugin's members are listed under their FFI names. The TypeScript member is in parentheses where it differs. Scope is D (document) or W (workspace). Ports marked "Swift" need a foreign trait; the rest keep their Rust default.

The core libraries `epdf-geometry`, `epdf-stage-core`, `epdf-ui-core` and `epdf-annotation-core` have no handle. Their types appear as records inside the handles that return them, and the shared ones live in `epdf-ffi`. `epdf-signature-core` and `epdf-acrojs` are reached only through `signature` and `actions`.

### A.1 Delivered by this plan

**metadata** (D), Task 6: `snapshot` (`getSnapshot`), `status` (`getStatus`), `can_edit`, `update`, `refresh`, `subscribe` (`onUpdated`).

**search** (D, optional `stage`), Task 7: `can_search`, `search`, `refresh`, `cancel`, `clear`, `next_hit`, `previous_hit`, `go_to_hit`, `reveal_active_hit`, `query` (`getQuery`), `status`, `hits` (`listHits`), `hit_count`, `pages_with_hits` (`listPagesWithHits`), `active_hit_index`, `active_hit`, `progress`, `error`, `find_all`, `subscribe` (7 events).

**render** (D), Task 8: `can_render`, `render_page`, `render_thumbnail`, `render_pages`, `render_epoch`, `invalidate`, `subscribe` (`onInvalidated`, `onRenderCompleted`, `onRenderFailed`). In B1: `render_policy`, `render_source`, `source_key`, `conform_viewport`, `paint_settings`, `create_view_demand`. Ports: `request_frame` keeps the kernel timer, and `release_image` is wired to `HostTable`.

### A.2 Navigation and input, follow-on plan B1

**stage** (D, optional `actions`). Port: `Scheduler` keeps kernel timers. Members:
- Read: `camera`, `viewport_size`, `scroll_metrics`, `visible_pages`, `is_page_visible`, `current_page_index`, `current_page`, `current_item_pages`, `page_frame`, `page_at`, `viewport_to_page`, `page_to_world`, `page_rect_to_viewport`, `world_to_viewport`, `viewport_to_world`, `page_to_viewport`, `view_rotation`, `zoom_level`, `zoom_mode`, `viewpoint`.
- Motion: `fling`, `is_moving`, `stop_motion`, `set_camera`, `pan_by`, `scroll_to`, `scroll_by`, `zoom_around`, `begin_gesture`, `end_gesture`.
- Navigation: `go_to_page_index`, `go_to_page`, `go_to_first_page`, `go_to_last_page`, `can_go_next`, `can_go_previous`, `next_page`, `previous_page`, `reset_view`, `reveal`, `reveal_rect`, `reveal_index`.
- Settings: `settings`, `reset_settings`, `update_settings`, `set_responsive_rules`, `matches_rule`, `active_rules`, `set_flow`, `set_layout`, `set_spread`, `set_sizing`, `apply_view_state`.
- Zoom: `double_tap_zoom`, `zoom_in`, `zoom_out`, `zoom_to`, `zoom_by`, `fit_width`, `fit_page`, `fit_all`, `fit_automatic`, `set_view_rotation`, `rotate_view_by`.
- Lifecycle: `set_viewport_size`, `set_device_pixel_ratio`, `refit`, `view_state`, `lens_id`, `provide_initial_view`, `place_initial`.
- `subscribe`: page, zoom, camera, motion-ended, settings and viewport events.

`pan_by`, `scroll_by`, `zoom_around` and `set_camera` are `fire` methods during a gesture.

**interaction** (D) plus **feedback** (W). Ports: `InteractionHandler` (Swift), `PlatformFeedback` (Swift: `UIImpactFeedbackGenerator`). Members: `active_tool`, `active_tool_id`, `default_tool_id`, `tools`, `tool`, `has_tool`, `active_tool_enables`, `activate_tool`, `activate_default_tool`, `push_tool`, `pop_tool`, `set_tool_cursor`, `register_tool`, `register_handler`, `claim_cursor`, `cursor`, `would_claim_touch`, `dispatch_pointer` (a `fire` method), `subscribe` (tool, gesture started/ended/cancelled, cursor). B1 measures `dispatch_pointer`'s round trip at 120 Hz before building gestures on it.

**selection** (D, requires `interaction`, optional `feedback`). Members: `can_select`, `can_copy`, `select`, `select_all`, `select_page`, `select_word_at`, `select_line_at`, `extend_to`, `clear`, `has_selection`, `snapshot`, `range`, `selected_pages`, `segments`, `rects`, `anchor`, `read_text`, `read_text_in_range`, `ensure_loaded`, `is_loaded`, `is_over_text`, `begin_gesture`, `begin_gesture_at`, `end_gesture`, `is_gesture_active`, `set_highlight_visible`, `is_highlight_visible`, `subscribe` (changed, committed, cleared).

**link** (D, requires `interaction`, optional `stage`, `annotation`, `actions`). Members: `links` (`listLinks`), `link`, `link_at`, `all_links`, `ensure_loaded`, `is_loaded`, `resolve`, `activate`, `activate_at`, `label`, `is_navigation_engaged`, `subscribe` (activated, loaded).

### A.3 Content editing, follow-on plan B2

Engine paths to map: `page.annotations.*`, `annotations.listRawAll`, `forms.*`, `redaction.apply`, `page.measure.*` (Appendix C).

**annotation** (D, requires `interaction`, optional `selection`, `actions`). Port: `AnnotationHost` keeps its defaults (logging, `random_uuid`, system fonts). It has 142 members. B2 splits them across several handle objects that mirror the TypeScript capability's sub-objects (`AnnotationHandle` returns `AnnotationDraftsHandle`, `AnnotationSelectionHandle`, `AnnotationThreadsHandle`, `AnnotationToolsHandle`), and copies the full list from `crates/plugins/annotation/src/harness.rs` `MEMBERS`. Events: resynced, draft captured, created, updated, deleted, selection changed, draft changed, editing changed, thread changed. `AnnotationState` and `AnnotationAction` are Serialize-only and carry engine `Value`s, so expect mirrors for most of this plugin's records.

**form** (D, requires `interaction`, optional `annotation`, `actions`). Members:
- Read: `snapshot`, `field`, `field_for_widget`, `fields`, `value`, `export_values`, `fill_items`, `fill_item`, `ensure_loaded`, `widget_at`, `page_box`, `widgets`, `refresh`, `status`.
- Fill: `set_value`, `set_text`, `set_checked`, `set_choice`, `set_value_raw`, `set_values`, `import_values`, `reset`, `reset_all`, `reset_form_action`.
- Design: `create_field`, `update_field`, `delete_field`, `detach_widget`, `attach_widget`.
- Data: `export_data`, `import_data`, `repair`.
- Scripting: `activate_widget`, `notify_widget_event`, `resolve_submit_dataset`, `commit_script_form_effects`.
- Permissions: `can_read`, `can_fill`, `can_design`.
- `subscribe`: value changed, field created/updated/deleted, validation rejected, resynced.

**redaction** (D, requires `annotation`, optional `selection`, `search`). Members: `pending` (`listPending`), `pending_item` (`getPending`), `pending_count`, `estimate_collateral`, `mark_selection`, `mark_area`, `mark_page`, `mark_matches`, `unmark`, `clear_pending`, `update_label`, `apply`, `apply_pages`, `apply_all`, `can_mark`, `can_apply`, `is_applying`, `last_result`, `subscribe` (applied, pending changed).

**measurement** (D, requires `annotation`, `interaction`). Members: `can_measure`, `page_scale`, `is_busy`, `last_reports`, `presets`, `units`, `area_units`, `readout`, `measure_distance`, `measure_area`, `calibration_request`, `set_scale`, `calibrate`, `set_unit`, `set_area_unit`, `set_precision`, `set_preset`, `clear_scale`, `start_calibration`, `dismiss_calibration`, `create_measurement`, `can_calibrate`, `ensure_loaded`, `subscribe` (scale changed, calibration requested/completed/dismissed).

**stamp** (W, optional `annotation`, `actions`). Port: `AssetEngine` is a second `NativeEngine` the host builds for stamp assets. Members: `libraries`, `library`, `assets`, `asset`, `asset_preview`, `render_asset_preview`, `read_asset_bytes`, `export_library`, `can_place`, `can_import`, `create_library`, `update_library`, `import_library`, `delete_library`, `create_asset`, `create_asset_from_annotations`, `update_asset`, `delete_asset`, `move_asset`, `duplicate_asset`, `arm_asset`, `place_asset`, `place_asset_on_pages`, `disarm`, `armed_asset`, `subscribe` (library changed/created/updated/deleted, asset created/updated/deleted, arm changed).

### A.4 Document and workspace, follow-on plan B3

**page-edit** (D), delivered by 10-page-edit.md. Engine paths: `pages.move`, `pages.rotate`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.extract`. Its state and actions are `Value` in Rust, so every record is new and modelled on `packages/plugin/page-edit`'s TypeScript types. Members: `can_edit`, `rotate_by`, `set_rotation`, `move_pages` (`move`, which is a keyword in Swift), `delete_pages` (`delete`), `insert_blank`, `insert_from_bytes` (takes `Vec<u8>`), `insert_from_document`, `duplicate`, `extract` (returns `Vec<u8>`).

**view-manager** (W, requires documents). Members: `panes`, `pane`, `pane_order`, `focused_pane_id`, `pane_of_document`, `create_pane`, `remove_pane`, `move_pane`, `set_focused_pane`, `split_pane`, `set_active_document`, `add_document`, `remove_document`, `move_document_within`, `move_document_between`, `subscribe` (pane created/removed, focus changed, document moved).

**shell** (D). Members: `is_open`, `surface`, `open_surfaces`, `open`, `close`, `toggle`, `update_surface_props`, `close_all`, `is_menu_open`, `open_menus`, `open_menu`, `close_menu`, `toggle_menu`, `close_all_menus`, `snapshot`, `apply_snapshot`, `subscribe` (surface opened/closed, menu opened/closed). Swift exposure is optional. Shell models web chrome, and B3 decides whether a native reader wants it.

**commands** (W, optional `i18n`, `shell`). Port: a Swift-defined command needs a `CommandRunner` foreign trait (Swift). Members: `register_command`, `register_commands`, `has_command`, `command`, `command_ids`, `resolve_command`, `commands`, `search_commands`, `shortcuts`, `execute`, `can_execute`, `disabled_categories`, `is_category_disabled`, `disable_category`, `enable_category`, `set_disabled_categories`, `match_stroke`, `menu_target`, `subscribe` (executed, execution failed).

**i18n** (W). Port: `Loader` (Swift: loads locale JSON from the app bundle). Members: `t`, `has_key`, `locale`, `direction`, `locales`, `loading_locale`, `set_locale`, `register_locale`, `add_translations`, `subscribe` (locale changed, load failed).

**actions** (D). Ports: `ScriptSandbox` (Swift: JavaScriptCore, `multiplatform-plugins-plan.md` §1; none, meaning scripting off, until B3 lands) and the `JavaScriptConfig` callbacks. Members: `policy`, `update_policy`, `register_executor`, `register_annot_commit_sink`, `register_form_commit_sink`, `set_submit_handler`, `register_submit_resolver`, `is_scripting_enabled`, `script_realm`, `create_detached_script_realm`, `surface_script_result`, `surface_script_commit`, `report_page_state`, `set_ui_adapter`, `run_document_verb`, `execute`, `can_execute`, `execute_named`, `action_tree`, `dispatch`, `can_dispatch`, `prepare_close`, `subscribe` (executed, diagnostic, script diagnostic, script error, open sequence completed). The `register_*` and `set_*` members take Swift implementations, so each becomes a foreign trait.

### A.5 Signatures, follow-on plan B4

**signature** (D, requires `form`, optional `interaction`, `stamp`, `annotation`). Ports: `SignerSource` (Swift: Keychain `SecIdentity`) and `TrustPort` (Swift: `SecTrust`). Engine paths `signatures.*` don't exist in `host.rs`, so B4 starts in `epdf-engine`. Members: `snapshot`, `status`, `signatures`, `unsigned_fields`, `signature`, `verdicts`, `verdict`, `protection`, `pending`, `is_busy`, `mode`, `target`, `refresh`, `validate`, `validate_field`, `analyze_changes`, `read_revision`, `set_target`, `request_inspection`, `sign`, `prepare_signature`, `complete_signature`, `abort_pending`, `fill_field`, `clear_field`, `place_mark`, `can_sign`, `can_fill`, `can_certify`, `subscribe` (signed, filled, cleared, validated, protection changed, invalidating, target changed, sign requested, inspection requested). Signature state is raw `Value` in Rust, so every record here is a mirror.

## Appendix B: Follow-on plans

| Plan | Contents | Depends on |
|---|---|---|
| B1 navigation | stage, interaction + feedback, selection and link slices; render's viewport members; `dispatch_pointer` latency at 120 Hz; a SwiftUI scrolling reader sample | this plan |
| B2 content | annotation, form, redaction, measurement and stamp slices; their engine paths | B1 (`interaction`) |
| B3 document | page-edit, view-manager, shell, commands, i18n and actions slices; JavaScriptCore `ScriptSandbox` | this plan |
| B4 signatures and security | `signatures.*` in `epdf-engine`; Keychain signer; `SecTrust` port; `NativeSecurity` that enforces document permissions | B2 (`form`) |
| B5 hardening | TSAN soak of two concurrent viewers (`multiplatform-plan.md` Phase 5); signed device run (Phase 1 exit), including Task 11's benchmarks on device; a hand-written raster path if Task 11 recommends one; binary size budget for all 19 slices; macOS slice once a static darwin PDFium exists | this plan |

## Appendix C: Kernel engine paths and their native status

Every path a plugin calls (`grep -rho 'call("…"' crates/plugins/*/src`), with what `translate.rs` does after this plan:

| Kernel path | Wire kind | Status after this plan |
|---|---|---|
| `close` | `close` | mapped (Task 2) |
| `pages.list` | `pages.list` | mapped (Task 2) |
| `metadata.read`, `metadata.update` | same | mapped (Task 2) |
| `page.text.read` | `pages.text` | mapped (Task 2) |
| `page.geometry.read` | `pages.geometry` | mapped (Task 2) |
| `page.render.image` | `pages.render` | mapped (Task 2) |
| `search.query` | `search.query` | mapped (Task 2) |
| `pages.move`, `pages.rotate`, `pages.delete`, `pages.insert`, `pages.insertBlank`, `pages.extract` | same | mapped (10-page-edit.md) |
| `page.annotations.list` / `create` / `update` / `delete` | `annotations.listRawPage` / `create` / `update` / `delete` | B2 |
| `annotations.listRawAll` | same | B2 |
| `forms.list`, `forms.setValue`, `forms.createField`, `forms.updateField`, `forms.deleteField`, `forms.repair`, `forms.applyEffects` | same | B2 |
| `forms.exportData`, `forms.importData` | `forms.export`, `forms.import` | B2 |
| `forms.submit` | none on the wire | B3 (host submit handler) |
| `redaction.apply` | same | B2 |
| `page.measure.setScale`, `page.measure.viewports` | none on the wire | B2 (needs an `epdf-engine` addition) |
| `actions.read` | same | B3 |
| `signatures.*` (`list`, `analyze`, `contents`, `prepare`, `abort`, `revisionBytes`) | none on the wire | B4 (needs `epdf-engine` work) |
