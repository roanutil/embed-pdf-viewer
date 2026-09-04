# Adding mobile and a Wasm-EH wasm target to embedpdf/runtime

Someone opened `rust-core-scaffold/apps/ios/EpdfScaffold.xcodeproj`, picked an iOS Simulator, and
pressed build. Xcode said this:

```
Building for 'iOS-simulator', but linking in dylib
(.../Frameworks/macos-arm64/libepdf_uniffi.dylib) built for 'macOS'
```

That particular message was my bug, and it's fixed. But the reason a macOS dylib was sitting in the
search path at all is not a bug. It's that `ios-sim-arm64` does not exist. The fork builds nine
targets and neither mobile one, so the scaffold stages the only slice it can get and the linker
tells you what it thinks of that.

This is the document for closing that gap. It covers five new targets in
[embedpdf/runtime](https://github.com/embedpdf/runtime): two for iOS, two for Android, and one more
wasm build that fixes something unrelated but lives in exactly the same machinery.

## What the fork builds today

`scripts/embedpdf-runtime/build-target.sh` has nine cases: `wasm32`, `darwin-arm64`, `darwin-x64`,
`linux-x64`, `linux-arm64`, `linuxmusl-x64`, `linuxmusl-arm64`, `win32-x64`, `win32-arm64`. The
release workflow at `.github/workflows/release-libpdfium.yml:63-71` matches them one for one. Each
one produces a tarball, and `write-manifest.sh` turns those into a JSON manifest of URLs and
sha256s.

The scaffold consumes that manifest directly. `rust-core-scaffold/build/runtime-build.json` holds
thirteen entries: the nine real ones, pinned to fork sha `d7b4caa26289d4846e94ff54e63b586b9bbe9fd6`,
plus `ios-arm64`, `ios-sim-arm64`, `android-arm64` and `android-x64` marked `"status": "pending"`.
`fetch-libpdfium.sh` reads that field and prints an explanation instead of a 404.

So the consumer side is already built and waiting. Adding a target means adding it to the fork and
flipping a `pending` to a URL and a hash. That's the whole integration story, and it's why I don't
think any of this needs new npm packages.

(The nine existing targets each get one, under `packages/engine/runtime/npm/`. Mobile doesn't fit
that shape. An iOS app consumes a static library through Swift Package Manager, an Android app
consumes a `.so` through Gradle, and neither of them wants an npm tarball. I'd rather ship release
artifacts and let the Swift and Kotlin packaging come later, when someone actually needs it.)

## Android is the easy one

I expected Android to be the hard target and it isn't, because upstream PDFium already builds for
it.

`is_android` shows up in PDFium's own `BUILD.gn` at lines 345, 408, 469 and 471. More usefully,
`core/fxge/BUILD.gn:153` compiles a complete Android font backend: `android/cfx_androidfontinfo.cpp`,
`android/cfpf_skiafontmgr.cpp`, `android/cfpf_skiadevicemodule.cpp` and eight more files. That's the
piece I'd have expected to be missing, and it's there. `DEPS` carries a `checkout_android` variable
at line 30 with NDK and SDK entries conditioned on it at lines 360, 370, 382 and 447.

So the work is: sync with `checkout_android: True`, add two cases to `build-target.sh` with
`target_os="android"` and `target_cpu` of `arm64` and `x64`, and package the resulting `.so`. The
existing thread-confinement rule already covers it, since `build-target.sh:76` turns on
`embedpdf_thread_local_globals` for every non-wasm target and Android is one.

Android artifacts are static, not shared. I originally specified a `.so` here and that was wrong,
found by building one: Chromium's Android configuration exports nothing from a shared library, so
`--gc-sections` drops all of PDFium and you get a structurally valid `libembedpdf.so` with a 190 KB
`.text`, no `FPDF_` exports, and not one occurrence of the string `FPDF` anywhere in the binary. The
build succeeds and `file` reports exactly what you expect. Static sidesteps the whole problem: the
Rust `cdylib` absorbs the archive, `jniLibs` ships one `.so` rather than two, and Android now matches
iOS, wasm and linuxmusl instead of being the only target needing an export list.

`android-arm64` maps to the `arm64-v8a` ABI and `android-x64` to `x86_64`, which is what
`rust-core-scaffold/build/build-kotlin.sh:25-33` already expects. I'm deliberately not adding
`armeabi-v7a` or `x86`. The goal is proving the runtime works on a phone and an emulator, and two
ABIs do that. Adding the other two doubles Android CI time for devices this project has no user on
yet.

## iOS is not as bad as it looks

There is no `is_ios` anywhere in PDFium's build files. There's no `checkout_ios` in `DEPS` either.
On a first read that looks like iOS support is simply absent, and I spent a while assuming it meant
a port.

It doesn't. Look at where the Apple rendering backend actually lives.

`core/fxge/BUILD.gn:173` sits behind `if (is_mac)` and compiles `apple/fx_apple_impl.cpp`,
`apple/fx_apple_platform.cpp` and `apple/fx_quartz_device.cpp`, linking `CoreGraphics.framework`.
Every one of those exists on iOS. And the sources are clean: grep them for AppKit, NSFont, Carbon or
ApplicationServices and you get nothing. Their only non-standard include is
`<CoreGraphics/CoreGraphics.h>`.

The files are already named `apple/`. Someone upstream drew the Apple-family boundary correctly and
then gated it on the wrong variable.

AppKit appears exactly once in the entire fork, at `BUILD.gn:289`, alongside `CoreFoundation.framework`
in an `is_mac` block. AppKit is macOS-only and CoreFoundation is not, so that block needs splitting
rather than widening.

Which makes the source change two gn edits. Widen `core/fxge/BUILD.gn:173` from `is_mac` to the
Apple family, and at `BUILD.gn:287` keep AppKit under `is_mac` while CoreFoundation moves to the
Apple family. Chromium's `build/config/BUILDCONFIG.gn` defines `is_apple` as `is_mac || is_ios`, and
PDFium's own files never reference it, so it should be available and unused. That's a claim to
verify, not one to build on.

Because here's what I actually don't know. The unknowns on iOS are all in the toolchain, not the
source. Does Chromium's `build/config/ios` work in a standalone PDFium checkout with
`build_with_chromium=False`? Does `DEPS` fetch what an iOS toolchain needs, given there's no
`checkout_ios` to turn on? I can't answer either from this checkout, because `build/` isn't in it.

So the first task in this spec is not "add ios-arm64". It's "get `gn gen` to succeed for
`target_os="ios"` and report what it took". If that turns out to need real upstream work, I'd rather
find out in a task whose deliverable is a finding than halfway through one whose deliverable is a
release artifact.

iOS also wants a static library where darwin wants a shared one. `build-target.sh:29` sets
`PDF_IS_COMPLETE_LIB=false` for `darwin-arm64` and `apply-patches.sh` applies
`shared-library.patch`. Both iOS targets should keep `pdf_is_complete_lib=true` and skip that patch,
which matches what the scaffold's `build-swift.sh` wants to link.

## The wasm exception standoff

This one is unrelated to mobile and I'm folding it in anyway, because it touches the same six files.

`rust-core-scaffold/build/link-wasm.sh:100-121` records a genuine standoff. rustc 1.98.0's
precompiled std for `wasm32-unknown-emscripten` lowers Rust panics through native WebAssembly
exception handling, and the compiled object imports `__cpp_exception` as a wasm tag. Without
`em++ -fwasm-exceptions`, Node refuses to instantiate the module at all:

```
Import #31 "env" "__cpp_exception": tag import requires a WebAssembly.Tag
```

But the fork's `libembedpdf.a` has eight undefined references to `emscripten_longjmp`, because
PDFium really does use setjmp/longjmp for libpng and libjpeg error handling in
`core/fxcodec/png/pngmodule.cpp` and `core/fxcodec/jpeg/jpeg_common.c`. That needs
`-sSUPPORT_LONGJMP=emscripten`. And em++ refuses both flags on one link: "SUPPORT_LONGJMP=emscripten
is not compatible with -fwasm-exceptions". The two mechanisms need mutually exclusive JS glue.

The scaffold's workaround is `-C panic=abort` for the one wasm crate. It works. It costs
recoverability rather than correctness: a Rust panic aborts the whole wasm instance instead of
surfacing as `OpsError::Internal`, so the engine dies and the page has to reload.

The fix is to compile PDFium's wasm with `-sSUPPORT_LONGJMP=wasm`, so both sides use the native
mechanism and both flags agree. `patches/embedpdf-runtime/wasm/config.gn` is where that goes. Right
now its `config("compiler")` sets a single define, `_POSIX_C_SOURCE=200112`, and no exception flags
at all, so the archive is getting emscripten's defaults.

I want this as a new target, `wasm32-eh`, with `wasm32` left byte-identical. Replacing `wasm32`
would be cleaner and I'm not doing it, because `packages/engine/runtime/npm/wasm32` publishes
`@embedpdf/engine-runtime-wasm32` to real users and Wasm EH raises the browser floor. Firefox
shipped it in 131, October 2024. The cost of not replacing it is honest and permanent: two wasm
artifacts, two exception models, and a build that can drift from its twin without anything failing.

There's one trap waiting in the target name. `build-target.sh:76` reads
`if [[ "$TARGET" != "wasm32" ]]`, and `wasm32-eh` is not equal to `wasm32`, so a target added
naively would silently get `embedpdf_thread_local_globals=true`. That's wrong for wasm, where each
instance already isolates globals in its own linear memory, and nothing would fail loudly. The
condition has to become a prefix match before `wasm32-eh` exists.

(I have not decided this is safe yet, and neither should the implementer. emsdk is pinned at 3.1.72
in `release-libpdfium.yml:74`, and I don't know whether that version's `-fwasm-exceptions` emits
legacy or final EH opcodes. That, not the flag itself, determines which browsers can load the
result. Settling it is a task, and it comes before the build.)

## What done means

A target is not done when ninja exits 0. It's done when the scaffold's own gates go green.

For the four mobile targets: flip the `pending` entries in
`rust-core-scaffold/build/runtime-build.json`, then `bash build/build-swift.sh ios-sim-arm64` links
instead of exiting 2, the Xcode project builds for a simulator instead of hitting the gate script,
and `bash build/build-kotlin.sh android-arm64` produces a `.so` that the Android app packages.

For `wasm32-eh`: `epdf-wasm` builds without `-C panic=abort`, and a deliberate Rust panic surfaces
as `OpsError::Internal` rather than killing the instance. That's the behavior change the whole
exercise is for, so it gets a test rather than a note.

One thing to settle before any of it. The scaffold's artifacts come from fork sha `d7b4caa262`, but
this repo pins the submodule at `fce2b000cc`. Those are different commits and the spec's
implementer needs to be told which one the new targets are cut from, because building five targets
at the wrong sha produces artifacts that link against headers the scaffold's bindgen never saw.

## What this will cost

The prerequisite is the expensive part and it's invisible until you try. The submodule checkout is
source-only: `build/` isn't there, `third_party/freetype` holds 8 files, and `DEPS` lists 54
dependencies to fetch, `build` among them. Before a single object compiles, someone needs
depot_tools and a full `gclient sync`, which is gigabytes and hours.

CI goes from nine build jobs to fourteen, all downstream of the existing `source-tests` gate.

And iOS might just not work. I've argued the source is ready and I believe that, but the toolchain
question is open and I've never seen PDFium built for iOS. If `gn gen` can't be made to work
standalone, this spec ships four targets instead of five and says so.

That would still fix the thing that started it.
