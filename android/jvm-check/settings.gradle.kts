// Compiles the Android app's platform-free Kotlin (state + UniFFI bindings) on a plain
// JVM and tests it against the real Rust core through JNA. Runs anywhere a JDK and
// Rust are installed — no Android SDK needed.
pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}
dependencyResolutionManagement {
    repositories { mavenCentral() }
}
rootProject.name = "devti-jvm-check"
