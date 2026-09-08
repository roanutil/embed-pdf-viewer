# Android scaffold app

Both Gradle targets below need three environment variables. `local.properties` is
gitignored, so a fresh clone has no SDK path and Gradle fails with `SDK location not
found` before it gets anywhere near the app's own code:

```
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$HOME/Library/Android/sdk/ndk/29.0.14206865"
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
```

`ANDROID_HOME` is what Gradle actually needs to start; without it every target fails
the same way, including `:app:compileDebugKotlin`, which otherwise needs no native
library at all. `ANDROID_NDK_HOME` matters only if you re-run the Rust cross-compile
(`build/build-kotlin.sh android-arm64`); Gradle itself never invokes the NDK here,
there's no `externalNativeBuild` in `app/build.gradle.kts`.

`JAVA_HOME` needs to be set explicitly, not discovered. `/usr/bin/java` is the macOS
stub that errors when nothing is configured, and `/usr/libexec/java_home` won't find
Android Studio's bundled JBR either, since it's never registered as a system JVM. The
JBR is what runs Gradle itself; Kotlin's own compile target is a separate pin
(`kotlin { jvmToolchain(21) }` in `app/build.gradle.kts`), resolved through the Foojay
plugin in `settings.gradle.kts` rather than through `JAVA_HOME`.

With those set:

```
./gradlew :app:compileDebugKotlin   # succeeds: no native library needed to compile
./gradlew :app:assembleDebug        # requires the staged native libraries
```

The packaging failure occurs when the staged native libraries are missing.
The fork builds Android, but the committed release pin has no Android artifacts.
Use the [local artifact procedure](../../README.md#building-with-local-artifacts),
then run `bash build/build-kotlin.sh android-arm64` from the prototype root.
That stages both `libepdf_uniffi.so` and its matching `libc++_shared.so`.
The Gradle guard checks for staged files; it does not inspect the release pin.
Packaging and installation can proceed once those files are present.

On 2026-09-08, the existing local debug APK installed and rendered successfully on
an ARM64 Android emulator with 16 KB pages. The Compose UI displayed 14 pages,
612 × 792 pt, 120 search hits, the rendered page, and extracted text. This was an
existing APK smoke test, not a fresh build or a physical-device/x64 test. See the
[full audit](../../../docs/research/platform-gap-audit.md).
