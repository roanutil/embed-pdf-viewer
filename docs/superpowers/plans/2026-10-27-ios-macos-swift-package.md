# iOS and macOS Swift Package Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make both `EmbedPDF` and `EmbedPDFUI` build and run on iOS and native macOS, including when SwiftPM downloads the engine on a fresh checkout.

**Architecture:** Keep the existing source wrappers and compiled `EmbedPDFBindings` engine. Add self-contained macOS static slices to both XCFramework layouts, isolate platform feedback at the UI boundary, and verify local-source, local-prebuilt, and downloaded-prebuilt consumption separately.

**Tech Stack:** Swift 6.2 package manifest, SwiftUI, UIKit/AppKit, UniFFI, Rust, PDFium GN/Ninja builds, XCFrameworks, Swift Testing, GitHub Actions.

**Spec:** The user's request in this session, narrowed to “just ios and macos”; the proposed support contract below. Preserve the binary distribution architecture in `docs/superpowers/specs/2026-10-26-apple-engine-binary-design.md` while extending its iOS-only scope.

## Support contract and global constraints

1. Proposed deployment floors: preserve iOS 17.0; add macOS 14.0. Keep Swift tools 6.2. These are planning choices, not previously approved product requirements.
2. Preserve existing iOS arm64 device/simulator support; add native macOS arm64 and x86_64. Adding Intel iOS simulator slices, Catalyst, tvOS, watchOS, and visionOS is outside this plan.
3. Both public products must compile on both OSes. Preserve public names and signatures, including `HapticFeedback`; macOS feedback is best effort through AppKit.
4. Default consumer builds need no Rust, fork checkout, generated Swift, or local engine artifacts. Preserve `EMBEDPDF_LOCAL_ENGINE=1`, `EMBEDPDF_PREBUILT_ENGINE=local`, and the experimental bridge's existing opt-in behavior.
5. Preserve PDFium thread confinement and the existing shared `darwin-*` runtime artifacts. New macOS static artifacts need separate identities and caches. No release upload is part of writing this plan.

## Findings at planning time

| Location | Current behavior | Required change |
| --- | --- | --- |
| `platforms/apple/EmbedPDF/Package.swift` | Declares only `.iOS(.v17)`; default engine is a remote binary | Declare macOS and ship matching binary slices; a manifest change alone is insufficient |
| `platforms/apple/build-xcframework.sh` | Builds only `aarch64-apple-ios` and `aarch64-apple-ios-sim` | Build macOS Rust archives and assemble one universal macOS library slice |
| `platforms/apple/package-engine-framework.sh` | Compiles Swift for two iOS triples; strips only `MIN_IOS` from module filenames | Add macOS triples, per-OS minimum versions, and universal framework assembly |
| Fork `scripts/embedpdf-runtime/{target-args,package-target}.sh` | `darwin-*` builds and stages `libembedpdf.dylib` | Add separate complete static macOS artifacts; a dylib cannot be merged into the engine archive |
| `EmbedPDFUI/{HapticFeedback,ScrollVisibility}.swift` | Unconditional UIKit import; only iOS 18 availability guard | AppKit implementation and `#available(iOS 18, macOS 15, *)` |

The selected Xcode SDK confirms `onScrollVisibilityChange` requires macOS 15. The fallback is needed for the proposed macOS 14 floor. Other SwiftUI APIs, JavaScriptCore, Security, and generated bindings still need actual compilation and runtime verification; do not assume this inventory is exhaustive.

The local `EmbedPDFFFI.xcframework/Info.plist` contains only iOS device/simulator entries. Full package build reproduction was attempted with temporary scratch/module caches, but dependency resolution stopped because `github.com` could not resolve in the restricted environment. No package compiler diagnostic or passing build is claimed here. The remote release's contents were not downloaded or inspected.

