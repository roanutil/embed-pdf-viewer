# A multiplatform scaffold for the Rust core

I went looking for the iOS `libembedpdf` and there isn't one.

`packages/engine/runtime/engine-runtime-build.json` pins nine targets: `wasm32`, two darwin, two
linux, two linuxmusl, two win32. The fork's latest release ships exactly those nine assets, and
`runtime-src/scripts/embedpdf-runtime/build-target.sh` has a `case` arm for each and no others.
Recommendation 6 of [the runtime decision](../../research/pdfium-rust-wrapper-decision.md) says to
add iOS and Android, and calls it independent of every port wave. Nobody has done it.

So the thing this scaffold is supposed to prove, that `packages/engine/runtime` is feasible on web
and native mobile, is half-blocked before the first line. That shapes everything below, and I would
rather lead with it than bury it in a caveat.

The decision: build the scaffold now, with its mobile build wiring written against target names that
do not exist yet, and do the fork matrix work in parallel as its own spec against
`embedpdf/runtime`. The cost is real. Two of the four demo surfaces will not run when this is
finished, and a scaffold you cannot run is a scaffold you half-believe.

## Where it lives

`rust-core-scaffold/` at the repo root, next to `pdfium-rust-spike/` and `ts-wasm-interop-poc/`.

The `pnpm-workspace.yaml` globs cover `packages/**`, `examples/**`, `cloudpdf/**`, `tooling/*`,
`website` and `docs/*`, so a root-level directory sits outside the workspace without anyone having
to remember to exclude it. Nothing published depends on it. A dead end costs
`rm -rf rust-core-scaffold`, which is the same contract `pdfium-rust-spike/README.md` opens with and
the reason both spikes have been safe to be wrong in.

```
rust-core-scaffold/
  README.md  rust-toolchain.toml  Cargo.toml
  crates/
    pdfium-sys/   bindgen over the fork headers, FPDF|EPDF|FORM|PDFiumExt_ filter
    pdfium/       safe RAII: Library, Document, Page, TextPage, Bitmap
    epdf-ops/     the six coarse operations, the only surface any host sees
    epdf-uniffi/  uniffi's newtype view of epdf-ops, for Swift and Kotlin
    epdf-node/    napi-rs addon over epdf-ops
    epdf-wasm/    staticlib for wasm32-unknown-emscripten, extern "C" exports
    uniffi-bindgen/  the local bindgen binary uniffi 0.32 wants
  build/
    runtime-build.json  fetch-libpdfium.sh  build-swift.sh  build-kotlin.sh
    build-node.sh  link-wasm.sh
  packages/
    swift/EpdfOps/    Package.swift, generated sources plus a facade, swift-testing tests
    kotlin/epdf-ops/  Gradle library module, generated Kotlin, jniLibs, JUnit tests
    node/             private package.json, napi loader, node --test
    web/              em++ module output, TypeScript glue, tests
  apps/
    ios/       SwiftUI, multiplatform, Xcode project referencing the local Swift package
    android/   Jetpack Compose, depends on :epdf-ops
    node-cli/  opens, prints, writes a PNG
    web-demo/  Vite page, canvas
  fixtures/report.pdf        copy of examples/snippet-react/public/report.pdf
  vectors/report-page4.json  committed parity vectors, read by all four test suites
```

Seven crates for six operations is more ceremony than the code deserves. I want it anyway, because
`pdfium-sys` and `pdfium` are the two that survive into the real port while the rest are per-host
adapters that get thrown away or rewritten. Keeping that line visible now is cheaper than discovering
it later.

uniffi lives in `epdf-uniffi` rather than on `epdf-ops`'s own types for one reason: `epdf-ops` has to
compile for `wasm32-unknown-emscripten`, and whether `uniffi::setup_scaffolding!()` builds for that
target is unverified. A separate crate means the question never has to be answered. The cost is a
newtype and a `From` impl per type.

The JavaScript side uses `npm` with its own lockfile inside the directory, not the repo's `pnpm`
workspace, because that is what `pdfium-rust-spike` does and it is what keeps the directory
self-contained. `rust-toolchain.toml` pins `1.98.0` with the targets each host needs, matching
`pdfium-rust-spike/rust-toolchain.toml`.

