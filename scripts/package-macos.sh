#!/bin/bash
# Build a local, ad-hoc-signed application. Does not install or launch it.
set -euo pipefail

CAH_MAKE_DMG=false
case "${1:-}" in
  --help|-h)
    echo "Usage: bash scripts/package-macos.sh [--dmg]"
    echo "Creates a local .app with its icon in a fresh dist/package.* directory."
    echo "Requires Rust/rustup and Xcode command-line tools. Not signed for public distribution."
    exit 0 ;;
  --dmg) CAH_MAKE_DMG=true ;;
  "") ;;
  *) echo "Unknown option: $1" >&2; exit 2 ;;
esac
if [ "$(uname -s)" != "Darwin" ]; then
  echo "This packager requires macOS." >&2
  exit 1
fi
for CAH_TOOL in cargo rustc rustup xcrun sips iconutil codesign plutil python3; do
  if ! command -v "$CAH_TOOL" >/dev/null 2>&1; then
    echo "Missing prerequisite: $CAH_TOOL. Install Rust and Xcode command-line tools first." >&2
    exit 1
  fi
done
xcrun --find clang >/dev/null
xcrun --find swift >/dev/null
CAH_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$CAH_ROOT"
python3 scripts/release.py check
echo "Building Codex Account Hub for this Mac…"
CAH_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
CAH_BUILD="$(sed -n 's/^build-number = \([0-9]*\)$/\1/p' Cargo.toml)"
if ! [[ "$CAH_BUILD" =~ ^[1-9][0-9]*$ ]]; then echo 'Invalid release build-number'; exit 1; fi
CAH_MIN_OS="13.0"
CAH_BUNDLE_ID="local.codex-account-hub"
case "$(uname -m)" in
  arm64) CAH_TARGET="aarch64-apple-darwin"; CAH_ARCH="arm64" ;;
  x86_64) CAH_TARGET="x86_64-apple-darwin"; CAH_ARCH="x86_64" ;;
  *) echo "Use this section on a Mac."; exit 1 ;;
esac
mkdir -p dist
CAH_PACK_DIR="$(mktemp -d "$PWD/dist/package.XXXXXX")"
CAH_STAGE="$CAH_PACK_DIR/image"
CAH_APP="$CAH_STAGE/Codex Account Hub.app"
CAH_DMG="$CAH_PACK_DIR/Codex-Account-Hub-$CAH_VERSION-$CAH_ARCH.dmg"
mkdir -p "$CAH_APP/Contents/MacOS" "$CAH_APP/Contents/Resources"
rustup target add "$CAH_TARGET"
MACOSX_DEPLOYMENT_TARGET="$CAH_MIN_OS" cargo build --locked --release --target "$CAH_TARGET"
cp "target/$CAH_TARGET/release/codex-account-hub" "$CAH_APP/Contents/MacOS/codex-account-hub"
cp LICENSE "$CAH_APP/Contents/Resources/LICENSE"
cargo metadata --locked --format-version 1 --filter-platform "$CAH_TARGET" > "$CAH_PACK_DIR/dependencies.json"
python3 scripts/release.py licenses "$CAH_PACK_DIR/dependencies.json" "$CAH_APP/Contents/Resources/DEPENDENCIES.md"

cp src/icon.rs "$CAH_PACK_DIR/icon.rs"
cat > "$CAH_PACK_DIR/export_icon.rs" <<'RUST'
#[allow(dead_code)]
mod icon;
fn main() -> std::io::Result<()> {
    let destination = std::env::args().nth(1).expect("output RGBA path");
    std::fs::write(destination, icon::rgba_icon(1024))
}
RUST
rustc --edition=2021 -O "$CAH_PACK_DIR/export_icon.rs" -o "$CAH_PACK_DIR/export-icon"
"$CAH_PACK_DIR/export-icon" "$CAH_PACK_DIR/icon.rgba"

cat > "$CAH_PACK_DIR/rgba_to_png.swift" <<'SWIFT'
import AppKit
import Foundation
let raw = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let size = 1024
guard raw.count == size * size * 4,
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
        isPlanar: false, colorSpaceName: .deviceRGB,
        bitmapFormat: .alphaNonpremultiplied,
        bytesPerRow: size * 4, bitsPerPixel: 32),
      let pixels = bitmap.bitmapData else { fatalError("Invalid icon data") }
