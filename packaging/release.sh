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

按系统和架构选择文件。Windows 提供 Inno Setup 安装包和免安装压缩包；macOS 提供 DMG；Linux 提供 AppImage。

| 平台 | 文件 |
| --- | --- |
NOTES
for channel in win-x64 win-arm64 osx-arm64 linux-x64; do
  source="$packages/packages-$channel"
  prefix="SukiClash-$version-$channel"
  case $channel in
    win-x64) label="Windows x64"; files=("$prefix-Setup.exe" "$prefix-Portable.zip") ;;
    win-arm64) label="Windows ARM64"; files=("$prefix-Setup.exe" "$prefix-Portable.zip") ;;
    osx-arm64) label="macOS Apple Silicon"; files=("$prefix.dmg") ;;
    linux-x64) label="Linux x64"; files=("$prefix.AppImage") ;;
  esac
  for name in "${files[@]}"; do
    test -s "$source/$name"
    printf '| %s | [%s](%s/%s) |\n' "$label" "$name" "$base" "$name" >> release-notes.md
  done
  manifest="$source/releases.$channel.json"
  test -s "$manifest"
  # The application and service consume these same exact versioned assets.
  while IFS= read -r name; do test -s "$source/$name"; done < <(jq -r '.portable.file_name, (.installer // empty | .file_name), .service.file_name' "$manifest")
  cp "$source"/* Releases/
done
(cd Releases && sha256sum -- * > SHA256SUMS)
cat >> release-notes.md <<'NOTES'

## 使用说明

- 已内置 Mihomo 内核和 Geo 数据。
- Windows 安装版为所有用户安装到 Program Files，更新时运行 Inno Setup 并请求权限；用户文件保存在各自的 `%LOCALAPPDATA%\suki-clash`。
- 安装包和压缩包未签名：Windows 可能出现 SmartScreen 提示；macOS 首次打开需在“系统设置 → 隐私与安全性”中允许。
- 免安装版的用户配置仍保存在系统应用目录；TUN 需要相应服务与权限。
- Linux 下载后需为 AppImage 添加执行权限。
- 应用内更新：Windows 安装版运行新版安装程序，普通版替换主程序；macOS 替换完整应用包；Linux 替换 AppImage。
- Windows 安装包负责系统服务的安装、升级及卸载。Windows 普通版随应用更新按需下载并替换程序目录中的服务文件，系统服务仍由虚拟网卡页面管理；macOS 和 Linux 的应用更新不处理系统服务。
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
