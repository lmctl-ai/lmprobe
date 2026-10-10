package com.example.repo

import kotlinx.coroutines.flow.Flow
import javax.inject.Inject

@Singleton
interface UserRepository {
    val users: Flow<List<String>>
    suspend fun add(name: String)
}

class DefaultUserRepository @Inject constructor() : UserRepository {
    @field:Inject lateinit var clock: java.time.Clock
    override val users = TODO()
    override suspend fun add(name: String) = Unit
}
