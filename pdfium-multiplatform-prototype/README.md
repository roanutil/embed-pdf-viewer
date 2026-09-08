# pdfium-multiplatform-prototype

`embedpdf/runtime` ships nine targets from a published release, `wasm32`, two darwin, two linux, two
linuxmusl and two win32. `ios-arm64`, `ios-sim-arm64`, `android-arm64`, `android-x64` and `wasm32-eh`
are pending: `build/runtime-build.json` carries all five with `"status": "pending"` instead of a
`url` and `sha256`, and `build/fetch-libpdfium.sh <target>` exits 2 naming the target when you ask
for one. The fork does build all five now, from commit `7f0fa5dbe`; there just isn't a release
publishing them yet, and this checkout has no push access to cut one. Verifying that the scaffold's
code was ready for them meant pinning local `file://` builds of that commit temporarily (never
committed) and fetching, building, and running against them for real.

An earlier verification, against fork commit `76e25559b`, found a real problem: `ios-arm64`,
`ios-sim-arm64`, `android-arm64` and `android-x64` are "complete" static archives
(`pdf_is_complete_lib=true`) that didn't statically bundle libc++, 33 to 36 undefined `std::__Cr::`
symbols (`locale`, `ios_base`, `basic_ostream`, `mutex`, `condition_variable`) in each one. That's
fixed as of `7f0fa5dbe`: the fork now also sets `use_clang_modules=false` alongside the existing
`use_custom_libcxx=false` for these targets (Chromium builds libc++ header modules from its vendored
copy regardless of the latter flag alone), on iOS and, since confirmed, on Android too. `llvm-nm` on
all four rebuilt `lib/libembedpdf.a` archives shows zero `std::__Cr::` symbols, defined or undefined.
Re-verified locally: `bash build/build-swift.sh ios-sim-arm64` now links, `xcodebuild` reaches
`** BUILD SUCCEEDED **` for the iOS simulator destination, and the Android `.so` staged into
`jniLibs` carries zero unresolved `std::__Cr::` symbols (still, no device has actually loaded it).
Full account in `.superpowers/sdd/task-9-report.md`.

## What this is

A proof that a Rust core over PDFium can bind cleanly to Swift, Kotlin, Node and the web, built
before committing to the real port described in [the port plan](../docs/research/rust-core-port.md).
It lives at the repo root, outside `packages/**`, so nothing published depends on it and no workspace
glob has to remember to exclude it. Deleting it costs `rm -rf pdfium-multiplatform-prototype` and nothing else,
the same contract `pdfium-binding-benchmarks/README.md` opens with.

## The six operations, and why the boundary is coarse

Two uniffi objects, six methods:

