plugins {
    id("com.android.application") version "8.13.0" apply false
    id("org.jetbrains.kotlin.android") version "2.2.20" apply false
    // Kotlin 2.0+ moves the Compose compiler into its own Gradle plugin, replacing the old
    // composeOptions.kotlinCompilerExtensionVersion. Version tracks the Kotlin version.
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.20" apply false
}
