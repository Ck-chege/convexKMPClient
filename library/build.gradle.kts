import org.jetbrains.kotlin.gradle.ExperimentalKotlinGradlePluginApi
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.androidLibrary)
    alias(libs.plugins.vanniktech.mavenPublish)


    alias(libs.plugins.kotlinxSerialization)
    alias(libs.plugins.cargo)
    alias(libs.plugins.uniffi)
    alias(libs.plugins.atomicfu)
}

group = "dev.convex.kmp"
version = "1.0.0"

kotlin {
    jvm("desktop") {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_11)
        }
    }
    androidTarget {
        publishLibraryVariants("release")
        @OptIn(ExperimentalKotlinGradlePluginApi::class)
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_11)
        }
    }
    iosX64()
    iosArm64()
    iosSimulatorArm64()


//    macosX64()      // macOS Intel
//    macosArm64()    // macOS Apple Silicon
//    mingwX64()      // Windows 64-bit
////    linuxX64()      // Linux 64-bit




    sourceSets {
        commonMain.dependencies {
            implementation(libs.serialization)
            implementation(libs.kermit.logging)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
        }
    }
}

android {
    namespace = "dev.convex.kmp"
    compileSdk = libs.versions.android.compileSdk.get().toInt()
    defaultConfig {
        minSdk = libs.versions.android.minSdk.get().toInt()
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
}


//
//mavenPublishing {
//    publishToMavenCentral(SonatypeHost.CENTRAL_PORTAL)
//
//    signAllPublications()
//
//    coordinates(group.toString(), "library", version.toString())
//
//    pom {
//        name = "My library"
//        description = "A library."
//        inceptionYear = "2024"
//        url = "https://github.com/kotlin/multiplatform-library-template/"
//        licenses {
//            license {
//                name = "XXX"
//                url = "YYY"
//                distribution = "ZZZ"
//            }
//        }
//        developers {
//            developer {
//                id = "XXX"
//                name = "YYY"
//                url = "ZZZ"
//            }
//        }
//        scm {
//            url = "XXX"
//            connection = "YYY"
//            developerConnection = "ZZZ"
//        }
//    }
//}


