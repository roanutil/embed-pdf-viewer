# pdfium-multiplatform-prototype

As of the [2026-09-08 audit](../docs/research/platform-gap-audit.md), the committed
release pin supplies nine targets. `ios-arm64`, `ios-sim-arm64`, `android-arm64`,
`android-x64`, and `wasm32-eh` remain `"status": "pending"` in
`build/runtime-build.json`, so fetching them with that pin exits 2. The local fork
builds all five; publishing them still requires release-matrix work and a maintainer
with push access. This checkout's authenticated account had no push access when checked.

Local artifacts are a working development route: the audit rebuilt WASM, ran all
22 Rust and 20 JavaScript tests, and displayed rendered pages, text, and search
results in the iOS simulator and Android emulator. The Android run used an existing
local APK on a 16 KB emulator; the iOS app was rebuilt for its simulator. Neither
physical-device distribution nor a complete platform CI matrix has been verified.

The earlier mobile libc++ problem is fixed in fork commit `7f0fa5dbe`:
`use_custom_libcxx=false` plus `use_clang_modules=false` removes the mismatched
Chromium `std::__Cr::` references. The audit rechecked all four cached mobile
archives and found zero `__Cr` symbols. Android also loaded and rendered successfully;
that runtime check establishes more than inspecting unresolved symbols alone.

## Building with local artifacts

Keep `build/runtime-build.json` unchanged. Copy it to a local file and replace each
pending entry you want to build with the corresponding archive URL and checksum,
omitting `status: pending` for that entry:

```json
"wasm32-eh": {
  "url": "file:///absolute/path/to/libembedpdf-runtime-wasm32-eh.tar.gz",
  "sha256": "<SHA-256 of that archive>"
}
```

Use `shasum -a 256 /absolute/path/to/archive.tar.gz` to obtain the checksum. The
archive must contain the matching fork headers and library in the same layout as
the published artifacts. From this directory:

```bash
export EPDF_SCAFFOLD_PIN_FILE=/absolute/path/to/local-runtime-build.json
# Select a compatible Binaryen explicitly; setting PATH alone may not override emsdk.
export EM_BINARYEN_ROOT=/absolute/path/to/binaryen
bash build/link-wasm.sh
node --test packages/web/*.test.mjs
# After adding the corresponding mobile entries to the same local pin:
bash build/build-swift.sh ios-sim-arm64
bash build/build-kotlin.sh android-arm64
```

The audit used Emscripten 3.1.72 with Binaryen 132. `EM_BINARYEN_ROOT` points to the
directory containing `bin/wasm-opt`. The SDK emits a version-mismatch warning, but
this combination built and passed the web tests. Keep the default library output
location when using the host build scripts: some staging/link paths still assume
`build/libpdfium`, even though the fetcher itself accepts `EPDF_SCAFFOLD_LIB_DIR`.

After moving this directory, rebuild generated native artifacts: their absolute
rpaths can still name the old checkout. If Cargo reuses a test-support artifact with
an old fixture path, run `cargo clean -p test-support` before `cargo test --workspace`.

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

`runtime-src/scripts/embedpdf-runtime/target-args.sh` sets `EMBEDPDF_TLS_GLOBALS=true` for
native targets and leaves it off for `wasm32` and `wasm32-eh`, so the server worker
pool can render in parallel in-process. PDFium's
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
page size, text, and search hits, is checked exactly. The JavaScript vector-consistency
test also checks `charCount` against the frozen text's UTF-16 length; Swift and Kotlin
do not yet make that length assertion against their extracted strings.

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

| Host | Build | Test | Tests | Verification / clean-checkout requirement |
|---|---|---|---|---|
| Rust (native) | `cargo build --workspace` | `cargo test --workspace` | 22 | yes |
| Rust (wasm32-unknown-emscripten) | `bash build/link-wasm.sh` | (covered by the web suite below) | see Web | passes with local artifacts; committed `wasm32-eh` pin is pending |
| Swift, macOS destination | `bash build/build-swift.sh` | `bash build/test-swift.sh` | 4 | historical pass; not rerun in September 8 audit |
| Swift, iOS destination | `bash build/build-swift.sh ios-arm64` | n/a | n/a | local link verified; release pin pending; physical device not tested |
| Kotlin, JVM desktop | `bash build/build-kotlin.sh` | `cd packages/kotlin && ./gradlew :epdf-ops:test` | 5 | historical pass on ARM64 macOS; not rerun in September 8 audit |
| Kotlin, Android | `bash build/build-kotlin.sh android-arm64` | n/a | n/a | local APK loaded on ARM64 emulator; release pin pending |
| Node (napi) | `bash build/build-node.sh` | `node --test packages/node/*.test.mjs` | 6 | yes |
| Web (em++ module) | `bash build/link-wasm.sh` | `node --test packages/web/*.test.mjs` | 4 | passes with local artifacts; committed `wasm32-eh` pin is pending |
| `apps/node-cli` | n/a | `node --test apps/node-cli/*.test.mjs` | 1 | yes |
| `apps/web-demo` | `npm run build` (in `apps/web-demo`) | `node --test bgra-to-rgba.test.mjs` | 1 | yes, wired into `npm test`'s glob |
| `apps/ios`, macOS destination | `bash build/build-ios-app.sh` | none wired | 0 | historically built; macOS window not verified |
| `apps/ios`, iOS destination | `bash build/build-ios-app.sh 'platform=iOS Simulator,name=iPhone 17'` | none wired | n/a | rebuilt and rendered on iOS 26.5 simulator with staged libraries; release pin pending |
| `apps/android` | `./gradlew :app:compileDebugKotlin` | `./gradlew :app:testDebugUnitTest` | 11 | 11 historical unit tests; existing local APK rendered on emulator; packaging requires staged native libraries |