```
Engine::new() -> Result<Arc<Engine>, OpsError>
Engine::open(bytes, password) -> Result<Arc<Document>, OpsError>
Document::page_count() -> Result<u32, OpsError>
Document::page_size(index) -> Result<Size, OpsError>
Document::render_page(index, scale) -> Result<Bitmap, OpsError>
Document::page_text(index) -> Result<String, OpsError>
Document::search(index, query, case_sensitive) -> Result<Vec<SearchHit>, OpsError>
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

What that six-way match proves is that the byte pipeline is transparent: nothing between PDFium's
render call and a host's own buffer inserts, drops, or reorders a byte. It does not prove channel
order, because page 4 is entirely grayscale: B == G == R in every one of its 30,294 pixels, alpha 255
throughout. Transforming the committed golden with a B<->R swap and re-running this scaffold's own
diff against it gives mean 0.0 and max 0, the same result a correct host reports. A grayscale golden
cannot tell a correct BGRA read from an RGBA-for-BGRA one, or from any other two-channel swap, so
channel order has to be established separately: `apps/web-demo/bgra-to-rgba.test.mjs`,
`bgraToCGImagePreservesChannelOrder` in `VectorsTests.swift`, and `bgraToArgbPacksChannelsCorrectly`
in `VectorsTest.kt` each carry a synthetic buffer whose three channels are never equal, and check the
channel order directly rather than through this golden.

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
`build/generate-vectors.mjs`, which imports `createPdfRuntime` and `engineRuntimeBuildId` from
`packages/engine/runtime`'s built `dist` (`dist/index.node.js` and `dist/build-id.js`) and drives
that over `fixtures/report.pdf`, printing page 4's page size, extracted text, `charCount`, search
hits and a rendered bitmap. Page 4, because that is the page `pdfium-binding-benchmarks/bench/fixture.mjs`
uses, so the numbers were already a committed receipt rather than one this scaffold invented.
`build/generate-vectors.mjs` is never run by any test or by `build/test-all.sh`; it is a one-time
generator, and every suite in this repository only ever reads the two files it already wrote.

What "drifts from those numbers" means for the render vector is not byte-identity. `render` in
`vectors/report-page4.json` sets `meanAbsDiffTolerance: 2` and `maxAbsDiffTolerance: 16`, and every
host's test diffs its own render against the golden with those two gates, not a hash (see "The
tolerances, measured" below for what that does and does not catch). Every other field, page count,
page size, text, `charCount`, search hits, is an exact assertion: any drift there fails a test
outright.

## The tolerances, measured

`meanAbsDiffTolerance: 2` and `maxAbsDiffTolerance: 16` are not zero, and it is worth knowing what
they do and do not catch before trusting the six-way match above. Scored against the golden with
`crates/test-support/src/lib.rs`'s own `mean_abs_diff` and `max_abs_diff`: an all-white buffer scores
mean 16.78, an all-black buffer 238.22, the golden shifted one pixel horizontally 6.12, the golden
with its alpha channel zeroed 63.75. All four fail both gates by a wide margin, so a host that
renders nothing, or renders the wrong page, or misaligns a row, or drops alpha, is caught with room
to spare. But darkening every non-white byte by 6, which is the signature a wrong color space or an
unapplied gamma curve would leave, scores mean 1.74 and max 6, and passes both gates. A tolerance
this wide would let that class of bug through today; none of the six code paths currently produce
it, so the gap is theoretical for this scaffold, not a bug it has already missed.

The fix is not to tighten the tolerance to zero. `crates/test-support/src/lib.rs` compares by
pixel diff rather than by hash for a specific reason: a hash pins float and rasterization behavior
across four architectures, and it would fail for reasons that have nothing to do with this scaffold's
code. That reasoning does not change with the numbers above; it just means the tolerance's job is to
catch gross drift, not to prove exact reproduction, and a subtler color-handling bug needs a test
that is not this golden.

## Every host

| Host | Build | Test | Tests | Runs today |
|---|---|---|---|---|
| Rust (native) | `cargo build --workspace` | `cargo test --workspace` | 22 | yes |
| Rust (wasm32-unknown-emscripten) | `bash build/link-wasm.sh` | (covered by the web suite below) | see Web | no, `wasm32-eh` is pending |
| Swift, macOS destination | `bash build/build-swift.sh` | `bash build/test-swift.sh` | 4 | yes |
| Swift, iOS destination | `bash build/build-swift.sh ios-arm64` | n/a | n/a | no, `ios-arm64` is pending; verified locally that it now links and `xcodebuild` succeeds, see above |
| Kotlin, JVM desktop | `bash build/build-kotlin.sh` | `cd packages/kotlin && ./gradlew :epdf-ops:test` | 5 | yes |
| Kotlin, Android | `bash build/build-kotlin.sh android-arm64` | n/a | n/a | no, `android-arm64` is pending; verified locally that it links and `assembleDebug` succeeds with zero unresolved symbols, see above |
| Node (napi) | `bash build/build-node.sh` | `node --test packages/node/*.test.mjs` | 6 | yes |
| Web (em++ module) | `bash build/link-wasm.sh` | `node --test packages/web/*.test.mjs` | 4 | no, `wasm32-eh` is pending |
| `apps/node-cli` | n/a | `node --test apps/node-cli/*.test.mjs` | 1 | yes |
| `apps/web-demo` | `npm run build` (in `apps/web-demo`) | `node --test bgra-to-rgba.test.mjs` | 1 | yes, wired into `npm test`'s glob |
| `apps/ios`, macOS destination | `bash build/build-ios-app.sh` | none wired | 0 | builds; see "What this scaffold has not shown" below |
| `apps/ios`, iOS destination | `bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 16'` | none wired | n/a | no, `ios-sim-arm64` is pending |
| `apps/android` | `./gradlew :app:compileDebugKotlin` | `./gradlew :app:testDebugUnitTest` | 11 | compiles and tests; `assembleDebug` and every other packaging task fail on purpose while `android-arm64` is pending; see below |

`cargo test --workspace`'s 22, the Swift row's 4, the Kotlin row's 5 and the Android row's 11 (3 in
`ErrorMappingTest`, 8 in `DocumentLifecycleTest`) are each their own process. The Node and Web rows'
6 and 4, plus `apps/node-cli`'s 1 and `apps/web-demo`'s 1, are not: all six run inside one `npm test`
invocation, along with two suites this table names nowhere else, `build/fetch-libpdfium.test.mjs`
(7 tests) and `vectors/vectors.test.mjs` (1 test, asserting the frozen vectors file is internally
consistent), for 7 + 1 + 6 + 4 + 1 + 1 = 20 once `wasm32-eh` ships. `bash build/test-all.sh` does not
run the rows above in the table's order; it runs `build/build-node.sh`, then `cargo test --workspace`,
then `build/link-wasm.sh`, then `npm test` (dropping the Web row's 4 and running the other 16 by name
if `link-wasm.sh` exits 2 for a pending `wasm32-eh`, which is what a stock checkout does today), then
the Swift build and test scripts, then Kotlin, then Android, skipping what a stock checkout can't
reach and printing what it did not run and why. On a stock checkout that is 22 + 16 + 4 + 5 + 11 = 58
tests, zero failures, the last time this was run in full.

## What this scaffold has not shown

The table above says "yes" or "builds" for several rows that still leave real gaps unproven, and
those gaps are worth naming rather than leaving for a reader to find.

`apps/ios` builds and runs on its macOS destination, and its data path, opening a document,
rendering a page, running a search, was checked through a standalone CLI rather than the app itself.
The SwiftUI window has never been displayed by anything: `ContentView`, the `CGImage` produced
inside a live render loop, and `@Observable` state propagating out to the view are all unproven code
paths. `apps/android` compiles and its 11 unit tests pass, but nothing has run the Compose UI: there
is no `android-arm64` artifact to link against and no emulator image to run one on, so the app has
never been launched.

Both of those gaps are narrower than they were: the fork's `ios-arm64`, `ios-sim-arm64`,
`android-arm64` and `android-x64` builds went through two rounds of local verification (never
committed, `file://`-pinned). Against commit `76e25559b`, none of the four static archives bundled
libc++, 33 to 36 undefined `std::__Cr::` symbols each, fatal on iOS (Mach-O link) and silent on
Android (ELF defers to `dlopen`, so `./gradlew assembleDebug` passed anyway while shipping a broken
`.so`). Against commit `7f0fa5dbe`, with `use_clang_modules=false` added alongside the existing
`use_custom_libcxx=false`, `llvm-nm` shows zero `std::__Cr::` symbols in any of the four archives,
`bash build/build-swift.sh ios-sim-arm64` links, `xcodebuild` reaches `** BUILD SUCCEEDED **` for the
iOS simulator, and the `.so` `build-kotlin.sh android-arm64` stages also carries zero unresolved
`std::__Cr::` symbols. What's still unproven: nothing has run either app on a device or emulator, so
"links cleanly" is as far as either mobile platform has been taken. Full account in
`.superpowers/sdd/task-9-report.md`.

win32 gets its own caveat below, in "Five things a reader will otherwise be surprised by": it is
reasoned, not built.

`linuxmusl-x64` and `linuxmusl-arm64` get no caveat anywhere else in this document, which is its own
problem: this scaffold has never linked or run either one. Their static-link decision follows from
`runtime-src/scripts/embedpdf-runtime/build-target.sh:47-55`, which builds them with GCC against
libstdc++ rather than LLVM's libc++, a fact recorded only in a comment at
`crates/build-support/src/lib.rs:145-152`. `build/build-node.sh` cannot even name a musl target
today.

And the honest scope line: nothing in `.github/workflows/` references this scaffold,
`pnpm-workspace.yaml` excludes it from the pnpm workspace by design (it globs `packages/**`,
`examples/**`, `cloudpdf/**`, `tooling/*`, `website` and `docs/*`, and this directory is none of
those), and the six-way render agreement above is one manual run on one darwin-arm64 machine, not a
CI matrix. That is a reasonable place for a scaffold meant to be deletable to sit. It should be
written down, not discovered by whoever reads this next.

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
`d7b4caa26289d4846e94ff54e63b586b9bbe9fd6` for the nine published targets; `wasm32-eh` and the four
mobile targets were verified against fork commit `7f0fa5dbe`, locally built, never published.

## Five things a reader will otherwise be surprised by

**The web build now needs `wasm32-eh`, not `wasm32`, and no longer builds `epdf-wasm` with
`-C panic=abort`.** rustc's std for `wasm32-unknown-emscripten` lowers a Rust panic through native
WebAssembly exception handling, which needs `em++`'s `-fwasm-exceptions`. The old `wasm32` artifact
needed the older `-sSUPPORT_LONGJMP=emscripten` instead, because PDFium genuinely uses
`setjmp`/`longjmp` for libpng and libjpeg error handling (`core/fxcodec/png/pngmodule.cpp`,
`core/fxcodec/jpeg/jpeg_common.c`), and `em++` refuses both flags on the same link. `wasm32-eh` is
the same source built with `-sSUPPORT_LONGJMP=wasm` instead, so both sides agree on the native
mechanism and `RUSTFLAGS="-C panic=abort"` is gone from `build/link-wasm.sh`. Verified locally
against a `file://`-pinned build of fork commit `7f0fa5dbe` (see above): `epdf-wasm`'s new
`panic-probe` feature (`crates/epdf-wasm/src/lib.rs`) panics on purpose, and the panic surfaces to
the JS caller as a catchable `RuntimeError` rather than killing the module. Calling the probe twice
in a row still throws twice, proof the instance survives. The probe is opt-in, so reproducing that
check means `EPDF_PANIC_PROBE=1 bash build/link-wasm.sh`; a plain run ships neither the feature nor
the `_epdf_test_panic` export. `wasm32-eh` isn't in a published release
yet, same as the four mobile targets, so a stock checkout still can't build this today; `wasm32`
stays in `runtime-build.json` with its working URL, just unused by any code path now.

**The Kotlin API is not name-identical to Swift's.** Two separate collisions, both renamed by
`packages/kotlin/uniffi.toml`. `EpdfDocument.close` is `closeSync` in Kotlin because uniffi's Kotlin
generator makes every object `AutoCloseable` and emits its own `override fun close()`, and the
UDL-exported `close` would otherwise be a second method with the same signature in the same class.
The `EngineFailed` and `Internal` error variants' `message` fields are `detail` for a different
reason: uniffi's Kotlin generator gives every exception variant its own `override val message` for
the human-readable string, and `OpsError::EngineFailed` and `::Internal` each carry a Rust field
literally named `message`, which collides with that generated property, not with `AutoCloseable`.
`use { }` calls uniffi's own non-blocking `close()`; reach for the explicit `closeSync()` when a call
site needs the document's slot in the engine to be gone the instant the call returns.
`packages/kotlin/README.md` has the full account.

**win32 has never been linked.** Not "builds but cannot run": nothing has built it, because no win32
artifact has ever been fetched here. PE has no rpath equivalent, so `build-support`'s link directives
have nothing to emit for locating `embedpdf.dll` at runtime even if one were. `rust-toolchain.toml`
installs `aarch64-apple-darwin`, `wasm32-unknown-emscripten`, `aarch64-linux-android` and
`x86_64-linux-android`, and no Windows Rust target at all, and the import-library link name
(`embedpdf.dll`, not `embedpdf`) has never touched a real MSVC toolchain.

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
