# Platform gaps and hardening audit — 2026-09-08

The documented gaps do **not** establish that there are no reasonable fixes or
workarounds. Several are stale, several describe missing automation, and several
already have working local overrides. Native device distribution, release-browser
coverage, Windows, and musl still need verification.

Scope: the three research projects introduced on this branch, their READMEs,
research notes, scaffold design/plan, and the implementation behind their stated
limitations. This is an audit, not a claim of production readiness or a full audit
of the shipping SDK/server. Historical plans describe their original constraints;
they are not evidence that those constraints remain technical blockers.

## Implementation follow-up

The following gaps were addressed after the original audit (the findings below
remain a record of what was found):

- Web password support now matches Node's optional password argument; shared
  tests cover missing/wrong passwords and the UTF-8 password `hôtel`.
- A colored PDF and specified interior BGRA samples now run through Node, WASM,
  Swift, and Kotlin. Swift/Kotlin also assert the extracted text's UTF-16 length.
- Node build and runtime target selection share one glibc/musl detector. Cross-host
  labels are rejected rather than silently producing the wrong architecture.
  Actual Windows/musl execution is still unverified.
- `build/local-pin.mjs` generates a separate pin and checksums from local archives.
  The fetcher decodes file URLs correctly, including paths with spaces.
- The optional WASM panic probe now runs inside dispatch; two caught panics each
  leave the same document usable, testing borrow-guard release.
- WASM returned buffers use boxed slices, removing reliance on `Vec::shrink_to_fit`
  producing exact capacity.
- iOS native libraries now link statically. The simulator build rendered successfully
  and the unsigned ARM64 device build also succeeded. Both builds
  have no dynamic Rust/PDFium dependency. Physical-device
  signing/provisioning and execution remain separate requirements.

Verified follow-up suites: 22 Rust tests, 27 JavaScript tests, 5 Swift tests, and
6 Kotlin JVM tests. The fetcher extraction test now uses an isolated temporary
library directory, fixing a race with parallel native-addon tests.
The normal WASM build was restored after running the optional panic test. Release
publication, full platform/browser CI, cancellation, admission/memory budgets,
and process crash containment remain open.

## Verification performed

- `cargo test --workspace --offline`: **22 passed**.
- `npm test` in `pdfium-multiplatform-prototype`: **20 passed**, including the
  four WASM binding tests. Initially, the old Node addon retained the pre-rename
  absolute rpath, and Cargo reused a test-support artifact containing the old
  fixture path. `bash build/build-node.sh` and `cargo clean -p test-support`
  followed by rebuilding resolved those cache failures.
- Rebuilt WASM with the committed build script, a separate local artifact pin,
  and `EM_BINARYEN_ROOT=/opt/homebrew/opt/binaryen` (Binaryen 132). All four web
  binding tests passed again. No SDK binary replacement was needed.
- Installed and launched the existing Android debug APK on the installed ARM64
  Android emulator. `getconf PAGE_SIZE` returned **16384**. Visually verified
  the Compose UI: **14 pages, 612 × 792 pt, 120 hits**, rendered page and extracted
  text. This used an existing local APK, not a new Android build from scratch.
- Rebuilt the iOS app with `build/build-ios-app.sh` for the installed iPhone 17,
  iOS 26.5 simulator, then installed and launched it. Visually verified the same
  values, rendered page, and extracted text. The initially tried old build
  crashed in dyld looking for `libepdf_uniffi.dylib`; rebuilding after the directory
  rename resolved that simulator failure.
- Ran the freshly rebuilt WASM through a temporary localhost browser harness in
  **Chrome 152.0.7977.83** and the cached **Playwright Firefox 132.0 Nightly**.
  Both opened the fixture, reported 14 pages and 120 hits, and rendered a
  121,176-byte bitmap. The latter is a Playwright browser build, **not release
  Firefox**; its success does not establish release-Firefox compatibility.
- Disassembled that WASM with Binaryen: 441 `try`, 367 `catch_all`, 285 `rethrow`,
  and zero `try_table` instructions. The tested module still uses legacy EH.
- Built the optional panic probe. Two calls threw `RuntimeError: unreachable`;
  a real `page_count` call on the same module/document still returned 14 afterward.
  Restored the normal build afterward. This proves that limited recovery case,
  not arbitrary state recovery or unwinding through the inline borrow guard.
