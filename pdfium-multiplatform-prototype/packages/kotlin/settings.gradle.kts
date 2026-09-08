pluginManagement {
    repositories {
        gradlePluginPortal()
    }
}

plugins {
    // jvmToolchain(21) in epdf-ops/build.gradle.kts needs a JDK 21 that this
    // machine does not have (only the JBR's 25.0.3). This resolver lets
    // Gradle download one from disco.foojay.io instead of requiring a second
    // JDK to be installed by hand.
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "epdf-scaffold-kotlin"
include(":epdf-ops")

// The brief's snippet omits this; without it compileClasspath resolution
// fails with "no repositories are defined" for both the Kotlin stdlib and JNA.
dependencyResolutionManagement {
    repositories {
        mavenCentral()
    }
}
