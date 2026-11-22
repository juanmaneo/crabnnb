plugins {
    id("com.android.application") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.0.21"
    jacoco
}

layout.buildDirectory.set(layout.projectDirectory.dir("../target/phone-bridge-android"))

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

    buildTypes {
        debug {
            enableUnitTestCoverage = true
        }
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

jacoco {
    toolVersion = "0.8.12"
}

val debugClasses = layout.buildDirectory.dir("tmp/kotlin-classes/debug")
val unitTestCoverageData = layout.buildDirectory.file(
    "outputs/unit_test_code_coverage/debugUnitTest/testDebugUnitTest.exec",
)

val jacocoTestReport = tasks.register<JacocoReport>("jacocoTestReport") {
    dependsOn("testDebugUnitTest")
    sourceDirectories.setFrom(files("src/main/java"))
    classDirectories.setFrom(fileTree(debugClasses) {
        // These entry points require Android or Garmin runtime integration tests.
        // All bridge protocol, socket, and relay orchestration code stays covered.
        exclude(
            "com/example/crabnnbbridge/MainActivity*.class",
            "com/example/crabnnbbridge/BridgeForegroundService*.class",
            "com/example/crabnnbbridge/ConnectIqRelayFactory*.class",
            "com/example/crabnnbbridge/GarminConnectIqSdk*.class",
        )
    })
    executionData.setFrom(unitTestCoverageData)
    reports {
        xml.required = true
        html.required = true
        csv.required = false
    }
}

tasks.register<JacocoCoverageVerification>("jacocoTestCoverageVerification") {
    dependsOn(jacocoTestReport)
    executionData.setFrom(unitTestCoverageData)
    classDirectories.setFrom(fileTree(debugClasses) {
        exclude(
            "com/example/crabnnbbridge/MainActivity*.class",
            "com/example/crabnnbbridge/BridgeForegroundService*.class",
            "com/example/crabnnbbridge/ConnectIqRelayFactory*.class",
            "com/example/crabnnbbridge/GarminConnectIqSdk*.class",
        )
    })
    violationRules {
        rule {
            limit { counter = "LINE"; value = "COVEREDRATIO"; minimum = "1.0".toBigDecimal() }
            limit { counter = "BRANCH"; value = "COVEREDRATIO"; minimum = "1.0".toBigDecimal() }
            limit { counter = "INSTRUCTION"; value = "COVEREDRATIO"; minimum = "1.0".toBigDecimal() }
        }
    }
}

tasks.configureEach {
    if (name == "testDebugUnitTest") finalizedBy(jacocoTestReport)
    if (name == "check") dependsOn("jacocoTestCoverageVerification")
}
