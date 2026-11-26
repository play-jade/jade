#!/bin/sh
# Build and launch the native macOS SwiftUI + AppKit + Core Graphics demo app (works with Command Line Tools or Xcode).
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$REPO_ROOT/target/macos-app/Jade.app"
MACOS_BIN="$APP_DIR/Contents/MacOS/JadeMacOS"

# 1. Build the Rust core-ffi static library and regenerate every Swift UniFFI artifact (bindings, header, module
#    map). Cargo rebuilds only what changed, so an up-to-date checkout pays little, and no stale output survives.
"$SCRIPT_DIR/build-apple-ffi.sh"

# 2. Ensure C++26 local UCI engine exists so "Play vs Engine" works out of the box
if [ ! -x "$REPO_ROOT/engines/mobile-bridge/build/jade-uci-engine" ]; then
  "$SCRIPT_DIR/build-engines.sh"
fi

mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"
cp "$REPO_ROOT/engines/mobile-bridge/build/jade-uci-engine" "$APP_DIR/Contents/MacOS/jade-uci-engine"

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>Jade</string>
  <key>CFBundleExecutable</key>
  <string>JadeMacOS</string>
  <key>CFBundleIdentifier</key>
  <string>play.jade.macos.dev</string>
  <key>CFBundleName</key>
  <string>Jade</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

echo "Compiling native macOS SwiftUI + AppKit + Core Graphics client..."
SWIFT_BUILD_DIR="/tmp/jade-macos-swift-build"
mkdir -p "$SWIFT_BUILD_DIR"

/usr/bin/swiftc -O \
  -whole-module-optimization \
  -parse-as-library \
  -emit-module \
  -emit-object \
  -module-name JadeCoreBindings \
  -module-cache-path /tmp/jade-swift-modcache \
  -Xcc -fmodules-cache-path=/tmp/jade-clang-modcache \
  -I "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include" \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/core_ffi.swift" \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/CoreGraphicsBoardPainter.swift" \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/ReplaySession.swift" \
  -emit-module-path "$SWIFT_BUILD_DIR/JadeCoreBindings.swiftmodule" \
  -o "$SWIFT_BUILD_DIR/JadeCoreBindings.o"

/usr/bin/swiftc -O \
  -parse-as-library \
  -module-cache-path /tmp/jade-swift-modcache \
  -Xcc -fmodules-cache-path=/tmp/jade-clang-modcache \
  -I "$SWIFT_BUILD_DIR" \
  -I "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include" \
  -L "$REPO_ROOT/packages/apple/Artifacts" \
  -lcore_ffi \
  -lsqlite3 \
  -framework AppKit \
  -framework SwiftUI \
  -framework AVFoundation \
  -framework CoreGraphics \
  -framework ImageIO \
  -framework Security \
  -framework UniformTypeIdentifiers \
  "$SWIFT_BUILD_DIR/JadeCoreBindings.o" \
  "$REPO_ROOT/apps/macos/Sources/JadeMacOS/JadeMacOSApp.swift" \
  -o "$MACOS_BIN"

echo "Built native macOS app bundle: $APP_DIR"

if [ "${1:-}" = "--build-only" ]; then
  exit 0
fi

export JADE_REPO_ROOT="$REPO_ROOT"
exec "$MACOS_BIN" "$@"
