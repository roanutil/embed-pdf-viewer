# rust-core-scaffold

`embedpdf/runtime` ships nine targets, `wasm32`, two darwin, two linux, two linuxmusl and two win32,
and neither mobile one. `ios-arm64`, `ios-sim-arm64`, `android-arm64` and `android-x64` are pending:
`build/runtime-build.json` carries them with `"status": "pending"` instead of a `url` and `sha256`,
and `build/fetch-libpdfium.sh <target>` exits 2 naming the target when you ask for one. The practical
consequence is that the iOS destination and the Android APK cannot be built here. Adding those two
targets to the fork is recommendation 6 of
[the runtime decision](../docs/research/pdfium-rust-wrapper-decision.md) and has its own design doc;
it is not this scaffold's job to do that work, only to be ready for it.

## What this is

A proof that a Rust core over PDFium can bind cleanly to Swift, Kotlin, Node and the web, built
before committing to the real port described in [the port plan](../docs/research/rust-core-port.md).
It lives at the repo root, outside `packages/**`, so nothing published depends on it and no workspace
glob has to remember to exclude it. Deleting it costs `rm -rf rust-core-scaffold` and nothing else,
the same contract `pdfium-rust-spike/README.md` opens with.

## The six operations, and why the boundary is coarse

Two uniffi objects, six methods:

```
Engine::new() -> Arc<Engine>
Engine::open(bytes, password) -> Result<Arc<Document>, OpsError>
Document::page_count() -> u32
Document::page_size(index) -> Result<Size, OpsError>
Document::render_page(index, scale) -> Result<Bitmap, OpsError>
Document::page_text(index) -> Result<String, OpsError>
Document::search(query, case_sensitive) -> Result<Vec<SearchHit>, OpsError>
```

No host reaches an arbitrary `FPDF_*` call. `pdfium-sys` stays internal to the crate graph, and every
host talks to `epdf-ops` through a whole operation, never a single PDFium function. That is
recommendation 3 of [the runtime decision](../docs/research/pdfium-rust-wrapper-decision.md), read
literally: a per-item JavaScript boundary measured 2.6x slower than a per-operation one in wasm, and
7.16x slower on Node, on the same glyph-geometry workload. Exposing PDFium's C ABI to a host the way
`@embedpdf/engine-runtime` exposes it to TypeScript today would be building the expensive shape on
purpose. This scaffold's whole point is proving the cheap shape works end to end, so it does not
build the other one to compare.

That coarseness is also the headline result. The render vectors in `vectors/report-page4.json`
freeze a 121,176-byte BGRA golden buffer produced once by driving the shipping TypeScript runtime,
and every test suite in this scaffold renders the same page at the same scale and diffs its own
output against that buffer. Every one of them reports a mean absolute difference of 0 and a max
absolute difference of 0: Rust calling PDFium directly (`crates/pdfium`), Rust through the
worker-thread dispatch (`crates/epdf-ops`), Swift through uniffi, JavaScript through napi's `Buffer`,
JavaScript through the em++ module's linear memory, and Kotlin through JNA. Six code paths agreeing,
five of them crossing a language boundary, and none of them move a byte. The golden was not produced
by any of the six, so none of this is self-referential.

That match does not, on its own, prove channel order. Page 4 is entirely grayscale: B == G == R in
every one of its 30,294 pixels, alpha 255 throughout. A host that read PDFium's BGRA output as RGBA,
or swapped any two of B, G and R, would diff against this golden at mean 0 and max 0 too.
`apps/web-demo/bgra-to-rgba.test.mjs`, `bgraToCGImagePreservesChannelOrder` in `VectorsTests.swift`,
and `bgraToArgbPacksChannelsCorrectly` in `VectorsTest.kt` cover that gap directly, with synthetic
buffers whose three channels are never equal.

## The crates

Nine crates for six operations is more ceremony than the code deserves, and that is deliberate.

```
crates/
  build-support/   shared build.rs logic: resolves the fork's libpdfium per target, emits link directives
  pdfium-sys/      bindgen over the fork headers, FPDF|EPDF|FORM|PDFiumExt_ filter
  pdfium/          safe RAII: Library, Document, Page, TextPage, Bitmap
  test-support/    fixture bytes, frozen vectors, pixel-diff helpers shared by every Rust test
  epdf-ops/        the six coarse operations, the only surface any host sees
  epdf-uniffi/     uniffi's newtype view of epdf-ops, for Swift and Kotlin
  epdf-node/       napi-rs addon over epdf-ops
  epdf-wasm/       staticlib for wasm32-unknown-emscripten, extern "C" exports
  uniffi-bindgen/  the local bindgen binary uniffi 0.32 wants
```

