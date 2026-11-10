plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.relay.mobile"
    compileSdk = 35
    defaultConfig {
        applicationId = "dev.relay.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 2
        versionName = "0.1.1"
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    testOptions { unitTests.isIncludeAndroidResources = true }
    buildFeatures { compose = true }
    packaging {
        resources.excludes += setOf("META-INF/versions/*/OSGI-INF/MANIFEST.MF", "META-INF/DEPENDENCIES", "META-INF/LICENSE*", "META-INF/NOTICE*")
    }
    // The phone runs the very same server helper as the desktop app.
    sourceSets["main"].assets.srcDir(layout.buildDirectory.dir("generated/bridge"))
}

val copyBridge by tasks.registering(Copy::class) {
    from(rootProject.file("../bridge")) {
        include("relay_bridge.py", "desktop_mcp.py", "board.py", "cluster_mcp.py", "attachments.py")
    }
    into(layout.buildDirectory.dir("generated/bridge/bridge"))
}
tasks.named("preBuild") { dependsOn(copyBridge) }

dependencies {
    val compose = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(compose)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
    implementation("com.github.mwiede:jsch:0.2.21")
    implementation("org.bouncycastle:bcprov-jdk18on:1.79")
    implementation("io.noties.markwon:core:4.6.2")
    implementation("io.noties.markwon:ext-latex:4.6.2")
    implementation("io.noties.markwon:inline-parser:4.6.2")
    implementation("io.noties.markwon:ext-tables:4.6.2")
    implementation("io.noties.markwon:ext-strikethrough:4.6.2")
    implementation("ru.noties:jlatexmath-android-font-cyrillic:0.2.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("androidx.test.ext:junit:1.2.1")
    testImplementation("androidx.compose.ui:ui-test-junit4")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
