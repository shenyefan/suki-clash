#!/usr/bin/env python3
"""Check, build and package release targets locally without publishing anything."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile

import runtime
import velopack

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "win-x64": "x86_64-pc-windows-msvc",
    "win-arm64": "aarch64-pc-windows-msvc",
    "linux-x64": "x86_64-unknown-linux-gnu",
}


def run(command, env=None, **kwargs):
    print("+ " + shlex.join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), cwd=ROOT, env=env, check=True, **kwargs)


def rust_environment(toolchain, jobs):
    rustup = shutil.which("rustup")
    if not rustup:
        raise ValueError("rustup is required; Homebrew Rust does not own rustup target libraries")
    rustc = Path(subprocess.check_output(
        [rustup, "which", "--toolchain", toolchain, "rustc"], text=True).strip())
    env = os.environ.copy()
    paths = [str(rustc.parent), str(Path(rustup).parent)]
    if platform.system() == "Darwin":
        llvm = subprocess.check_output(["brew", "--prefix", "llvm"], text=True).strip()
        paths.append(str(Path(llvm) / "bin"))
        env["LIBCLANG_PATH"] = str(Path(llvm) / "lib")
        # python.org's macOS Python may not have its optional certifi setup.
        env.setdefault("SSL_CERT_FILE", "/etc/ssl/cert.pem")
    env["PATH"] = os.pathsep.join([*paths, env.get("PATH", "")])
    env["RUSTC"] = str(rustc)
    env["RUSTDOC"] = str(rustc.with_name("rustdoc"))
    env["CARGO_BUILD_JOBS"] = str(jobs)
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_PROFILE_RELEASE_DEBUG"] = "0"
    env["CARGO_PROFILE_DEV_DEBUG"] = "0"
    version = subprocess.check_output([str(rustc), "--version"], text=True).split()[1]
    print(f"Rust {version}: {rustc}", flush=True)
    return env, version


def linux_image(rust_version, rebuild=False):
    image = f"suki-clash-build-linux:{rust_version}"
    run(["docker", "info"], stdout=subprocess.DEVNULL)
    if not rebuild and subprocess.run(
            ["docker", "image", "inspect", image], stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL).returncode == 0:
        return image
    run(["docker", "build", "--platform", "linux/amd64", "-t", image,
         "--build-arg", f"RUST_VERSION={rust_version}",
         "--build-arg", f"VELOPACK_VERSION={velopack.VPK_VERSION}",
         "-f", ROOT / "packaging/linux/Dockerfile", ROOT / "packaging/linux"])
    return image


def linux_container(args, output, rust_version):
    image = linux_image(rust_version, args.rebuild_tools)
    output.mkdir(parents=True, exist_ok=True)
    command = ["docker", "run", "--rm", "--platform", "linux/amd64",
               "--mount", f"type=bind,source={ROOT},target=/workspace,readonly",
               "--mount", f"type=bind,source={output},target=/output",
               "--mount", "type=volume,source=suki-clash-linux-build,target=/build",
               "--mount", "type=volume,source=suki-clash-linux-registry,target=/opt/cargo/registry",
               "--mount", "type=volume,source=suki-clash-linux-git,target=/opt/cargo/git",
               "--env", "CARGO_TARGET_DIR=/build", image,
               "python3", "/workspace/packaging/local.py", args.action,
               "--target", "linux-x64", "--toolchain", rust_version,
               "--jobs", str(args.jobs), "--output", "/output"]
    run(command)


def package(target, output, binaries, env):
    lock = output / "runtime-lock.json"
    if not lock.exists():
        run([sys.executable, ROOT / "packaging/runtime.py", "--resolve", lock], env)
    bundle = output / "runtime"
    if not bundle.exists():
        with tempfile.TemporaryDirectory(dir=output) as temporary:
            prepared = Path(temporary) / "runtime"
            runtime.prepare(json.loads(lock.read_text()), target, prepared)
            prepared.rename(bundle)
    vpk = shutil.which("vpk", path=env["PATH"])
    if not vpk:
        tools = ROOT / "target/cross/tools" / velopack.VPK_VERSION
        vpk = str(tools / ("vpk.exe" if platform.system() == "Windows" else "vpk"))
        if not Path(vpk).exists():
            run(["dotnet", "tool", "install", "vpk", "--version", velopack.VPK_VERSION,
                 "--tool-path", tools], env)
    # The pinned CLI targets .NET 8; the host may have a later supported runtime.
    env["DOTNET_ROLL_FORWARD"] = "LatestMajor"
    work = output / "packages" / datetime.datetime.now().strftime("%Y%m%dT%H%M%S%f")
    run([sys.executable, ROOT / "packaging/velopack.py", "--target", target,
         "--app-dir", binaries, "--service-dir", binaries, "--runtime-dir", bundle,
         "--work", work, "--vpk", vpk], env)
    print(f"Packages: {work / 'Releases'}", flush=True)


def build(args, name, env, rust_version):
    target = TARGETS[name]
    output = (args.output or ROOT / "target/cross" / name).resolve()
    if name == "linux-x64" and platform.system() != "Linux":
        linux_container(args, output, rust_version)
        return
    output.mkdir(parents=True, exist_ok=True)
    env = env.copy()
    env.setdefault("CARGO_TARGET_DIR", str(output / "cargo"))
    run(["rustup", "target", "add", "--toolchain", args.toolchain, target], env)
    cargo = ["cargo"]
    if name.startswith("win-"):
        env["RUSTFLAGS"] = "-C target-feature=+crt-static"
        if platform.system() != "Windows":
            if not shutil.which("cargo-xwin", path=env["PATH"]):
                raise ValueError("install cargo-xwin first: cargo install --locked cargo-xwin")
            cargo.append("xwin")
            env["XWIN_CACHE_DIR"] = str(ROOT / "target/cross/xwin")
            linux_image(rust_version, args.rebuild_tools)
            shader_source = ROOT / "packaging/windows"
            digest = hashlib.sha256(b"".join((shader_source / name).read_bytes() for name in
                ("Dockerfile", "shader-compiler.c", "compile-shaders.py"))).hexdigest()[:12]
            shader_image = f"suki-clash-windows-shaders:{rust_version}-{digest}"
            if args.rebuild_tools or subprocess.run(
                    ["docker", "image", "inspect", shader_image], stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL).returncode != 0:
                run(["docker", "build", "--platform", "linux/amd64", "-t", shader_image,
                     "--build-arg", f"RUST_VERSION={rust_version}",
                     "-f", ROOT / "packaging/windows/Dockerfile", ROOT / "packaging/windows"])
            env["RUSTC_WRAPPER"] = str(ROOT / "packaging/windows/rustc-wrapper.py")
            env["SUKI_SHADER_IMAGE"] = shader_image
            if name == "win-arm64":
                env["SUKI_REAL_CLANG"] = shutil.which("clang", path=env["PATH"])
                env["PATH"] = str(ROOT / "packaging/windows/bin") + os.pathsep + env["PATH"]
    action = "check" if args.action == "check" else "build"
    common = [action, "--locked", "--release", "--target", target]
    run([*cargo, *common, "--bin", "suki-clash"], env)
    run([*cargo, *common, "--manifest-path", "platform-service/Cargo.toml",
         "--features", "standalone,client", "--bin", "suki-clash-service"], env)
    if args.action == "check":
        print(f"Checked: {name} (application + service)", flush=True)
        return
    source = Path(env["CARGO_TARGET_DIR"]) / target / "release"
    binaries = output / "bin"
    binaries.mkdir(exist_ok=True)
    extension = ".exe" if name.startswith("win-") else ""
    for binary in velopack.BINARIES:
        shutil.copy2(source / (binary + extension), binaries / (binary + extension))
    print(f"Binaries: {binaries}", flush=True)
    if args.action == "package":
        package(target, output, binaries, env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("check", "build", "package"))
    parser.add_argument("--target", choices=(*TARGETS, "all"), required=True)
    parser.add_argument("--toolchain", default="stable")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--rebuild-tools", action="store_true", help="rebuild the cached native build image")
    args = parser.parse_args()
    if args.jobs < 1 or (args.output and args.target == "all"):
        parser.error("jobs must be positive; --output requires one target")
    env, version = rust_environment(args.toolchain, args.jobs)
    if platform.system() == "Darwin":
        os.environ.setdefault("SSL_CERT_FILE", env["SSL_CERT_FILE"])
    for name in TARGETS if args.target == "all" else (args.target,):
        build(args, name, env, version)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
