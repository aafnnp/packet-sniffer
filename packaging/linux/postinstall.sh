#!/bin/sh
# 安装 / 卸载后刷新桌面条目与图标缓存；缓存工具缺失或失败都不影响事务。
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi

exit 0
