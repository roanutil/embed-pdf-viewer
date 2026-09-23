# Android

`multiplatform-plan.md` Phase 1 asks for an Android AAR with both ABIs,
`libc++_shared`, the shrinker rules and the 16 KB checks. This is that, built
locally and published nowhere.

```bash
bash platforms/android/build-aar.sh          # build the AAR and run the checks
bash platforms/android/build-aar.sh test     # also run the smoke test on a device
```

`ANDROID_HOME` and `JAVA_HOME` have to be set; the script finds the NDK at
`$ANDROID_NDK_HOME`, defaulting to `~/Library/Android/sdk/ndk/29.0.14206865`.

## The fork slices are prebuilt, and have to be

`libembedpdf.a` for `android-arm64` and `android-x64` comes from fork commit
`7f0fa5dbe`, the same commit the iOS slices come from, registered in
`engine-runtime-build.local.json` as `file://` URLs. They are not built here and
cannot be: `runtime-src/build/config/BUILDCONFIG.gn` asserts

```
assert(host_os == "linux", "Android builds are only supported on Linux.")
```

so `scripts/embedpdf-runtime/build-target.sh android-arm64` fails at `gn gen` on
macOS, after a full gclient resync that is wasted. Rebuilding these needs a Linux
host. That is worth knowing before anyone tries, and it is the one part of the
Android path that is not reproducible on this machine.

## What the script checks

Every `.so` it packages is read with `llvm-readelf` and its `LOAD` alignment has
to be 16 KB or better. Both are `0x4000` today, because NDK r28 and later align
that way by default. A default is exactly the thing that changes back under you,
and a 4 KB-aligned library does not load at all on a 16 KB page-size device, so
it is checked rather than trusted.

The APK is checked separately, with `zipalign -c -P 16`, because an ELF with
16 KB `LOAD` segments still fails to map if the zip entry holding it does not
start on a 16 KB boundary. The two checks answer different questions. The only
APK this project builds is the test one; an app that packages the AAR has to
run the same check on its own output.

`useLegacyPackaging = false` keeps the libraries stored rather than deflated
inside the archive, which is what lets the linker map one straight out of the
APK. Compressing would also throw away the alignment the ELF was built with.

`library/consumer-rules.pro` keeps JNA and the generated bindings. JNA reaches
both by reflection, so R8 sees no reference and strips them; the failure is an
`UnsatisfiedLinkError` in release builds only, naming a class the source
obviously contains.

## The smoke test

`library/src/androidTest/kotlin/com/embedpdf/SmokeTest.kt` asserts what
`crates/epdf-uniffi/tests/native_surface.rs` and the iOS bundle assert, in the
same order. Identical assertions across the three mean a divergence is a
packaging problem rather than a logic one.

Results, 2026-09-23: 8 tests, 0 failures, on the `Pixel_4` AVD running
`system-images/android-37.1/google_apis_playstore_ps16k/arm64-v8a`. That image
is the 16 KB page-size one, confirmed by `getconf PAGE_SIZE` reporting 16384 on
the device, so this is the 16 KB emulator run the plan asks for and not a 4 KB
image that happens to load a 16 KB-aligned library.

Not covered: a signed run on physical hardware, which needs a device and a
signing identity, and `x86_64`, which is built and checked but not executed
because the only image installed here is arm64.

## Two things this shook out

Both were invisible from Rust and from Swift, and both came from uniffi's Kotlin
backend mapping exported items onto Java types that already have members.

`PdfError::Engine` carried a field named `message`. Kotlin renders an error
variant as a `Throwable` subclass with the fields as constructor properties, so
the generated file declared `val message: String` twice and did not compile. The
field is `detail` now.

`PdfEngine::close` and `PdfDocument::close` collided the same way: uniffi gives
every exported object an `AutoCloseable.close()` that disposes the handle, and
two `fun close(): Unit` in one class is not legal Kotlin. Both are `shutdown()`
now. Dropping the last reference already does the same work through
`OwnerThread`'s `Drop`, so `use { }` in Kotlin and ARC in Swift stay correct
whether or not anyone calls it.

The cost of the second rename is that `shutdown` is not the word either platform
expects for this, and a facade will want to present it as `close`. That is the
facade's job and a reason it exists.
