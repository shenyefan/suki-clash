#!/usr/bin/env python3
"""Prepare optional CI signing; secrets never enter release artifacts."""
import argparse
import base64
import os
from pathlib import Path
import platform
import subprocess
import uuid


def run(*args):
    try:
        subprocess.run(args, check=True, stdout=subprocess.DEVNULL)
    except subprocess.CalledProcessError:
        raise RuntimeError(f"{args[0]} signing operation failed; secret arguments omitted") from None


def export(name, value):
    if "\n" in value or "\r" in value:
        raise ValueError("signing environment values must be single-line")
    with open(os.environ["GITHUB_ENV"], "a") as output:
        output.write(name + "=" + value + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cleanup", action="store_true")
    args = parser.parse_args()
    root = Path(os.environ["RUNNER_TEMP"]) / "suki-signing"
    if args.cleanup:
        if platform.system() == "Windows" and os.environ.get("SUKI_WINDOWS_CERT_THUMBPRINT"):
            run("pwsh", "-NoProfile", "-Command", "Remove-Item -Path ('Cert:\\CurrentUser\\My\\'+$env:SUKI_WINDOWS_CERT_THUMBPRINT) -DeleteKey")
        keychain = root / "build.keychain-db"
        if keychain.exists():
            run("security", "delete-keychain", str(keychain))
        import shutil
        shutil.rmtree(root, ignore_errors=True)
        return
    if platform.system() == "Windows":
        p12 = os.environ.get("SUKI_WINDOWS_CERTIFICATE_P12", "")
        password = os.environ.get("SUKI_WINDOWS_CERTIFICATE_PASSWORD", "")
        if not p12:
            if password:
                raise ValueError("Windows signing password was supplied without a certificate")
            print("Windows package is unsigned; draft acceptance only")
            return
        root.mkdir()
        path = root / "certificate.pfx"
        path.write_bytes(base64.b64decode(p12, validate=True))
        environment = os.environ.copy()
        environment["SUKI_PFX_PATH"] = str(path)
        result = subprocess.check_output([
            "pwsh", "-NoProfile", "-Command",
            "$password=ConvertTo-SecureString $env:SUKI_WINDOWS_CERTIFICATE_PASSWORD -AsPlainText -Force; "
            "$certificate=Import-PfxCertificate -FilePath $env:SUKI_PFX_PATH -CertStoreLocation Cert:\\CurrentUser\\My -Password $password; "
            "$certificate.Thumbprint"], env=environment, text=True).strip()
        if not result or "\n" in result:
            raise ValueError("Windows certificate import must return one signing identity")
        export("SUKI_WINDOWS_CERT_THUMBPRINT", result)
        path.unlink()
    elif platform.system() == "Darwin":
        fields = ("SUKI_MACOS_CERTIFICATE_P12", "SUKI_MACOS_CERTIFICATE_PASSWORD",
                  "SUKI_MACOS_APPLICATION_IDENTITY", "SUKI_MACOS_INSTALLER_IDENTITY",
                  "SUKI_APPLE_API_KEY_P8", "SUKI_APPLE_API_KEY_ID", "SUKI_APPLE_API_ISSUER")
        values = [os.environ.get(field, "") for field in fields]
        if not any(values):
            print("macOS package is unsigned and unnotarized; draft acceptance only")
            return
        if not all(values):
            raise ValueError("macOS signing requires the complete certificate and notary secret set")
        root.mkdir()
        keychain = root / "build.keychain-db"
        p12 = root / "certificate.p12"
        key = root / "AuthKey.p8"
        p12.write_bytes(base64.b64decode(values[0], validate=True))
        key.write_text(values[4])
        password = uuid.uuid4().hex
        run("security", "create-keychain", "-p", password, str(keychain))
        run("security", "set-keychain-settings", "-lut", "21600", str(keychain))
        run("security", "unlock-keychain", "-p", password, str(keychain))
        run("security", "import", str(p12), "-k", str(keychain), "-P", values[1], "-T", "/usr/bin/codesign", "-T", "/usr/bin/productsign")
        run("security", "set-key-partition-list", "-S", "apple-tool:,apple:", "-s", "-k", password, str(keychain))
        run("xcrun", "notarytool", "store-credentials", "suki-ci", "--key", str(key), "--key-id", values[5], "--issuer", values[6], "--keychain", str(keychain))
        export("SUKI_MACOS_APPLICATION_IDENTITY", values[2])
        export("SUKI_MACOS_INSTALLER_IDENTITY", values[3])
        export("SUKI_MACOS_NOTARY_PROFILE", "suki-ci")
        export("SUKI_MACOS_KEYCHAIN", str(keychain))
        p12.unlink()
        key.unlink()


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError):
        raise SystemExit("CI signing failed; check certificate identities and signing credentials") from None