Only `pdfium-sys` and `pdfium` survive into the real port. Everything else here is a per-host
adapter, thrown away or rewritten once the port owns the boundary directly. Keeping that line visible
now, crate by crate, is cheaper than discovering it later.

`build-support` is not a per-host adapter; it exists because of a Cargo limitation, not a host
requirement. Cargo does not propagate a dependency's `cargo:rustc-link-arg` to a dependent package's
own test or bin targets, only `rustc-link-lib` and `rustc-link-search` are transitive, so every crate
that links `libembedpdf` (directly, like `pdfium-sys`, or by re-emitting the rpath half, like
`pdfium`, `epdf-ops`, `epdf-uniffi`, `epdf-node` and `epdf-wasm`) needs its own `build.rs`. The
fork's per-target artifact shape and its Node-spelled target names (`darwin-arm64`, not
`aarch64-apple-darwin`) were duplicated across those build scripts before this crate existed, and had
already diverged. `build-support` is the one place that logic lives now.

`epdf-uniffi` is its own crate rather than living on `epdf-ops`'s types for one reason: `epdf-ops`
has to compile for `wasm32-unknown-emscripten`, and whether `uniffi::setup_scaffolding!()` builds for
that target was unverified when this scaffold was designed. A separate crate means the question never
has to be answered. The cost is a newtype and a `From` impl per type, visible in
`crates/epdf-uniffi/src/lib.rs`.

## The thread thing

`runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77` sets `EMBEDPDF_TLS_GLOBALS=true` for
every target except `wasm32`, so the server worker pool can render in parallel in-process. PDFium's
globals are per-thread in every native build this scaffold links against, which means a document
handle cannot be touched from a thread other than the one that opened it.

uniffi requires exported objects to be `Send + Sync`. A `Mutex` does not reconcile that with
thread-confined globals: a mutex serializes access, it does not pin a thread, and Swift's
concurrency runtime will happily call a `Mutex`-wrapped handle from whatever thread it feels like, on
a PDFium instance that was never initialized there. So `Engine` owns one dedicated OS thread
(`crates/epdf-ops/src/dispatch/threaded.rs`), and every operation is a channel round-trip to it;
handles never leave. Under `cfg(target_os = "emscripten")` the same `dispatch` module compiles to
direct calls instead (`crates/epdf-ops/src/dispatch/inline.rs`), because wasm is single-threaded and
each instance already isolates its globals in its own linear memory.

## The frozen vectors

`vectors/report-page4.json` and `vectors/report-p4-scale0.25.bgra` were written once by
`build/generate-vectors.mjs`, which drives `@embedpdf/engine-runtime` and
`@embedpdf/engine-services` over `fixtures/report.pdf` and prints page 4's page size, extracted
text, glyph count, search hits and a rendered bitmap. Page 4, because that is the page
`pdfium-rust-spike/bench/fixture.mjs` uses, so the numbers were already a committed receipt rather
than one this scaffold invented. `build/generate-vectors.mjs` is never run by any test or by
`build/test-all.sh`; it is a one-time generator, and every suite in this repository only ever reads
the two files it already wrote. A host that drifts from those numbers fails a test instead of being
discovered in a browser.

## Every host

| Host | Build | Test | Runs today |
|---|---|---|---|
| Rust (native) | `cargo build --workspace` | `cargo test --workspace` | yes |
| Rust (wasm32-unknown-emscripten) | `bash build/link-wasm.sh` | (covered by the web suite below) | yes |
| Swift, macOS destination | `bash build/build-swift.sh` | `bash build/test-swift.sh` | yes |
| Swift, iOS destination | `bash build/build-swift.sh ios-arm64` | n/a | no, `ios-arm64` is pending |
| Kotlin, JVM desktop | `bash build/build-kotlin.sh` | `cd packages/kotlin && ./gradlew :epdf-ops:test` | yes |
| Kotlin, Android | `bash build/build-kotlin.sh android-arm64` | n/a | no, `android-arm64` is pending |
| Node (napi) | `bash build/build-node.sh` | `node --test packages/node/*.test.mjs` | yes |
| Web (em++ module) | `bash build/link-wasm.sh` | `node --test packages/web/*.test.mjs` | yes |
| `apps/node-cli` | n/a | `node --test apps/node-cli/*.test.mjs` | yes |
| `apps/web-demo` | `npm run build` (in `apps/web-demo`) | `node --test bgra-to-rgba.test.mjs`, not wired into any script | builds; its one test passes when run by hand |
| `apps/ios`, macOS destination | `bash build/build-ios-app.sh` | none wired | yes, builds |
| `apps/ios`, iOS destination | `bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 16'` | none wired | no, `ios-sim-arm64` is pending |
| `apps/android` | `./gradlew :app:compileDebugKotlin` | `./gradlew :app:testDebugUnitTest` | compiles and tests; `assembleDebug` and every other packaging task fail on purpose, `android-arm64` is pending |

