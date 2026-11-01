#!/usr/bin/env bash
# 生成 AppImage：单文件、无需 root，chmod +x 后双击或命令行运行。
#
# 用法：packaging/linux/make-appimage.sh <二进制目录> <版本号> <AppImage 架构> <输出目录>
# 例：  packaging/linux/make-appimage.sh dist/staging 0.1.0 x86_64 dist
#
# 注意：AppImage 里不打包 GTK4，运行时需要宿主已安装 GTK4（Ubuntu 24.04 / Fedora 等桌面版默认自带）；
# 另外 AppImage 运行时依赖 FUSE2，Ubuntu 24.04 需 `sudo apt install libfuse2t64`，
# 或使用 APPIMAGE_EXTRACT_AND_RUN=1 直接解包运行。
set -euo pipefail

BIN_DIR="${1:?用法: make-appimage.sh <二进制目录> <版本号> <AppImage 架构> <输出目录>}"
VERSION="${2:?缺少版本号}"
ARCH="${3:?缺少架构（x86_64 / aarch64）}"
OUT_DIR="${4:?缺少输出目录}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

APPDIR="$WORK_DIR/AppDir"
mkdir -p "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m 0755 "$BIN_DIR/packet-sniffer" "$APPDIR/usr/bin/packet-sniffer"
install -m 0755 "$BIN_DIR/packet-sniffer-ui" "$APPDIR/usr/bin/packet-sniffer-ui"
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.desktop" \
  "$APPDIR/usr/share/applications/packet-sniffer.desktop"
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.desktop" \
  "$APPDIR/packet-sniffer.desktop"
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.svg" \
  "$APPDIR/usr/share/icons/hicolor/scalable/apps/packet-sniffer.svg"
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.png" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps/packet-sniffer.png"
# appimagetool 要求 .DirIcon 是 PNG，SVG 会直接拒绝打包
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.png" "$APPDIR/packet-sniffer.png"
ln -s packet-sniffer.png "$APPDIR/.DirIcon"
install -m 0644 "$ROOT_DIR/packaging/linux/packet-sniffer.svg" "$APPDIR/packet-sniffer.svg"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/packet-sniffer-ui" "$@"
APPRUN
chmod 0755 "$APPDIR/AppRun"

# APPIMAGETOOL 环境变量可用于指定已下载的工具（便于本地/离线构建），否则按官方 release 下载。
TOOL="${APPIMAGETOOL:-}"
if [ -z "$TOOL" ]; then
  TOOL="$WORK_DIR/appimagetool.AppImage"
  curl -fsSL --retry 3 --retry-delay 2 -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage"
fi
chmod +x "$TOOL"

OUT_FILE="$OUT_DIR/PacketSniffer-${VERSION}-linux-${ARCH}.AppImage"
rm -f "$OUT_FILE"
ARCH="$ARCH" VERSION="$VERSION" "$TOOL" --appimage-extract-and-run --no-appstream "$APPDIR" "$OUT_FILE"
chmod +x "$OUT_FILE"

echo "已生成 $OUT_FILE"
