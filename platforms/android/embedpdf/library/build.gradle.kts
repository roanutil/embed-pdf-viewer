plugins {
    id("com.android.library")
}

android {
    namespace = "com.embedpdf"
    compileSdk = 36

    defaultConfig {
        // Matches API in build-aar.sh. A lower floor here than the one the .so
        // was linked against would ship a library that fails to resolve
        // symbols on old devices rather than refusing to install.
        minSdk = 24
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // The .so files and the generated Kotlin are produced by build-aar.sh and
    // are not in the repository, so the source sets point at where it puts
    // them rather than at the Gradle defaults.
    sourceSets {
        getByName("main") {
            kotlin.srcDir("src/main/kotlin")
            jniLibs.srcDir("src/main/jniLibs")
        }
        getByName("androidTest") {
            kotlin.srcDir("src/androidTest/kotlin")
            assets.srcDir("src/androidTest/assets")
        }
    }

    packaging {
        jniLibs {
            // Stored rather than deflated, so Android's linker can mmap the
            // library straight out of the APK. Compressing it also defeats the
            // 16 KB alignment the ELF was built with.
            useLegacyPackaging = false
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Shipped to consumers rather than applied here: the library is
            // not what runs the shrinker, the app is.
            consumerProguardFiles("consumer-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    // uniffi's Kotlin backend calls the .so through JNA, so the AAR variant
    // has to be used: the plain jar cannot find a library inside an APK.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
}
