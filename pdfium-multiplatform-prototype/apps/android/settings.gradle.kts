pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

plugins {
    // Same reason as packages/kotlin/settings.gradle.kts: kotlin { jvmToolchain(21) }
    // in app/build.gradle.kts needs a JDK 21 this machine's JAVA_HOME (the
    // Android Studio JBR, 25.0.3) is not. This resolver lets Gradle find or
    // download one from disco.foojay.io.
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "epdf-scaffold-android"
include(":app")

// packages/kotlin's epdf-ops has no `group`/`version` of its own (it is
// consumed today only by its own gradlew, never by coordinate), so the
// substitution is spelled out explicitly rather than relying on Gradle to
// match "com.embedpdf.scaffold:epdf-ops" against whatever the included
// build's default group happens to be.
includeBuild("../../packages/kotlin") {
    dependencySubstitution {
        substitute(module("com.embedpdf.scaffold:epdf-ops")).using(project(":epdf-ops"))
    }
}
