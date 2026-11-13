plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "com.sheneternity.shen08_music"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "com.sheneternity.shen08_music"
        // gpui-mobile targets API 26+ (Vulkan 1.0 is mandatory from API 26).
        // The project template asked for 35; both satisfy gpui-mobile.
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "1.0"

        // Emulator is x86_64. Add arm64-v8a for real devices.
        ndk {
            abiFilters += listOf("x86_64")
        }
    }

    buildTypes {
        release {
            optimization {
                enable = true
                packageScope = setOf("androidx.**", "kotlin.**", "kotlinx.**")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }

    // The Rust .so is built externally by `cargo ndk` and dropped into
    // app/src/main/jniLibs/<abi>/, so we disable Gradle's own native build.
    sourceSets {
        getByName("main") {
            // AGP 9 deprecates `srcDirs(...)` in favor of the `directories`
            // mutable set.
            jniLibs.directories += "src/main/jniLibs"
        }
    }

    packaging {
        // cargo already strips in release; re-stripping can break backtraces.
        jniLibs {
            keepDebugSymbols += listOf("*/x86_64/libandroid_lib.so")
        }
    }

    lint {
        abortOnError = false
    }
}

dependencies {
}