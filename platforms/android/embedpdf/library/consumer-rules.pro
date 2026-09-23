# JNA maps these by reflection, so R8 has no reference to follow and strips
# them. The failure is at runtime, in a release build only, with an
# UnsatisfiedLinkError that names a class the source obviously contains.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }

# The generated bindings, for the same reason: every Structure subclass and
# every callback is instantiated by JNA rather than by Kotlin.
-keep class uniffi.epdf_uniffi.** { *; }
