package com.example.app

import com.example.data.Model
import kotlinx.coroutines.flow.Flow

interface Repository<T> {
    fun all(): Flow<T>
    suspend fun byId(id: Long): T?
}

class MyModelRepository : Repository<Model> {
    @field:Volatile
    private var cache: List<Model> = emptyList()

    override fun all(): Flow<Model> = TODO()

    override suspend fun byId(id: Long): Model? = cache.firstOrNull { it.id == id }
}