`bash build/test-all.sh` runs every row marked yes in the Test column, in the order above, and prints
what it did not run and why.

## Tool versions, measured on the machine that verified this scaffold

`rustc 1.98.0`, every crate `edition = "2024"`. `bindgen = "=0.72.1"`, `uniffi = "=0.32.0"`,
`napi = "=3.12.2"`, `napi-derive = "=3.6.3"`, `napi-build = "=2.4.1"`, `cargo-ndk 4.1.2`. emsdk
3.1.72, with `wasm-opt` version 132 shadowing the SDK's bundled v119, which rejects two wasm
features present in rustc's precompiled std. Swift 6.3.3, Xcode 26.6 build 17F113, XcodeGen 2.46.0.
Gradle 9.7.1, Kotlin 2.2.20, Android Gradle Plugin 9.4.0. Android SDK `platforms/android-37.0`,
`build-tools/36.0.0`, NDK `29.0.14206865`. Two JDKs: Android Studio's bundled JBR, OpenJDK 25.0.3 at
`/Applications/Android Studio.app/Contents/jbr/Contents/Home`, which every `gradlew` invocation here
needs as `JAVA_HOME` because neither `/usr/bin/java` nor `/usr/libexec/java_home` finds it on their
own; and Eclipse Adoptium 21.0.12.1+1, provisioned by Gradle's Foojay resolver, because Kotlin 2.2.20
refuses a JVM target of 25. Node 24.20.0, npm 11.19.0, Vite 6.x. Fork pin
`d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`.

## Five things a reader will otherwise be surprised by

**`epdf-wasm` builds with `-C panic=abort`, only on `wasm32-unknown-emscripten`.** rustc's std for
that target lowers Rust panics through native WebAssembly exception handling, which needs
`em++`'s `-fwasm-exceptions`. The fork's prebuilt `libembedpdf.a` needs the older
`-sSUPPORT_LONGJMP=emscripten` instead, because PDFium genuinely uses `setjmp`/`longjmp` for libpng
and libjpeg error handling (`core/fxcodec/png/pngmodule.cpp`, `libpng_png_decoder.cpp`,
`core/fxcodec/jpeg/jpeg_common.{c,h}`). `em++` refuses both flags on the same link. `build/link-wasm.sh`
resolves it by building `epdf-wasm` alone with `panic=abort`, which costs recoverability, not
correctness: a genuine Rust panic aborts the wasm instance instead of surfacing as `OpsError::Internal`,
so the engine dies and the page has to reload. Native is unaffected and still unwinds.

**The Kotlin API is not name-identical to Swift's.** `EpdfDocument.close` is `closeSync` in Kotlin,
and the `EngineFailed` and `Internal` error variants' `message` fields are `detail`, all renamed by
`packages/kotlin/uniffi.toml` because uniffi's Kotlin generator claims `close()` and `message` for
its own `AutoCloseable` boilerplate and would otherwise collide. `use { }` calls uniffi's own
non-blocking `close()`; reach for the explicit `closeSync()` when a call site needs the document's
slot in the engine to be gone the instant the call returns. `packages/kotlin/README.md` has the full
account.

**win32 builds but cannot run.** PE has no rpath equivalent, so `build-support`'s link directives
have nothing to emit for locating `embedpdf.dll` at runtime, and the import-library link name
(`embedpdf.dll`, not `embedpdf`) has never been exercised on a real MSVC toolchain.

**`swift build` does not resolve on a fresh checkout.** `packages/swift/EpdfOps`'s generated
sources, its modulemap, and the staged dylibs are all gitignored. Run `bash build/build-swift.sh`
first.

**`charCount` in the frozen vectors is 8,106.** That is UTF-16 code units: JavaScript's
`text.length` and Kotlin's `String.length` return it directly, verified with
`text.utf16.count` in Swift too. Swift's default `String.count` gives 7,992 on the same text,
because it counts grapheme clusters and the fixture's page 4 text holds 114 combining sequences.
Neither number is wrong; they are counting different things, and no test in this scaffold currently
asserts `charCount` against either language's string length, so nothing catches a host that picks
the wrong one.
