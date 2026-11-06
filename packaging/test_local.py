import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import local


def executable(path, body):
    path.write_text("#!/usr/bin/env python3\n" + body)
    path.chmod(0o755)
    return path


@unittest.skipIf(os.name == "nt", "the cross-compilation wrapper runs on POSIX hosts")
class LocalBuildTests(unittest.TestCase):
    def test_rustup_compiler_and_cargo_take_priority_over_homebrew(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            brew, rustup, toolchain = [root / name for name in ("brew", "rustup", "toolchain")]
            for directory in (brew, rustup, toolchain):
                directory.mkdir()
            executable(brew / "rustc", "print('wrong compiler')\n")
            executable(brew / "cargo", "print('wrong cargo')\n")
            executable(toolchain / "rustc", "print('rustc 1.99.1 (test)')\n")
            executable(toolchain / "cargo", "print('rustup cargo')\n")
            executable(rustup / "rustup", f"print({str(toolchain / 'rustc')!r})\n")
            original_path = os.pathsep.join((str(brew), str(rustup), os.environ["PATH"]))
            with patch.dict(os.environ, {"PATH": original_path}), patch("local.platform.system", return_value="Linux"):
                env, version = local.rust_environment("stable", 2)
            self.assertEqual(version, "1.99.1")
            self.assertEqual(env["RUSTC"], str(toolchain / "rustc"))
            self.assertEqual(subprocess.check_output(["cargo"], env=env, text=True).strip(), "rustup cargo")
            self.assertTrue(subprocess.check_output(["rustc"], env=env, text=True).startswith("rustc 1.99.1"))

    def test_compiler_wrapper_preserves_arguments_and_failure_exit_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            observed = root / "arguments.json"
            compiler = executable(root / "compiler", f"import json,sys\nopen({str(observed)!r},'w').write(json.dumps(sys.argv[1:]))\nsys.exit(23)\n")
            arguments = ["--crate-name", "ordinary", "an argument with spaces", "中文"]
            env = {**os.environ, "CARGO_PKG_NAME": "ordinary"}
            result = subprocess.run([sys.executable, local.ROOT / "packaging/windows/rustc-wrapper.py", compiler, *arguments], env=env)
            self.assertEqual(result.returncode, 23)
            self.assertEqual(json.loads(observed.read_text()), arguments)

    def test_shader_failure_stops_compilation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            marker = root / "compiler-started"
            compiler = executable(root / "compiler", f"open({str(marker)!r},'w').write('started')\n")
            executable(root / "docker", "import sys\nsys.exit(42)\n")
            env = {**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"],
                   "CARGO_PKG_NAME": "gpui-pre-windows", "CARGO_MANIFEST_DIR": str(root),
                   "OUT_DIR": str(root), "SUKI_SHADER_IMAGE": "test-shaders"}
            result = subprocess.run([sys.executable, local.ROOT / "packaging/windows/rustc-wrapper.py",
                                     compiler, "--crate-name", "gpui_windows"], env=env, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(marker.exists())
            self.assertIn(b"42", result.stderr)

    def test_arm64_clang_adapter_preserves_paths_and_reports_compiler_failures(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            observed = root / "arguments.json"
            compiler = executable(root / "real-clang", f"import json,sys\nopen({str(observed)!r},'w').write(json.dumps(sys.argv[1:]))\nsys.exit(7)\n")
            arguments = ["-c", "source with spaces.c", "/imsvc", "SDK with spaces", "-DVALUE=/imsvc"]
            env = {**os.environ, "SUKI_REAL_CLANG": str(compiler)}
            result = subprocess.run([sys.executable, local.ROOT / "packaging/windows/bin/clang", *arguments], env=env)
            self.assertEqual(result.returncode, 7)
            self.assertEqual(json.loads(observed.read_text()), ["-c", "source with spaces.c", "-isystem", "SDK with spaces", "-DVALUE=/imsvc"])


if __name__ == "__main__":
    unittest.main()
