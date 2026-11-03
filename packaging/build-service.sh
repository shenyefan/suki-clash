#!/bin/sh
# Build only: never installs a service or changes host networking.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
output_dir=${1:?Usage: build-service.sh OUTPUT_DIRECTORY [TARGET]}
target_arg=${2:-}
if [ -n "$target_arg" ]; then
    cargo build --target-dir "$repo_dir/platform-service/target" --locked --release --manifest-path "$repo_dir/platform-service/Cargo.toml" --features standalone,client --bin suki-clash-service --bin suki-clash-service-install --bin suki-clash-service-uninstall --target "$target_arg"
    binary_dir="$repo_dir/platform-service/target/$target_arg/release"
else
    cargo build --target-dir "$repo_dir/platform-service/target" --locked --release --manifest-path "$repo_dir/platform-service/Cargo.toml" --features standalone,client --bin suki-clash-service --bin suki-clash-service-install --bin suki-clash-service-uninstall
    binary_dir="$repo_dir/platform-service/target/release"
fi
mkdir -p "$output_dir"
for name in suki-clash-service suki-clash-service-install suki-clash-service-uninstall; do
    if [ -f "$binary_dir/$name.exe" ]; then
        cp "$binary_dir/$name.exe" "$output_dir/$name.exe"
    else
        cp "$binary_dir/$name" "$output_dir/$name"
    fi
done
