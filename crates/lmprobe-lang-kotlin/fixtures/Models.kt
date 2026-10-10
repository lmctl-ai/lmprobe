package com.example.models

data class Model(val id: Long, val label: String)

object Constants {
    const val DEFAULT_LIMIT: Int = 50
}

typealias Id = Long
