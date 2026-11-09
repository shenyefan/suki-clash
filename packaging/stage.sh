#!/usr/bin/env bash
# Stage the release binaries and offline runtime in the layout packed by vpk.
# Usage: stage.sh TARGET VERSION RUNTIME_DIR STAGE_DIR   (run on the target's own runner)
set -euo pipefail

target=$1 version=$2 runtime=$3 stage=$4
root=$(cd "$(dirname "$0")/.." && pwd)
app="$root/target/$target/release"
service="$root/platform-service/target/$target/release"

case $target in
  *-pc-windows-msvc)
    mkdir -p "$stage"
    cp "$app/suki-clash.exe" "$service/suki-clash-service.exe" "$stage/"
    cp -R "$runtime" "$stage/resources"
    ;;
  aarch64-apple-darwin)
    package="$stage/Suki Clash.app"
    mkdir -p "$package/Contents/MacOS"
    cp "$app/suki-clash" "$service/suki-clash-service" "$package/Contents/MacOS/"
    cp -R "$runtime" "$package/Contents/Resources"
    plist="$package/Contents/Info.plist"
    cp "$root/packaging/macos/Info.plist" "$plist"
    plutil -replace CFBundleShortVersionString -string "$version" "$plist"
    plutil -replace CFBundleVersion -string "$version" "$plist"
    plutil -replace CFBundleIconFile -string suki-clash.icns "$plist"
    iconset="$(mktemp -d)/suki-clash.iconset"
    mkdir "$iconset"
    for size in 16 32 128 256 512; do
      sips -z "$size" "$size" "$root/assets/logo.png" --out "$iconset/icon_${size}x${size}.png" > /dev/null
      sips -z $((size * 2)) $((size * 2)) "$root/assets/logo.png" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
    done
    iconutil -c icns "$iconset" -o "$package/Contents/Resources/suki-clash.icns"
    ;;
  x86_64-unknown-linux-gnu)
    appdir="$stage/SukiClash.AppDir"
    mkdir -p "$appdir/usr/bin"
    cp "$app/suki-clash" "$service/suki-clash-service" "$appdir/usr/bin/"
    cp -R "$runtime" "$appdir/usr/bin/resources"
    cp "$root/assets/logo.png" "$appdir/suki-clash.png"
    cat > "$appdir/suki-clash.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Suki Clash
Exec=suki-clash
Icon=suki-clash
Terminal=false
StartupWMClass=suki-clash
Categories=Network;
X-AppImage-Version=$version
MimeType=x-scheme-handler/suki-clash;x-scheme-handler/clash;
EOF
    # linuxdeploy collects the shared libraries; extracting avoids FUSE on the runner.
    tools=$(mktemp -d)
    tool=linuxdeploy-x86_64.AppImage
    for attempt in 1 2 3 4; do
      gh release download 1-alpha-20251107-1 -R linuxdeploy/linuxdeploy -p "$tool" -D "$tools" --clobber && break
      [[ $attempt == 4 ]] && exit 1
      sleep $((attempt * 2))
    done
    (cd "$tools" && echo "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d  $tool" | sha256sum -c - \
      && chmod +x "$tool" && "./$tool" --appimage-extract > /dev/null)
    "$tools/squashfs-root/AppRun" --appdir "$appdir" --desktop-file "$appdir/suki-clash.desktop" \
      --icon-file "$appdir/suki-clash.png" \
      --executable "$appdir/usr/bin/suki-clash" --executable "$appdir/usr/bin/suki-clash-service"
    ;;
  *)
    echo "::error::Unsupported target $target"
    exit 1
    ;;
esac
