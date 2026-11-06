#!/usr/bin/env python3
"""Supply GPUI's Windows release shaders when its build script runs on macOS."""
import os
from pathlib import Path
import subprocess
import sys

compiler, *arguments = sys.argv[1:]
if (os.environ.get("CARGO_PKG_NAME") == "gpui-pre-windows"
        and "--crate-name" in arguments
        and arguments[arguments.index("--crate-name") + 1] == "gpui_windows"):
    source = Path(os.environ["CARGO_MANIFEST_DIR"]) / "src"
    output = Path(os.environ["OUT_DIR"])
    subprocess.run([
        "docker", "run", "--rm", "--platform", "linux/amd64", "--network", "none",
        "--mount", f"type=bind,source={source},target=/shaders,readonly",
        "--mount", f"type=bind,source={output},target=/out",
        os.environ["SUKI_SHADER_IMAGE"], "python3", "/opt/compile-shaders.py",
    ], check=True)
os.execv(compiler, [compiler, *arguments])
