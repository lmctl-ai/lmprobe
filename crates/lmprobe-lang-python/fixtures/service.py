"""Fixture for lmprobe-lang-python smoke tests — service + decorators."""

import sys
from typing import Optional

from .models import User


class UserService:
    @staticmethod
    def find(user_id: int) -> Optional[User]:
        return None

    @classmethod
    def create(cls, email: str) -> User:
        return User(id=0, email=email)


def greet(user: User) -> str:
    print(f"hello {user.email}", file=sys.stderr)
    return user.email


async def fetch(user_id: int) -> Optional[User]:
    return UserService.find(user_id)
