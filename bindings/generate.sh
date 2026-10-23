#!/usr/bin/env bash
# Generate the Kotlin and Swift bindings for crates/parking-lot-ffi (cr-44).
#
#   bindings/generate.sh [out-dir]        default out-dir: bindings/generated
#   PROFILE=release bindings/generate.sh  bind the release library instead of debug
#
# Library mode: UniFFI reads the interface out of the compiled cdylib, so there is no
# UDL file to keep in step with the Rust code, and the bindings always match the
# library they ship with. Nothing generated is checked in; bindings/generated/ is
# git-ignored. Output:
#
#   <out>/kotlin/uniffi/parking_lot_ffi/parking_lot_ffi.kt
#   <out>/swift/parking_lot_ffi.swift, parking_lot_ffiFFI.h, parking_lot_ffiFFI.modulemap
#
# The generator is bindings/uniffi-bindgen, a standalone workspace (its own Cargo.lock),
# so clap/askama are built only here and never by the root workspace's CI jobs.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
out="${1:-bindings/generated}"
profile="${PROFILE:-debug}"
case "$profile" in
  debug) cargo_profile_flag=() ;;
  release) cargo_profile_flag=(--release) ;;
  *) echo "PROFILE must be debug or release, got '$profile'" >&2; exit 2 ;;
esac

# The generator must be the same UniFFI version as the library it reads, or the
# metadata embedded in the cdylib is rejected (or, worse, misread).
locked_version() { # <Cargo.lock> -> version of the `uniffi` package
  awk '/^name = "uniffi"$/ { getline; gsub(/"/, "", $3); print $3; exit }' "$1"
}
lib_version=$(locked_version Cargo.lock)
gen_version=$(locked_version bindings/uniffi-bindgen/Cargo.lock)
if [ -z "$lib_version" ] || [ "$lib_version" != "$gen_version" ]; then
  echo "uniffi version mismatch: Cargo.lock has '${lib_version}', bindings/uniffi-bindgen/Cargo.lock has '${gen_version}'." >&2
  echo "Bump both together (bindings/uniffi-bindgen/Cargo.toml pins it with '=')." >&2
  exit 1
fi

cargo build --locked "${cargo_profile_flag[@]}" -p prometheus-parking-lot-ffi

target_dir=$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
case "$(uname -s)" in
  Darwin) library="$target_dir/$profile/libparking_lot_ffi.dylib" ;;
  *) library="$target_dir/$profile/libparking_lot_ffi.so" ;;
esac
test -f "$library" || { echo "library not found: $library" >&2; exit 1; }

rm -rf "$out/kotlin" "$out/swift"
for language in kotlin swift; do
  cargo run --locked --quiet --manifest-path bindings/uniffi-bindgen/Cargo.toml -- \
    generate --no-format --library "$library" --language "$language" --out-dir "$out/$language"
done

# Fail loudly on an empty or partial result: "generated" must mean the files exist.
for expected in \
  "$out/kotlin/uniffi/parking_lot_ffi/parking_lot_ffi.kt" \
  "$out/swift/parking_lot_ffi.swift" \
  "$out/swift/parking_lot_ffiFFI.h" \
  "$out/swift/parking_lot_ffiFFI.modulemap"; do
  test -s "$expected" || { echo "missing or empty: $expected" >&2; exit 1; }
done
echo "uniffi $lib_version bindings written to $out:"
find "$out" -type f | sort
