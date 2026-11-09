#!/usr/bin/env bash
# Pack the staged app with the official Velopack CLI and record build metadata.
# Usage: pack.sh VERSION TARGET CHANNEL STAGE_DIR OUTPUT_DIR
set -euo pipefail

version=$1 target=$2 channel=$3 stage=$4 output=$5
root=$(cd "$(dirname "$0")/.." && pwd)
options=()
case $channel in
  win-*)
    # Native Windows tools need D:/ paths.
    root=$(cygpath -m "$root")
    pack_dir=$stage
    main_exe=suki-clash.exe
    options=(--icon "$root/assets/logo.ico")
    ;;
  osx-*)
    pack_dir="$stage/Suki Clash.app"
    main_exe=suki-clash
    ;;
  linux-*)
    pack_dir="$stage/SukiClash.AppDir"
    main_exe=suki-clash
    ;;
esac

dotnet vpk --skip-updates --yes pack --packId SukiClash --packTitle 'Suki Clash' --packAuthors Moenya \
  --packVersion "$version" --packDir "$pack_dir" --mainExe "$main_exe" --runtime "$channel" --channel "$channel" \
  --delta None --outputDir "$output" ${options[@]+"${options[@]}"}
jq -n --arg version "$version" --arg target "$target" --arg channel "$channel" \
  --arg velopack "$(jq -r .tools.vpk.version "$root/.config/dotnet-tools.json")" \
  '{$version, $target, $channel, $velopack, offline_runtime: true}' > "$output/build.$channel.json"
