#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" -p core-wasm --target wasm32-unknown-unknown --target-dir "$TARGET_DIR"

mkdir -p "$REPO_ROOT/apps/web/public/wasm"
cp "$TARGET_DIR/wasm32-unknown-unknown/release/core_wasm.wasm" "$REPO_ROOT/apps/web/public/wasm/core_wasm.wasm"
echo "Staged core_wasm.wasm -> apps/web/public/wasm/core_wasm.wasm"
