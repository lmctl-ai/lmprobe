package com.example.models

sealed trait Animal {
  def speak: String
}

class Dog(val name: String) extends Animal {
  override def speak: String = s"$name says woof"
}

class Cat(val name: String) extends Animal with Serializable {
  override def speak: String = s"$name says meow"
}

enum Color {
  case Red, Green, Blue
}

object Constants {
  val Pi: Double = 3.14159
  type StringMap = Map[String, String]
}
