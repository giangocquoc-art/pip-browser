plugins {
    id("com.android.application")
}
val pipVersionName = providers.gradleProperty("pipVersion").orElse("0.1.0-preview").get()
val pipVersionCode = providers.gradleProperty("pipVersionCode").orElse("1").get().toInt()
android {
    namespace = "com.psharehub.pip"
    compileSdk = 36
    buildFeatures {
        buildConfig = true
    }
    defaultConfig {
        applicationId = "com.psharehub.pip"
        minSdk = 26
        targetSdk = 36
        versionCode = pipVersionCode
        versionName = pipVersionName
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        testInstrumentationRunnerArguments["disableAnalytics"] = "true"
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    lint {
        abortOnError = true
        checkReleaseBuilds = true
    }
}
dependencies {
    // A debug-only GeckoView pilot keeps the production WebView APK unchanged while
    // validating isolated sessions, private mode, and Google DoH on Android 15 CI.
    debugImplementation("org.mozilla.geckoview:geckoview:153.0.20260810162159")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:core:1.7.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
}