Relevant Apple references: [scroll visibility](https://developer.apple.com/documentation/swiftui/view/onscrollvisibilitychange(threshold:_:)) and [AppKit haptic patterns](https://developer.apple.com/documentation/appkit/nshapticfeedbackmanager/feedbackpattern).

## Approach

Extend the existing static engine distribution. Keep standard shared macOS PDFium artifacts available to other consumers and introduce `darwin-static-arm64` / `darwin-static-x64` for this package. This costs a fork change and two native builds, but keeps SwiftPM consumption self-contained. Shipping a second dylib introduces embedding and runtime search-path obligations; supporting only the core product leaves `EmbedPDFUI` broken. Neither meets the proposed contract as directly.

The user approved execution. Task checkboxes track implementation; the verification record below records what has been exercised.

## Review focus

1. An artifact that builds on the developer's machine must also link outside the fork/build directories: exercise a minimal external consumer and inspect dynamic dependencies in Task 3.
2. macOS 14 compilation must exercise the visibility fallback: pin deployment targets explicitly in Task 4 and run on macOS 14 when a runner is available in Task 5.
3. Intel macOS needs both native objects and the correct Swift interface: inspect both architectures and cross-build an x86_64 consumer in Task 3; run natively when an Intel runner is available in Task 5.
4. Source-mode tests can hide a broken published framework: independently test source and extracted prebuilt layouts in Task 5.
5. A cached shared artifact must never be reused as a static one: distinct target names, staging paths, and recipe invalidation are tested in Task 1.

## File responsibilities

| Files | Responsibility |
| --- | --- |
| Fork `scripts/embedpdf-runtime/{target-args,package-target}.sh`, `tests/target-args.test.sh`; parent submodule pointer | New static macOS PDFium targets without changing shared targets |
| `perf-compare/scripts/build-arm.mjs`, `rust-toolchain.toml`, `platforms/apple/build-xcframework.sh` | Build/cache correct native inputs; package local C/FFI slices |
| `platforms/apple/package-engine-framework.sh`, `EmbedPDF/Package.swift`, `apple-engine-hash.sh` | Distributable Swift engine, platform metadata, linking, and artifact identity |
| `platforms/apple/EmbedPDF/Sources/EmbedPDFUI/{HapticFeedback,ScrollVisibility}.swift` | Platform-specific feedback and deployment availability |
| `scripts/publish-engine-payload.mjs`, `.github/workflows/apple-swift.yml`, Apple READMEs and DocC | Validate, release, and document the supported contract |

### Task 1: Provide static macOS PDFium inputs

**Estimate:** 2–4 hours engineering, plus 1–4 hours for cold fork builds depending on dependency caches.

**Files:** Modify fork `packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/{target-args,package-target}.sh` and `tests/target-args.test.sh`; update the parent submodule pointer after the fork change is available. Modify `perf-compare/scripts/build-arm.mjs`.

**Interfaces:** Extend `buildFork(dir, target)` to accept `darwin-static-arm64` and `darwin-static-x64` and retain its `{ recipe, sha, staging, libSha256, ... }` result. Each staging directory contains `include/` and `lib/libembedpdf.a`, with no shared runtime dependency.

- [x] Add shell assertions for both static targets: `target_os="mac"`, matching `target_cpu`, `pdf_is_complete_lib=true`, `mac_deployment_target="14.0"`, `use_custom_libcxx=false`, `use_clang_modules=false`, and `embedpdf_thread_local_globals=true`. Preserve existing shared `darwin-*` assertions. Run `bash packages/engine/runtime/runtime-src/scripts/embedpdf-runtime/tests/target-args.test.sh`; expect the new targets to fail initially.
- [x] Implement the two fork targets. Route static variants before the broad `darwin-*` packaging branch; stage `obj/libembedpdf.a`, not a dylib, and skip `install_name_tool`. Audit patch/sync scripts for broad `darwin-*` matches. Run the shell assertions again; expect all to pass.
- [x] Extend `FORK_TARGETS`, add `mac` to `FORK_TARGET_OS_LIST`, and increment `FORK_RECIPE`. Distinguish wasm from all native targets so macOS never enters the current `if (!ios)` emsdk setup. Validate dependency sync against the expanded OS list before reusing caches. Keep wasm behavior unchanged.
- [x] Build each new target through `buildFork`, check its archive exists and defines `_FPDF_InitLibraryWithConfig`, then repeat to confirm cache reuse. Verify existing shared `darwin-arm64` remains a dylib and existing iOS/wasm target dispatch still works. A shared-only staging directory must fail static validation rather than satisfy a cache hit.
- [x] Commit fork changes in the fork repository, then the parent pointer and builder change in the parent repository. Do not publish a fork or engine release as part of this task.

### Task 2: Extend the local FFI XCFramework

**Estimate:** 2–3 hours, plus 20–60 minutes for Rust compilation.

**Files:** Modify `rust-toolchain.toml`, `platforms/apple/build-xcframework.sh`. Create `platforms/apple/tests/xcframework-layout.test.py` using Python standard-library `unittest`/`plistlib`.

**Interfaces:** Task 1's static staging root is consumed through `LIB_ROOT`. Outputs retain `EmbedPDFFFI.xcframework`, `Sources/EmbedPDFBindings`, test fixtures, and per-target archives under `platforms/apple/.build`.

- [x] Add an artifact test that reads an XCFramework path argument and asserts three platform entries: iOS arm64 device, iOS arm64 simulator, and macOS with arm64/x86_64. Run it on the current local FFI artifact; expect failure for missing macOS.
- [x] Add `x86_64-apple-darwin` to the pinned Rust targets. Extend slice records with `aarch64-apple-darwin:darwin-static-arm64` and `x86_64-apple-darwin:darwin-static-x64`. Set `EMBEDPDF_FORK_TARGET` per Cargo invocation so build-support resolves the static variant, and set `MACOSX_DEPLOYMENT_TARGET=14.0` / `IPHONEOS_DEPLOYMENT_TARGET=17.0` for the corresponding builds. Preserve existing duplicate-symbol checks and the experimental feature flags.
- [x] Merge the two macOS archives with `lipo` into one universal library before `xcodebuild -create-xcframework`. Do not pass two independent libraries for the same macOS platform. Keep UniFFI generation from one existing iOS simulator archive and reuse its headers for every slice.
- [x] Run `LIB_ROOT=<static-and-ios-staging-root> bash platforms/apple/build-xcframework.sh`, then `python3 platforms/apple/tests/xcframework-layout.test.py platforms/apple/EmbedPDF/EmbedPDFFFI.xcframework`. Expect success; inspect `lipo -archs` for `arm64 x86_64` on macOS and verify both macOS architectures contain PDFium and UniFFI symbols. Confirm the experimental bridge can still be assembled when explicitly requested.
- [x] Commit toolchain, builder, and layout test changes. Generated archives, bindings, and fixtures remain ignored.

### Task 3: Package a distributable macOS Swift engine

**Estimate:** 3–5 hours, plus 20–60 minutes for packaging and consumer builds.

**Files:** Modify `platforms/apple/package-engine-framework.sh`, `platforms/apple/EmbedPDF/Package.swift`, and `platforms/apple/tests/xcframework-layout.test.py`. Add `platforms/apple/tests/package-engine-framework.test.py` for a real-toolchain packaging regression without PDFium. Modify `apple-engine-hash.sh` if new shared recipe files are introduced.

**Interfaces:** Consume Task 2's per-architecture native archives and generated Swift. Produce the existing `EmbedPDFBindings.xcframework.zip` and extracted local-prebuilt directory, now including a universal macOS static framework.

- [x] Extend the artifact test to validate `EmbedPDFBindings`: macOS contains `arm64-apple-macos.swiftinterface` and `x86_64-apple-macos.swiftinterface`, iOS interfaces remain present, and public interfaces leak no private `*FFI` modules. Run against the existing package artifact if available; otherwise capture its absence as the initial failing check.
- [x] Declare `.macOS(.v14)` beside `.iOS(.v17)`. Supply macOS-only AppKit/Foundation linkage required by native PDFium in both source and prebuilt manifest paths, preserving existing C++/CoreFoundation/CoreGraphics settings and module names. Keep non-macOS framework requirements platform-conditioned.
- [x] Compile generated Swift with `arm64-apple-macosx14.0` and `x86_64-apple-macosx14.0`, using the macOS SDK. Replace iOS-only module filename derivation with explicit versionless triples per slice. Combine native and Swift archives per architecture, then merge macOS framework binaries and both interface sets into one framework. Use macOS-appropriate bundle layout/version metadata; retain iOS metadata at 17.0. Preserve interface sanitization, frozen declarations, zip checks, and checksum generation.
- [x] Run `bash platforms/apple/package-engine-framework.sh` and the extended layout test against its extracted output. Build and run a temporary external package depending only on the `EmbedPDF` product, opening/rendering/closing a fixture. Use `EMBEDPDF_PREBUILT_ENGINE=local swift build` and `swift run` on the host; cross-build with `--triple x86_64-apple-macosx14.0`. Inspect the linked executable with `otool -L`: no `libembedpdf.dylib`, fork staging paths, or builder paths. Build an iOS core consumer as a regression check.
- [x] Confirm packaging-script changes alter `bash platforms/apple/apple-engine-hash.sh`; include any new recipe files in that hash. Commit tracked changes. Keep the remote pin unchanged until a verified artifact is actually published in Task 5.

### Task 4: Make the UI product portable

**Estimate:** 1–2 hours, plus 15–30 minutes for tests.

**Files:** Modify `platforms/apple/EmbedPDF/Sources/EmbedPDFUI/HapticFeedback.swift`, `ScrollVisibility.swift`; add `platforms/apple/EmbedPDF/Tests/EmbedPDFUITests/HapticFeedbackTests.swift`.

**Interfaces:** Preserve `HapticFeedback.init()`, `selection()`, `impact(weight:)`, and `notify(kind:)`, satisfying the existing `PlatformFeedback` protocol on both OSes. Preserve `trackingScrollVisibility(_:)` and its 1% threshold.

- [x] Build the `EmbedPDFUI` product for macOS 14 with Task 3's local-prebuilt engine and record compiler errors. Add a smoke test constructing `HapticFeedback` and calling selection, all impact weights including nil, and each notification kind. Run it on macOS; expect the current unconditional UIKit import to prevent compilation.
- [x] Use conditional imports/implementations inside `HapticFeedback.swift`: UIKit on iOS, AppKit on macOS. Keep calls nonblocking and dispatch native delivery to the main actor. Map macOS selection to `.alignment` and impact/notification requests to `.generic` through `NSHapticFeedbackManager.defaultPerformer.perform(_:performanceTime:)` with `.default`; document that macOS cannot express the iOS intensity/outcome distinctions. Do not assert physical output in automated tests.
- [x] Change the visibility guard to `#available(iOS 18, macOS 15, *)`; retain appear/disappear behavior on iOS 17 and macOS 14. Audit every remaining UI API against the declared floors; address only demonstrated compilation blockers. Keep desktop wheel/keyboard interaction redesign outside this buildability change.
- [x] Run `EMBEDPDF_PREBUILT_ENGINE=local swift test --package-path platforms/apple/EmbedPDF` on macOS. Compile with explicit macOS 14 deployment for both architectures and build/test the package on an iOS simulator. Expect existing search, stage, rendering, signing, and JavaScriptCore tests plus the feedback smoke test to pass; inspect failures before adding further platform conditions.
- [x] Commit UI portability and its smoke test. Manually display a rendered page on macOS and confirm the adapter tolerates a machine without a haptic trackpad.

### Task 5: Verify delivery paths and ship the supported contract

**Estimate:** 3–5 hours, plus 30–90 minutes per complete verification run with warm native caches.

**Files:** Modify `scripts/publish-engine-payload.mjs`, `.github/workflows/apple-swift.yml`, `platforms/apple/{README.md,EmbedPDF/README.md}`, and `Sources/EmbedPDFUI/Documentation.docc/{EmbedPDFUI,DisplaySwiftUIReader}.md` under the Swift package. Create `platforms/apple/copy-test-fixtures.sh`; move the existing fixture list/copy loop out of `build-xcframework.sh` and invoke it there.

**Interfaces:** Retain `node scripts/publish-engine-payload.mjs --dry-run --only apple` as the build/validate entry point without upload or pin mutation. Publishing uses the existing tag/checksum workflow only after both OS gates pass. Fixture copying must work independently of building the engine.

- [x] Extend the publisher to build all four fork inputs and report each target's SHA in release notes. Test the extracted packaged framework on macOS and iOS before any upload. Preserve the iOS destination override and add `EMBEDPDF_MACOS_TEST_DESTINATION` defaulting to `platform=macOS`. Keep separate derived-data directories and clear conflicting engine environment variables for each mode.
- [x] Extract fixture copying so prebuilt tests do not require Rust or generated artifacts. Verify fixtures copied by the new script match the existing list, including benchmark fixtures only when experimental mode is selected. Replace the workflow's misleading default-mode test after building local FFI with explicit `EMBEDPDF_LOCAL_ENGINE=1`; add separate `EMBEDPDF_PREBUILT_ENGINE=local` jobs after packaging.
- [x] Add pull-request builds of both products using the pinned downloadable binary on macOS and iOS, independent of the manually supplied runtime-build JSON. Include Swift sources, manifest, build/release scripts, and workflow files in triggers. Keep a separate source-engine job requiring all native inputs; do not assume standard `darwin-*` release artifacts provide the new static variants. Run Intel native tests and macOS 14 runtime tests on available matching runners; report these as unverified if unavailable rather than equating cross-compilation with execution.
- [ ] Run the matrix below and the publisher dry run. Update READMEs/DocC with floors, architectures, haptic limitations, fixture setup, and tested commands; remove claims that the package is iOS-only. Commit the implementation. During the release step, publish the verified artifact with the existing publisher, commit its generated pin/checksum bump, then repeat downloaded-mode checks from a clean external checkout. Do not advertise fresh-checkout macOS support before that new pin is downloadable.
- [ ] Review the final diff for generated artifacts and unrelated files, and record evidence for each matrix row. No JavaScript package changeset is needed solely for Swift/shell/docs changes; if implementation affects a published npm package, apply the repository's per-package changeset rules.

## Required verification matrix

Run Swift commands from `platforms/apple/EmbedPDF`. Obtain an available iPhone simulator UDID using `xcrun simctl list devices available -j` and substitute it below. Prepare fixtures with `bash ../copy-test-fixtures.sh`. Use separate scratch/derived-data directories per engine mode so manifest evaluation cannot reuse the wrong target graph.

| Engine mode | macOS | iOS |
| --- | --- | --- |
| Source: `EMBEDPDF_LOCAL_ENGINE=1` | `swift build` and `swift test`; execute bindings smoke tests | `xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,id=<UDID>'` |
| Extracted binary: `EMBEDPDF_PREBUILT_ENGINE=local` | `swift build`, `swift test`, external consumer execution | Same simulator test command; also build for `generic/platform=iOS` with signing disabled |
| Downloaded binary: all engine overrides unset | Clean external consumer linking both products; package tests after fixture setup | Clean external consumer linking both products; simulator package tests after fixture setup |

Also cross-build the macOS external consumer for x86_64, inspect both architectures' archive/interface metadata, and compile at macOS 14. Runtime coverage on Intel/macOS 14 is recorded separately. Success means both public products build on both supported OSes, the existing functional tests pass, and a fresh consumer resolves the newly published engine without local native tooling.

## Execution verification record

On 2026-10-27, source-mode tests passed on macOS and iOS: 331 core tests, 13 UI tests, and 8 bindings smoke tests per platform. Packaged-mode tests passed 331 core and 13 UI tests on each OS, and the generic iOS device build succeeded. A separate consumer rendered a PDF on arm64 and on x86_64 through Rosetta; both executables link only system frameworks/libraries. A rendered 612×792 page was inspected visually. The real packager also passes a regression using tiny compiled inputs and validates deployment metadata and every architecture's Swift interface.

Native Intel hardware and macOS 14 runtime execution remain unverified locally; explicit macOS 14 compilation and Intel binary execution through Rosetta passed. CI includes native Intel coverage. Release and clean downloaded-consumer verification are the final execution steps.

The release dry run also passed both 344-test packaged suites for `apple-engine-794440390c78e90a`. Its tested archive is 278,154,122 bytes with SHA-256 `6d752d47dfbad982579b1bc7c1ecc520237c8b46c008b30cb76fd023884d9d7c`. A consumer using stable Xcode 27.1 builds and renders successfully.

After explicit publication approval, fork commit `ce43b6074431c86805d575e087fd6bcbb1fa9294` was pushed to the new `codex/apple-static-macos-ce43b6074` branch on `roanutil/embed-pdf-runtime`. The exact tested archive was published as [apple-engine-794440390c78e90a](https://github.com/roanutil/embed-pdf-runtime/releases/tag/apple-engine-794440390c78e90a), and the existing publisher updated the package pin from the release checksum. Existing remote branches were preserved. Clean downloaded-mode verification is the remaining execution check.
