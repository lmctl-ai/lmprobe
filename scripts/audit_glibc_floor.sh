#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: audit_glibc_floor.sh <binary> [max-version]

Inspect an ELF binary for GLIBC version requirements and fail if any required
version exceeds the configured floor.

Arguments:
  <binary>      Path to the binary to inspect
  [max-version] Maximum allowed GLIBC version (default: 2.31)

Examples:
  ./scripts/audit_glibc_floor.sh target/release/lmprobe
  ./scripts/audit_glibc_floor.sh target/release/lmprobe 2.34
USAGE
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
  usage >&2
  exit 2
fi

BINARY="$1"
MAX_VERSION="${2:-2.31}"

if [[ ! -f "$BINARY" ]]; then
  echo "error: binary not found: $BINARY" >&2
  exit 2
fi

if ! [[ "$MAX_VERSION" =~ ^[0-9]+(\.[0-9]+){1,2}$ ]]; then
  echo "error: max-version must be numeric like 2.34 or 2.34.0" >&2
  exit 2
fi

normalize_version() {
  local v="$1"
  IFS='.' read -r -a parts <<< "$v"
  printf "%s.%s.%s" "${parts[0]:-0}" "${parts[1]:-0}" "${parts[2]:-0}"
}

max_version=$(normalize_version "$MAX_VERSION")

needs_readelf() {
  command -v readelf >/dev/null 2>&1
}

needs_objdump() {
  command -v objdump >/dev/null 2>&1
}

extract_with_readelf() {
  readelf -W --version-info "$BINARY" 2>/dev/null \
    | grep -oE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' || true
}

extract_with_objdump() {
  objdump -p "$BINARY" 2>/dev/null \
    | grep -oE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' || true
}

extract_with_strings() {
  strings "$BINARY" \
    | grep -oE 'GLIBC_[0-9]+\.[0-9]+(\.[0-9]+)?' || true
}

if needs_readelf; then
  TOOL="readelf"
  REQUIREMENTS=$(extract_with_readelf | sort -uV)
elif needs_objdump; then
  TOOL="objdump"
  REQUIREMENTS=$(extract_with_objdump | sort -uV)
else
  TOOL="strings"
  REQUIREMENTS=$(extract_with_strings | sort -uV)
fi

if [[ -z "$REQUIREMENTS" ]]; then
  echo "GLIBC requirement scan for: $BINARY"
  echo "scanned_with=$TOOL"
  echo "max_allowed: GLIBC_${max_version} (${MAX_VERSION})"
  echo "no GLIBC_* version symbols found (likely static or already-lowered binary)"
  echo "status=OK"
  exit 0
fi

max_seen="$(echo "$REQUIREMENTS" | sed -E 's/GLIBC_//' | sort -V | tail -n 1)"
max_seen_full="GLIBC_${max_seen}"

exceeds=0
exceeded_list=()
for req in $REQUIREMENTS; do
  req_norm=$(printf '%s' "$req" | sed -E 's/^GLIBC_//')
  req_norm=$(normalize_version "$req_norm")
  if [[ "$(printf '%s\n%s\n' "$req_norm" "$max_version" | sort -V | head -n 1)" != "$req_norm" ]]; then
    exceeds=1
    exceeded_list+=("$req")
  fi
done

echo "GLIBC requirement scan for: $BINARY"
echo "scanned_with=$TOOL"
echo "max_allowed: GLIBC_${max_version} (configured ${MAX_VERSION})"
echo "requirements:"
while IFS= read -r req; do
  printf '  - %s\n' "$req"
done <<< "$REQUIREMENTS"

echo "max_seen: $max_seen_full"

echo "max_count: $(echo "$REQUIREMENTS" | wc -l | tr -d '[:space:]')"

if [[ "$exceeds" -eq 0 ]]; then
  echo "status=OK"
  echo "status_message=all GLIBC requirements are within GLIBC_${max_version}"
  exit 0
fi

echo "status=FAIL"
echo "status_message=requires GLIBC requirements above GLIBC_${max_version}:"
for req in "${exceeded_list[@]}"; do
  echo "  - $req"
done

exit 1
