plugins {
    id("com.android.application") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.0.21"
}

android {
    namespace = "com.example.crabnnbbridge"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.example.crabnnbbridge"
        minSdk = 23
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
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
    implementation("androidx.appcompat:appcompat:1.7.0")
    // Garmin Connect IQ Mobile SDK, published on Maven Central (not redistributable,
    // so it is fetched from the repository instead of being vendored into git).
    implementation("com.garmin.connectiq:ciq-companion-app-sdk:2.4.0")
    testImplementation("junit:junit:4.13.2")
}
