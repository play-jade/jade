#!/usr/bin/env sh
# Build host UniFFI Kotlin bindings and cross-compile `libcore_ffi.so` for Android ABIs (`arm64-v8a` and `x86_64`).
set -eu

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
cd "$REPO_ROOT"

# 1. Host build + Kotlin UniFFI binding generation (also provides host library for JVM unit tests)
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" -p core-ffi --bins --lib

case "$(uname -s)" in
  Darwin)
    HOST_LIB_EXT="dylib"
    NDK_HOST_TAG="darwin-x86_64"
    ;;
  *)
    HOST_LIB_EXT="so"
    NDK_HOST_TAG="linux-x86_64"
    ;;
esac

mkdir -p "$REPO_ROOT/apps/android/app/src/main/java"
"$TARGET_DIR/release/uniffi-bindgen" generate \
  --library "$TARGET_DIR/release/libcore_ffi.$HOST_LIB_EXT" \
  --language kotlin \
  --out-dir "$REPO_ROOT/apps/android/app/src/main/java"

# 2. Locate Android NDK toolchain and cross-compile `libcore_ffi.so` for `arm64-v8a` and `x86_64`
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-/usr/local/lib/android/sdk}}"
NDK_DIR="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [ -z "$NDK_DIR" ] && [ -d "$SDK_ROOT/ndk" ]; then
  NDK_DIR="$(ls -d "$SDK_ROOT"/ndk/* 2>/dev/null | sort -V | tail -n 1 || true)"
fi
if [ -z "$NDK_DIR" ] || [ ! -d "$NDK_DIR" ]; then
  echo "ERROR: Android NDK not found under ANDROID_NDK_HOME or $SDK_ROOT/ndk" >&2
  exit 1
fi

NDK_BIN="$NDK_DIR/toolchains/llvm/prebuilt/$NDK_HOST_TAG/bin"
if [ ! -d "$NDK_BIN" ]; then
  NDK_BIN="$(ls -d "$NDK_DIR"/toolchains/llvm/prebuilt/*/bin | head -n 1)"
fi

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android28-clang"
export CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android28-clang"
export AR_aarch64_linux_android="$NDK_BIN/llvm-ar"

export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_BIN/x86_64-linux-android28-clang"
export CC_x86_64_linux_android="$NDK_BIN/x86_64-linux-android28-clang"
export AR_x86_64_linux_android="$NDK_BIN/llvm-ar"

cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" -p core-ffi \
  --target aarch64-linux-android \
  --target x86_64-linux-android

JNI_LIBS_DIR="$REPO_ROOT/apps/android/app/src/main/jniLibs"
mkdir -p "$JNI_LIBS_DIR/arm64-v8a" "$JNI_LIBS_DIR/x86_64"
cp -f "$TARGET_DIR/aarch64-linux-android/release/libcore_ffi.so" "$JNI_LIBS_DIR/arm64-v8a/libcore_ffi.so"
cp -f "$TARGET_DIR/x86_64-linux-android/release/libcore_ffi.so" "$JNI_LIBS_DIR/x86_64/libcore_ffi.so"

echo "Staged Android JNI libraries:"
ls -lh "$JNI_LIBS_DIR/arm64-v8a/libcore_ffi.so" "$JNI_LIBS_DIR/x86_64/libcore_ffi.so"
