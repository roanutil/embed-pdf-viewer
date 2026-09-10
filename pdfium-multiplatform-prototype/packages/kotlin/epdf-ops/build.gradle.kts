plugins {
    kotlin("jvm") version "2.2.20"
}

val scaffoldRoot = file("../../..").canonicalPath
val hostLibs = file("$scaffoldRoot/packages/kotlin/epdf-ops/libs/darwin-aarch64")
val jvmHome = "/Applications/Android Studio.app/Contents/jbr/Contents/Home"

// jvmToolchain(21) cannot resolve against the one JDK on this machine (JBR
// 25.0.3) on its own. Retargeting to 25 was tried first and rejected: the
// Kotlin 2.1.0/2.2.20 compiler backends both refuse JVM target 25
// ("Kotlin does not yet support 25 JDK target"), which then conflicts with
// compileJava's target. So this keeps the 21 pin and leans on the Foojay
// toolchain resolver (settings.gradle.kts) to download a JDK 21 for
// compilation and test execution; Gradle itself still runs on the JBR via
// JAVA_HOME.
kotlin { jvmToolchain(21) }

sourceSets {
    main {
        kotlin.srcDirs("src/main/kotlin", "src/main/kotlin/generated")
    }
}

dependencies {
    api(libs.jna)
    testImplementation(kotlin("test"))
    testImplementation(libs.json)
}

tasks.test {
    useJUnitPlatform()
    environment("JAVA_HOME", jvmHome)
    // JNA finds libepdf_uniffi here; libembedpdf resolves beside it through the
    // @loader_path rpath build/build-kotlin.sh installs.
    systemProperty("jna.library.path", hostLibs.absolutePath)
    systemProperty("epdf.scaffold.root", scaffoldRoot)
    doFirst {
        require(File(hostLibs, "libepdf_uniffi.dylib").exists()) {
            "no host library at $hostLibs. Fix with: bash build/build-kotlin.sh"
        }
    }
}
