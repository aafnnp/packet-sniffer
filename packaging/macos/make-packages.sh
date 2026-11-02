#!/usr/bin/env bash
# 生成 macOS 的三种产物：Packet Sniffer.app（中间产物）、.dmg（拖拽安装）、.pkg（安装器，附带 CLI）。
#
# 用法：packaging/macos/make-packages.sh <二进制目录> <版本号> <架构标签> <输出目录>
# 例：  packaging/macos/make-packages.sh target/universal/release 0.1.0 universal dist
set -euo pipefail

BIN_DIR="${1:?用法: make-packages.sh <二进制目录> <版本号> <架构标签> <输出目录>}"
VERSION="${2:?缺少版本号}"
ARCH="${3:?缺少架构标签}"
OUT_DIR="${4:?缺少输出目录}"

APP_NAME="Packet Sniffer"
BUNDLE_ID="dev.packet-sniffer.app"
# CFBundleVersion 与 pkg 版本只接受数字与点，因此去掉 +g<sha> 之类的构建元数据。
BUNDLE_VERSION="${VERSION%%+*}"
BUNDLE_VERSION="${BUNDLE_VERSION%%-*}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

APP_DIR="$WORK_DIR/app/$APP_NAME.app"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources" "$OUT_DIR"

install -m 0755 "$BIN_DIR/packet-sniffer-ui" "$APP_DIR/Contents/MacOS/packet-sniffer-ui"
install -m 0755 "$BIN_DIR/packet-sniffer" "$APP_DIR/Contents/MacOS/packet-sniffer"
cp "$ROOT_DIR/README.md" "$ROOT_DIR/LICENSE" "$APP_DIR/Contents/Resources/"
install -m 0644 "$ROOT_DIR/assets/icons/packet-sniffer.icns" \
  "$APP_DIR/Contents/Resources/AppIcon.icns"
sed -e "s/__APP_VERSION__/$BUNDLE_VERSION/g" \
  "$ROOT_DIR/packaging/macos/Info.plist" > "$APP_DIR/Contents/Info.plist"

# 本地构建用 ad-hoc 签名，避免 Gatekeeper 直接拒绝启动；正式分发需 Developer ID。
codesign --force --deep --sign - "$APP_DIR" 2>/dev/null \
  || echo "警告：ad-hoc 签名失败，产物仍可右键「打开」启动"

# 1) dmg：拖拽进 /Applications 即完成安装
DMG_STAGE="$WORK_DIR/dmg"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"

DMG="$OUT_DIR/PacketSniffer-${VERSION}-macos-${ARCH}.dmg"
rm -f "$DMG"
hdiutil create -quiet -volname "$APP_NAME $BUNDLE_VERSION" \
  -srcfolder "$DMG_STAGE" -fs HFS+ -format UDZO -ov "$DMG"

# 2) pkg：安装器，额外把 CLI 链接到 /usr/local/bin，装完即可在终端使用
PKG_ROOT="$WORK_DIR/pkgroot"
mkdir -p "$PKG_ROOT/Applications" "$PKG_ROOT/usr/local/bin"
cp -R "$APP_DIR" "$PKG_ROOT/Applications/"
ln -s "/Applications/$APP_NAME.app/Contents/MacOS/packet-sniffer" \
  "$PKG_ROOT/usr/local/bin/packet-sniffer"
# 去掉扩展属性，避免 pkgbuild 在载荷里塞入 ._* 之类的 AppleDouble 文件
xattr -cr -s "$PKG_ROOT"

PKG="$OUT_DIR/PacketSniffer-${VERSION}-macos-${ARCH}.pkg"
rm -f "$PKG"
pkgbuild --quiet --root "$PKG_ROOT" \
  --identifier "${BUNDLE_ID}.pkg" \
  --version "$BUNDLE_VERSION" \
  --install-location / \
  --ownership recommended \
  "$PKG"

echo "已生成 $DMG"
echo "已生成 $PKG"
