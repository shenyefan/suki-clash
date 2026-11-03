#!/usr/bin/env python3
"""Stage only distributable files and invoke the pinned Velopack packer."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tomllib
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VPK_VERSION = "1.2.161"
PACK_ID = "SukiClash"
TARGETS = {
    "x86_64-pc-windows-msvc": ("win", "x64"),
    "aarch64-pc-windows-msvc": ("win", "arm64"),
    "x86_64-apple-darwin": ("osx", "x64"),
    "aarch64-apple-darwin": ("osx", "arm64"),
    "x86_64-unknown-linux-gnu": ("linux", "x64"),
    "aarch64-unknown-linux-gnu": ("linux", "arm64"),
}
LINUXDEPLOY = {
    "x64": ("x86_64", "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"),
    "arm64": ("aarch64", "620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff"),
}
BINARIES = ("suki-clash", "suki-clash-service", "suki-clash-service-install", "suki-clash-service-uninstall")


def run(args, **kwargs):
    subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def copy_runtime(source, target, runtime_target):
    """An optional offline bundle follows the app's existing exact manifest contract."""
    if source is None:
        return
    manifest = json.loads((source / "manifest.json").read_text())
    names = {"mihomo.exe" if runtime_target.startswith("windows-") else "mihomo",
             "Country.mmdb", "ASN.mmdb", "geoip.dat", "geosite.dat"}
    if manifest["version"] != 1 or manifest["target"] != runtime_target:
        raise ValueError("runtime bundle version/target does not match the application")
    if {asset["name"] for asset in manifest["assets"]} != names or len(manifest["assets"]) != len(names):
        raise ValueError("runtime bundle must contain the exact five approved assets")
    target.mkdir(parents=True)
    for asset in manifest["assets"]:
        path = source / asset["name"]
        if path.is_symlink() or not path.is_file() or path.stat().st_size != asset["size"]:
            raise ValueError("runtime asset type or size mismatch")
        if hashlib.sha256(path.read_bytes()).hexdigest() != asset["sha256"]:
            raise ValueError("runtime asset digest mismatch")
        shutil.copy2(path, target / path.name)
    shutil.copy2(source / "manifest.json", target / "manifest.json")


