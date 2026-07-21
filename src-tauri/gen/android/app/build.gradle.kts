plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.pomodoro.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.pomodoro.app"
        minSdk = 31
        targetSdk = 31
        versionCode = 1
        versionName = "0.6.0"
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
        release {
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    buildFeatures {
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    implementation("androidx.webkit:webkit:1.14.0")
}

// Copy frontend files into APK assets before building.
val copyFrontend = tasks.register<Copy>("copyFrontend") {
    from("../../ui") {
        include("index.html", "app.js", "style.css")
    }
    into("src/main/assets")
}

tasks.named("preBuild") {
    dependsOn(copyFrontend)
}

// Tauri v2 expects assembleUniversalDebug/assembleUniversalRelease tasks.
// Map them to the standard assembleDebug/assembleRelease tasks.
tasks.register("assembleUniversalDebug") {
    dependsOn("assembleDebug")
}
tasks.register("assembleUniversalRelease") {
    dependsOn("assembleRelease")
}
tasks.register("bundleUniversalDebug") {
    dependsOn("bundleDebug")
}
tasks.register("bundleUniversalRelease") {
    dependsOn("bundleRelease")
}

apply(from = "tauri.build.gradle.kts")
