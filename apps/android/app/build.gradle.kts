import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    id("com.google.gms.google-services") version "4.4.2" apply false
    alias(libs.plugins.kotlin.compose)
}

if (file("google-services.json").isFile) apply(plugin = "com.google.gms.google-services")

val keystoreProps = Properties().apply {
    val propsFile = rootProject.file("keystore.properties")
    if (propsFile.isFile) {
        propsFile.inputStream().use(::load)
    }
}

fun signingProp(envKey: String, propKey: String): String? =
    System.getenv(envKey)?.takeIf { it.isNotBlank() }
        ?: keystoreProps.getProperty(propKey)?.takeIf { it.isNotBlank() }

val releaseKeystorePath = signingProp("ORBIT_ANDROID_KEYSTORE_PATH", "storeFile")
val releaseKeystorePassword = signingProp("ORBIT_ANDROID_KEYSTORE_PASSWORD", "storePassword")
val releaseKeyAlias = signingProp("ORBIT_ANDROID_KEY_ALIAS", "keyAlias")
val releaseKeyPassword = signingProp("ORBIT_ANDROID_KEY_PASSWORD", "keyPassword")
val hasReleaseSigning = !releaseKeystorePath.isNullOrBlank() &&
    !releaseKeystorePassword.isNullOrBlank() &&
    !releaseKeyAlias.isNullOrBlank() &&
    !releaseKeyPassword.isNullOrBlank()

val configuredAbis = providers.gradleProperty("orbitAndroidAbis")
    .orNull
    ?.split(",")
    ?.map { it.trim() }
    ?.filter { it.isNotEmpty() }
    ?.takeIf { it.isNotEmpty() }
    ?: listOf("arm64-v8a", "x86_64")

android {
    namespace = "dev.orbit.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.orbit.android"
        minSdk = 29
        targetSdk = 36
        versionCode = 11010
        versionName = "0.11.10"

        ndk {
            abiFilters += configuredAbis
        }
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = rootProject.file(releaseKeystorePath!!)
                storePassword = releaseKeystorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = if (hasReleaseSigning) {
                signingConfigs.getByName("release")
            } else {
                null
            }
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

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
        checkReleaseBuilds = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("com.google.firebase:firebase-messaging:24.1.2")
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)
    implementation(libs.androidx.material.icons.extended)
    implementation(libs.androidx.window)
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.zxing.core)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.aar.get()}@aar")

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}
