#!/usr/bin/env sh
set -eu

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
cd "$REPO_ROOT"

cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" -p core-ffi --bins --lib

mkdir -p \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings" \
  "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include" \
  "$REPO_ROOT/packages/apple/Artifacts"

case "$(uname -s)" in
  Darwin) LIB_EXT="dylib" ;;
  *)      LIB_EXT="so" ;;
esac

"$TARGET_DIR/release/uniffi-bindgen" generate \
  --library "$TARGET_DIR/release/libcore_ffi.$LIB_EXT" \
  --language swift \
  --out-dir "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings"

mv -f "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/core_ffiFFI.h" \
      "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include/core_ffiFFI.h"
mv -f "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/core_ffiFFI.modulemap" \
      "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include/module.modulemap"
cp -f "$TARGET_DIR/release/libcore_ffi.a" "$REPO_ROOT/packages/apple/Artifacts/libcore_ffi.a"
