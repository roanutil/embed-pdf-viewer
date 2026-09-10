# Android prototype app

The app targets ARM64 Android (`arm64-v8a`). Build the Kotlin bindings and stage
native libraries before packaging it.

## Configure the tools

These are example macOS paths; adjust them to your installed tools:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
```

Gradle needs an Android SDK path, supplied through `ANDROID_HOME` or an existing
`local.properties`. This app configures compile/target SDK 37 and build tools
36.0.0. It requests a Java 21 compilation toolchain through the Foojay resolver.
The NDK is needed by the Rust Android build; Gradle does not invoke a native
build through `externalNativeBuild` in this project.

## Build the bindings and app

From the prototype root, follow the
[local artifact procedure](../../README.md#building-with-local-artifacts)
to supply an `android-arm64` archive, then run:

```bash
bash build/build-kotlin.sh android-arm64
cd apps/android
./gradlew :app:compileDebugKotlin
./gradlew :app:assembleDebug
```

The Kotlin script generates source and stages `libepdf_uniffi.so` plus
`libc++_shared.so` under the Kotlin package's `jniLibs/arm64-v8a` directory.
PDFium is linked into `libepdf_uniffi.so` from a static archive.

Compilation requires generated Kotlin bindings, but not the staged Android
libraries. Packaging runs `requireNativeLibraries` before merging native
libraries. That guard checks only for `libepdf_uniffi.so`; it does not verify
`libc++_shared.so`, the release pin, or library compatibility. Use the build
script to stage both libraries.

## Tests and recorded execution

From `apps/android`:

```bash
./gradlew :app:testDebugUnitTest
```

The JVM tests cover error mapping and document lifecycle behavior without a
device. They do not verify the native PDFium library or the rendered UI.
