# Release Desk Distribution & Packaging Guide

This guide details the release packaging, code signing, notarization, and distribution workflows for macOS, Linux, and Windows.

---

## 1. macOS Application Distribution

### 1.1 Packaging Standalone `.app` & `.dmg`
Release Desk includes an automated packaging script that builds optimized release binaries, constructs the native macOS application bundle, and generates an installable disk image (DMG):

```bash
./scripts/package_macos.sh
```

The script produces:
- `dist/Release Desk.app`: Standalone application bundle.
- `dist/Release-Desk-0.1.0-macos.dmg`: Compressed, mountable disk image for user distribution.
- `dist/Release-Desk-0.1.0-macos.tar.gz`: Standalone archive suitable for CLI or Homebrew cask packaging.

### 1.2 Code Signing with Apple Developer ID
For distribution outside the Mac App Store, macOS Gatekeeper requires signing with an Apple Developer ID Application certificate:

```bash
export DEVELOPER_ID_APPLICATION="Developer ID Application: Your Name (TEAM_ID)"
./scripts/package_macos.sh
```

### 1.3 Apple Notarization Pipeline
Once signed, submit the disk image to Apple's notarization service:

```bash
# 1. Store notarization credentials in keychain
xcrun notarytool store-credentials "RELEASE_DESK_NOTARY" \
  --apple-id "developer@example.com" \
  --team-id "TEAM_ID" \
  --password "xxxx-xxxx-xxxx-xxxx"

# 2. Submit DMG for notarization
xcrun notarytool submit dist/Release-Desk-0.1.0-macos.dmg \
  --keychain-profile "RELEASE_DESK_NOTARY" \
  --wait

# 3. Staple the notarization ticket to the DMG
xcrun stapler staple dist/Release-Desk-0.1.0-macos.dmg

# 4. Verify Gatekeeper acceptance
spctl --assess --type open --context context:primary-signature dist/Release-Desk-0.1.0-macos.dmg
```

---

## 2. Linux Distribution Plan

### 2.1 Binary Targets
Release Desk compiles natively for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`:

```bash
cargo build --release -p release-desk-cli -p release-desk-tui
```

### 2.2 System Integration
- **Config Directory**: `$XDG_CONFIG_HOME/release-desk` (defaults to `~/.config/release-desk`).
- **Data Directory**: `$XDG_DATA_HOME/release-desk` (defaults to `~/.local/share/release-desk`).
- **Secret Store**: Integrates with Linux Secret Service / DBus Secret Portal via `secret-service` / `libsecret`.
- **Background Scheduler**: User `systemd` timer (`~/.config/systemd/user/release-desk-sync.timer`) triggers headless sync jobs.

---

## 3. Windows Distribution Plan

### 3.1 Binary Targets
Release Desk compiles for `x86_64-pc-windows-msvc`:

```bash
cargo build --release -p release-desk-cli -p release-desk-tui
```

### 3.2 System Integration
- **Config & Storage**: `%APPDATA%\Release Desk` and `%LOCALAPPDATA%\Release Desk`.
- **Secret Store**: Windows Credential Manager (`wincred`).
- **Background Scheduler**: Windows Task Scheduler registration via `schtasks.exe`.
- **Installer**: WiX Toolset / MSIX packaging for clean enterprise deployments.