The dependency pins are `uniffi = "=0.32.0"`, `napi = "=3.12.2"`, `napi-derive = "=3.6.3"`,
`napi-build = "=2.4.1"`, `bindgen = "=0.72.1"` and `cargo-ndk 4.1.2`. Exact, the way
`ts-wasm-interop-poc` pins `wasm-bindgen = "=0.2.127"` and fails the build on a CLI mismatch. The
declared MSRVs are 1.88 for the napi crates, 1.86 for `cargo-ndk` and 1.70.0 for `bindgen`, all under
the 1.98.0 in `rust-toolchain.toml`. `uniffi 0.32.0` declares no MSRV.

(I first wrote that these were unresolved because crates.io was unreachable. It wasn't. The API
returns an empty body without a `User-Agent` header, and I read that as a network failure. Update:
resolved, and the pins above are the real ones.)

One pin that is a decision rather than a lookup: `[profile.release]` does not set `panic = "abort"`,
which is what `pdfium-rust-spike/Cargo.toml` uses. Both uniffi and napi map a Rust panic to a
host-language exception through `catch_unwind`, and `panic = "abort"` deletes that. So this scaffold
unwinds, and pays the code size for it.

`build/fetch-libpdfium.sh` is adapted from `packages/engine/runtime/scripts/fetch-libpdfium.sh` and
`build/link-wasm.sh` from `pdfium-rust-spike/build/link-wasm.sh`. Neither is interesting. Both are
already known to work.

## What crosses the boundary

Six operations on two uniffi objects, and nothing finer:

```rust
Engine::new() -> Arc<Engine>
Engine::open(bytes: Vec<u8>, password: Option<String>) -> Result<Arc<Document>, OpsError>
Document::page_count() -> u32
Document::page_size(index: u32) -> Result<Size, OpsError>
Document::render_page(index: u32, scale: f32) -> Result<Bitmap, OpsError>
Document::page_text(index: u32) -> Result<String, OpsError>
Document::search(query: String, case_sensitive: bool) -> Result<Vec<SearchHit>, OpsError>
```

`Size` is two `f32` in points. `Bitmap` is `width`, `height`, `stride` and a BGRA `Vec<u8>`, which
uniffi copies once on the way out. `SearchHit` is `char_index`, `char_count` and a `Vec<Rect>` of
page-space rects. `OpsError` is a uniffi error enum: `LoadFailed`, `PasswordRequired`,
`PageOutOfRange`, `RenderFailed`, `Internal(String)`.

Both objects also get an explicit `close()`. Swift releases a uniffi object on ARC and that is fine,
but Kotlin releases it on the GC, whenever that is, and a PDFium document holding a file's worth of
memory until a collection happens is not behavior I want to ship even in a scaffold. So `close()` is
idempotent, and dropping without it still frees.

This is recommendation 3 of the runtime decision, taken literally. That document prices a per-item
boundary at 2.6x in wasm and 7.16x natively against a per-operation one, so the temptation to expose
`PdfFunctions` the way `@embedpdf/engine-runtime` exposes it to TypeScript today is a temptation to
build the expensive thing on purpose. `pdfium-sys` stays internal. No host reaches an arbitrary
`FPDF_*` call.

(The literal reading of "a Rust crate that wraps `packages/engine/runtime`" would be the 1:1 mirror,
and I am deliberately not building it. The runtime's job is to hand the raw C ABI to JavaScript. Once
Rust owns the loop, that job stops existing, which is the whole point of the port.)

## The thread thing

Here is the constraint I did not expect to matter this early.

`runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77` sets `embedpdf_thread_local_globals=true`
for every target except `wasm32`, with the comment "so the server worker pool can render in parallel
in-process". PDFium's globals are per-thread in every native build we ship. So `FPDF_InitLibrary` is
per-thread, and a document handle cannot be touched from a thread other than the one that opened it.

And uniffi requires that exported objects be `Send + Sync`.

A `Mutex` does not fix this. A mutex serializes access; it does not pin the thread. Wrap the handle
in one and Swift will call it from whatever thread the Swift concurrency runtime feels like, on a
PDFium instance that was never initialized there.

So `Engine` owns one dedicated OS thread. Every operation is a channel round-trip to it, and handles
never leave. Under `cfg(target_os = "emscripten")` the same `dispatch` module compiles to direct
calls instead, because wasm is single-threaded and each instance already isolates its globals in its
own linear memory. Two implementations, one small module, chosen by cfg.

