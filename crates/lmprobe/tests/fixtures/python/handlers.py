import functools
from flask import Blueprint

bp = Blueprint("api", __name__)

@bp.route("/users")
@functools.lru_cache(maxsize=128)
def list_users():
    return []
