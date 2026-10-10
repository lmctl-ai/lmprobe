#!/usr/bin/env bash
set -euo pipefail

mode="${1:-build}"
glibc_floor="${LMPROBE_GLIBC_FLOOR:-2.31}"
binary="${LMPROBE_BINARY:-target/release/lmprobe}"

install_deps() {
  if command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      build-essential \
      pkg-config \
      perl \
      make \
      cmake \
      binutils \
      file
    rm -rf /var/lib/apt/lists/*
  fi
}

install_rust() {
  if command -v cargo >/dev/null 2>&1; then
    return
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
  # shellcheck disable=SC1090
  source "${CARGO_HOME:-$HOME/.cargo}/env"
}

restore_workspace_owner() {
  if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" && -d target ]]; then
    chown -R "$HOST_UID:$HOST_GID" target
  fi
}
trap restore_workspace_owner EXIT

install_deps
install_rust

case "$mode" in
  build)
    cargo build -p lmprobe --release --locked --features linux-portable-vendored
    ;;
  test-build)
    cargo test -p lmprobe --locked --features linux-portable-vendored
    cargo build -p lmprobe --release --locked --features linux-portable-vendored
    ;;
  *)
    echo "usage: $0 [build|test-build]" >&2
    exit 2
    ;;
esac

"$binary" --version
"$binary" --help >/dev/null
ldd "$binary" || true
./scripts/audit_glibc_floor.sh "$binary" "$glibc_floor"
