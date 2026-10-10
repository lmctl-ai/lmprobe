"""Fixture for lmprobe-lang-python smoke tests — domain models."""

from dataclasses import dataclass


class Animal:
    def __init__(self, name: str) -> None:
        self.name = name

    def speak(self) -> str:
        return ""


class Dog(Animal):
    def speak(self) -> str:
        return "woof"


@dataclass
class User:
    id: int
    email: str


__all__ = ["Animal", "Dog", "User"]
