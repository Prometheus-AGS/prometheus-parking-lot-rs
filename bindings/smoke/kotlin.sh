#!/usr/bin/env bash
# Compile the generated Kotlin bindings with Smoke.kt and run it on a host JVM against
# the host build of libparking_lot_ffi (needs bindings/generate.sh to have run, and
# `kotlinc` and `java` on PATH). JNA and kotlinx-coroutines are the only runtime
# dependencies of the generated code; they are fetched from Maven Central and verified
# against pinned SHA-256 digests.
#   bindings/smoke/kotlin.sh [generated-dir]     default: bindings/generated
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
generated="${1:-bindings/generated}"
profile="${PROFILE:-debug}"
target_dir=$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
cache="${PARKING_LOT_SMOKE_CACHE:-${TMPDIR:-/tmp}/parking-lot-ffi-smoke-jars}"
mkdir -p "$cache"

JNA_JAR="jna-5.17.0.jar"
JNA_URL="https://repo1.maven.org/maven2/net/java/dev/jna/jna/5.17.0/$JNA_JAR"
JNA_SHA256="b3a9408e7c51e08ef0e3bfcc08f443f6ec0f6191ba8cd7c18d53d2b22e5bdbc0"
COROUTINES_JAR="kotlinx-coroutines-core-jvm-1.10.2.jar"
COROUTINES_URL="https://repo1.maven.org/maven2/org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/1.10.2/$COROUTINES_JAR"
COROUTINES_SHA256="5ca175b38df331fd64155b35cd8cae1251fa9ee369709b36d42e0a288ccce3fd"

sha256_of() { python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$1"; }
fetch() { # <file> <url> <sha256>
  local file="$cache/$1"
  [ -f "$file" ] || curl -fsSL -o "$file" "$2"
  [ "$(sha256_of "$file")" = "$3" ] || { echo "checksum mismatch for $1" >&2; rm -f "$file"; exit 1; }
}
fetch "$JNA_JAR" "$JNA_URL" "$JNA_SHA256"
fetch "$COROUTINES_JAR" "$COROUTINES_URL" "$COROUTINES_SHA256"
classpath="$cache/$JNA_JAR:$cache/$COROUTINES_JAR"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
kotlinc -nowarn -cp "$classpath" -include-runtime -d "$work/smoke.jar" \
  bindings/smoke/Smoke.kt "$generated/kotlin/uniffi/parking_lot_ffi/parking_lot_ffi.kt"
java -Djna.library.path="$target_dir/$profile" -cp "$work/smoke.jar:$classpath" SmokeKt
