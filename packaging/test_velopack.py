import hashlib
import json
from pathlib import Path
import plistlib
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import velopack


class ServiceBundleTests(unittest.TestCase):
    def test_windows_packages_on_macos_use_velopacks_explicit_cross_platform_directive(self):
        with tempfile.TemporaryDirectory() as temporary:
            work = Path(temporary)
            stage = work / "stage"
            stage.mkdir()
            commands = []

            def pack(command):
                commands.append(list(map(str, command)))
                (work / "Releases").mkdir()

            arguments = ["velopack.py", "--target", "x86_64-pc-windows-msvc",
                         "--app-dir", temporary, "--service-dir", temporary,
                         "--runtime-dir", temporary, "--work", temporary, "--vpk", "vpk"]
            with patch("sys.argv", arguments), patch("velopack.platform.system", return_value="Darwin"), \
                 patch("velopack.prepare", return_value=stage), patch("velopack.run", side_effect=pack), \
                 patch("velopack.subprocess.check_output", return_value=f"Velopack CLI {velopack.VPK_VERSION}"):
                velopack.main()
            self.assertEqual(commands[0][:5], ["vpk", "[win]", "--skip-updates", "--yes", "pack"])
            self.assertIn("--icon", commands[0])
            self.assertEqual(commands[0][commands[0].index("--packAuthors") + 1], "Moenya")
            self.assertTrue((work / "Releases/build.win-x64.json").is_file())

    def test_linux_staging_requires_linux_native_tools(self):
        arguments = ["velopack.py", "--target", "x86_64-unknown-linux-gnu",
                     "--app-dir", ".", "--service-dir", ".", "--runtime-dir", ".", "--work", "."]
        with patch("sys.argv", arguments), patch("velopack.platform.system", return_value="Darwin"), \
             self.assertRaisesRegex(ValueError, "packaging/local.py"):
            velopack.main()

    def test_stage_needs_only_app_and_integrated_service_on_each_platform(self):
        for os_name, arch, runtime_target in [
            ("win", "x64", "windows-x86_64"),
            ("osx", "arm64", "macos-aarch64"),
            ("linux", "x64", "linux-x86_64"),
        ]:
            with self.subTest(platform=os_name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                app, service, runtime, work = [root / name for name in ("app", "service", "runtime", "work")]
                for directory in (app, service, runtime, work):
                    directory.mkdir()
                extension = ".exe" if os_name == "win" else ""
                (app / ("suki-clash" + extension)).write_bytes(b"application")
                helper = service / ("suki-clash-service" + extension)
                helper.write_bytes(b"integrated service")
                # Stale build products must not become release payloads.
                (service / "unrelated-tool").write_bytes(b"not distributable")
                names = ["mihomo" + extension, "Country.mmdb", "ASN.mmdb", "geoip.dat", "geosite.dat"]
                assets = []
                for name in names:
                    body = name.encode()
                    (runtime / name).write_bytes(body)
                    assets.append({"name": name, "size": len(body), "sha256": hashlib.sha256(body).hexdigest()})
                (runtime / "manifest.json").write_text(json.dumps({"version": 1, "target": runtime_target, "assets": assets}))
                args = SimpleNamespace(app_dir=app, service_dir=service, runtime_dir=runtime, work=work)
                # Icon conversion and linuxdeploy are external tools; staging and
                # the required binary/runtime contract are exercised here.
                def stage_tool(command, **kwargs):
                    if command[0] == "iconutil":
                        Path(command[-1]).write_bytes(b"icns fixture")

                with patch("velopack.run", side_effect=stage_tool), patch("velopack.urllib.request.urlopen") as download, patch.dict(velopack.LINUXDEPLOY, {"x64": ("x86_64", hashlib.sha256(b"").hexdigest())}):
                    download.return_value.__enter__.return_value.read.return_value = b""
                    package = velopack.prepare(args, "0.2.3", os_name, arch)
                binaries = package / "Contents/MacOS" if os_name == "osx" else package / "usr/bin" if os_name == "linux" else package
                staged = {path.name for path in binaries.iterdir() if path.is_file()}
                self.assertEqual(staged, {"suki-clash" + extension, "suki-clash-service" + extension})
                self.assertEqual((binaries / helper.name).read_bytes(), b"integrated service")
                resources = package / "Contents/Resources" if os_name == "osx" else binaries / "resources"
                # Shell icons remain platform assets; no unused PNG or license
                # directory should return to the runtime payload.
                expected_resources = {*names, "manifest.json"}
                if os_name == "osx":
                    expected_resources.add("suki-clash.icns")
                self.assertEqual({path.name for path in resources.iterdir()}, expected_resources)
                for name in names:
                    self.assertEqual((resources / name).read_bytes(), name.encode())
                if os_name == "osx":
                    info = plistlib.loads((package / "Contents/Info.plist").read_bytes())
                    self.assertEqual(info["NSHumanReadableCopyright"], "Copyright © Moenya")
                    self.assertEqual(info["CFBundleIconFile"], "suki-clash.icns")
                elif os_name == "linux":
                    self.assertEqual((package / "suki-clash.png").read_bytes(), (velopack.ROOT / "assets/logo.png").read_bytes())
                helper.unlink()
                args.work = root / "missing-helper"
                args.work.mkdir()
                with patch("velopack.run"), self.assertRaisesRegex(ValueError, "required binary missing"):
                    velopack.prepare(args, "0.2.3", os_name, arch)


if __name__ == "__main__":
    unittest.main()