- `llvm-nm` completed successfully on all four cached mobile PDFium archives;
  each had zero `__Cr` symbols.
- Queried GitHub releases and current repository permissions. The latest release
  is still `runtime-d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`, with nine runtime
  targets. The authenticated account has `push: false`.

The screenshots and temporary build logs were saved under `/tmp/epdf-audit-*`.
Those are local audit evidence, not committed reproducibility artifacts. No
physical device, Windows or musl runtime, release Firefox, or Safari was tested.
Swift/JVM unit suites were not rerun; the passing Rust/JS counts above are the
freshly executed suites, not the README's historical aggregate of 58.

## Platform and toolchain findings

| Documented gap | Assessment and reasonable route forward |
|---|---|
| Five pending targets: iOS device/simulator, Android ARM64/x64, WASM EH | **Accurate for the committed release pin.** Inaccurate when phrased as “the fork does not build it yet” or “cannot be built.” Local artifacts exist. `EPDF_SCAFFOLD_PIN_FILE` accepts a separate JSON file with `file://` URLs and SHA-256 values; that route rebuilt WASM successfully here. Publish the fork artifacts and update the pin for clean-checkout reproducibility. |
| Fork release matrix and lack of push access | **Accurate.** The local fork's `target-args.sh` supports the new targets, but `.github/workflows/release-libpdfium.yml` still lists nine. Completing release CI is real work, not just flipping the pin. A maintainer can publish it; local development does not require push access. |
| Mobile `std::__Cr::` unresolved symbols | **Already fixed in the local fork.** The four archives confirm the README's zero-symbol claim. `use_custom_libcxx=false` plus `use_clang_modules=false` is in `target-args.sh`. Keep those settings, package the matching Android `libc++_shared.so`, and test loading; absence of `__Cr` alone does not prove all dependencies resolve. Android loading now has direct evidence too. |
| Android cannot package/install and has no emulator image | **Stale as a blanket claim.** The committed pin still blocks fetching, and the Gradle guard rejects missing native files. Staged local files satisfy that guard. The existing APK installed and rendered on the available 16 KB emulator. Add a repeatable emulator test and verify freshly built APKs, x64, and physical devices. |
| SwiftUI and Compose paths have never run | **Closed for the tested simulator/emulator builds.** Both UIs rendered and displayed extracted data during this audit. This is a smoke test, not lifecycle, accessibility, navigation, or screenshot-regression coverage. Android's legacy `uiautomator dump` itself crashed serializing U+FFFE from the extracted text; the app remained usable and screenshot verification succeeded. |
| iOS device distribution | **Still incomplete.** `project.yml` supplies checkout-absolute library/runpath directories, but does not embed the native dylib. A simulator can resolve those host paths; a phone cannot. Package a supported framework/XCFramework and embed/sign it with bundle-relative runpaths, or statically link the Rust/PDFium libraries. Device signing/provisioning and an actual device run remain necessary. Merely removing the pending flag will not establish this. |
| Windows has never linked; PE has no rpath | **Verification gap is credible; missing rpath is not a blocker.** Windows artifacts exist in the release. Test the `embedpdf.dll.lib` import library with MSVC, stage the DLL using an explicit loader/package strategy, and add a Windows CI job. `build-node.sh` also rejects non-Darwin/Linux hosts and does not implement a Windows DLL-to-addon staging branch. |
| musl has never linked/run; Node build cannot name it | **Untested remains accurate; “cannot name” is too strong.** `build-node.sh linuxmusl-x64` is accepted and forwarded, but automatic selection chooses `linux`, and Cargo is not given an explicit Rust cross-target. On a matching musl host, the explicit argument is a plausible route. Add matching target selection, test in Alpine, and verify `libstdc++` dependencies or a deliberately static C++ link. Runtime loader detection already distinguishes musl. |
| General multiplatform build scripts | **More limited than the library target mappings imply.** Kotlin's `host` means ARM64 macOS, Swift scripts stage ARM64 Apple slices, and `test-all.sh` unconditionally runs Apple scripts. Split CI by supported host and make host/target selection explicit. A Cargo target mapping alone does not make these scripts portable. |
| Emscripten 3.1.72's Binaryen is too old | **Real mismatch with a tested workaround.** `EM_BINARYEN_ROOT` selected Binaryen 132 and produced a working module. Emscripten emitted a version-mismatch warning. Merely changing `PATH` is not dependable when the SDK config supplies `BINARYEN_ROOT`; pin and check the actual tool Emscripten uses. A matching newer SDK is a longer-term alternative. |
| Legacy EH / browser compatibility | **Needs a narrower statement and a browser matrix.** The built artifact is legacy EH; Chrome and the cached Firefox Nightly passed. The existing note's tiny 4.0.0 probe demonstrates a final-EH option, not a fully linked Rust/PDFium product. Rebuild/link with a consistent final-EH toolchain and test the real module, or specify a supported-browser floor/fallback. Older-browser compatibility and release Firefox/Safari remain unverified. |
| Firefox is unavailable, so browser checks cannot run | **Stale for this environment.** There is no `/Applications/Firefox.app`, but a cached Playwright Firefox exists and was tested. It is not a substitute for a current release-Firefox test. |
| Fresh `swift build` lacks generated files | **Accurate, readily addressable.** Run `build/build-swift.sh` first. A documented bootstrap command is sufficient for a prototype; a distributable SDK should ship generated bindings and packaged binaries or automate their production. |
| No prototype CI matrix | **Accurate.** Nothing in the root workflows references this project. Add host-specific jobs for Rust, Node, WASM browsers, Swift, JVM, and mobile smoke tests, with explicit pending-target reporting. This is infrastructure work with no identified architectural blocker. |
| Binding benchmark native runner only supports macOS ARM64 | **Accurate.** `bench/run.mjs` and `bench/arm.mjs` hardcode that target. Parameterize platform/architecture and artifact loading, then rerun the fairness gate before collecting other-platform timing. The missing native arm-A diagnostic also has a straightforward fix: point to the shipping runtime's build command. |

