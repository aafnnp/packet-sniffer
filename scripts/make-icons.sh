#!/usr/bin/env bash
# 从 assets/icons/packet-sniffer.svg 生成各平台图标。
#
# 依赖：rsvg-convert（brew install librsvg）、python3；生成 .icns 还需要 macOS 自带的 iconutil。
# 用法：scripts/make-icons.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON_DIR="$ROOT_DIR/assets/icons"
SVG="$ICON_DIR/packet-sniffer.svg"
PNG_DIR="$ICON_DIR/png"

command -v rsvg-convert >/dev/null 2>&1 || {
  echo "缺少 rsvg-convert：brew install librsvg" >&2
  exit 1
}

mkdir -p "$PNG_DIR"
for size in 16 32 48 64 128 256 512 1024; do
  rsvg-convert -w "$size" -h "$size" "$SVG" -o "$PNG_DIR/packet-sniffer-$size.png"
done

# ICO 允许直接内嵌 PNG 帧（Windows Vista 起支持），因此不需要额外的转换工具。
python3 - "$PNG_DIR" "$ICON_DIR/packet-sniffer.ico" <<'PY'
import struct
import sys
from pathlib import Path

png_dir, out = Path(sys.argv[1]), Path(sys.argv[2])
sizes = (16, 32, 48, 64, 128, 256)
images = [(size, (png_dir / f"packet-sniffer-{size}.png").read_bytes()) for size in sizes]

header = struct.pack("<HHH", 0, 1, len(images))
entries, offset = b"", 6 + 16 * len(images)
for size, data in images:
    entries += struct.pack(
        "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset
    )
    offset += len(data)

out.write_bytes(header + entries + b"".join(data for _, data in images))
print(f"已生成 {out}")
PY

if command -v iconutil >/dev/null 2>&1; then
  ICONSET="$(mktemp -d)/AppIcon.iconset"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    cp "$PNG_DIR/packet-sniffer-$size.png" "$ICONSET/icon_${size}x${size}.png"
    cp "$PNG_DIR/packet-sniffer-$((size * 2)).png" "$ICONSET/icon_${size}x${size}@2x.png"
  done
  iconutil -c icns "$ICONSET" -o "$ICON_DIR/packet-sniffer.icns"
  rm -rf "$(dirname "$ICONSET")"
  echo "已生成 $ICON_DIR/packet-sniffer.icns"
else
  echo "未找到 iconutil，跳过 .icns（该步骤需要 macOS）" >&2
fi
