#!/usr/bin/env python3
"""Package Release Desk desktop app, CLI, and TUI into versioned folders.

Produces a stable layout regardless of the host OS::

    dist/v0.1.0/macos-aarch64/app/
        Release Desk.app/
        README.md
        LICENSE
    dist/v0.1.0/macos-aarch64/cli/
        release-desk
        README.md
        LICENSE
    dist/v0.1.0/macos-aarch64/tui/
        release-desk-tui
        README.md
        LICENSE
    dist/v0.1.0/macos-aarch64/release-desk-app-0.1.0-macos-aarch64.tar.gz
    dist/v0.1.0/macos-aarch64/release-desk-app-0.1.0-macos-aarch64.dmg
    dist/v0.1.0/macos-aarch64/SHA256SUMS.txt

Archives contain a single top-level folder named after the archive, so
extracting one never scatters files into the current directory.

Run locally from the repository root after building the binaries::

    python3 scripts/package_release.py

CI passes explicit ``--version``, ``--platform``, ``--arch``, and
``--binary-dir`` values for each target.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import plistlib
import platform as host_platform
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parent.parent

COMPONENTS = {
    "app": {
        "package": "repo-manager",
        "binary": "repo-manager",
        "description": "desktop application",
    },
    "cli": {
        "package": "release-desk-cli",
        # The CLI is installed as ``release-desk``; the crate is the
        # implementation detail.
        "binary": "release-desk",
        "description": "command-line interface",
    },
    "tui": {
        "package": "release-desk-tui",
        "binary": "release-desk-tui",
        "description": "terminal user interface",
    },
}

PLATFORM_ALIASES = {
    "darwin": "macos",
    "macos": "macos",
    "linux": "linux",
    "win32": "windows",
    "windows": "windows",
}

ARCH_ALIASES = {
    "x86_64": "x86_64",
    "amd64": "x86_64",
    "x64": "x86_64",
    "arm64": "aarch64",
    "aarch64": "aarch64",
}


def fail(message: str) -> NoReturn:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def detect_platform() -> str:
    name = host_platform.system().lower()
    if name not in PLATFORM_ALIASES:
        fail(f"unsupported platform {name!r}; pass --platform macos|linux|windows")
    return PLATFORM_ALIASES[name]


def detect_arch() -> str:
    machine = host_platform.machine().lower()
    if machine not in ARCH_ALIASES:
        fail(f"unsupported architecture {machine!r}; pass --arch x86_64|aarch64")
    return ARCH_ALIASES[machine]


def detect_version() -> str:
    cargo_toml = ROOT / "Cargo.toml"
    try:
        text = cargo_toml.read_text(encoding="utf-8")
    except OSError as err:
        fail(f"cannot read {cargo_toml}: {err}")

    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        fail(f"no [package] version found in {cargo_toml}")
    return match.group(1)


def normalize_choice(value: str, aliases: dict[str, str], label: str) -> str:
    normalized = aliases.get(value.lower())
    if not normalized:
        allowed = "|".join(sorted(set(aliases.values())))
        fail(f"unknown {label} {value!r}; expected one of {allowed}")
    return normalized


def display(path: Path) -> str:
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        return str(path)


def copy_file(source: Path, destination: Path, mode: int) -> None:
    if not source.is_file():
        fail(f"missing required file: {source}")
    shutil.copyfile(source, destination)
    destination.chmod(mode)


def copy_docs(destination: Path) -> None:
    copy_file(ROOT / "README.md", destination / "README.md", 0o644)
    copy_file(ROOT / "LICENSE", destination / "LICENSE", 0o644)


def sign_app(bundle: Path) -> None:
    codesign = shutil.which("codesign")
    if not codesign:
        print("  warning: codesign not found; leaving app unsigned", file=sys.stderr)
        return

    identity = os.environ.get("DEVELOPER_ID_APPLICATION", "-")
    if identity == "-":
        command = [codesign, "--force", "--deep", "-s", "-", str(bundle)]
    else:
        command = [
            codesign,
            "--force",
            "--deep",
            "--options",
            "runtime",
            "-s",
            identity,
            str(bundle),
        ]
    subprocess.run(command, check=True)
    subprocess.run(
        [codesign, "--verify", "--deep", "--strict", str(bundle)],
        check=True,
    )


def stage_app(
    component_dir: Path,
    binary_dir: Path,
    version: str,
    platform: str,
    suffix: str,
) -> None:
    spec = COMPONENTS["app"]
    source = binary_dir / f"{spec['package']}{suffix}"
    if not source.is_file():
        fail(f"missing desktop application binary: {source}")

    if platform == "macos":
        bundle = component_dir / "Release Desk.app"
        macos_dir = bundle / "Contents" / "MacOS"
        resources_dir = bundle / "Contents" / "Resources"
        macos_dir.mkdir(parents=True)
        resources_dir.mkdir(parents=True)

        copy_file(source, macos_dir / "repo-manager", 0o755)

        helper = binary_dir / "release-desk-webview"
        if not helper.is_file():
            fail(
                f"missing macOS webview helper: {helper} "
                "(build bin/release-desk-webview.swift with swiftc)"
            )
        copy_file(helper, macos_dir / "release-desk-webview", 0o755)

        with (ROOT / "resources" / "Info.plist").open("rb") as handle:
            info = plistlib.load(handle)
        info["CFBundleVersion"] = version
        info["CFBundleShortVersionString"] = version
        with (bundle / "Contents" / "Info.plist").open("wb") as handle:
            plistlib.dump(info, handle)

        copy_file(
            ROOT / "resources" / "AppIcon.icns",
            resources_dir / "AppIcon.icns",
            0o644,
        )
        sign_app(bundle)
    elif platform == "windows":
        copy_file(source, component_dir / "Release Desk.exe", 0o755)
    else:
        copy_file(source, component_dir / "repo-manager", 0o755)
        desktop_entry = ROOT / "resources" / "release-desk.desktop"
        if desktop_entry.is_file():
            text = desktop_entry.read_text(encoding="utf-8")
            text = re.sub(r"(?m)^Exec=.*$", "Exec=repo-manager", text)
            text = re.sub(r"(?m)^Icon=.*$", "Icon=release-desk", text)
            (component_dir / "release-desk.desktop").write_text(text, encoding="utf-8")
            copy_file(
                ROOT / "resources" / "app-icon-256.png",
                component_dir / "release-desk.png",
                0o644,
            )

    copy_docs(component_dir)


def stage_standard(
    component: str,
    component_dir: Path,
    binary_dir: Path,
    suffix: str,
) -> None:
    spec = COMPONENTS[component]
    source = binary_dir / f"{spec['package']}{suffix}"
    if not source.is_file():
        fail(f"missing {component} binary: {source}")
    copy_file(source, component_dir / f"{spec['binary']}{suffix}", 0o755)
    copy_docs(component_dir)


def write_tar_gz(archive: Path, folder: str, source_dir: Path) -> None:
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(source_dir, arcname=folder)


def write_zip(archive: Path, folder: str, source_dir: Path) -> None:
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(source_dir.rglob("*")):
            if path.is_file():
                arcname = Path(folder) / path.relative_to(source_dir)
                bundle.write(path, arcname=str(arcname))


def create_dmg(component_dir: Path, destination: Path) -> Path | None:
    hdiutil = shutil.which("hdiutil")
    if not hdiutil:
        print("  warning: hdiutil not found; skipping DMG", file=sys.stderr)
        return None

    subprocess.run(
        [
            hdiutil,
            "create",
            "-volname",
            "Release Desk",
            "-srcfolder",
            str(component_dir),
            "-ov",
            "-format",
            "UDZO",
            str(destination),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    return destination


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def package(args: argparse.Namespace) -> int:
    version = args.version.lstrip("v")
    platform = normalize_choice(args.platform, PLATFORM_ALIASES, "platform")
    arch = normalize_choice(args.arch, ARCH_ALIASES, "architecture")

    binary_dir = Path(args.binary_dir)
    if not binary_dir.is_absolute():
        binary_dir = ROOT / binary_dir
    if not binary_dir.is_dir():
        fail(f"binary directory does not exist: {binary_dir} (build first)")

    out_dir = Path(args.out)
    if not out_dir.is_absolute():
        out_dir = ROOT / out_dir

    platform_dir = out_dir / f"v{version}" / f"{platform}-{arch}"
    suffix = ".exe" if platform == "windows" else ""
    archive_kind = "zip" if platform == "windows" else "tar.gz"

    print(f"Packaging Release Desk {version} for {platform}-{arch}")
    print(f"  binaries: {binary_dir}")
    print(f"  output:   {platform_dir}")

    archives: list[Path] = []
    for component in args.components:
        spec = COMPONENTS[component]
        folder = f"release-desk-{component}-{version}-{platform}-{arch}"
        component_dir = platform_dir / component
        component_dir.mkdir(parents=True, exist_ok=True)

        if component == "app":
            stage_app(component_dir, binary_dir, version, platform, suffix)
        else:
            stage_standard(component, component_dir, binary_dir, suffix)

        archive = platform_dir / f"{folder}.{archive_kind}"
        if archive_kind == "zip":
            write_zip(archive, folder, component_dir)
        else:
            write_tar_gz(archive, folder, component_dir)
        archives.append(archive)
        print(f"  packaged {spec['description']}: {display(archive)}")

        if component == "app" and platform == "macos":
            dmg = create_dmg(component_dir, platform_dir / f"{folder}.dmg")
            if dmg:
                archives.append(dmg)
                print(f"  packaged macOS disk image: {display(dmg)}")

    checksums = platform_dir / "SHA256SUMS.txt"
    lines = [f"{sha256(archive)}  {archive.name}" for archive in sorted(archives)]
    checksums.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"  checksums: {display(checksums)}")

    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Package the Release Desk app, CLI, and TUI with versioned names.",
    )
    parser.add_argument(
        "--version",
        default=None,
        help="Version to embed in paths/names (default: root Cargo.toml)",
    )
    parser.add_argument(
        "--platform",
        default=None,
        help="macos, linux, or windows (default: detect from host)",
    )
    parser.add_argument(
        "--arch",
        default=None,
        help="x86_64 or aarch64 (default: detect from host)",
    )
    parser.add_argument(
        "--binary-dir",
        default=None,
        help="Directory containing built binaries (default: target/release, "
        "or target/<rust-target>/release with --target)",
    )
    parser.add_argument(
        "--target",
        default=None,
        help="Rust target triple, used only to derive the default --binary-dir",
    )
    parser.add_argument(
        "--out",
        default="dist",
        help="Output root (default: dist)",
    )
    parser.add_argument(
        "--components",
        default="app,cli,tui",
        help="Comma-separated components to package (default: app,cli,tui)",
    )

    args = parser.parse_args(argv)
    args.version = args.version or detect_version()
    args.platform = args.platform or detect_platform()
    args.arch = args.arch or detect_arch()
    if args.binary_dir is None:
        target = f"{args.target}/release" if args.target else "release"
        args.binary_dir = f"target/{target}"

    requested = [part.strip().lower() for part in args.components.split(",") if part.strip()]
    unknown = sorted(set(requested) - set(COMPONENTS))
    if unknown:
        fail(f"unknown component(s): {', '.join(unknown)} (expected app,cli,tui)")
    args.components = requested
    return args


def main(argv: list[str]) -> int:
    return package(parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
