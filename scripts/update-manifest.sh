#!/usr/bin/env bash
#
# 生成应用自更新清单 latest.json。
#
# 发布 job 在 dist/ 里汇总各平台产物后调用本脚本；脚本只收录确实存在的文件，
# 并按 <os>-<arch> 写入下载地址、SHA-256 与大小。客户端（crates/updater）
# 读取 releases/latest/download/latest.json 判断是否有新版本。
#
# 用法：
#   scripts/update-manifest.sh --version 0.3.0 --tag v0.3.0 \
#     --repo owner/name --dist dist [--notes notes.md] [--out dist/latest.json]

set -euo pipefail

version=""
tag=""
repo=""
dist=""
notes=""
out=""
base_url=""

usage() {
  sed -n '3,11p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:?--version 缺少取值}"; shift 2 ;;
    --tag) tag="${2:?--tag 缺少取值}"; shift 2 ;;
    --repo) repo="${2:?--repo 缺少取值}"; shift 2 ;;
    --dist) dist="${2:?--dist 缺少取值}"; shift 2 ;;
    --notes) notes="${2:?--notes 缺少取值}"; shift 2 ;;
    --out) out="${2:?--out 缺少取值}"; shift 2 ;;
    --base-url) base_url="${2:?--base-url 缺少取值}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "未知参数：$1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "$version" ] || [ -z "$dist" ]; then
  echo "缺少 --version 或 --dist" >&2
  usage >&2
  exit 2
fi
if [ ! -d "$dist" ]; then
  echo "目录不存在：$dist" >&2
  exit 2
fi
if [ -z "$base_url" ]; then
  if [ -z "$repo" ] || [ -z "$tag" ]; then
    echo "缺少 --repo / --tag，或改用 --base-url" >&2
    exit 2
  fi
  base_url="https://github.com/$repo/releases/download/$tag/"
fi
case "$base_url" in
  */) ;;
  *) base_url="$base_url/" ;;
esac

python3 - "$version" "$dist" "$base_url" "$notes" "$out" <<'PY'
import hashlib
import json
import os
import sys

version, dist, base_url, notes_path, out_path = sys.argv[1:6]

platforms = {
    "linux-x86_64": {
        "archive": f"packet-sniffer-{version}-linux-x86_64.tar.gz",
        "installer": (f"PacketSniffer-{version}-linux-x86_64.AppImage", "appimage"),
    },
    "linux-aarch64": {
        "archive": f"packet-sniffer-{version}-linux-aarch64.tar.gz",
        "installer": (f"PacketSniffer-{version}-linux-aarch64.AppImage", "appimage"),
    },
    "macos-universal": {
        "archive": f"packet-sniffer-{version}-macos-universal.tar.gz",
        "installer": (f"PacketSniffer-{version}-macos-universal.dmg", "dmg"),
    },
    "windows-x86_64": {
        "archive": f"packet-sniffer-{version}-windows-x86_64.zip",
        "installer": (f"PacketSniffer-{version}-x64-setup.exe", "nsis"),
    },
}


def asset(name, kind=None):
    path = os.path.join(dist, name)
    if not os.path.isfile(path):
        return None
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    value = {
        "url": base_url + name,
        "sha256": digest.hexdigest(),
        "size": os.path.getsize(path),
        "name": name,
    }
    if kind:
        value["kind"] = kind
    return value


manifest = {
    "schema": 1,
    "id": "dev.packet-sniffer.app",
    "version": version,
    "platforms": {},
}

if notes_path:
    with open(notes_path, encoding="utf-8") as handle:
        manifest["notes"] = handle.read().strip()

for platform, names in platforms.items():
    entry = {}
    archive = asset(names["archive"])
    if archive:
        entry["archive"] = archive
    installer_name, installer_kind = names["installer"]
    installer = asset(installer_name, installer_kind)
    if installer:
        entry["installer"] = installer
    if entry:
        manifest["platforms"][platform] = entry

if not manifest["platforms"]:
    sys.exit(f"{dist} 里没有任何已发布的安装包")

text = json.dumps(manifest, ensure_ascii=False, indent=2) + "\n"
if out_path:
    with open(out_path, "w", encoding="utf-8") as handle:
        handle.write(text)
else:
    sys.stdout.write(text)
PY
