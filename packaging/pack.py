#!/usr/bin/env python3
"""Pack the staged app with the official Velopack CLI and record build metadata.

Usage: pack.py VERSION TARGET CHANNEL STAGE_DIR OUTPUT_DIR
"""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    version, target, channel = sys.argv[1:4]
    stage, output = Path(sys.argv[4]), Path(sys.argv[5])
    options = []
    if channel.startswith("win-"):
        pack_dir, main_exe = stage, "suki-clash.exe"
        options = ["--icon", str(ROOT / "assets/logo.ico"),
                   "--msi", "--instLocation", "PerMachine"]
    elif channel.startswith("osx-"):
        pack_dir, main_exe = stage / "Suki Clash.app", "suki-clash"
    else:
        pack_dir, main_exe = stage / "SukiClash.AppDir", "suki-clash"
    subprocess.run(["dotnet", "vpk", "--skip-updates", "--yes", "pack", "--packId", "SukiClash",
                    "--packTitle", "Suki Clash", "--packAuthors", "Moenya", "--packVersion", version,
                    "--packDir", str(pack_dir), "--mainExe", main_exe, "--runtime", channel,
                    "--channel", channel, "--delta", "None", "--outputDir", str(output), *options], check=True)
    velopack = json.loads((ROOT / ".config/dotnet-tools.json").read_text())["tools"]["vpk"]["version"]
    build = {"version": version, "target": target, "channel": channel, "velopack": velopack, "offline_runtime": True}
    (output / f"build.{channel}.json").write_text(json.dumps(build, indent=2) + "\n")


if __name__ == "__main__":
    main()