raw.copyBytes(to: pixels, count: raw.count)
guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fatalError("Could not encode PNG")
}
try png.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
SWIFT
xcrun swift -module-cache-path "$CAH_PACK_DIR/swift-module-cache" "$CAH_PACK_DIR/rgba_to_png.swift" "$CAH_PACK_DIR/icon.rgba" "$CAH_PACK_DIR/icon-1024.png"

mkdir "$CAH_PACK_DIR/AccountHub.iconset"
for CAH_SIZE in 16 32 128 256 512; do
  sips -z "$CAH_SIZE" "$CAH_SIZE" "$CAH_PACK_DIR/icon-1024.png" \
    --out "$CAH_PACK_DIR/AccountHub.iconset/icon_${CAH_SIZE}x${CAH_SIZE}.png" >/dev/null
  CAH_RETINA=$((CAH_SIZE * 2))
  sips -z "$CAH_RETINA" "$CAH_RETINA" "$CAH_PACK_DIR/icon-1024.png" \
    --out "$CAH_PACK_DIR/AccountHub.iconset/icon_${CAH_SIZE}x${CAH_SIZE}@2x.png" >/dev/null
done
iconutil -c icns "$CAH_PACK_DIR/AccountHub.iconset" \
  -o "$CAH_APP/Contents/Resources/AccountHub.icns"

# Finder must register the actual Rust executable as the bundle's main executable.

cat > "$CAH_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Codex Account Hub</string>
  <key>CFBundleDisplayName</key><string>Codex Account Hub</string>
  <key>CFBundleIdentifier</key><string>$CAH_BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>codex-account-hub</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$CAH_VERSION</string>
  <key>CFBundleVersion</key><string>$CAH_BUILD</string>
  <key>CFBundleIconFile</key><string>AccountHub.icns</string>
  <key>LSMinimumSystemVersion</key><string>$CAH_MIN_OS</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
</dict></plist>
PLIST
plutil -lint "$CAH_APP/Contents/Info.plist"
file "$CAH_APP/Contents/MacOS/codex-account-hub"
otool -L "$CAH_APP/Contents/MacOS/codex-account-hub"

codesign --force --sign - "$CAH_APP/Contents/MacOS/codex-account-hub"
codesign --force --sign - "$CAH_APP"
codesign --verify --deep --strict --verbose=2 "$CAH_APP"
"$CAH_APP/Contents/MacOS/codex-account-hub" --diagnose-startup

if [ "$CAH_MAKE_DMG" = true ]; then
  ln -s /Applications "$CAH_STAGE/Applications"
  hdiutil create -volname "Codex Account Hub" -srcfolder "$CAH_STAGE" \
    -format UDZO -ov "$CAH_DMG"
  hdiutil verify "$CAH_DMG"
  CAH_ASSETS="$CAH_PACK_DIR/release-assets"
  mkdir "$CAH_ASSETS"
  cp "$CAH_DMG" "$CAH_ASSETS/"
  cp "$CAH_APP/Contents/Resources/DEPENDENCIES.md" "$CAH_ASSETS/DEPENDENCIES-$CAH_ARCH.md"
  cp "$CAH_APP/Contents/Resources/THIRD_PARTY_NOTICES.txt" "$CAH_ASSETS/THIRD_PARTY_NOTICES-$CAH_ARCH.txt"
  (cd "$CAH_ASSETS" && shasum -a 256 "$(basename "$CAH_DMG")" > "$(basename "$CAH_DMG").sha256")
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    printf 'assets=%s\n' "$CAH_ASSETS" >> "$GITHUB_OUTPUT"
  fi
  echo "Disk image: $CAH_DMG"
fi
echo "App ready: $CAH_APP"
echo "Quit the previous Account Hub, then drag this app into Applications."
echo "This local build is not Developer ID signed or notarized for distribution."
# Reveal the output; do not launch it or touch running Codex sessions.
if [ "${CAH_REVEAL:-1}" != "0" ]; then
  open -R "$CAH_APP" || true
fi
