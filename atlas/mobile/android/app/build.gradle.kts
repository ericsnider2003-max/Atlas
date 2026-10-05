plugins { id("com.android.application"); id("org.jetbrains.kotlin.android") }

android {
    namespace = "app.atlas"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.ericsnider.atlas"
        minSdk = 29
        targetSdk = 35
        // Android installs an update only if its versionCode is higher than
        // the installed one's. CI passes its run number; a hand build passes
        // ATLAS_VERSION_CODE, and the first APK handed out was 1.
        versionCode = (System.getenv("ATLAS_VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("ATLAS_VERSION_NAME") ?: "0.1.0-dev"
        // Every phone sold for years is 64-bit ARM; the core is built for that.
        ndk { abiFilters += listOf("arm64-v8a") }
    }
    externalNativeBuild { cmake { path = file("src/main/cpp/CMakeLists.txt") } }
    // Java and Kotlin compile for the same JVM level, whatever JDK builds it.
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    // The default config ships as an asset and is copied into the app's
    // private folder on first run (AtlasCore.ensure).
    sourceSets["main"].assets.srcDirs(layout.buildDirectory.dir("atlas-assets"))
}

val shipConfig by tasks.registering(Copy::class) {
    from(rootProject.file("../../config"))
    into(layout.buildDirectory.dir("atlas-assets/config"))
}
tasks.named("preBuild") { dependsOn(shipConfig) }

dependencies {
    // UnifiedPush (Apache-2.0): the laptop reaches this phone with Atlas
    // closed, through whichever distributor the phone has (item 15). It
    // opens the Web Push messages the laptop seals (RFC 8291).
    implementation("org.unifiedpush.android:connector:3.3.5")
}

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
