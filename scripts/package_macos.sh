#!/usr/bin/env bash
set -euo pipefail

# -----------------------------------------------------------------------------
# Release Desk macOS Application Packaging Script
# -----------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$WORKSPACE_ROOT/dist"
BUNDLE_NAME="Release Desk.app"
BUNDLE_PATH="$DIST_DIR/$BUNDLE_NAME"

echo "=== Building Release Desk for macOS ==="

# 1. Build release binaries
echo "-> Building release binaries..."
cargo build --release -p repo-manager -p release-desk-cli --manifest-path "$WORKSPACE_ROOT/Cargo.toml"

# 2. Prepare distribution directory
echo "-> Creating application bundle layout..."
rm -rf "$BUNDLE_PATH"
mkdir -p "$BUNDLE_PATH/Contents/MacOS"
mkdir -p "$BUNDLE_PATH/Contents/Resources"

# 3. Copy binaries
echo "-> Installing executables..."
cp "$WORKSPACE_ROOT/target/release/repo-manager" "$BUNDLE_PATH/Contents/MacOS/repo-manager"
CLI_BIN="$WORKSPACE_ROOT/target/release/release-desk"
if [ ! -f "$CLI_BIN" ]; then
    CLI_BIN="$WORKSPACE_ROOT/target/release/release-desk-cli"
fi
cp "$CLI_BIN" "$BUNDLE_PATH/Contents/MacOS/release-desk"
chmod +x "$BUNDLE_PATH/Contents/MacOS/repo-manager"
chmod +x "$BUNDLE_PATH/Contents/MacOS/release-desk"

# 4. Copy Info.plist and AppIcon
echo "-> Installing Info.plist and AppIcon..."
cp "$WORKSPACE_ROOT/resources/Info.plist" "$BUNDLE_PATH/Contents/Info.plist"
if [ -f "$WORKSPACE_ROOT/resources/AppIcon.icns" ]; then
    cp "$WORKSPACE_ROOT/resources/AppIcon.icns" "$BUNDLE_PATH/Contents/Resources/AppIcon.icns"
fi

# 5. Code signing
if [ -n "${DEVELOPER_ID_APPLICATION:-}" ]; then
    echo "-> Signing bundle with Developer ID: $DEVELOPER_ID_APPLICATION..."
    codesign --deep --force --options runtime --sign "$DEVELOPER_ID_APPLICATION" "$BUNDLE_PATH"
    echo "✓ Bundle successfully signed."
else
    echo "-> Signing bundle with ad-hoc signature..."
    codesign --force --deep -s - "$BUNDLE_PATH"
    echo "ℹ Ad-hoc signed. Provide DEVELOPER_ID_APPLICATION for Apple notarization."
fi

# 6. Verify signature
codesign --verify --deep --strict "$BUNDLE_PATH"
echo "✓ Bundle verification passed."

# 7. Create distribution DMG / archive
DMG_OUTPUT="$DIST_DIR/Release-Desk-0.1.0-macos.dmg"
if command -v hdiutil >/dev/null 2>&1; then
    echo "-> Creating disk image (DMG)..."
    rm -f "$DMG_OUTPUT"
    hdiutil create -volname "Release Desk" -srcfolder "$BUNDLE_PATH" -ov -format UDZO "$DMG_OUTPUT"
    echo "✓ Generated DMG: $DMG_OUTPUT"
fi

TAR_OUTPUT="$DIST_DIR/Release-Desk-0.1.0-macos.tar.gz"
echo "-> Creating compressed archive..."
tar -czf "$TAR_OUTPUT" -C "$DIST_DIR" "$BUNDLE_NAME"
echo "✓ Generated Archive: $TAR_OUTPUT"

echo "=== Release Desk macOS Packaging Complete ==="
