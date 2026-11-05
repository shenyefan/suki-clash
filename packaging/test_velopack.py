import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import velopack


class ServiceBundleTests(unittest.TestCase):
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
                with patch("velopack.run"), patch("velopack.urllib.request.urlopen") as download, patch.dict(velopack.LINUXDEPLOY, {"x64": ("x86_64", hashlib.sha256(b"").hexdigest())}):
                    download.return_value.__enter__.return_value.read.return_value = b""
                    package = velopack.prepare(args, "0.2.3", os_name, arch)
                binaries = package / "Contents/MacOS" if os_name == "osx" else package / "usr/bin" if os_name == "linux" else package
                staged = {path.name for path in binaries.iterdir() if path.is_file()}
                self.assertEqual(staged, {"suki-clash" + extension, "suki-clash-service" + extension})
                self.assertEqual((binaries / helper.name).read_bytes(), b"integrated service")
                helper.unlink()
                args.work = root / "missing-helper"
                args.work.mkdir()
                with patch("velopack.run"), self.assertRaisesRegex(ValueError, "required binary missing"):
                    velopack.prepare(args, "0.2.3", os_name, arch)


if __name__ == "__main__":
    unittest.main()
