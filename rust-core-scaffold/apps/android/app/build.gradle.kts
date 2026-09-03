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
    implementation("com.embedpdf.scaffold:epdf-ops")
    // 2026.08.00: the newest stable Compose BOM (dl.google.com's
    // maven-metadata.xml for androidx.compose:compose-bom), the release that
    // moved to compileSdk 37 / AGP 9.1.1 minimum.
    implementation(platform("androidx.compose:compose-bom:2026.08.00"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.10.0")
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

            embedpdf/runtime does not build android-arm64 yet. That is
            recommendation 6 of docs/research/pdfium-rust-wrapper-decision.md and
            has its own design doc. Until it lands this app cannot be packaged.

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
// tasks.named("assembleDebug") fails configuration outright: AGP registers
// per-variant tasks lazily, inside its own afterEvaluate/variant callback,
// so "assembleDebug" does not exist yet when this script's top level runs.
// configureEach reacts as tasks are added instead of requiring the name to
// already be registered.
tasks.configureEach {
    if (name == "assembleDebug") dependsOn(requireNativeLibraries)
}
