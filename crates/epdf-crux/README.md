# epdf-crux: Crux as the platform wrapper

The question was whether [Crux](https://github.com/redbadger/crux) could generate the platform
wrappers for the Rust core. Crux isn't a binding generator. It's an app
architecture: a pure core, a shell that does all the I/O, and typed messages between them. So I
went in expecting to find the seam where it stops fitting.

It fits better than I expected. There's one seam, and it's in a place I didn't predict.

## The mismatch

A Crux app is a function from an event and a model to a new model plus a list of effects. The app
never does anything itself. It asks. The shell (Swift, Kotlin, a browser) performs the HTTP call or
reads the file, then hands the answer back. That's why Crux cores are so easy to test: every effect
is a value you can resolve by hand.

PDFium breaks this. From Crux's point of view, a PDFium call is I/O: slow, stateful, and nothing a
pure `update` is allowed to do. But it can't go to the shell either. The whole point of the port is
that the engine lives in Rust, on one owner thread, and never crosses the FFI.

So the engine has to be an effect that Rust resolves, not the shell.

## The fit: middleware

Crux has a layer for exactly that, and it's marked experimental. `crux_core::middleware` lets you
wrap the core in an `EffectMiddleware` that takes one effect type, resolves it in Rust, and passes
every other effect up to the shell. Crux's own example uses it for a random number generator on a
background thread.

Swap "random number generator" for "PDFium owner thread" and you have this crate:

- [`src/app.rs`](src/app.rs) is a normal Crux `App`. It asks for an `EngineRequest` (the same JSON
  envelope plus payloads that `Host::call` takes) and never sees PDFium.
- [`src/middleware.rs`](src/middleware.rs) resolves those requests on the existing
  `OwnerThread<Host>`.
- [`ShellEffect`](src/lib.rs) is what's left for a shell: redraw, a finished raster, and the answer
  to an untyped call.

One change was needed in the engine. `OwnerThread::run` blocks, and Crux asserts that a middleware
never resolves synchronously on the calling thread. Worse, when a resolved effect asks for the next
one, the middleware gets called from the owner thread itself, where `run` would deadlock. So
[`dispatch.rs`](../epdf-engine/src/dispatch.rs) gains `OwnerThread::submit`, which queues a job and
returns. The test for it submits a follow-up from inside a job, because that's the case that
matters.

## What a shell sees

`cargo run -p epdf-crux --features codegen --bin codegen -- swift <dir>` generates a Swift package
with `Event`, `ViewModel`, `DocumentState`, `PageBitmap` and the effect enum as real Swift types.
Kotlin and TypeScript generate too. The FFI itself is uniffi, the version the workspace already
pins, exporting Crux's three functions (`update`, `resolve`, `view`) plus a `CruxShell` callback.
Crux only moves bytes, so it doesn't care what generates the FFI.

(Crux's own examples moved from uniffi to BoltFFI in 0.19, and master deprecates the uniffi
path. That deprecation is about Crux's bundled bindgen helper. We export our own functions, so it
doesn't touch us.)

[`shells/swift-cli`](shells/swift-cli) is a command-line Swift shell over all of it. On
`annotation_kinds.pdf` it opened the document in 6 ms, rendered page 0 at scale 2 (1224 by 1584
RGBA), wrote a correct PNG, and read metadata through the untyped `Call` event. The Swift code never
writes a wire envelope except for that last call, and never parses engine JSON at all.

```sh
crates/epdf-crux/shells/swift-cli/generate.sh
cd crates/epdf-crux/shells/swift-cli
swift run -c release Shell ../../../../tooling/engine-conformance/fixtures/annotation_kinds.pdf page.png
```

Both need `PDF_RUNTIME_LIB_DIR` pointing at the fork's `darwin-arm64` prebuilt if this checkout
doesn't have it.

## The seam: bytes

The first version put the raster in the effect as a `Vec<u8>`. The render round trip took 719 ms
with a release Swift build, and 240 ms of that was Swift decoding the bincode. The generated
deserializer reads a byte array one element at a time. In a debug build it was 2 seconds.

That's not a bug in Crux, exactly. Its messages are meant to be small. But ours aren't: a PDF is
megabytes going in and a raster is megabytes coming out.

So bytes don't go through Crux at all. [`src/blobs.rs`](src/blobs.rs) is a store shared by the FFI
and the middleware. The shell calls `put_blob` with the document, which crosses as uniffi `Data`
(a memcpy), and sends `Event::Open` with the returned `Blob` id. The middleware swaps ids for bytes
before calling the engine and bytes for ids after. The shell takes the raster with `take_blob`.
Same render, 269 ms, and the decode line is gone. Most of what's left is the Rust side, which was a
debug build for that measurement.

The cost is that every blob has to be taken exactly once. A shell that ignores a bitmap leaks it
until the core is dropped. I think that's the right trade for a spike, and the wrong one to ship
without a sweep.

## What else it costs

Engine answers arrive through the `CruxShell` callback on the PDFium owner thread. A UI shell has to
hop to its main thread there, every time, and nothing enforces that.

The internal events (`Opened`, `Rendered`, `Closed`) are hidden from the shell with
`#[serde(skip)]` and `#[facet(skip)]`, and they sit last in the enum. Bincode addresses variants by
index, so keeping them last guarantees the generated Swift cases number the same as Rust's. I
didn't test what happens with one in the middle, and I wouldn't find out in production.

`From<app::Effect> for ShellEffect` has an `unreachable!` for the engine variant. It holds as long
as the middleware is installed. A construction mistake is a panic across the FFI.

Crux is pre-1.0. This pins `crux_core = "=0.20.0"` from crates.io, and master is already 0.21.

## The bigger question

Here's the part I'd want you to think hard about. The typed surface a shell gets is whatever the
app models. Right now that's open, close, render and a page list, plus `Call` as an escape hatch for
the other sixty-odd wire kinds. Typing all of them means an event per kind, which is the
sixty-five-signatures problem `host.rs` was written to avoid, just moved into Rust where the
compiler at least checks it.

And the moment the app holds real state (documents, pages, eventually selection and zoom), it stops
being a wrapper. It's a viewer core in Rust, shared by every platform. That might be exactly what we
want! It's also a much bigger decision than "which tool generates the bindings", and the plan
currently puts that state in handwritten platform facades.

## Not done

The web. Crux's web shells compile the core with wasm-bindgen for `wasm32-unknown-unknown`, and our
PDFium is an emscripten build (`epdf-wasm`). The middleware also assumes a thread. I think a
single-threaded version could queue engine requests and resolve them from a `pump()` the JS calls
after each `update`, but I haven't tried it, and I don't know yet whether the Crux core and the
emscripten module can share one binary cleanly.

Android. The Kotlin types generate. No Kotlin shell has run against them.

So: Crux can be the wrapper. Whether it should depends on whether we want the viewer core in Rust.
