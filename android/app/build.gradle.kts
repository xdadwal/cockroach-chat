// Imported at the top level: inside the `android { }` block, `java` resolves to Gradle's java
// extension and shadows the java.util package.
import java.util.Properties

// --- Release-driven versioning -----------------------------------------------------------------
// The git tag is the single source of truth (see docs/RELEASING.md). A signed release is cut by
// pushing a `vX.Y.Z` tag; this derives the build's versionName/versionCode from it so the artifact
// always self-reports the release it came from. Dev builds get a `git describe` string. Graceful
// fallback keeps a `.git`-less source tarball building.
fun git(vararg args: String): String? = runCatching {
    // Discard stderr (not merge it): git prints "fatal: No names found" when a repo has no tags,
    // and a merged stream would return that text as a bogus value. Only exit 0 counts.
    val proc = ProcessBuilder(listOf("git") + args)
        .directory(rootProject.projectDir)
        .redirectError(ProcessBuilder.Redirect.DISCARD)
        .start()
    val out = proc.inputStream.bufferedReader().readText().trim()
    if (proc.waitFor() == 0) out.takeIf { it.isNotEmpty() } else null
}.getOrNull()

// On an exact release tag: "0.2.0". Between tags: "0.2.0-4-g1a2b3c". No tag: "0.0.0-dev+1a2b3c".
val derivedVersionName: String = run {
    val exact = git("describe", "--tags", "--exact-match")?.removePrefix("v")
    val described = git("describe", "--tags", "--always", "--dirty")?.removePrefix("v")
    val hasTag = git("describe", "--tags", "--abbrev=0") != null
    when {
        exact != null -> exact
        described != null && hasTag -> described
        described != null -> "0.0.0-dev+$described"
        else -> "0.0.0-dev"
    }
}

// Monotonic per release: MAJOR*10000 + MINOR*100 + PATCH from the nearest tag (v0.2.0 -> 200).
// Missing/malformed components read as 0; defaults to 1 when untagged (Android requires >= 1).
val derivedVersionCode: Int = run {
    val tag = git("describe", "--tags", "--abbrev=0")?.removePrefix("v")
    val (major, minor, patch) = (tag?.substringBefore("-")?.split(".").orEmpty() + listOf("0", "0", "0"))
        .take(3).map { it.toIntOrNull() ?: 0 }
    (major * 10000 + minor * 100 + patch).coerceAtLeast(1)
}

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "chat.cockroach"
    compileSdk = 35

    defaultConfig {
        applicationId = "chat.cockroach"
        minSdk = 26
        targetSdk = 35
        versionCode = derivedVersionCode
        versionName = derivedVersionName
        ndk {
            // Rust .so libs are prebuilt into jniLibs by scripts/build-android-lib.sh.
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    // Release signing. Local builds read android/keystore.properties (gitignored); CI writes the
    // same file from repository secrets, so both paths are identical. Absent that file, the release
    // build stays unsigned rather than silently falling back to the debug key — an APK signed with
    // a publicly-known debug key would be worse than no signature at all.
    val keystorePropsFile = rootProject.file("keystore.properties")
    val keystoreProps = Properties().apply {
        if (keystorePropsFile.exists()) keystorePropsFile.inputStream().use { load(it) }
    }

    signingConfigs {
        if (keystorePropsFile.exists()) {
            create("release") {
                storeFile = rootProject.file(keystoreProps.getProperty("storeFile"))
                storePassword = keystoreProps.getProperty("storePassword")
                keyAlias = keystoreProps.getProperty("keyAlias")
                keyPassword = keystoreProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            // R8 is off for now: JNA and the generated UniFFI bindings both resolve through
            // reflection, so enabling it needs keep-rules and its own on-device verification.
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions {
            jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
        }
    }
    buildFeatures {
        compose = true
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.06.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.activity:activity-compose:1.10.1")
    // FileProvider for "Share this app" (serving the staged APK to the share sheet).
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.9.4")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    // UniFFI-generated Kotlin bindings load the native library through JNA.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    // QR generation + camera scanning for in-person verification.
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}
