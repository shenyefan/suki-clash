#!/usr/bin/env bash
# Collect the user downloads and update feeds from every build, write the
# release description and create or update one draft release.
# Usage: release.sh VERSION TAG PACKAGES_DIR   (GH_TOKEN and GH_REPO set)
set -euo pipefail

version=$1 tag=$2 packages=$3
mkdir Releases
base="https://github.com/$GH_REPO/releases/download/$tag"
cat > release-notes.md <<'NOTES'
## 下载

按系统和架构选择文件。Windows 提供安装版和免安装压缩包；macOS 提供安装包和应用压缩包；Linux 提供 AppImage。

| 平台 | 文件 |
| --- | --- |
NOTES
for channel in win-x64 win-arm64 osx-arm64 linux-x64; do
  source="$packages/packages-$channel"
  case $channel in
    win-x64) label="Windows x64"; files=(Setup.exe Portable.zip) ;;
    win-arm64) label="Windows ARM64"; files=(Setup.exe Portable.zip) ;;
    osx-arm64) label="macOS Apple Silicon"; files=(Setup.pkg Portable.zip) ;;
    linux-x64) label="Linux x64"; files=(AppImage) ;;
  esac
  for suffix in "${files[@]}"; do
    name="SukiClash-$channel-$suffix"
    if [[ $suffix == AppImage ]]; then name="SukiClash-$channel.AppImage"; fi
    test -s "$source/$name"
    cp "$source/$name" Releases/
    printf '| %s | [%s](%s/%s) |\n' "$label" "$name" "$base" "$name" >> release-notes.md
  done
  for name in "releases.$channel.json" "SukiClash-$version-$channel-full.nupkg"; do
    test -s "$source/$name"
    cp "$source/$name" Releases/
  done
done
cat >> release-notes.md <<'NOTES'

## 使用说明

- 已内置 Mihomo 内核和 Geo 数据。
- 安装包和压缩包未签名：Windows 可能出现 SmartScreen 提示；macOS 首次打开需在“系统设置 → 隐私与安全性”中允许。
- 免安装版的用户配置仍保存在系统应用目录；TUN 需要相应服务与权限。
- Linux 下载后需为 AppImage 添加执行权限。
- 通过 Velopack 安装的版本可在应用内检查并安装更新；其他方式下载安装包或压缩包升级。
NOTES

printf '\n' >> release-notes.md
gh api "repos/$GH_REPO/releases/generate-notes" -f tag_name="$tag" --jq .body \
  | sed 's/^\*\*Full Changelog\*\*: */\n## 完整变更\n\n/' >> release-notes.md
if gh release view "$tag" --json isDraft,assets > release.json; then
  jq -e '.isDraft == true' release.json > /dev/null || { echo "::error::Refusing to change a published release"; exit 1; }
  gh release edit "$tag" --title "Suki Clash $tag" --notes-file release-notes.md
  while IFS= read -r asset; do
    if [[ ! -f Releases/$asset ]]; then gh release delete-asset "$tag" "$asset" --yes; fi
  done < <(jq -r '.assets[].name' release.json)
else
  gh release create "$tag" --verify-tag --draft --title "Suki Clash $tag" --notes-file release-notes.md
fi
gh release upload "$tag" Releases/* --clobber
