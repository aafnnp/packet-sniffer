# Packet Sniffer

[![Build](https://github.com/aafnnp/packet-sniffer/actions/workflows/build.yml/badge.svg)](https://github.com/aafnnp/packet-sniffer/actions/workflows/build.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

本地 HTTP / HTTPS 抓包与调试工具。抓包引擎是独立的 Rust 库，界面使用 [NSH](https://nsh.dev/zh-CN/docs/) 的 Rust 原生组件（macOS AppKit / Windows / Linux GTK4），**不包含 TSX、QuickJS 或 JS 运行时**。布局参考 [Proxyman](https://proxyman.com/)：左侧来源列表、中间请求表格、底部请求 / 响应并列检查器。

```
客户端 ──► packet-sniffer (127.0.0.1:8899)
             ├─ 明文 HTTP ───────────────────────► 上游
             └─ CONNECT → MITM (rustls) ─────────► 上游
                    │
        EngineEvent (broadcast) → EventBridge (async-channel)
                    │
        App / Model 状态机 ──► NSH 原生界面 / 终端输出
                    │
        MemoryStore（默认 2000 条）+ HistoryStore（SQLite）
```

---

## 一、功能特性

| 能力 | 说明 |
|------|------|
| HTTP 代理 | 明文入口支持 HTTP/1.1 与 h2c（HTTP/2 prior knowledge），HTTP / HTTPS 统一抓取 |
| HTTP/2 双向 | MITM 与上游都按 ALPN 协商 `h2` / `http/1.1`，两侧协议相互独立（可 h2→h1 降级或 h1→h2 升级）；记录里请求记客户端协议、响应记上游协议，并采集响应尾部首部 |
| 上游连接复用 | HTTP/2 连接按 `(host, port, TLS)` 池化，同一目标的并发请求复用一条连接开多条流；空闲 90s 或连接关闭后自动回收，可通过 `EngineConfig::with_upstream_pool` 调整或关闭 |
| HTTPS 解密 | CONNECT 隧道 + MITM；首次启动生成根 CA，按 SNI 动态签发带 SAN 的叶子证书（缓存上限 512 个） |
| 只转发模式 | 关闭解密后 CONNECT 仅做透明隧道转发 |
| 响应体解压 | 自动解开 `gzip` / `deflate` / `br`，并按解码后的内容展示 |
| 自定义上游信任根 | 上游已被企业代理二次拦截时，可追加 PEM 根证书，无需关闭证书校验 |
| 规则引擎 | 按方法 / URL 子串 / 域名后缀匹配，执行阻断、模拟响应、请求或响应首部增改 |
| 请求记录 | 内存环形存储；图形界面另行写入 SQLite（WAL）持久化历史、高亮与备注，重启后恢复 |
| 系统集成 | 接管 / 释放系统代理，安装 / 移除根证书，按平台条件编译 |
| 双形态运行 | `packet-sniffer`（headless CLI）与 `packet-sniffer-ui`（NSH 原生图形界面）共用同一套状态机与事件桥 |
| 应用更新 | 检查 GitHub Releases 的 `latest.json`，校验 SHA-256 后替换绿色版并重启；安装包版拉起 dmg / NSIS，包管理器安装则打开发布页 |
| 跨平台 | macOS（Apple Silicon / Intel）、Windows x86_64、Linux x86_64 |

## 二、Workspace 结构

```
Proxy/
├── Cargo.toml                  # workspace 清单，统一管理依赖版本；release 使用 thin LTO + codegen-units=1
├── Cargo.lock
├── LICENSE                     # MIT
├── assets/
│   ├── cert/README.md          # 可选的固定根证书说明（默认在运行时生成）
│   ├── icons/                  # 应用图标：SVG 母版 + PNG / ICO / ICNS
│   └── themes/default-dark.json
├── scripts/
│   ├── make-icons.sh           # 由 SVG 重新生成各平台图标
│   ├── update-manifest.sh      # 生成 latest.json 自更新清单
│   └── verify.sh               # 端到端自检脚本
├── crates/
│   ├── common/                 # 跨包共享类型与工具，不依赖网络 / TLS / UI
│   │   └── src/{types,error,utils}.rs
│   ├── proxy-engine/           # 抓包核心引擎，纯逻辑、无 UI 依赖
│   │   ├── src/config.rs       # EngineConfig 与默认值
│   │   ├── src/rules.rs        # 规则匹配与动作
│   │   ├── src/protocol/       # http1 解析、body 录制包装、响应体解压
│   │   ├── src/proxy/          # tcp 监听、http 转发、CONNECT 隧道、上游连接
│   │   ├── src/mitm/           # 根 CA 管理（ca.rs）与 TLS 握手（tls.rs）
│   │   ├── src/store/          # 内存存储 MemoryStore
│   │   └── tests/proxy_e2e.rs  # 端到端集成测试
│   ├── system-bridge/          # 系统代理与根证书，按 `cfg(target_os)` 条件编译
│   ├── desktop/                # headless CLI + 共享状态机（库）
│   │   ├── src/main.rs         # packet-sniffer：子命令解析与终端输出
│   │   ├── src/app.rs          # 与 UI 无关的状态与事件分发
│   │   ├── src/bridge/         # 引擎事件 → async-channel 事件桥
│   │   ├── src/state/          # 请求列表、设置读写
│   │   └── src/storage.rs      # SQLite 历史记录、高亮与备注（供图形界面使用）
│   ├── updater/                # 应用自更新：清单检查、校验下载、替换与重启
│   │   ├── src/lib.rs          # 清单解析、版本比较、HTTP 下载与安装策略
│   │   └── src/install.rs      # 解压、原地替换、Windows 延迟替换与回滚
│   └── desktop-ui/             # NSH 原生图形界面
│       ├── nsh.json            # NSH 应用描述（根组件 App）
│       ├── build.rs            # Windows 下把图标与版本信息嵌入 exe
│       ├── src/main.rs         # 挂载平台后端并启动窗口
│       ├── src/lib.rs          # 根组件、120ms 轮询与快捷键
│       ├── src/model.rs        # 引擎生命周期、筛选、排序、选中与详情
│       ├── src/{format,theme}.rs
│       └── src/ui/             # chrome / table / inspector / settings / rules / widgets / icons
```

### 设计要点

1. **引擎与 UI 解耦**：`proxy-engine` 不依赖任何 UI 或系统 GUI 库，可单独编译与测试。
2. **统一数据模型**：`CapturedRequest` / `CapturedResponse` / `CapturedExchange` / `EngineEvent` 全部定义在 `common`，避免重复定义与转换。
3. **平台隔离**：系统能力收敛在 `system-bridge`，上层只调用统一接口。
4. **异步解耦**：引擎在 tokio 运行时中通过 `broadcast` 推送事件，`EventBridge` 转成 `async-channel`，UI 用 120ms 定时器批量搬运，不阻塞渲染线程。

### 主要依赖

| 领域 | 依赖 |
|------|------|
| 异步与网络 | `tokio`（full）、`hyper` 1.0、`hyper-util`、`http`、`http-body-util`、`bytes` |
| TLS | `rustls` 0.23（ring）、`tokio-rustls` 0.26、`rustls-pki-types`、`rustls-pemfile`、`webpki-roots`、`rcgen` 0.13 |
| 数据处理 | `serde` / `serde_json`、`flate2`（gzip / deflate）、`brotli` |
| 存储与并发 | `rusqlite` 0.41（bundled）、`indexmap`、`async-channel` |
| 日志与错误 | `tracing`、`tracing-subscriber`、`thiserror`、`anyhow`、`chrono`、`uuid` |
| 原生界面 | `nsh-core` / `nsh-base` / `nsh-platform-macos` / `nsh-platform-linux` / `nsh-platform-windows` 0.3.0 |
| 应用更新 | `semver`、`sha2`、`ureq` 3（rustls）、`tar` + `flate2`、`zip` |

## 三、环境要求

- **Rust stable**：`desktop-ui` 使用 edition 2024，需要 Rust 1.85 及以上（CI 使用 stable）。
- **macOS**：CI 以 `MACOSX_DEPLOYMENT_TARGET=14.0` 构建。
- **Windows**：MSVC 工具链（`x86_64-pc-windows-msvc`）。
- **Linux**：NSH 的 GTK4 后端需要以下开发包：

```bash
sudo apt-get update
sudo apt-get install -y libgtk-4-dev libpango1.0-dev libgdk-pixbuf-2.0-dev \
  libcairo2-dev libglib2.0-dev pkg-config
```

可选：安装 NSH CLI（`nsh`）用于 `nsh run` / 打包；直接 `cargo run` 不需要它。

## 四、快速开始

> 不想从源码编译？Release 页面提供 `.deb`、`.rpm`、pacman 包、AppImage、`.dmg`、`.pkg`、`.exe`、`.msi` 等安装包，装好即用，详见 [十一、CI 与构建产物](#十一ci-与构建产物)。

### 1. 图形界面（推荐）

```bash
# 启动 NSH 原生窗口，默认监听 127.0.0.1:8899 并自动启动引擎
cargo run -p desktop-ui

# 或用 NSH CLI 运行 / 打包
nsh run crates/desktop-ui
```

### 2. 命令行（headless）

```bash
cargo run -p desktop -- run --port 8899          # 事件打印到终端
cargo run -p desktop -- run --json               # 额外输出完整记录的 JSON 行
cargo run -p desktop -- --help                   # 查看全部参数
```

### 3. 信任根证书

HTTPS 解密要求系统信任本机根证书，首次运行时自动生成到 `~/.packet-sniffer/certs/`：

```bash
cargo run -p desktop -- export-ca --out root-ca.pem   # 导出 root-ca.pem
cargo run -p desktop -- install-ca                     # 安装到系统信任存储（需要管理员权限）
cargo run -p desktop -- uninstall-ca                   # 移除
```

界面「⋯ 菜单 → 设置」的证书卡片也提供安装与导出按钮；检测到根证书已进入系统信任存储时，界面不再提示安装。

### 4. 接管系统代理

```bash
cargo run -p desktop -- system-proxy enable
cargo run -p desktop -- system-proxy disable
```

界面「设置」中打开「接管系统代理」后，引擎启动时自动设置。

### 5. 验证

> `curl` 默认对 localhost 直连，测试本机代理必须加 `--noproxy ""`。

```bash
curl -x http://127.0.0.1:8899 --noproxy "" http://127.0.0.1:18080/index.html
curl -x http://127.0.0.1:8899 --cacert root-ca.pem https://example.com/
```

运行完整自检（监听、状态页、明文抓包、MITM 解密、系统信任、系统代理）：

```bash
cargo run -p desktop -- run --port 8899   # 另开一个终端
scripts/verify.sh http://127.0.0.1:8899
```

## 五、命令行参考

```
packet-sniffer run [选项]                启动代理（headless 模式输出到终端）
packet-sniffer export-ca [--out <文件>]  导出根证书（默认 root-ca.pem）
packet-sniffer install-ca                安装根证书到系统信任存储
packet-sniffer uninstall-ca              从系统信任存储移除根证书
packet-sniffer system-proxy enable|disable
packet-sniffer update [--check] [-y]     检查更新；默认交互确认后安装
packet-sniffer --help
```

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--host <地址>` | `127.0.0.1` | 监听地址 |
| `--port <端口>` | `8899` | 监听端口 |
| `--ca-dir <目录>` | `~/.packet-sniffer/certs` | 根证书目录 |
| `--settings <文件>` | `~/.packet-sniffer/settings.json` | 设置文件路径 |
| `--no-capture` / `--capture` | `--capture` | 关闭 / 开启 HTTPS 解密 |
| `--max-body <字节>` | `2097152`（2 MiB） | 单个 body 抓取上限 |
| `--store-capacity <条>` | `2000` | 内存存储条数上限 |
| `--json` | 关闭 | `run` 模式下额外输出完整记录的 JSON 行 |

命令行参数会覆盖设置文件中的同名项，但不会写回设置文件。

## 六、配置

设置持久化为 JSON（`~/.packet-sniffer/settings.json`），结构级 `serde(default)`：文件损坏或新增字段缺失时回落到默认值，不阻断启动。

```json
{
  "listen_addr": "127.0.0.1:8899",
  "capture_enabled": true,
  "max_body_bytes": 2097152,
  "store_capacity": 2000,
  "ca_dir": "/Users/<name>/.packet-sniffer/certs",
  "system_proxy": false,
  "theme": "system",
  "rules": []
}
```

| 字段 | 说明 |
|------|------|
| `listen_addr` | 代理监听地址，默认 `127.0.0.1:8899` |
| `capture_enabled` | 是否解密 HTTPS；关闭后 CONNECT 仅透明转发 |
| `max_body_bytes` | 单个请求 / 响应 body 的抓取上限，超出部分标记 `body_truncated`，转发不受影响 |
| `store_capacity` | 内存中保留的最近请求条数，同时作为历史库裁剪上限 |
| `ca_dir` | 根证书与私钥目录，文件为 `root-ca.pem` / `root-ca.key` |
| `system_proxy` | 引擎启动后是否自动接管系统代理（默认关闭） |
| `theme` | `system` / `light` / `dark` |
| `rules` | 规则列表，见下节 |

引擎层固定默认值（`EngineConfig`）：根证书 CN 为 `Packet Sniffer Local CA`，连接上游超时 10s，等待响应体读完最长 120s。

### 运行时文件

| 路径 | 内容 |
|------|------|
| `~/.packet-sniffer/settings.json` | 界面与引擎设置、规则 |
| `~/.packet-sniffer/history.db` | SQLite 历史记录（WAL 模式）、高亮与备注 |
| `~/.packet-sniffer/certs/root-ca.pem` | 根证书，供系统信任 |
| `~/.packet-sniffer/certs/root-ca.key` | 根证书私钥，**不要提交或外传** |

## 七、规则引擎

规则按列表顺序匹配，条件为空表示不限制：

| 匹配条件 | 说明 |
|----------|------|
| `method` | HTTP 方法，大小写不敏感 |
| `url_contains` | URL 包含该子串（大小写不敏感） |
| `host_suffix` | Host 以该后缀结尾（大小写不敏感） |

| 动作 | 说明 |
|------|------|
| `block` | 直接返回指定状态码，不连接上游 |
| `mock` | 直接返回指定状态码、`Content-Type` 与响应体 |
| `add_request_header` | 请求发往上游前设置 / 覆盖首部 |
| `add_response_header` | 响应返回客户端前设置 / 覆盖首部 |

`block` 与 `mock` 会终止转发，取第一条命中的终止规则；首部规则会全部生效。在界面「规则引擎」页编辑并点击保存后会重启代理。

## 八、图形界面

- **工具栏**：启动 / 停止代理、筛选聚焦、清空、代理状态胶囊、切换排序、导出请求 JSON、⋯ 菜单（设置、规则引擎）。
- **侧边栏**：收藏（固定请求、错误请求）、全部请求、工具（规则引擎）、按客户端（User-Agent）分组、按域名分组；底部筛选框支持 URL / 域名 / 方法。
- **请求表格**：虚拟滚动；展示序号、URL、客户端、方法、状态、时间、耗时、请求 / 响应大小、HTTPS 与固定标记；支持全部 / HTTP / HTTPS / JSON / 错误类型筛选与按列排序。
- **右键菜单**：复制 URL、复制为 cURL、复制请求 / 响应原文、固定、高亮颜色、导出筛选结果、删除单条（清空全部在工具栏）。
- **检查器**：请求与响应并列。请求有概要、首部、查询参数、正文、原始 HTTP；响应有概要、首部、正文、原始、JSON 美化。JSON 正文用只读编辑器展示：自动格式化、语法高亮、行号与折叠；其余正文带行号，二进制以十六进制展示。顶部可选择高亮色并填写备注。
- **规则页**：新增 / 编辑 / 删除规则，应用后自动重启引擎。
- **设置页**：监听端口、正文上限、HTTPS 解密、系统代理接管、外观、根证书状态 / 安装与导出、历史库路径 / 条数与清空。
- **快捷键**：`⌘F` 聚焦筛选框，`⌘K` 清空请求，`⌘,` 打开设置。

导出的请求列表写到 `~/Downloads/packet-sniffer-requests-<时间戳>.json`，包含当前筛选与排序结果。

## 九、平台适配

`system-bridge` 通过调用系统命令实现，不引入重型平台 SDK：

| 平台 | 系统代理 | 根证书 |
|------|----------|--------|
| macOS | `networksetup`（遍历网络服务） | `security import` + `add-trusted-cert`（系统钥匙串，需要授权） |
| Windows | `reg` 写入 `HKCU\...\Internet Settings`（WinINET） | `certutil -addstore -user Root` / `-delstore` |
| Linux | GNOME `gsettings`（`org.gnome.system.proxy`） | NSS 数据库 `certutil`（Firefox / Chromium 等） |

其他平台走 `unsupported` 实现并返回明确错误。

## 十、开发与校验

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
cargo build --release --workspace
```

当前状态：`cargo test --workspace` 全部通过（共 95 个单元 / 集成测试，含 `proxy-engine/tests/proxy_e2e.rs`）；`cargo clippy` 仅剩 `ProxyContext::log` 未被使用的提示。

## 十一、CI 与构建产物

[`.github/workflows/build.yml`](.github/workflows/build.yml) 在推送 `master` / `main`、`v*` 标签、Pull Request 与手动触发时运行：

1. **test**（Ubuntu 24.04）：安装 GTK4 依赖，执行 `cargo fmt --check`、`cargo clippy --workspace --all-targets --locked`、`cargo test --workspace --locked`。
2. **package**（矩阵）：`linux-x86_64`、`linux-aarch64`、`windows-x86_64`、`macos-universal`，编译后用 `nfpm`、`appimagetool`、`hdiutil`/`pkgbuild`、`NSIS`、`WiX` 生成各平台安装包。
3. **release**（仅 `v*` 标签）：汇总全部产物，生成 `SHA256SUMS.txt`、`latest.json` 更新清单与包管理器清单，发布到 GitHub Release。

### 产物格式

| 平台 | 格式 | 文件名 | 一键安装 |
| --- | --- | --- | --- |
| Debian / Ubuntu | `.deb` | `packet-sniffer_<版本>-<修订>_<amd64,arm64>.deb` | `sudo apt install ./packet-sniffer_*.deb` |
| Fedora / RHEL / openSUSE | `.rpm` | `packet-sniffer-<版本>-<修订>.<x86_64,aarch64>.rpm` | `sudo dnf install ./packet-sniffer-*.rpm` |
| Arch / Manjaro | pacman | `packet-sniffer-<版本>-<修订>-<x86_64,aarch64>.pkg.tar.zst` | `sudo pacman -U packet-sniffer-*.pkg.tar.zst` |
| 任意 Linux | AppImage | `PacketSniffer-<版本>-linux-<x86_64,aarch64>.AppImage` | `chmod +x` 后双击或直接运行，免 root |
| macOS | `.dmg` | `PacketSniffer-<版本>-macos-universal.dmg` | 打开镜像，把 `Packet Sniffer.app` 拖进 `Applications` |
| macOS | `.pkg` | `PacketSniffer-<版本>-macos-universal.pkg` | 双击安装；附带 `/usr/local/bin/packet-sniffer` |
| Windows | `.exe`（NSIS） | `PacketSniffer-<版本>-x64-setup.exe` | 双击安装，自动建开始菜单与桌面快捷方式 |
| Windows | `.msi`（WiX） | `PacketSniffer-<版本>-x64.msi` | 双击，或 `msiexec /i x.msi /qn` 静默部署 |
| Windows | Chocolatey | `packet-sniffer.<版本>.nupkg` | `choco install packet-sniffer -s .` |
| Windows | winget 清单 | `packet-sniffer-<版本>-manifests.zip` 内 `winget/*.yaml` | `winget install --manifest <目录>` |
| Windows | Scoop 清单 | 同上 `scoop/packet-sniffer.json` | `scoop install ./packet-sniffer.json` |
| 任意 | 绿色版 | `packet-sniffer-<版本>-<平台>.tar.gz` / `.zip` | 解压即用 |

- `.deb` / `.rpm` / pacman 包安装后提供 `/usr/bin/packet-sniffer`（CLI）与 `/usr/bin/packet-sniffer-ui`（图形界面），并注册应用菜单条目与图标；运行期依赖 GTK4（`libgtk-4-1` / `gtk4`）。
- macOS 用 `lipo` 把 arm64 与 x86_64 合并成通用二进制，一个包覆盖 Intel 与 Apple Silicon。
- AppImage 不打包 GTK4，运行时需要宿主已安装 GTK4；另外 AppImage 运行时依赖 FUSE2（Ubuntu 24.04 可 `sudo apt install libfuse2t64`），或设置 `APPIMAGE_EXTRACT_AND_RUN=1` 直接解包运行。
- 非标签构建的版本号形如 `0.2.0+g<提交号>`，便于区分同一版本的多次构建；`SHA256SUMS.txt` 用于校验下载。
- macOS 产物只有 ad-hoc 签名，未做 Developer ID 签名与公证；首次从网上下载后若提示「无法验证开发者」，可右键「打开」，或执行 `xattr -dr com.apple.quarantine "/Applications/Packet Sniffer.app"`。
- Windows 的 `.exe` / `.msi` 未做 Authenticode 签名，SmartScreen 会提示未知发布者。
- WiX 固定在 5.x（v6 起需接受 OSMF 许可）；企业环境可用 MSI 做静默部署（`msiexec /i x.msi /qn`）。
- 应用图标以 [`assets/icons/packet-sniffer.svg`](assets/icons/packet-sniffer.svg) 为母版，`.png` / `.ico` / `.icns` 已随仓库提供；改动母版后执行 `scripts/make-icons.sh` 重新生成（需要 `rsvg-convert`，`.icns` 另需 macOS 的 `iconutil`）。Linux 打包安装 hicolor 多尺寸 PNG 与 SVG，macOS 用 `.icns`，Windows 安装包快捷方式与 `packet-sniffer-ui.exe` 用 `.ico`。

安装完成后：

```bash
packet-sniffer run                    # 启动 headless 代理
packet-sniffer install-ca             # 安装根证书（需要管理员权限）
"/Applications/Packet Sniffer.app/Contents/MacOS/packet-sniffer" install-ca   # macOS 未用 pkg 装 CLI 时
```

### 发布说明

`v*` 标签触发的 `release` job 会用 [`scripts/release-notes.sh`](scripts/release-notes.sh) 生成 Release 正文：安装方式表格 + 按类型分组的变更清单 + 与上一个标签的 compare 链接（首次发布则链接到全部提交）。

变更清单按 [Conventional Commits](https://www.conventionalcommits.org/) 前缀归类（`feat` / `fix` / `perf` / `refactor` / `docs` / `test` / `build`、`ci`、`chore`、`style`，其余落入「其他」），所以提交信息请保持 `type(scope): 描述` 的形式。本地预览：

```bash
scripts/release-notes.sh v0.2.0
```

### 本地打包

打包配置与脚本都在 [`packaging/`](packaging) 下；Linux / Windows 打包前需先把两个二进制放进 `dist/staging/`。

```bash
# Linux deb / rpm / pacman（需要 nfpm：https://nfpm.goreleaser.com）
NFPM_ARCH=amd64 NFPM_VERSION=0.2.0 NFPM_RELEASE=1 \
  nfpm package --config packaging/linux/nfpm.yaml --packager deb \
  --target dist/packet-sniffer_0.2.0-1_amd64.deb

# Linux AppImage（需联网下载 appimagetool，或用 APPIMAGETOOL 指定本地工具）
packaging/linux/make-appimage.sh dist/staging 0.2.0 x86_64 dist

# macOS dmg + pkg（系统自带 hdiutil / pkgbuild / codesign）
packaging/macos/make-packages.sh target/release 0.2.0 universal dist

# Windows NSIS（makensis 的相对路径以脚本目录为基准，故传绝对路径）
makensis -DAPP_VERSION=0.2.0 -DOUTFILE=<绝对路径>\PacketSniffer-setup.exe \
  -DSTAGE_DIR=<仓库根>/dist/staging -DDOCS_DIR=<仓库根> packaging/windows/installer.nsi

# Windows MSI（WiX 5：dotnet tool install --global wix --version 5.*）
wix build -arch x64 -d SourceDir=<仓库根>/dist/staging -d IconDir=<仓库根>/assets/icons/ \
  -d ProductVersion=0.2.0 -o dist/PacketSniffer.msi packaging/windows/product.wxs

# winget / Scoop / Chocolatey 清单（需要 Chocolatey）
pwsh packaging/windows/make-manifests.ps1 -Version 1.2.3 -DistDir dist
```

## 十二、应用更新

界面「设置 → 更新」与 CLI 的 `packet-sniffer update` 共用 [`crates/updater`](crates/updater)：

1. 读取 `https://github.com/aafnnp/packet-sniffer/releases/latest/download/latest.json`（不走 GitHub API，因此没有速率限制），清单包含版本、发布说明与各平台的包地址、大小、SHA-256。
2. 版本高于当前 `Cargo.toml` 里的版本时提供更新；平台键为 `macos-universal`、`linux-x86_64`、`linux-aarch64`、`windows-x86_64`。
3. 下载时边下边算 SHA-256，并校验大小；不一致就丢弃，不会安装。
4. 安装策略：
   - 绿色版（安装目录可写）：解压后替换 `packet-sniffer` / `packet-sniffer-ui`，替换前写 `.old` 备份；等本进程退出后拉起新界面，新进程启动成功再清理备份，失败则回滚或保留备份。
   - Windows：运行中的 exe 无法直接覆盖，改为生成一个延迟替换脚本，等本进程退出后替换、失败时回滚并重启；`.old` 备份留在原目录。
   - AppImage：设置 `APPIMAGE` 时用新 AppImage 覆盖自身。
   - macOS `.app`、`Program Files` 等不可写安装：下载并打开对应 dmg / NSIS 安装包。
   - 通过 deb / rpm / pacman 安装且没有可用安装包：打开发布页，由包管理器升级。

发布时 [`scripts/update-manifest.sh`](scripts/update-manifest.sh) 根据 `dist/` 里实际存在的产物生成 `latest.json`，随 Release 一起上传。本地调试可把 `PACKET_SNIFFER_UPDATE_URL` 指向自己的清单地址（允许 `http://127.0.0.1`）。

清单没有签名，完整性依赖 GitHub 账号与 HTTPS；发布权限被接管时攻击者也能改清单。

## 十三、已知限制

- 抓取协议：客户端与上游都支持 HTTP/1.1 / HTTP/2（明文 h2c 与 TLS ALPN），两侧独立协商，因此 `version` 里请求记客户端协议、响应记上游协议。
- 只有上游 HTTP/2 连接会进入连接池；HTTP/1.1 上游仍按请求新建（一条 h1 连接同时只能跑一个请求）。
- HTTP/1.1 只有在响应声明 `Trailer` 时才允许携带尾部首部，把 h2 的尾部首部转给 h1 客户端时可能被丢弃；抓包记录始终保留。
- WebSocket（依赖 HTTP Upgrade 抓帧）尚未支持；gRPC 走端到端 HTTP/2 已可抓包。请求方向的 trailers 暂不采集。
- 响应记录在客户端消费完 body（或达到 120s 空闲超时）后入库，因此列表会先出现「进行中」条目。
- 根证书需系统信任；`install-ca` 封装了平台命令，但系统授权对话框仍需用户确认。
- 规则引擎支持阻断、模拟响应与首部改写；断点、请求改写与重发未实现。
- 历史记录、高亮与备注已持久化；多选与「按规则自动高亮」未实现。
- Linux 的系统代理仅覆盖 GNOME `gsettings`，根证书通过 NSS 数据库安装。
- 未提供 Alpine `apk`（需 musl 构建环境）与 Flatpak / Snap（沙箱会限制系统代理接管与根证书安装）。

## 十四、安全提示

- 抓包数据包含完整请求 / 响应内容与凭据，`history.db` 与导出的 JSON 请按敏感数据保管。
- `root-ca.key` 是签发任意站点证书的能力来源，切勿提交到版本库或分发给他人；`.gitignore` 已忽略 `certs/`。
- 默认只监听 `127.0.0.1`。若改为对外监听，代理端口无认证，请自行加访问控制。
- 只抓取自己有权调试的流量。

## 十五、许可证

[MIT](LICENSE)
