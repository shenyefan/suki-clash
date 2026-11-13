#!/usr/bin/env python3
"""Create native first-install packages, update payloads, and a per-platform manifest.

Usage: pack.py VERSION TARGET CHANNEL STAGE_DIR OUTPUT_DIR
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def zip_tree(source, destination):
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(source))


def asset(path):
    with path.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"file_name": path.name, "size": path.stat().st_size, "sha256": digest}


def main():
    version, target, channel = sys.argv[1:4]
    stage, output = Path(sys.argv[4]).resolve(), Path(sys.argv[5]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    prefix = f"SukiClash-{version}-{channel}"
    installer = None
    if channel.startswith("win-"):
        zip_tree(stage, output / f"{prefix}-Portable.zip")
        update = output / f"{prefix}-Update.zip"
        with zipfile.ZipFile(update, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(stage / "suki-clash.exe", "suki-clash.exe")
        service = stage / "suki-clash-service.exe"
        compiler = os.environ.get("ISCC_PATH") or shutil.which("ISCC.exe")
        if not compiler:
            sys.exit("Set ISCC_PATH to the Inno Setup compiler")
        subprocess.run([compiler, f"/DAppVersion={version}", f"/DChannel={channel}",
                        f"/DAppArch={'arm64' if channel == 'win-arm64' else 'x64compatible'}",
                        f"/DStageDir={stage}", f"/DOutputDir={output}",
                        str(ROOT / "packaging/windows/setup.iss")], check=True)
        installer = output / f"{prefix}-Setup.exe"
    elif channel.startswith("osx-"):
        service = stage / "Suki Clash.app/Contents/MacOS/suki-clash-service"
        update = output / f"{prefix}-Update.tar.gz"
        with tarfile.open(update, "w:gz") as archive:
            archive.add(stage / "Suki Clash.app", arcname="Suki Clash.app")
        with tempfile.TemporaryDirectory() as temporary:
            image = Path(temporary)
            shutil.copytree(stage / "Suki Clash.app", image / "Suki Clash.app", symlinks=True)
            (image / "Applications").symlink_to("/Applications")
            subprocess.run(["hdiutil", "create", "-volname", "Suki Clash", "-srcfolder", str(image),
                            "-ov", "-format", "UDZO", str(output / f"{prefix}.dmg")], check=True)
    elif channel == "linux-x64":
        service = stage / "SukiClash.AppDir/usr/bin/suki-clash-service"
        update = output / f"{prefix}.AppImage"
        shutil.copy2(stage / "SukiClash.AppImage", update)
        update.chmod(0o755)
    else:
        sys.exit(f"Unsupported channel {channel}")
    service_asset = output / f"{prefix}-Service{service.suffix}"
    shutil.copy2(service, service_asset)
    service_version = tomllib.loads((ROOT / "platform-service/Cargo.toml").read_text())["package"]["version"]
    manifest = {"version": version, "channel": channel, "portable": asset(update),
                "installer": asset(installer) if installer else None,
                "service_version": service_version, "service": asset(service_asset)}
    (output / f"releases.{channel}.json").write_text(json.dumps(manifest, indent=2) + "\n")
    build = {"version": version, "target": target, "channel": channel, "offline_runtime": True}
    (output / f"build.{channel}.json").write_text(json.dumps(build, indent=2) + "\n")


if __name__ == "__main__":
    main()
