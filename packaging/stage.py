#!/usr/bin/env python3
"""Stage the release binaries and offline runtime in the layout packed by vpk.

Usage: stage.py TARGET VERSION RUNTIME_DIR STAGE_DIR   (run on the target's own runner)
"""
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
LINUXDEPLOY = ("1-alpha-20251107-1", "linuxdeploy-x86_64.AppImage",
               "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d")


def run(*args, **kwargs):
    subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def macos(package, version):
    resources = package / "Contents/Resources"
    info = plistlib.loads((ROOT / "packaging/macos/Info.plist").read_bytes())
    info["CFBundleShortVersionString"] = info["CFBundleVersion"] = version
    info["CFBundleIconFile"] = "suki-clash.icns"
    (package / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
    with tempfile.TemporaryDirectory() as temporary:
        iconset = Path(temporary) / "suki-clash.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale, suffix in ((1, ""), (2, "@2x")):
                pixels = size * scale
                run("sips", "-z", pixels, pixels, ROOT / "assets/logo.png",
                    "--out", iconset / f"icon_{size}x{size}{suffix}.png", stdout=subprocess.DEVNULL)
        run("iconutil", "-c", "icns", iconset, "-o", resources / "suki-clash.icns")


def linux(appdir, version):
    shutil.copy2(ROOT / "assets/logo.png", appdir / "suki-clash.png")
    (appdir / "suki-clash.desktop").write_text(
        "[Desktop Entry]\nType=Application\nName=Suki Clash\nExec=suki-clash\nIcon=suki-clash\n"
        "Terminal=false\nStartupWMClass=suki-clash\nCategories=Network;\n"
        f"X-AppImage-Version={version}\nMimeType=x-scheme-handler/suki-clash;x-scheme-handler/clash;\n")
    # linuxdeploy collects the shared libraries; extracting avoids FUSE on the runner.
    tag, tool, digest = LINUXDEPLOY
    with tempfile.TemporaryDirectory() as temporary:
        tools = Path(temporary)
        for attempt in range(1, 5):
            try:
                run("gh", "release", "download", tag, "-R", "linuxdeploy/linuxdeploy", "-p", tool, "-D", tools, "--clobber")
                break
            except subprocess.CalledProcessError:
                if attempt == 4:
                    raise
                time.sleep(attempt * 2)
        run("sha256sum", "-c", "-", input=f"{digest}  {tool}\n".encode(), cwd=tools)
        (tools / tool).chmod(0o755)
        run(tools / tool, "--appimage-extract", cwd=tools, stdout=subprocess.DEVNULL)
        binaries = appdir / "usr/bin"
        run(tools / "squashfs-root/AppRun", "--appdir", appdir,
            "--desktop-file", appdir / "suki-clash.desktop", "--icon-file", appdir / "suki-clash.png",
            "--executable", binaries / "suki-clash", "--executable", binaries / "suki-clash-service")


def main():
    target, version, runtime, stage = sys.argv[1], sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4])
    if target.endswith("-pc-windows-msvc"):
        package = binaries = stage
        resources = stage / "resources"
        extension = ".exe"
    elif target == "aarch64-apple-darwin":
        package = stage / "Suki Clash.app"
        binaries = package / "Contents/MacOS"
        resources = package / "Contents/Resources"
        extension = ""
    elif target == "x86_64-unknown-linux-gnu":
        package = stage / "SukiClash.AppDir"
        binaries = package / "usr/bin"
        resources = binaries / "resources"
        extension = ""
    else:
        sys.exit(f"Unsupported target {target}")
    binaries.mkdir(parents=True)
    shutil.copy2(ROOT / "target" / target / "release" / f"suki-clash{extension}", binaries)
    shutil.copy2(ROOT / "platform-service/target" / target / "release" / f"suki-clash-service{extension}", binaries)
    shutil.copytree(runtime, resources)
    if target == "aarch64-apple-darwin":
        macos(package, version)
    elif target == "x86_64-unknown-linux-gnu":
        linux(package, version)


if __name__ == "__main__":
    main()
