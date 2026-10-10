package com.example.annot

@Inject
class Service(@param:Named("primary") val name: String) {
    @get:JvmName("idValue")
    val id: Int = 0

    @Deprecated("use newApi") fun oldApi() {}

    @JvmStatic fun helper(): Int = 42
}
