plugins {
    alias(libs.plugins.androidLibrary) apply false
    alias(libs.plugins.kotlinMultiplatform) apply  false
    alias(libs.plugins.vanniktech.mavenPublish) apply false

    alias(libs.plugins.cargo) apply false
    alias(libs.plugins.uniffi) apply false
    alias(libs.plugins.atomicfu) apply false
    alias(libs.plugins.kotlinxSerialization) apply false

}