`cargo test --workspace`'s 22, the Swift row's 4, the Kotlin row's 5 and the Android row's 11 (3 in
`ErrorMappingTest`, 8 in `DocumentLifecycleTest`) are each their own process. The Node and Web rows'
6 and 4, plus `apps/node-cli`'s 1 and `apps/web-demo`'s 1, are not: all four rows run inside one `npm test`
invocation, along with two suites this table names nowhere else, `build/fetch-libpdfium.test.mjs`
(7 tests) and `vectors/vectors.test.mjs` (1 test, asserting the frozen vectors file is internally
consistent), for 7 + 1 + 6 + 4 + 1 + 1 = 20 when the web module is built, including
via a local pin. `bash build/test-all.sh` does not
run the rows above in the table's order; it runs `build/build-node.sh`, then `cargo test --workspace`,
then `build/link-wasm.sh`, then `npm test` (dropping the Web row's 4 and running the other 16 by name
if `link-wasm.sh` exits 2 for a pending `wasm32-eh`, which is what a stock checkout does today), then
the Swift build and test scripts, then Kotlin, then Android, skipping what a stock checkout can't
reach and printing what it did not run and why. On a stock checkout that is 22 + 16 + 4 + 5 + 11 = 58
tests, zero failures, the last time this was run in full.

## What remains unverified

The [2026-09-08 audit](../docs/research/platform-gap-audit.md) displayed both mobile
UIs and verified the fixture's 14 pages, 612 × 792-point page size, 120 search hits,
rendered page, and extracted text. These are manual smoke tests, not automated
lifecycle or visual-regression suites. The macOS SwiftUI window was not checked in
that audit; the earlier macOS evidence covered compilation and a standalone CLI.

The iOS simulator uses an absolute checkout runpath to find `libepdf_uniffi.dylib`.
The app does not embed that library, so a self-contained device build still needs
framework/XCFramework packaging or static linking, signing, and a physical-device
test. Android was tested using an existing ARM64 APK; a fresh Android build, x64,
and physical devices were not verified in the audit.

Windows and both musl targets remain unbuilt/unrun in this prototype's recorded
verification. The musl C++ runtime choice follows
`runtime-src/scripts/embedpdf-runtime/target-args.sh` (GCC/libstdc++).
`build/build-node.sh` accepts an explicit `linuxmusl-*` argument, but its automatic
host selection does not detect musl and it does not select a Rust cross-target.
Kotlin's `host` build is ARM64 macOS, and `test-all.sh` assumes Apple tools.

The rebuilt legacy-EH WASM module ran in Chrome 152 and cached Playwright Firefox
132 Nightly. Release Firefox and Safari remain unverified; Nightly success is not
evidence of release-browser compatibility. See [the EH notes](../docs/research/wasm-eh-encoding.md).

No root CI workflow runs this prototype. Its workspace exclusion is intentional,
but adding host-specific CI and emulator/browser smoke tests is feasible. Cancellation,
bounded admission, aggregate memory budgets, engine replacement, and crash containment
also remain production work; the audit records practical implementation options.

## Tool versions, measured on the machine that verified this scaffold

`rustc 1.98.0`, every crate `edition = "2024"`. `bindgen = "=0.72.1"`, `uniffi = "=0.32.0"`,
`napi = "=3.12.2"`, `napi-derive = "=3.6.3"`, `napi-build = "=2.4.1"`, `cargo-ndk 4.1.2`. emsdk
3.1.72, with Binaryen 132 selected using `EM_BINARYEN_ROOT` in the September 8 audit;
the SDK's bundled v119 rejects two wasm features present in rustc's precompiled std. Swift 6.3.3, Xcode 26.6 build 17F113, XcodeGen 2.46.0.
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
the JS caller as a catchable `RuntimeError`. The September 8 audit called the probe
twice and then verified that a real
`page_count` call on the same document still returned 14. This establishes that
limited recovery case, not arbitrary state recovery or inline borrow-guard unwinding.
The probe is opt-in, so reproducing that
check means `EPDF_PANIC_PROBE=1 bash build/link-wasm.sh`; a plain run ships neither the feature nor
the `_epdf_test_panic` export. `wasm32-eh` isn't in a published release
yet, same as the four mobile targets; use the local-pin procedure above to build it. `wasm32`
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
installs ARM64 macOS, iOS device/simulator, Emscripten, and both Android Rust targets,
but no Windows Rust target, and the import-library link name
(`embedpdf.dll`, not `embedpdf`) has never touched a real MSVC toolchain.

**`swift build` does not resolve on a fresh checkout.** `packages/swift/EpdfOps`'s generated
sources, its modulemap, and the staged dylibs are all gitignored. Run `bash build/build-swift.sh`
first.

**`charCount` in the frozen vectors is 8,106.** That is UTF-16 code units: JavaScript's
`text.length` and Kotlin's `String.length` return it directly, verified with
`text.utf16.count` in Swift too. Swift's default `String.count` gives 7,992 on the same text,
because it counts grapheme clusters and the fixture's page 4 text holds 114 combining sequences.
Neither number is wrong; they count different things. The JS vector-consistency
test asserts the UTF-16 length. Swift and Kotlin tests still need explicit length
assertions against their extracted strings.
