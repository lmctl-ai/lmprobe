package com.example.users

import com.example.db.Database
import com.example.util._

trait Repository[T] {
  def find(id: Long): Option[T]
  def save(entity: T): T
}

class User(val id: Long, val name: String, val email: String)

class UserService(db: Database) extends Repository[User] {
  override def find(id: Long): Option[User] = db.queryOne(id)

  override def save(user: User): User = {
    db.insert(user)
    user
  }

  def greet(name: String): String = s"Hello, $name"
}

object UserService {
  val DEFAULT_LIMIT: Int = 100
  var counter: Long = 0L

  def apply(db: Database): UserService = new UserService(db)
}
