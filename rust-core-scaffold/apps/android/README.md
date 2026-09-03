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
./gradlew :app:assembleDebug        # fails on purpose, naming the missing .so
```

The `assembleDebug` failure is expected. `embedpdf/runtime` doesn't build
`android-arm64` yet (see `app/build.gradle.kts`'s `requireNativeLibraries` task for
the full message), so this app can't be packaged or installed until that lands. Every
packaging and install path (`assembleRelease`, `bundleDebug`, `installDebug` included)
fails the same way rather than producing an APK that would crash on launch.
