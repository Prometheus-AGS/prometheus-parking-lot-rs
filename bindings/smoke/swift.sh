#!/usr/bin/env bash
# Compile the generated Swift bindings with main.swift and run it against the host
# build of libparking_lot_ffi (macOS only; needs bindings/generate.sh to have run).
#   bindings/smoke/swift.sh [generated-dir]      default: bindings/generated
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
generated="${1:-bindings/generated}"
profile="${PROFILE:-debug}"
target_dir=$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

swiftc -O bindings/smoke/main.swift "$generated/swift/parking_lot_ffi.swift" \
  -Xcc -fmodule-map-file="$generated/swift/parking_lot_ffiFFI.modulemap" -I "$generated/swift" \
  -L "$target_dir/$profile" -lparking_lot_ffi -o "$work/swift-smoke"
DYLD_LIBRARY_PATH="$target_dir/$profile" "$work/swift-smoke"