The DLL workaround follows Microsoft's documented
[DLL search and loading mechanisms](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order).
Emscripten documents configuration of
[LLVM and Binaryen paths](https://emscripten.org/docs/building_from_source/configuring_emscripten_settings.html).
Android's [16 KB testing guidance](https://developer.android.com/guide/practices/page-sizes)
requires checking native dependencies as well as packaging; one successful emulator
run is useful evidence, not complete device coverage.

## Hardening and validation findings

Some rows below are directly documented omissions; others are adjacent implementation
limits needed to assess the broad “production-hardening omissions” description.

| Area | What is true, and what can reasonably be done |
|---|---|
| Cancellation | The design explicitly defers it. `Worker::run` blocks on `recv`, and rendering calls monolithic `FPDF_RenderPageBitmap`. Add cancellation/deadline checks before queued work and between search operations; use PDFium's progressive render APIs and `IFSDK_PAUSE` where supported. The shipping `PageRenderReader` already demonstrates boundary abort checks. Some native calls remain non-preemptible; hard deadlines need process/worker replacement and document reopening, not unsafe cross-thread handle destruction. |
| UI/event-loop responsiveness | Node exports and web operations are synchronous even though native PDFium has its own thread. Mobile demos already offload their load work, but blocking `close`/`closeSync` can still wait behind work. Use host async adapters or a web worker, explicit cleanup scheduling, and stale-result suppression. An async signature alone would not cancel native work. |
| Admission and memory limits | Native dispatch uses unbounded `mpsc::channel`; documents and search hits have no configured quotas. The 256 MiB bitmap cap is present and tested, but does not bound aggregate memory, PDFium's internal allocations, document copies, or total open documents. Add admission limits, configurable byte/document budgets, checked input lengths, and bounded scheduling while preserving guaranteed cleanup. |
| Allocation/input robustness | `Document::from_bytes` copies data and casts its length to `i32`; the low-level public `render_bgra` has weaker guards than `epdf-ops`. Add checks at the lowest public boundary, use the size_t load API where appropriate, and use fallible allocation where it can help. Web glue should validate allocations and clean up all partial-allocation failure paths. OOM inside native PDFium still cannot be made recoverable just by catching Rust panics. |
| Native crash containment | Thread confinement is correctly represented as a correctness requirement, not crash isolation. A native segfault still kills the process. The repository already contains a supervised engine-host design in `cloudpdf/server/src/runtime`; adapt that for server deployment. Mobile platform constraints require a separate design. A mutex or `catch_unwind` cannot contain a native crash. |
| Panic recovery | Native worker panics are caught and have a regression test. The optional WASM probe throws twice and allows a subsequent real operation, as verified here. However, the probe panics outside `Worker::run`; it does not demonstrate WASM borrow-guard cleanup or safety after partially mutating state. Add an in-dispatch WASM panic test and define when an engine must be discarded. `AssertUnwindSafe` is not proof of state rollback. |
| Engine failure/recreation | Errors describe recreating a failed engine, but Node holds a process-wide `OnceLock<Arc<Engine>>` with no reset API. If recovery is required, expose owned engine instances or restart the containing worker/process. This is missing lifecycle design, not a language limitation. |
| Kotlin method/error naming | The generated `close` and `message` collisions are real, and `uniffi.toml` already works around them with `closeSync`/`detail`. For a consistent public API, use a handwritten facade or rename the shared exported method/fields. A scoped ownership helper can perform both deterministic close and generated-handle release. Original plan restrictions on editing the UniFFI crate are not technical prohibitions. |
| Swift/Kotlin UTF-16 counts | The README correctly distinguishes 8,106 UTF-16 units from Swift's 7,992 grapheme clusters. Swift/Kotlin host tests do not assert that count against their extracted string; the JS vector-consistency test already does. Add `text.utf16.count` and Kotlin `text.length` assertions, plus a non-BMP fixture to distinguish code units from Unicode scalar values. |
| Grayscale golden / loose tolerances | The stated limits are real. Synthetic distinct-channel tests already cover byte-order conversion. Add a small colored PDF with known interior pixel values and explicit color-space expectations, and separate exact buffer-conversion tests from cross-platform raster tolerance. Blindly setting the whole-page tolerance to zero is unnecessary. |
| Web/native API parity | Adjacent omission: native/UniFFI can open password-protected documents, while `epdf_open` always supplies `None` and the JS web API accepts only bytes. Add a password argument and a shared encrypted fixture. Web error handling also exposes message strings rather than Node's stable error codes. Neither disparity is forced by WASM. |
| Handwritten wire types and explicit resource ownership | The research plan already proposes generating TypeScript DTOs from Rust. The web adapter also requires explicit `close` to remove documents from its map. Generate/check wire types and provide ownership helpers; finalization may be a fallback but should not define deterministic cleanup. |
| Separate UniFFI crate | This is an intentional isolation choice, not an unresolved defect. It avoids requiring UniFFI to build for Emscripten. Keep it unless reducing wrapper duplication has demonstrated value; if consolidation is desired, test target gating rather than assuming the dependency cannot compile. |
| Toy viewer scope and benchmark limitations | These are three implementations of a rectangle viewer, not a production PDF viewer. Shared vectors and fairness gates address equivalence within that scope. Broader documents, devices, browsers, distributions and real service workloads are reasonable additional experiments; the published timings do not establish universal performance or production completeness. |

Implementation anchors: `pdfium-multiplatform-prototype/crates/epdf-ops/src/{engine.rs,dispatch/threaded.rs,state.rs}`,
`crates/pdfium/src/{document.rs,page.rs}`, `crates/epdf-node/src/lib.rs`,
`crates/epdf-wasm/src/lib.rs`, `packages/web/index.mjs`, the mobile app models,
and `packages/engine/runtime/runtime-src/public/fpdf_progressive.h`.

## Recommended order

1. Correct current README and diagnostic claims that the fork cannot build mobile,
   no emulator/browser is available, or no UI has run. Preserve historical reports
   as dated evidence. Record the local-pin/Binaryen commands explicitly.
2. Complete fork release automation and publish verified artifacts through an
   authorized maintainer; update the prototype pin from a real release.
3. Make Apple packaging self-contained, add Windows/musl build paths, and automate
   simulator/emulator and release-browser checks. Fix `test-all.sh` reporting too:
   its final message says Web was not run even when the local override let it run.
4. Define cancellation, admission, cleanup, failure recovery, and isolation
   contracts before promoting these adapters to production. Add the small UTF-16,
   colored-fixture, encrypted-document, and in-dispatch panic tests independently.

There is no verified “unfixable” platform gap in this audit. There are genuine
external publication permissions and untested environments, plus engineering work
whose cost and platform coverage should be stated explicitly. Full production
readiness cannot be inferred from the successful smoke tests.

## Documentation follow-up

The current READMEs, build diagnostics, and research status notes were updated
following this audit. Historical plans now point here for current status. The
`test-all.sh` summary now reports Web as skipped only when it was actually skipped.
The recommendations above record the state found during the audit; the remaining
packaging, platform automation, and runtime-hardening work is still outstanding.
