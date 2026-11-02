#!/usr/bin/env bash
# 生成 GitHub Release 说明：安装指引 + 按类型分组的变更列表。
#
# 用法：scripts/release-notes.sh <tag> [owner/repo] > notes.md
# 需要完整历史（actions/checkout 的 fetch-depth: 0），否则找不到上一个标签。
set -euo pipefail

TAG="${1:?用法: release-notes.sh <tag> [owner/repo]}"
REPO="${2:-${GITHUB_REPOSITORY:-aafnnp/packet-sniffer}}"
VERSION="${TAG#v}"

if PREV="$(git describe --tags --abbrev=0 --match 'v*' "${TAG}^" 2>/dev/null)"; then
  RANGE="${PREV}..${TAG}"
  CHANGELOG_URL="https://github.com/${REPO}/compare/${PREV}...${TAG}"
  SCOPE="\`${PREV}\` 以来的变更"
else
  PREV=""
  RANGE="${TAG}"
  CHANGELOG_URL="https://github.com/${REPO}/commits/${TAG}"
  SCOPE="首次发布，包含 \`${TAG}\` 之前的全部提交"
fi

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
for bucket in feat fix perf refactor docs tests build other; do
  : > "$WORK_DIR/$bucket"
done

count=0
while IFS= read -r subject || [ -n "$subject" ]; do
  [ -n "$subject" ] || continue
  count=$((count + 1))
  bucket=other
  desc="$subject"
  # 解析 Conventional Commits：type(scope)!: description
  if [[ "$subject" =~ ^([a-z]+)(\([^\)]*\))?!?:[[:space:]]*(.+)$ ]]; then
    type="${BASH_REMATCH[1]}"
    desc="${BASH_REMATCH[3]}"
    case "$type" in
      feat) bucket=feat ;;
      fix) bucket=fix ;;
      perf) bucket=perf ;;
      refactor) bucket=refactor ;;
      docs) bucket=docs ;;
      test) bucket=tests ;;
      build|ci|chore|style) bucket=build ;;
      *) bucket=other ;;
    esac
  fi
  printf -- '- %s\n' "$desc" >> "$WORK_DIR/$bucket"
done < <(git log --no-merges --pretty=format:'%s' "$RANGE")

section() {
  [ -s "$WORK_DIR/$1" ] || return 0
  printf '### %s\n\n' "$2"
  cat "$WORK_DIR/$1"
  printf '\n'
}

cat <<EOF
## 安装

| 平台 | 一键安装 |
| --- | --- |
| Debian / Ubuntu | \`sudo apt install ./packet-sniffer_*_amd64.deb\` |
| Fedora / RHEL / openSUSE | \`sudo dnf install ./packet-sniffer-*.x86_64.rpm\` |
| Arch / Manjaro | \`sudo pacman -U packet-sniffer-*-x86_64.pkg.tar.zst\` |
| 任意 Linux（免 root） | 给 \`PacketSniffer-*-linux-x86_64.AppImage\` 加可执行权限后直接运行 |
| macOS | 打开 \`PacketSniffer-${VERSION}-macos-universal.dmg\` 拖入「应用程序」，或用 \`.pkg\` 安装以获得 \`/usr/local/bin/packet-sniffer\` |
| Windows | 双击 \`PacketSniffer-${VERSION}-x64-setup.exe\`；企业静默部署用 \`.msi\` |
| 包管理器 | 清单见 \`packet-sniffer-${VERSION}-manifests.zip\`（winget / Scoop / Chocolatey） |
| 绿色版 | 解压 \`.tar.gz\`（Linux / macOS）或 \`.zip\`（Windows）即可运行 |

下载后可用 \`SHA256SUMS.txt\` 校验完整性。

## 变更

${SCOPE}，共 ${count} 个提交。

EOF

section feat '✨ 新功能'
section fix '🐛 问题修复'
section perf '⚡ 性能'
section refactor '♻️ 重构'
section docs '📝 文档'
section tests '✅ 测试'
section build '🔧 构建与 CI'
section other '📦 其他'

printf '**Full Changelog**: %s\n' "$CHANGELOG_URL"