def prepare(args, version, os_name, arch):
    stage = args.work / "stage"
    if stage.exists():
        raise ValueError(f"staging directory already exists: {stage}; use a fresh work directory")
    if os_name == "osx":
        package = stage / "Suki Clash.app"
        binaries = package / "Contents/MacOS"
        resources = package / "Contents/Resources"
        binaries.mkdir(parents=True)
        resources.mkdir(parents=True)
        info = plistlib.loads((ROOT / "packaging/macos/Info.plist").read_bytes())
        apple_version = version.split("-", 1)[0].split("+", 1)[0]
        info["CFBundleShortVersionString"] = apple_version
        info["CFBundleVersion"] = apple_version
        info["CFBundleIconFile"] = "suki-clash.icns"
        (package / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
        iconset = args.work / "suki-clash.iconset"
        iconset.mkdir(parents=True)
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = "@2x" if scale == 2 else ""
                run(["sips", "-z", size * scale, size * scale, ROOT / "assets/logo.png", "--out", iconset / f"icon_{size}x{size}{suffix}.png"], stdout=subprocess.DEVNULL)
        run(["iconutil", "-c", "icns", iconset, "-o", resources / "suki-clash.icns"])
    elif os_name == "linux":
        package = stage / "SukiClash.AppDir"
        binaries = package / "usr/bin"
        resources = binaries / "resources"
        binaries.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(ROOT / "assets/logo.png", package / "suki-clash.png")
        (package / "suki-clash.desktop").write_text(
            "[Desktop Entry]\nType=Application\nName=Suki Clash\nExec=suki-clash\n"
            f"Icon=suki-clash\nCategories=Network;\nX-AppImage-Version={version}\n"
            "MimeType=x-scheme-handler/suki-clash;x-scheme-handler/clash;\n")
    else:
        package = stage
        binaries = package
        resources = package / "resources"
        resources.mkdir(parents=True)
    extension = ".exe" if os_name == "win" else ""
    for name in BINARIES:
        source = args.app_dir / (name + extension) if name == "suki-clash" else args.service_dir / (name + extension)
        if source.is_symlink() or not source.is_file():
            raise ValueError(f"required binary missing or invalid: {source}")
        shutil.copy2(source, binaries / source.name)
    shutil.copytree(ROOT / "licenses", resources / "licenses")
    shutil.copy2(ROOT / "platform-service/LICENSE", resources / "licenses/service-GPL-3.0.txt")
    shutil.copy2(ROOT / "assets/logo.png", resources / "logo.png")
    runtime_os = {"win": "windows", "osx": "macos", "linux": "linux"}[os_name]
    runtime_arch = {"x64": "x86_64", "arm64": "aarch64"}[arch]
    copy_runtime(args.runtime_dir, resources / "runtime", f"{runtime_os}-{runtime_arch}")
    if os_name == "linux":
        machine, digest = LINUXDEPLOY[arch]
        tool = args.work / "linuxdeploy.AppImage"
        url = f"https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-{machine}.AppImage"
        with urllib.request.urlopen(url, timeout=120) as response, tool.open("wb") as output:
            shutil.copyfileobj(response, output)
        if hashlib.sha256(tool.read_bytes()).hexdigest() != digest:
            raise ValueError("linuxdeploy digest mismatch")
        tool.chmod(0o755)
        # Extract to avoid requiring FUSE inside a hosted runner.
        run([tool, "--appimage-extract"], cwd=args.work, stdout=subprocess.DEVNULL)
        deploy = args.work / "squashfs-root/AppRun"
        command = [deploy, "--appdir", package, "--desktop-file", package / "suki-clash.desktop", "--icon-file", package / "suki-clash.png"]
        for name in BINARIES:
            command.extend(["--executable", binaries / name])
        run(command)
    return package


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--app-dir", type=Path, required=True)
    parser.add_argument("--service-dir", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--runtime-dir", type=Path)
    parser.add_argument("--vpk", default="vpk")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    args.work = args.work.resolve()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    os_name, arch = TARGETS[args.target]
    expected_host = {"win": "Windows", "osx": "Darwin", "linux": "Linux"}[os_name]
    if platform.system() != expected_host:
        raise ValueError("build and package on the native target OS")
    args.work.mkdir(parents=True, exist_ok=True)
    args.app_dir = args.app_dir.resolve()
    args.service_dir = args.service_dir.resolve()
    package = prepare(args, version, os_name, arch)
    if args.prepare_only:
        print(package)
        return
    installed = subprocess.check_output([args.vpk, "--help"], text=True, encoding="utf-8")
    if f"Velopack CLI {VPK_VERSION}" not in installed:
        raise ValueError(f"vpk must be exactly {VPK_VERSION}")
    channel = f"{os_name}-{arch}"
    output = args.work / "Releases"
    command = [args.vpk, "--skip-updates", "--yes", "pack", "--packId", PACK_ID,
               "--packTitle", "Suki Clash", "--packAuthors", "Suki", "--packVersion", version,
               "--packDir", package, "--mainExe", "suki-clash.exe" if os_name == "win" else "suki-clash",
               "--runtime", f"{os_name}-{arch}", "--channel", channel,
               "--delta", "None", "--outputDir", output]
    signed = False
    if os_name == "win":
        icon = args.work / "suki-clash.ico"
        icon_env = os.environ.copy()
        icon_env.update(SUKI_ICON_SOURCE=str(ROOT / "assets/logo.png"), SUKI_ICON_TARGET=str(icon))
        run(["pwsh", "-NoProfile", "-Command",
             "Add-Type -AssemblyName System.Drawing; "
             "$image=[System.Drawing.Bitmap]::new($env:SUKI_ICON_SOURCE); "
             "$icon=[System.Drawing.Icon]::FromHandle($image.GetHicon()); "
             "$stream=[System.IO.File]::Create($env:SUKI_ICON_TARGET); "
             "try {$icon.Save($stream)} finally {$stream.Dispose(); $icon.Dispose(); $image.Dispose()}"], env=icon_env)
        command.extend(["--icon", icon])
        if thumbprint := os.environ.get("SUKI_WINDOWS_CERT_THUMBPRINT"):
            command.extend(["--signParams", f"/sha1 {thumbprint} /fd SHA256 /tr http://timestamp.digicert.com /td SHA256"])
            signed = True
    elif os_name == "osx":
        app_id = os.environ.get("SUKI_MACOS_APPLICATION_IDENTITY")
        installer_id = os.environ.get("SUKI_MACOS_INSTALLER_IDENTITY")
        notary = os.environ.get("SUKI_MACOS_NOTARY_PROFILE")
        if any((app_id, installer_id, notary)) and not all((app_id, installer_id, notary)):
            raise ValueError("macOS signing requires both identities and a notary profile")
        if app_id:
            command.extend(["--signAppIdentity", app_id, "--signInstallIdentity", installer_id,
                            "--notaryProfile", notary, "--signEntitlements", ROOT / "packaging/macos/entitlements.plist"])
            if keychain := os.environ.get("SUKI_MACOS_KEYCHAIN"):
                command.extend(["--keychain", keychain])
            signed = True
    run(command)
    # This legacy index collides across architectures and is not used by Suki.
    (output / f"RELEASES-{channel}").unlink(missing_ok=True)
    (output / "RELEASES").unlink(missing_ok=True)
    (output / f"build.{channel}.json").write_text(json.dumps({
        "version": version, "target": args.target, "channel": channel, "velopack": VPK_VERSION,
        "signed": signed, "offline_runtime": args.runtime_dir is not None,
    }, indent=2) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
