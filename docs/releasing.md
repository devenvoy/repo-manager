# Releasing

Releases are built by `.github/workflows/release.yml` for macOS, Linux, and
Windows. Every target bundle includes the desktop app, the CLI, and the TUI.

## Cut a release

1. Bump `version` in the root `Cargo.toml` and in every crate under `crates/`
   (they are currently kept in lockstep).
2. Commit the bump and push it to `development`.
3. Tag the commit and push the tag:

   ```sh
   git tag v0.1.0
   git push origin v0.1.0
   ```

The tag version must match the root `Cargo.toml` version. The workflow builds,
packages, checksums, and attaches every archive to the GitHub release. To build
one target without publishing, run the workflow manually and supply a version.

Release notes live in `docs/release-notes/<tag>.md` (for example
`docs/release-notes/v0.1.0.md`). When that file exists, it becomes the GitHub
release body; otherwise the workflow generates notes from commits.

## Targets

| Platform | Architecture | Runner | Rust target |
| --- | --- | --- | --- |
| macOS | Apple Silicon | `macos-15` | `aarch64-apple-darwin` |
| macOS | Intel | `macos-15` | `x86_64-apple-darwin` |
| Linux | x86_64 | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` |
| Linux | aarch64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| Windows | x86_64 | `windows-latest` | `x86_64-pc-windows-msvc` |

## Components

| Component | Archive name | Payload |
| --- | --- | --- |
| Desktop app | `release-desk-app-<version>-<platform>-<arch>` | macOS: `Release Desk.app` + DMG; Windows: `Release Desk.exe`; Linux: `repo-manager` + `.desktop` entry and icon |
| CLI | `release-desk-cli-<version>-<platform>-<arch>` | `release-desk` |
| TUI | `release-desk-tui-<version>-<platform>-<arch>` | `release-desk-tui` |

Every archive contains one top-level folder named after the archive, for
example `release-desk-app-0.1.0-macos-aarch64/`, with the binary, `README.md`,
and `LICENSE`. Windows uses `.zip`; macOS and Linux use `.tar.gz`. macOS app
bundles also ship as `.dmg` disk images.

The workflow keeps the same layout on disk before uploading:

```text
dist/
  v0.1.0/
    macos-aarch64/
      app/
        Release Desk.app/
        README.md
        LICENSE
      cli/
        release-desk
        README.md
        LICENSE
      tui/
        release-desk-tui
        README.md
        LICENSE
      release-desk-app-0.1.0-macos-aarch64.tar.gz
      release-desk-app-0.1.0-macos-aarch64.dmg
      release-desk-cli-0.1.0-macos-aarch64.tar.gz
      release-desk-tui-0.1.0-macos-aarch64.tar.gz
      SHA256SUMS.txt
    linux-x86_64/
      ...
```

The published release also has a combined `SHA256SUMS.txt` covering every
archive.

## Package locally

Build all three binaries first. On macOS, also build the native webview helper:

```sh
cargo build --release -p repo-manager -p release-desk-cli -p release-desk-tui
swiftc bin/release-desk-webview.swift -o target/release/release-desk-webview -O
python3 scripts/package_release.py
```

The packager detects the host platform, architecture, and version. Override any
of them when cross-packaging or testing:

```sh
python3 scripts/package_release.py \
  --version 0.1.0 \
  --platform macos \
  --arch aarch64 \
  --target aarch64-apple-darwin \
  --binary-dir target/aarch64-apple-darwin/release \
  --out dist
```

Use `--components app`, `--components cli,tui`, or another comma-separated
subset to package only those artifacts. macOS app packaging requires the
`swiftc`-built helper next to `repo-manager`; it is ad-hoc signed unless
`DEVELOPER_ID_APPLICATION` is set.
