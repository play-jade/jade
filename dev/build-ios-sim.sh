#!/usr/bin/env sh
# Build the native iOS/iPadOS UIKit + SwiftUI + Core Graphics simulator bundle (`JadeIOS.app`).
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
APP_DIR="$REPO_ROOT/target/ios-sim-app/JadeIOS.app"
IOS_BIN="$APP_DIR/JadeIOS"

if [ ! -f "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/core_ffi.swift" ]; then
  "$SCRIPT_DIR/build-apple-ffi.sh"
fi

cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" -p core-ffi --target aarch64-apple-ios-sim

IOS_SIM_SDK="$(xcrun --sdk iphonesimulator --show-sdk-path)"
IOS_SWIFT_BUILD_DIR="/tmp/jade-ios-sim-swift-build"
mkdir -p "$IOS_SWIFT_BUILD_DIR" "$APP_DIR"

cat > "$APP_DIR/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>Jade — 玉枰象棋</string>
  <key>CFBundleExecutable</key>
  <string>JadeIOS</string>
  <key>CFBundleIdentifier</key>
  <string>play.jade.ios.dev</string>
  <key>CFBundleName</key>
  <string>JadeIOS</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>MinimumOSVersion</key>
  <string>17.0</string>
  <key>UIDeviceFamily</key>
  <array>
    <integer>1</integer>
    <integer>2</integer>
  </array>
</dict>
</plist>
PLIST

xcrun --sdk iphonesimulator swiftc -O \
  -whole-module-optimization \
  -sdk "$IOS_SIM_SDK" \
  -target arm64-apple-ios17.0-simulator \
  -module-cache-path /tmp/jade-ios-swift-modcache \
  -Xcc -fmodules-cache-path=/tmp/jade-ios-clang-modcache \
  -parse-as-library \
  -emit-module \
  -emit-object \
  -module-name JadeCoreBindings \
  -I "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include" \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/core_ffi.swift" \
  "$REPO_ROOT/packages/apple/Sources/JadeCoreBindings/CoreGraphicsBoardPainter.swift" \
  -emit-module-path "$IOS_SWIFT_BUILD_DIR/JadeCoreBindings.swiftmodule" \
  -o "$IOS_SWIFT_BUILD_DIR/JadeCoreBindings.o"

xcrun --sdk iphonesimulator swiftc -O \
  -whole-module-optimization \
  -sdk "$IOS_SIM_SDK" \
  -target arm64-apple-ios17.0-simulator \
  -module-cache-path /tmp/jade-ios-swift-modcache \
  -Xcc -fmodules-cache-path=/tmp/jade-ios-clang-modcache \
  -parse-as-library \
  -I "$IOS_SWIFT_BUILD_DIR" \
  -I "$REPO_ROOT/packages/apple/Sources/core_ffiFFI/include" \
  -L "$TARGET_DIR/aarch64-apple-ios-sim/release" \
  -lcore_ffi \
  -lsqlite3 \
  -framework UIKit \
  -framework SwiftUI \
  -framework AVFoundation \
  -framework CoreGraphics \
  -framework CoreText \
  -framework ImageIO \
  -framework PencilKit \
  -framework Security \
  "$IOS_SWIFT_BUILD_DIR/JadeCoreBindings.o" \
  "$REPO_ROOT/apps/ios/Sources/JadeIOS/JadeIOSApp.swift" \
  -o "$IOS_BIN"

echo "Built native iOS simulator app bundle: $APP_DIR"
