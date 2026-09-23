plugins {
    // AGP 9 bundles Kotlin support, so applying org.jetbrains.kotlin.android on
    // top of it fails: both register a `kotlin` extension.
    id("com.android.library") version "9.4.1" apply false
}
