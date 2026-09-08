plugins {
    // 9.4.0: the newest stable AGP as of this writing (dl.google.com's
    // maven-metadata.xml for com.android.tools.build:gradle lists 9.4.0 as
    // the latest non-alpha/rc release, with 9.5.0 still alpha-only). Its
    // minimum required Gradle is 9.6.0 per
    // https://developer.android.com/build/releases/agp-9-4-0-release-notes,
    // comfortably under the 9.7.1 this scaffold's Kotlin module already
    // pins. compileSdk 37 needs AGP 9.1.1 at minimum (per
    // https://developer.android.com/build/releases/about-agp's
    // compatibility table), so anything from 9.1.1 up would do; 9.4.0 is
    // current rather than merely sufficient, per this task's instructions.
    id("com.android.application") version "9.4.0"
    // Pinned to match packages/kotlin/epdf-ops/build.gradle.kts's Kotlin
    // 2.2.20 rather than whatever AGP 9.4's built-in Kotlin would bundle;
    // see gradle.properties for the opt-out this plugin needs.
    id("org.jetbrains.kotlin.android") version "2.2.20"
    // Required from Kotlin 2.0 on: the Compose compiler moved out of the
    // Kotlin compiler and into this plugin, versioned in lockstep with
    // Kotlin itself.
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.20"
}

android {
    namespace = "com.embedpdf.scaffold.app"
    compileSdk = 37
    buildToolsVersion = "36.0.0"

    defaultConfig {
        applicationId = "com.embedpdf.scaffold.app"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.0.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildFeatures { compose = true }
    sourceSets["main"].java.srcDirs("src/main/kotlin")
    sourceSets["test"].java.srcDirs("src/test/kotlin")

    // packages/kotlin/epdf-ops is a kotlin("jvm") library, not an Android
    // library, because its host test suite runs on the JVM. That means its
    // src/main/jniLibs is inert there: jniLibs is an Android packaging
    // convention and a JVM jar ignores it. So libepdf_uniffi.so never reached
    // the APK, and the app launched, ran, and reported UnsatisfiedLinkError at
    // the first call. Point the app's own jniLibs at that directory.
    sourceSets["main"].jniLibs.srcDirs("../../../packages/kotlin/epdf-ops/src/main/jniLibs")

    // Without an explicit target, compileDebugJavaWithJavac (AGP's own
    // default, JVM 11) and compileDebugKotlin (whatever the host JDK falls
    // back to; on this machine's JBR 25.0.3, Kotlin 2.2.20 falls back to
    // JVM_24) land on different bytecode targets, and the Kotlin plugin
    // refuses to compile: "Inconsistent JVM Target Compatibility Between
    // Java and Kotlin Tasks". 21 matches jvmToolchain(21) below and
    // packages/kotlin/epdf-ops/build.gradle.kts's same pin.
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
}

// Resolved via the Foojay convention plugin (settings.gradle.kts), same as
// packages/kotlin/epdf-ops/build.gradle.kts: this machine's only JAVA_HOME
// candidates are the JBR (25.0.3, which Kotlin 2.2.20 cannot target) and the
// Gradle-cached Adoptium 21 (already fetched by Task 13, so this needs no
// extra download).
kotlin { jvmToolchain(21) }

dependencies {
    // The plain jna jar arrives transitively from epdf-ops and collides with
    // the @aar variant below: same classes in both, so AGP fails
    // checkDebugDuplicateClasses. Exclude it here and let the aar be the only
    // JNA on the Android classpath.
    implementation("com.embedpdf.scaffold:epdf-ops") {
        exclude(group = "net.java.dev.jna", module = "jna")
    }
    // epdf-ops pulls in net.java.dev.jna:jna, whose plain jar carries
    // libjnidispatch.so for desktop platforms only. On Android JNA needs the
    // @aar variant, which packages libjnidispatch.so per ABI. Without it the
    // app fails at the first FFI call with
    //   Native library (com/sun/jna/android-aarch64/libjnidispatch.so)
    //   not found in resource path (.)
    // Keep the version in step with packages/kotlin/gradle/libs.versions.toml.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    // 2026.08.00: the newest stable Compose BOM (dl.google.com's
    // maven-metadata.xml for androidx.compose:compose-bom), the release that
    // moved to compileSdk 37 / AGP 9.1.1 minimum.
    implementation(platform("androidx.compose:compose-bom:2026.08.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
    // mapOpsError needs no native library or device, so it's covered by a
    // plain JVM unit test (src/test/kotlin) rather than an instrumented one.
    testImplementation(kotlin("test"))
}

/**
 * Fails the build with the same message build/fetch-libpdfium.sh prints, rather
 * than packaging an APK with no native library that would crash on launch. This
 * task goes away when embedpdf/runtime ships android-arm64.
 */
val requireNativeLibraries = tasks.register("requireNativeLibraries") {
    doFirst {
        val so = file("../../../packages/kotlin/epdf-ops/src/main/jniLibs/arm64-v8a/libepdf_uniffi.so")
        require(so.exists()) {
            """
            no libepdf_uniffi.so for arm64-v8a at ${so.path}

            The local fork builds Android, but the committed release pin is pending.
            Set EPDF_SCAFFOLD_PIN_FILE to a separate pin with a local Android archive
            URL and checksum, then run build/build-kotlin.sh android-arm64 from the
            prototype root. See README.md, "Building with local artifacts".
            Packaging can proceed once the required native libraries are staged.

            Everything else in the scaffold still works:
              bash build/test-swift.sh
              node --test packages/node/vectors.test.mjs
              node --test packages/web/vectors.test.mjs
              cd packages/kotlin && ./gradlew :epdf-ops:test
            """.trimIndent()
        }
    }
}

// NOT preBuild: AGP wires preBuild -> preDebugBuild -> the resource/R-class
// generation compileDebugKotlin itself depends on, so gating on preBuild (as
// the brief originally had it) makes compileDebugKotlin fail too, defeating
// the split this task exists to prove (compile succeeds without the .so,
// package fails because of it). assembleDebug is the one lifecycle task that
// actually produces (or, here, refuses to produce) an APK, so the gate hangs
// off that instead. Confirmed by `./gradlew :app:compileDebugKotlin
// --dry-run`, which still lists `:app:requireNativeLibraries` when it hangs
// off preBuild.
//
// NOT a name match against "assembleDebug" alone, either: that only catches
// the one lifecycle task named literally, and AGP creates several other paths
// to a packaged or installed artifact that all bypass it -
// assembleRelease (AGP creates a release build type by default even when
// none is declared), bundleDebug (a separate task producing an AAB), and
// installDebug (what Android Studio's Run button invokes). Enumerating
// lifecycle task names is also fragile: AGP is free to rename or add them
// across versions. Gating on the per-variant native-library merge task
// instead catches all of the above in one match, because every packaging
// path for a variant - assemble, bundle, or install, debug or release -
// depends on that variant's merge<Variant>NativeLibs task. Confirmed for
// AGP 9.4.0 with `--dry-run` on assembleDebug, assembleRelease, bundleDebug,
// and installDebug: all four list a mergeDebugNativeLibs or
// mergeReleaseNativeLibs task.
//
// tasks.named("mergeDebugNativeLibs") fails configuration outright: AGP
// registers per-variant tasks lazily, inside its own
// afterEvaluate/variant callback, so neither that task nor "assembleDebug"
// exists yet when this script's top level runs. configureEach reacts as
// tasks are added instead of requiring the name to already be registered.
tasks.configureEach {
    if (name.startsWith("merge") && name.endsWith("NativeLibs")) {
        dependsOn(requireNativeLibraries)
    }
}
