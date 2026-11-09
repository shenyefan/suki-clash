#!/usr/bin/env bash
# Download Mihomo and Geo data once, verify the upstream SHA-256 digests and
# build every target's offline runtime bundle under OUTPUT/<rust-target>/.
# Usage: runtime.sh OUTPUT   (Linux runner with gh, jq, unzip; GH_TOKEN set)
set -euo pipefail

mkdir -p "$1"
output=$(cd "$1" && pwd)
downloads=$(mktemp -d)
trap 'rm -rf "$downloads"' EXIT
cd "$downloads"

retry() {
  for attempt in 1 2 3 4; do
    "$@" && return 0
    [[ $attempt == 4 ]] && return 1
    echo "Retry $attempt/3: $*" >&2
    sleep $((attempt * 2))
  done
}

retry gh api repos/MetaCubeX/mihomo/releases/latest > mihomo.json
retry gh api repos/MetaCubeX/meta-rules-dat/releases/latest > geo.json
core_tag=$(jq -r .tag_name mihomo.json)
geo_tag=$(jq -r .tag_name geo.json)

# Rust target, application runtime target, upstream core archive.
targets=(
  "x86_64-pc-windows-msvc windows-x86_64 mihomo-windows-amd64-compatible-$core_tag.zip"
  "aarch64-pc-windows-msvc windows-aarch64 mihomo-windows-arm64-$core_tag.zip"
  "aarch64-apple-darwin macos-aarch64 mihomo-darwin-arm64-go122-$core_tag.gz"
  "x86_64-unknown-linux-gnu linux-x86_64 mihomo-linux-amd64-v1-$core_tag.gz"
)
# Application file name, upstream file name.
geo=(
  "Country.mmdb geoip.metadb"
  "ASN.mmdb GeoLite2-ASN.mmdb"
  "geoip.dat geoip-lite.dat"
  "geosite.dat geosite.dat"
)
cores=(); for entry in "${targets[@]}"; do cores+=("${entry##* }"); done
geo_files=(); for entry in "${geo[@]}"; do geo_files+=("${entry##* }"); done

checksums() {
  jq -r '.assets[] | select(.name as $name | $ARGS.positional | index($name))
    | if ((.digest // "") | test("^sha256:[0-9a-f]{64}$")) then "\(.digest[7:])  \(.name)"
      else error("missing SHA-256 for \(.name)") end' "$1" --args "${@:2}"
}
checksums mihomo.json "${cores[@]}" > SHA256SUMS
checksums geo.json "${geo_files[@]}" >> SHA256SUMS
[[ $(wc -l < SHA256SUMS) == 8 ]] || { echo "::error::Upstream release is missing runtime assets"; exit 1; }
for name in "${cores[@]}"; do retry gh release download "$core_tag" -R MetaCubeX/mihomo -p "$name" --clobber; done
for name in "${geo_files[@]}"; do retry gh release download "$geo_tag" -R MetaCubeX/meta-rules-dat -p "$name" --clobber; done
sha256sum -c SHA256SUMS
cp SHA256SUMS "$output/"

for entry in "${targets[@]}"; do
  read -r target runtime_target archive <<< "$entry"
  bundle="$output/$target"
  mkdir "$bundle"
  if [[ $archive == *.zip ]]; then
    core=mihomo.exe
    # Upstream names the executable after its platform/build, not mihomo.exe.
    mapfile -t entries < <(unzip -Z1 "$archive" | grep -E '(^|/)mihomo[^/]*\.exe$' || true)
    [[ ${#entries[@]} == 1 ]] || { echo "::error::$archive must contain exactly one mihomo*.exe"; exit 1; }
    unzip -p "$archive" "${entries[0]}" > "$bundle/$core"
  else
    core=mihomo
    gunzip -c "$archive" > "$bundle/$core"
  fi
  [[ -s $bundle/$core ]] || { echo "::error::$archive contains an empty core"; exit 1; }
  for pair in "${geo[@]}"; do
    read -r name remote <<< "$pair"
    cp "$remote" "$bundle/$name"
  done
  for name in "$core" Country.mmdb ASN.mmdb geoip.dat geosite.dat; do
    jq -n --arg name "$name" --argjson size "$(stat -c %s "$bundle/$name")" \
      --arg sha256 "$(sha256sum "$bundle/$name" | cut -d' ' -f1)" '{$name, $size, $sha256}'
  done | jq -s --arg target "$runtime_target" '{version: 1, target: $target, assets: .}' > "$bundle/manifest.json"
done