That thread is also the only sane place to put cancellation later. This scaffold does not answer the
wave-0 cancellation question from [the port plan](../../research/rust-core-port.md), and I am not
going to pretend the ops are abortable. What it does is stop the answer from requiring a redesign.

One more thing `pdfium-sys/build.rs` has to swallow: the fetched tarballs are not the same shape per
target. `darwin-arm64` ships `lib/libembedpdf.dylib`, `wasm32` ships `lib/libembedpdf.a`, because
`build-target.sh` sets `pdf_is_complete_lib=false` for darwin and true for wasm. iOS will need the
static one, and Android a `.so`. That is an input to the fork spec, not a thing this scaffold can
decide.

## What actually gets verified

I checked this machine before promising anything. `rustc 1.98.0`, `Xcode 26.6`, `Swift 6.3.3`. No
JDK: `/usr/libexec/java_home` fails outright. No Android SDK, no NDK, no `gradle`, no `cargo-ndk`.
The installed rustup targets are `aarch64-apple-darwin`, `wasm32-unknown-unknown` and
`wasm32-wasip2`, so `wasm32-unknown-emscripten` and every Apple and Android target still need
adding. emsdk 3.1.72 is already cloned at `pdfium-rust-spike/build/emsdk` and comes back by sourcing
`build/emsdk.sh`, but `em++` is not on `PATH` and the newer-`wasm-opt`-over-bundled-v119 substitution
that `pdfium-rust-spike/README.md` describes still has to be made.

Four of four host bindings get a real test run against `darwin-arm64`: `cargo test`, swift-testing on
the macOS destination, `node --test` against the napi addon, and the web package's em++ module under
Node. The Kotlin JUnit tests can join them on JVM desktop, against the same dylib, the moment a JDK
exists locally. That decoupling is deliberate: it means the Kotlin binding is verifiable without
waiting on the fork, and only the Android app is not.

`apps/node-cli` and `apps/web-demo` run. `apps/ios` runs on its macOS destination, which is why it is
a multiplatform SwiftUI target rather than iOS-only, and waits on `ios-arm64` and `ios-sim-arm64` for
the destination anyone actually cares about. `apps/android` waits on both the fork artifact and a
local toolchain install.

The gate is loud rather than quiet. `build/runtime-build.json` carries `ios-arm64`, `ios-sim-arm64`,
`android-arm64` and `android-x64` with `"status": "pending"` where `url` and `sha256` go, and
`build/fetch-libpdfium.sh` exits 2 naming the target and pointing at the fork spec. The README says
it in the first paragraph.

## Pinning it with vectors

`vectors/report-page4.json`, written once by `build/generate-vectors.mjs`, which drives
`@embedpdf/engine-runtime` and the `@embedpdf/engine-services` readers over `fixtures/report.pdf` and
prints the JSON. Page 4 because that is the page `pdfium-rust-spike/bench/fixture.mjs:19-31` uses and
`pdfium-rust-spike/bench/results/a-r1.json` records at 8,106 glyphs, so the glyph count is already a
committed receipt rather than a number I would be inventing. Then frozen. Never regenerated by a test run, and the script is not wired into any
task. That is the mechanism `ts-wasm-interop-poc/vectors/anchor-vectors.json`
established and the one the port plan leans on for wave 2.

Exact assertions on page count, page 4 size, extracted text, glyph count and search hits. Rendering
is different: I compare against a committed PNG by pixel diff with a tolerance, not by hash. A hash
pins float behavior and rasterization across four architectures, and it would fail for reasons that
have nothing to do with our code. Wave 2 step 1 of the port plan says "verified by pixel diff" for
the same job, so this is not a new invention.

I do not know whether the fixture's page 4 text will come out byte-identical from the `wasm32` and
`darwin-arm64` builds of the same fork SHA. It should. If it doesn't, that is a genuinely useful
finding and the vectors file is where it surfaces.

## The follow-up

The fork matrix gets its own design doc and its own PR to `embedpdf/runtime`: `target_os="ios"` and
`target_os="android"` arms in `build-target.sh`, `pdf_is_complete_lib=true` for iOS, a gclient
`target_os` list in `ensure-deps.sh` to pull the NDK and the iOS toolchain, `package-target.sh`
output, and matrix rows in `.github/workflows/release-libpdfium.yml`.

Until that lands, this scaffold proves the bindings and leaves the phones alone.
