#!/usr/bin/env python3
"""Snapshot upstream asset identities once per workflow, then build offline bundles."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import urllib.request
import zipfile

from velopack import TARGETS

LIMIT = 256 * 1024 * 1024
CORES = {
    "x86_64-pc-windows-msvc": "mihomo-windows-amd64-compatible",
    "aarch64-pc-windows-msvc": "mihomo-windows-arm64",
    "aarch64-apple-darwin": "mihomo-darwin-arm64-go122",
    "x86_64-unknown-linux-gnu": "mihomo-linux-amd64-v1",
}
GEO = {
    "Country.mmdb": "geoip.metadb",
    "ASN.mmdb": "GeoLite2-ASN.mmdb",
    "geoip.dat": "geoip-lite.dat",
    "geosite.dat": "geosite.dat",
}


def snapshot():
    releases = {}
    for repo in ("mihomo", "meta-rules-dat"):
        request = urllib.request.Request(f"https://api.github.com/repos/MetaCubeX/{repo}/releases/latest")
        # The workflow token is sent only to the official metadata API, never asset redirects.
        if token := os.environ.get("GH_TOKEN"):
            request.add_header("Authorization", f"Bearer {token}")
        with urllib.request.urlopen(request, timeout=60) as response:
            releases[repo] = json.load(response)
    version = releases["mihomo"]["tag_name"]
    result = {"mihomo": version, "targets": {}}
    for target, stem in CORES.items():
        extension = "zip" if "windows" in target else "gz"
        assets = [("mihomo", f"{stem}-{version}.{extension}", "mihomo.exe" if extension == "zip" else "mihomo")]
        assets.extend(("meta-rules-dat", remote, name) for name, remote in GEO.items())
        selected = []
        for repo, remote, name in assets:
            asset = next(a for a in releases[repo]["assets"] if a["name"] == remote)
            digest = asset["digest"]
            if not digest or not digest.startswith("sha256:") or len(digest) != 71:
                raise ValueError(f"upstream SHA-256 missing for {remote}")
            selected.append({"name": name, "url": asset["browser_download_url"],
                             "size": asset["size"], "sha256": digest[7:]})
        result["targets"][target] = selected
    return result


def prepare(lock, target, output):
    output.mkdir(parents=True, exist_ok=False)
    manifest = {"version": 1, "target": "", "assets": []}
    os_name, arch = TARGETS[target]
    manifest["target"] = f"{dict(win='windows', osx='macos', linux='linux')[os_name]}-{dict(x64='x86_64', arm64='aarch64')[arch]}"
    for asset in lock["targets"][target]:
        with urllib.request.urlopen(asset["url"], timeout=180) as response:
            data = response.read(asset["size"] + 1)
        if len(data) != asset["size"] or hashlib.sha256(data).hexdigest() != asset["sha256"]:
            raise ValueError(f"upstream size/SHA-256 mismatch: {asset['name']}")
        name = asset["name"]
        if name == "mihomo.exe":
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                # Upstream names the executable after its platform/build, not mihomo.exe.
                entries = [entry for entry in archive.infolist()
                           if PurePosixPath(entry.filename).name.startswith("mihomo")
                           and PurePosixPath(entry.filename).suffix == ".exe"
                           and not entry.is_dir()]
                if len(entries) != 1 or not 0 < entries[0].file_size <= LIMIT:
                    raise ValueError("invalid Mihomo zip entry")
                data = archive.read(entries[0])
        elif name == "mihomo":
            with gzip.GzipFile(fileobj=io.BytesIO(data)) as archive:
                data = archive.read(LIMIT + 1)
        if not 0 < len(data) <= LIMIT:
            raise ValueError(f"empty or oversized runtime asset: {name}")
        path = output / name
        path.write_bytes(data)
        path.chmod(0o755 if name.startswith("mihomo") else 0o644)
        manifest["assets"].append({"name": name, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resolve", type=Path, help="write a shared upstream SHA-256 snapshot")
    parser.add_argument("--lock", type=Path)
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.resolve:
        args.resolve.write_text(json.dumps(snapshot(), indent=2) + "\n")
    elif args.lock and args.target and args.output:
        prepare(json.loads(args.lock.read_text()), args.target, args.output)
    else:
        parser.error("use --resolve, or --lock with --target and --output")


if __name__ == "__main__":
    main()
