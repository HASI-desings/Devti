import org.gradle.internal.os.OperatingSystem

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "app.devti.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "app.devti.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 102
        versionName = "1.0.2"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        // Release builds are signed with a key from the environment when one is provided
        // (CI secrets); otherwise with the debug key so the APK can still be sideloaded.
        create("release") {
            val ks = System.getenv("DEVTI_KEYSTORE")
            if (ks != null && file(ks).exists()) {
                storeFile = file(ks)
                storePassword = System.getenv("DEVTI_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("DEVTI_KEY_ALIAS")
                keyPassword = System.getenv("DEVTI_KEY_PASSWORD")
            } else {
                initWith(getByName("debug"))
            }
        }
    }

    buildTypes {
        release {
            // UniFFI uses JNA reflection; shrinking stays off until rules are verified.
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            java.srcDir(layout.buildDirectory.dir("generated/uniffi"))
        }
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
        // cargo-ndk copies every cdylib it built; only devti-ffi is ours to load (iroh's
        // own cdylibs are linked into it statically).
        jniLibs.excludes += listOf("**/libiroh.so", "**/libiroh_relay.so")
    }
}

kotlin {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

// ---------------------------------------------------------------- Rust core
// Builds devti-ffi for every Android ABI with cargo-ndk and generates the Kotlin
// bindings with UniFFI. Requires: Rust, `cargo install cargo-ndk`, the Android NDK,
// and `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`.

val repoRoot: File = rootProject.projectDir.parentFile
val skipRust = providers.gradleProperty("devti.skipRust").map { it.toBoolean() }.getOrElse(false)

val cargoNdk by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds libdevti_ffi.so for arm64-v8a, armeabi-v7a and x86_64."
    workingDir = repoRoot
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
        "-o", file("src/main/jniLibs").absolutePath,
        "build", "--release", "-p", "devti-ffi",
    )
    onlyIf { !skipRust }
}

val cargoHost by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds devti-ffi for this computer so UniFFI can read its metadata."
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "devti-ffi")
    onlyIf { !skipRust }
}

val uniffiBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates the Kotlin bindings for Devti Core."
    dependsOn(cargoHost)
    workingDir = repoRoot
    val os = OperatingSystem.current()
    val lib = when {
        os.isWindows -> "target/debug/devti_ffi.dll"
        os.isMacOsX -> "target/debug/libdevti_ffi.dylib"
        else -> "target/debug/libdevti_ffi.so"
    }
    val out = layout.buildDirectory.dir("generated/uniffi").get().asFile
    commandLine(
        "cargo", "run", "-p", "devti-ffi", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", lib, "--language", "kotlin", "--no-format", "--out-dir", out.absolutePath,
    )
    onlyIf { !skipRust }
}

tasks.named("preBuild") {
    dependsOn(cargoNdk, uniffiBindings)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    // Shizuku: lets a computer switch Wi-Fi, data, Bluetooth, hotspot and airplane mode.
    implementation(libs.shizuku.api)
    implementation(libs.shizuku.provider)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.animation)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.zxing.core)
    implementation("${libs.jna.get()}@aar")
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.kotlinx.coroutines.android)
    debugImplementation(libs.compose.ui.tooling)
    testImplementation(libs.junit)
}
