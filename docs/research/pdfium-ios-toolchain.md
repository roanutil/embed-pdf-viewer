# Can PDFium's standalone build target iOS?

Fork sha: `73c64bcaa082869b7de182a2edd50ea3a52bbecc` (packages/engine/runtime/runtime-src, branch `embedpdf/feature/mobile-and-wasm-eh`)
Sync: `PDF_RUNTIME_TARGET_OS_LIST=ios`

## Verdict: gn gen succeeds

There's no host-OS assert for iOS. `build/config/BUILDCONFIG.gn` picks a
`//build/toolchain/ios:ios_clang_$target_cpu` toolchain for `target_os == "ios"`
unconditionally, no `host_os` check at all, unlike the `assert(host_os == "linux", ...)`
that sank Android in the previous task. `gn gen` doesn't refuse iOS on principle, and with
two small fixes now committed alongside `apple/pdfium.patch`, it doesn't refuse it in
practice either:

```
$ bash scripts/embedpdf-runtime/apply-patches.sh ios-sim-arm64
$ bash scripts/embedpdf-runtime/target-args.sh ios-sim-arm64 > out/embedpdf-runtime/ios-sim-arm64/args.gn
$ gn gen out/embedpdf-runtime/ios-sim-arm64
Done. Made 637 targets from 176 files in 3116ms
```

`ios-arm64` (the device slice) generates the same 637 targets from the same 176 files.
Both ran from committed state: a clean tree, `unapply-patches.sh`, then `apply-patches.sh`
for the target in question, no scratch edits.

This wasn't the shape I expected going in. The first pass at this patch (touching only
`BUILD.gn` and `core/fxge/BUILD.gn`) hit a real wall, chased below, and I wrote it up here
as blocked. It turned out to be two narrow, independently fixable gaps rather than a wall,
and both fixes are now part of `apple/pdfium.patch` and `target-args.sh`.

## Why: two independent gaps, not one

I chased this down before writing it off as a wall like Android's. It isn't one.

With only `BUILD.gn` and `core/fxge/BUILD.gn` patched, `gn gen` for `ios-sim-arm64` died
here:

```
ERROR at //testing/test.gni:210:11: Undefined identifier
      if (ios_automatically_manage_certs) {
          ^-----------------------------
See //BUILD.gn:323:1: whence it was called.
test("pdfium_unittests") {
^-------------------------
```

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

The fix is to guard the reference with GN's `defined()`,

```gn
if (defined(ios_automatically_manage_certs) && ios_automatically_manage_certs) {
```

in place of `testing/test.gni:210`'s bare `if (ios_automatically_manage_certs)`. That's a
one-line change to a file PDFium tracks directly (`git ls-files --error-unmatch
testing/test.gni` confirms it, mode `100644`, not a gitlink), not a vendored `build/`
dependency, so none of the Android assert's "no patch, wrong repo" problem applies here. It
is now part of `apple/pdfium.patch`, alongside the `BUILD.gn` and `core/fxge/BUILD.gn`
edits below.

But it wasn't the whole story: with just that guard in place, `gn gen` failed again,
differently:

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
used Blink for anything. The fix is `use_blink=true` in `args.gn` for both iOS targets, now
part of `target-args.sh`'s `EXTRA_ARGS` for `ios-arm64` and `ios-sim-arm64`, with a two-line
comment explaining why it's there. Without that comment, `use_blink=true` in a PDF library's
iOS build reads like a mistake, and a later cleanup would delete it as unused, right back
into this same assert.

With both fixes committed, `gn gen` succeeds from a clean checkout, no scratch edits, as
shown in the verdict above. `scripts/embedpdf-runtime/tests/target-args.test.sh` now also
asserts `use_blink=true` for both iOS targets, the same way it already asserted
`target_environment`, so a future edit that drops the arg fails the test before it fails
`gn gen`.

## Source changes needed

`BUILD.gn:287` split `AppKit` (`is_mac`) from `CoreFoundation` (`is_apple`).
`core/fxge/BUILD.gn:173` widened from `is_mac` to `is_apple`; the five `apple/*` sources
include only `<CoreGraphics/CoreGraphics.h>` and standard headers. `testing/test.gni:210`
guards `ios_automatically_manage_certs` with `defined()`, since nothing in this fork's
pinned `build/` revision declares it. All three are in `patches/embedpdf-runtime/apple/pdfium.patch`.

`target-args.sh`'s `ios-arm64`/`ios-sim-arm64` cases carry `use_blink=true` in `EXTRA_ARGS`,
alongside `target_environment` and `ios_enable_code_signing=false`.

## What is still unproven

`gn gen` succeeding is not a compile. The 637 targets it generated were never built; I have
no idea whether `core/fxge/apple/fx_apple_impl.cpp` and its four siblings actually compile
against the iOS SDK, whether `ios_enable_code_signing=false` is sufficient for a `ninja`
run with no signing identity at all, or whether forcing `use_blink=true` drags anything else
into the graph beyond `libjpeg_turbo` that iOS can't actually build. `target_environment`
and `ios_enable_code_signing` themselves were never rejected as unknown args by `gn gen`,
so at least the names I picked in Step 3 are real, but that's the only part of this that
`gn gen` actually exercised end to end.
