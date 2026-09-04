# Can PDFium's standalone build target iOS?

Fork sha: `1be49158d2525fe4c9d0eba90c509e9deaef595d`
Sync: `PDF_RUNTIME_TARGET_OS_LIST=ios`

## Verdict: blocked, but not the way Android was

The good news first: there's no host-OS assert for iOS. `build/config/BUILDCONFIG.gn`
picks a `//build/toolchain/ios:ios_clang_$target_cpu` toolchain for `target_os == "ios"`
unconditionally, no `host_os` check at all, unlike the `assert(host_os == "linux", ...)`
that sank Android in the previous task. `gn gen` does not refuse iOS on principle.

It still fails, though. With only the patch this task is scoped to produce
(`apple/pdfium.patch`, touching `BUILD.gn` and `core/fxge/BUILD.gn`), `gn gen` for
`ios-sim-arm64` dies here:

```
$ bash scripts/embedpdf-runtime/target-args.sh ios-sim-arm64 > out/embedpdf-runtime/ios-sim-arm64/args.gn
$ gn gen out/embedpdf-runtime/ios-sim-arm64
ERROR at //testing/test.gni:210:11: Undefined identifier
      if (ios_automatically_manage_certs) {
          ^-----------------------------
See //BUILD.gn:323:1: whence it was called.
test("pdfium_unittests") {
^-------------------------
```

## Why: two independent gaps, not one

I chased this down before writing it off as a wall like Android's. It isn't one.

`pdf_is_standalone=true` is required by every EmbedPDF target, not just iOS, and it pulls
`pdfium_all` into `group("default")` (`BUILD.gn:13-14`), which depends on `pdfium_unittests`
and `pdfium_embeddertests` (`BUILD.gn:446-451`). Both use the `test()` template, and its
`is_ios` branch (`testing/test.gni:185-210`) reads `ios_automatically_manage_certs`. Nothing
in this checkout declares it: I grepped the whole tree, and `testing/test.gni` is the only
file that even mentions the name. `build/config/ios/ios_sdk_overrides.gni`, the file that
declares this kind of arg upstream, only has one `declare_args()` block left in this fork's
pinned `build/` revision, and it's just `ios_deployment_target`. Every other target this
fork builds (`darwin-*`, `linux-*`, `win32-*`, `android-*`, `wasm32*`) is `is_android` or
neither `is_android` nor `is_ios`, so this branch of `test()` was simply never reached
before. iOS is the first target to touch it, and it fell straight through a gap between
PDFium's own `testing/test.gni` and the `build/` revision `DEPS` pins alongside it.

I confirmed the fix works, without committing it: guarding the reference with GN's
`defined()`,

```gn
if (defined(ios_automatically_manage_certs) && ios_automatically_manage_certs) {
```

in place of `testing/test.gni:210`'s bare `if (ios_automatically_manage_certs)`, gets past
this error. That's a one-line change to a file PDFium tracks directly (not a vendored
`build/` dependency, so nothing like the Android assert's "no patch, wrong repo" problem
applies here). But it isn't the whole story: with just that guard in place, `gn gen` fails
again, differently:

```
ERROR at //third_party/libjpeg_turbo/BUILD.gn:14:1: Assertion failed.
assert(
^-----
This is not used if blink is not enabled, don't drag it in unintentionally
See //third_party/libjpeg_turbo/BUILD.gn:15:5: 
    use_blink,
    ^--------
This is where it was set.
See //third_party/BUILD.gn:344:23: which caused the file to be included.
      public_deps = [ "//third_party/libjpeg_turbo:libjpeg" ]
                      ^------------------------------------
```

`build/config/features.gni:41` sets `use_blink = !is_ios`, so it's `false` specifically and
only for iOS among every target_os this fork touches. PDFium's own `third_party/BUILD.gn`
pulls in `libjpeg_turbo` unconditionally through its `group("jpeg")` meta-target, and
`libjpeg_turbo`'s own `BUILD.gn` asserts `use_blink` as a guard against exactly this kind of
accidental non-Blink inclusion. Nothing about JPEG decoding needs Blink; PDFium has never
used Blink for anything. Adding `use_blink=true` to `args.gn` clears it.

With both fixes in place (the `defined()` guard and `use_blink=true`), `gn gen` succeeds:

```
$ bash scripts/embedpdf-runtime/target-args.sh ios-sim-arm64 > out/embedpdf-runtime/ios-sim-arm64/args.gn
$ echo 'use_blink=true' >> out/embedpdf-runtime/ios-sim-arm64/args.gn
$ gn gen out/embedpdf-runtime/ios-sim-arm64
Done. Made 637 targets from 176 files in 841ms
```

I didn't fold either fix into `apple/pdfium.patch`. This task's Step 7 captures the patch
as `git diff -- BUILD.gn core/fxge/BUILD.gn`, deliberately scoped to those two files, and
Step 3 fixes `target-args.sh`'s two iOS cases to a specific, reviewed set of args that
doesn't include `use_blink`. Both fixes are real and I verified them together, not
separately, but committing them is a scope decision for whoever picks this back up, not
mine to make by editing files this task was never handed.

## Source changes needed

`BUILD.gn:287` split `AppKit` (`is_mac`) from `CoreFoundation` (`is_apple`).
`core/fxge/BUILD.gn:173` widened from `is_mac` to `is_apple`; the five `apple/*` sources
include only `<CoreGraphics/CoreGraphics.h>` and standard headers.

Two more edits are needed to get `gn gen` itself past this checkout's actual state,
neither committed here:
- `testing/test.gni:210`, guard `ios_automatically_manage_certs` with `defined()`.
- `ios-arm64`/`ios-sim-arm64`'s `args.gn`, add `use_blink=true`.

## What is still unproven

`gn gen` succeeding is not a compile. The 637 targets it generated were never built; I have
no idea whether `core/fxge/apple/fx_apple_impl.cpp` and its four siblings actually compile
against the iOS SDK, whether `ios_enable_code_signing=false` is sufficient for a `ninja`
run with no signing identity at all, or whether forcing `use_blink=true` drags anything else
into the graph beyond `libjpeg_turbo` that iOS can't actually build. `target_environment`
and `ios_enable_code_signing` themselves were never rejected as unknown args by `gn gen`,
so at least the names I picked in Step 3 are real, but that's the only part of this that
`gn gen` actually exercised end to end.
