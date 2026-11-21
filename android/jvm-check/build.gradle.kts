import org.gradle.internal.os.OperatingSystem

plugins {
    kotlin("jvm") version "2.2.20"
    kotlin("plugin.serialization") version "2.2.20"
}

val repoRoot: File = projectDir.parentFile.parentFile
val generated = layout.buildDirectory.dir("generated/uniffi")

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
    sourceSets["main"].kotlin.srcDirs(
        "../app/src/main/java/app/devti/android/core/state",
        generated,
    )
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    implementation("net.java.dev.jna:jna:5.17.0")
    testImplementation(kotlin("test"))
    testImplementation("junit:junit:4.13.2")
}

val os = OperatingSystem.current()
val libName = when {
    os.isWindows -> "devti_ffi.dll"
    os.isMacOsX -> "libdevti_ffi.dylib"
    else -> "libdevti_ffi.so"
}

val cargoHost by tasks.registering(Exec::class) {
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "devti-ffi")
}

val uniffiBindings by tasks.registering(Exec::class) {
    dependsOn(cargoHost)
    workingDir = repoRoot
    commandLine(
        "cargo", "run", "-p", "devti-ffi", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", "target/debug/$libName", "--language", "kotlin", "--no-format",
        "--out-dir", generated.get().asFile.absolutePath,
    )
}

tasks.named("compileKotlin") { dependsOn(uniffiBindings) }

tasks.test {
    systemProperty("jna.library.path", File(repoRoot, "target/debug").absolutePath)
    testLogging { events("passed", "failed"); showStandardStreams = false; exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL }
}
