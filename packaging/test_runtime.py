import copy
import gzip
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import runtime
import velopack


class OfflineRuntimeTests(unittest.TestCase):
    def bundle(self, target, output, corrupt=False):
        core = b"verified kernel"
        if "windows" in target:
            buffer = io.BytesIO()
            with zipfile.ZipFile(buffer, "w") as archive:
                archive.writestr("mihomo.exe", core)
            archive_bytes = buffer.getvalue()
            name = "mihomo.exe"
        else:
            archive_bytes = gzip.compress(core)
            name = "mihomo"
        bodies = {name: archive_bytes, **{name: b"database" for name in runtime.GEO}}
        assets = [{"name": name, "url": name, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                  for name, data in bodies.items()]
        if corrupt:
            assets[0]["sha256"] = "0" * 64
        with patch("runtime.urllib.request.urlopen", side_effect=lambda url, **kwargs: io.BytesIO(bodies[url])):
            runtime.prepare({"targets": {target: assets}}, target, output)
        return core

    def test_all_six_targets_install_exact_five_assets_with_correct_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for target in velopack.TARGETS:
                bundle = root / target
                core = self.bundle(target, bundle)
                manifest = json.loads((bundle / "manifest.json").read_text())
                staged = root / (target + "-staged")
                velopack.copy_runtime(bundle, staged, manifest["target"])
                self.assertEqual(len(manifest["assets"]), 5)
                self.assertEqual({a["name"] for a in manifest["assets"]}, {"mihomo.exe" if "windows" in target else "mihomo", *runtime.GEO})
                self.assertEqual((staged / manifest["assets"][0]["name"]).read_bytes(), core)

    def test_corrupted_upstream_archive_never_produces_installable_manifest(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = Path(temporary) / "bundle"
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                self.bundle("x86_64-unknown-linux-gnu", bundle, corrupt=True)
            self.assertFalse((bundle / "manifest.json").exists())

    def test_packaging_rejects_missing_wrong_target_and_changed_assets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle"
            self.bundle("aarch64-apple-darwin", bundle)
            with self.assertRaisesRegex(ValueError, "target"):
                velopack.copy_runtime(bundle, root / "wrong-target", "linux-aarch64")
            (bundle / "geoip.dat").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                velopack.copy_runtime(bundle, root / "changed", "macos-aarch64")
            (bundle / "geoip.dat").unlink()
            with self.assertRaisesRegex(ValueError, "type or size"):
                velopack.copy_runtime(bundle, root / "missing", "macos-aarch64")

    def test_snapshot_requires_official_digests_and_maps_every_architecture(self):
        def release(names):
            return {"tag_name": "v1.2.3", "assets": [{"name": name, "digest": "sha256:" + "a" * 64, "size": 10, "browser_download_url": "https://github.com/" + name} for name in names]}
        cores = release([stem + "-v1.2.3." + ("zip" if "windows" in target else "gz") for target, stem in runtime.CORES.items()])
        geo = release(runtime.GEO.values())
        with patch("runtime.urllib.request.urlopen", side_effect=lambda request, **kwargs: io.BytesIO(json.dumps(cores if "/mihomo/" in request.full_url else geo).encode())):
            snapshot = runtime.snapshot()
            self.assertEqual(set(snapshot["targets"]), set(velopack.TARGETS))
            self.assertTrue(all(len(assets) == 5 for assets in snapshot["targets"].values()))
            broken = copy.deepcopy(cores)
            broken["assets"][0]["digest"] = None
            cores = broken
            with self.assertRaisesRegex(ValueError, "SHA-256 missing"):
                runtime.snapshot()


if __name__ == "__main__":
    unittest.main()
