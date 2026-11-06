"""Generate the GPUI release bytecode with the native Microsoft compiler."""
from pathlib import Path
import re
import subprocess

# Keep the server alive between compiler processes for this container's batch.
subprocess.run(["wineserver", "--persistent"], check=True)
output = Path("/out")
bindings = []
for name in ("shaders.hlsl", "color_text_raster.hlsl"):
    source = Path("/shaders") / name
    entries = re.findall(r"^\w+\s+(\w+_(?:vertex|fragment))\s*\(", source.read_text(), re.MULTILINE)
    if not entries:
        raise ValueError(f"no shader entry points in {name}")
    for entry in entries:
        module, kind = entry.rsplit("_", 1)
        target = "vs_4_1" if kind == "vertex" else "ps_4_1"
        binary = output / f"{entry}.dxbc"
        subprocess.run(["/opt/shader-compiler.exe", f"Z:\\shaders\\{name}", entry, target, str(binary)], check=True)
        data = binary.read_bytes()
        if not data.startswith(b"DXBC"):
            raise ValueError(f"invalid compiled shader: {entry}")
        constant = f"{module.upper()}_{'VERTEX' if kind == 'vertex' else 'FRAGMENT'}_BYTES"
        bindings.append(f"const {constant}: &[u8] = &{list(data)};\n")
        print(f"Compiled {entry}: {len(data)} bytes", flush=True)
temporary = output / "shaders_bytes.rs.tmp"
temporary.write_text("".join(bindings))
temporary.replace(output / "shaders_bytes.rs")
