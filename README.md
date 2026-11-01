# Packet Sniffer：基于 NSH 的本地 HTTPS 抓包工具

[![Build](https://github.com/aafnnp/package-sniffer/actions/workflows/build.yml/badge.svg)](https://github.com/aafnnp/package-sniffer/actions/workflows/build.yml)

整体采用 **Rust Workspace 多包架构**，严格遵循「引擎与 UI 完全解耦」原则：抓包核心独立成库、可单独测试与命令行运行；桌面界面使用 [NSH](https://nsh.dev/zh-CN/docs/) 的 Rust 组件，在 macOS / Windows / Linux 上渲染为原生视图，不携带 QuickJS。界面布局参考 [Proxyman](https://proxyman.com/)：左侧来源列表、中间请求表格、底部请求 / 响应并列检查器。

---

## 一、完整项目目录结构
```
packet-sniffer/
├── Cargo.toml                  # Workspace 根配置，统一管理依赖版本
├── Cargo.lock
├── README.md
├── .gitignore
│
├── assets/                     # 静态资源（图标、证书说明、主题示例）
│
├── crates/                     # Rust 核心包（按职责拆分）
│   ├── common/                 # 公共类型与工具，所有包共享
│   ├── proxy-engine/           # 抓包核心引擎，纯逻辑、无 UI 依赖
│   │   ├── src/{proxy,mitm,protocol,store}/
│   │   └── src/rules.rs        # 规则匹配与动作（阻断 / 模拟 / 改首部）
│   ├── system-bridge/          # 系统代理与根证书，按平台条件编译
│   ├── desktop/                # 命令行入口 + 共享状态机
│   │   ├── src/lib.rs          # 导出 app / bridge / state
│   │   ├── src/main.rs         # packet-sniffer CLI
│   │   ├── src/app.rs          # 与 UI 无关的状态与事件分发
│   │   ├── src/bridge/         # 引擎事件 → UI 的异步通道
│   │   ├── src/storage.rs      # SQLite 历史记录、高亮与备注
│   │   └── src/state/          # 请求列表、设置持久化
│   └── desktop-ui/             # NSH 原生图形界面（Rust 组件，无 JS 运行时）
│       ├── nsh.json            # NSH 应用描述：根组件 App
│       ├── src/main.rs         # 挂载 NSH 应用
│       ├── src/lib.rs          # 根组件、定时器与快捷键
│       ├── src/model.rs        # 引擎生命周期、筛选、选中与详情
│       ├── src/format.rs       # 时间 / 大小 / body / UA 格式化
│       ├── src/theme.rs        # Proxyman 风格配色与密度
│       └── src/ui/             # chrome / table / inspector / settings / rules / widgets / icons
│
└── ui-legacy/                  # 早期 TSX 原型，仅作设计参考，不参与构建
```

### 架构设计关键点
1. **职责隔离**：`proxy-engine` 不依赖任何 UI 或系统 GUI 库，可单独编译、命令行运行，便于单元测试与后续复用。
2. **统一数据模型**：所有跨层数据结构定义在 `common` 包中，避免类型重复定义与转换开销。
3. **平台隔离**：系统相关能力全部收敛在 `system-bridge`，通过条件编译区分平台，上层调用统一接口。
4. **异步解耦**：引擎运行在独立的 tokio 异步线程池，通过消息通道向 UI 推送事件，不阻塞 NSH 主线程渲染。

---

## 二、核心依赖清单（按模块分类）
### 1. 公共基础依赖（所有包通用）
| 依赖包 | 版本建议 | 用途说明 | 必选 |
|--------|----------|----------|------|
| `serde` | `1.0` | 序列化/反序列化，配合 `derive` 宏为结构体添加序列化能力 | 是 |
| `serde_json` | `1.0` | JSON 格式解析与格式化，用于请求体展示与数据持久化 | 是 |
| `thiserror` | `1.0` | 自定义错误类型，定义业务错误枚举 | 是 |
| `anyhow` | `1.0` | 通用错误处理，简化原型开发阶段的错误传递 | 是 |
| `chrono` | `0.4` | 时间处理，记录请求时间戳、格式化展示 | 是 |
| `uuid` | `1.0` | 为每个请求生成唯一 ID，便于列表索引与详情查询 | 是 |
| `tracing` | `0.1` | 结构化日志，替代 `println!`，支持分级日志与异步输出 | 是 |
| `tracing-subscriber` | `0.3` | 日志订阅器，控制台与文件日志输出 | 是 |

### 2. 抓包引擎核心依赖（proxy-engine）
| 依赖包 | 版本建议 | 用途说明 | 必选 |
|--------|----------|----------|------|
| `tokio` | `1.0` | 全异步运行时，启用 `full` 特性，负责 TCP 网络 IO、并发调度、超时控制 | 是 |
| `tokio-util` | `0.7` | 提供 `Framed`、`Codec` 等流式工具，简化 HTTP 协议解析 | 是 |
| `rustls` | `0.23` | 纯 Rust 实现的 TLS 协议栈，替代 OpenSSL，跨平台编译零依赖 | 是 |
| `rustls-pki-types` | `0.1` | TLS 证书类型定义，配合 rustls 使用 | 是 |
| `rcgen` | `0.13` | 证书生成库，实现根 CA 管理、动态域名证书签发（带 SAN 字段） | 是 |
| `hyper` | `1.0` | HTTP/1.1 + HTTP/2 协议解析，支持流式 Body 与多路复用 | 是 |
| `http` | `1.0` | 标准 HTTP 类型定义（Request/Response/HeaderMap），与 hyper 兼容 | 是 |
| `flate2` | `1.0` | gzip / deflate 响应体解压 | 是 |
| `brotli` | `6.0` | brotli 响应体解压 | 是 |
| `url` | `2.0` | URL 解析、域名提取、路径规范化 | 是 |
| `bytes` | `1.0` | 高效字节缓冲区，零拷贝处理请求/响应体 | 是 |
| `once_cell` | `1.0` | 全局单例初始化，如根证书、配置项 | 是 |
| `rusqlite` | `0.41` | SQLite 持久化，保存历史请求、高亮与备注 | 是 |
| `tokio-tungstenite` | `0.21` | WebSocket 帧解析（进阶协议扩展可选） | 否 |
| `protobuf` | `3.0` | Protobuf 解码，用于 gRPC 协议支持（进阶可选） | 否 |

### 3. NSH UI 层依赖（desktop-ui）
| 依赖包 | 版本 | 用途说明 | 必选 |
|--------|------|----------|------|
| `nsh-core` | `0.3.0` | 原生元素、signal、布局与事件；`#[component]` 过程宏 | 是 |
| `nsh-base` | `0.3.0` | 输入框、菜单、主题 token、可调整分栏等基础组件 | 是 |
| `nsh-platform-macos` | `0.3.0` | macOS AppKit 后端（Windows / Linux 换对应平台包） | 是 |
| `tokio` | `1.0` | UI 进程内启动引擎运行时 | 是 |
| `async-channel` | `2.0` | 引擎事件 → UI 的事件桥 | 是 |
| `indexmap` | `2.0` | 有序请求列表与完整记录存储 | 是 |

> 说明：界面完全由 Rust 编写，不包含 TSX、QuickJS 或 `nsh-build`。依赖版本与 [nsh.dev 官方文档](https://nsh.dev/zh-CN/docs/) 的 0.3 系列一致。

### 4. 系统适配层依赖（system-bridge）
采用**平台条件编译**，仅对应系统编译对应依赖，不跨平台冗余引入。

#### macOS 平台
| 依赖包 | 用途说明 |
|--------|----------|
| `security-framework` | 操作 macOS 钥匙串，实现根证书一键安装、信任与删除 |
| `system-configuration` | 调用系统网络配置 API，设置/取消全局 HTTP/HTTPS 代理 |
| `objc` / `objc2` | 可选，用于调用原生 Objective-C API，处理更复杂的系统交互 |

#### Windows 平台
| 依赖包 | 用途说明 |
|--------|----------|
| `windows` | 微软官方 Rust 绑定，调用 Win32 API 操作证书存储、注册表代理设置 |
| `winreg` | 轻量级注册表操作库，快速修改系统代理配置 |

#### Linux 平台
| 依赖包 | 用途说明 |
|--------|----------|
| `dbus` | 通过 D-Bus 调用 NetworkManager 设置系统代理 |

---

## 三、关键配置文件示例
### 1. 根目录 `Cargo.toml`（Workspace 配置）
```toml
[workspace]
members = [
    "crates/common",
    "crates/proxy-engine",
    "crates/system-bridge",
    "crates/desktop",
    "crates/desktop-ui",
]
resolver = "2"

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
# 公共基础
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0"
anyhow = "1.0"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1.0", features = ["v4", "serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
bytes = "1.0"
async-channel = "2.0"
indexmap = { version = "2.0", features = ["serde"] }

# 网络与TLS
tokio = { version = "1.0", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
rustls = "0.23"
rcgen = "0.13"
hyper = { version = "1.0", features = ["full"] }
http = "1.0"
url = "2.0"
flate2 = "1.0"
brotli = "6.0"
```

### 2. `proxy-engine/Cargo.toml` 示例
```toml
[package]
name = "proxy-engine"
edition.workspace = true
version.workspace = true

[dependencies]
common = { path = "../common" }
# 从 workspace 继承版本
tokio.workspace = true
tokio-util.workspace = true
rustls.workspace = true
rcgen.workspace = true
hyper.workspace = true
http.workspace = true
url.workspace = true
flate2.workspace = true
brotli.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
anyhow.workspace = true
tracing.workspace = true
bytes.workspace = true
uuid.workspace = true
chrono.workspace = true
```

### 3. `desktop-ui/Cargo.toml` 示例
```toml
[package]
name = "desktop-ui"
edition = "2024"

[[bin]]
name = "packet-sniffer-ui"
path = "src/main.rs"

[dependencies]
desktop = { path = "../desktop" }
common = { path = "../common" }
proxy-engine = { path = "../proxy-engine" }
system-bridge = { path = "../system-bridge" }

nsh-core.workspace = true
nsh-base.workspace = true
tokio.workspace = true
async-channel.workspace = true
chrono.workspace = true

[target.'cfg(target_os = "macos")'.dependencies]
nsh-platform-macos.workspace = true
```

---

## 四、MVP 起步裁剪建议
如果是第一阶段快速跑通原型，可按以下顺序裁剪，降低初始复杂度：
1. **引擎层**：先不接入 SQLite，用 `Vec` + `HashMap` 做内存存储；暂不实现 HTTP/2、WebSocket，只支持 HTTP/1.1。
2. **系统层**：先通过命令调用 `networksetup`（macOS）或修改注册表（Windows）实现代理设置，不引入复杂系统框架依赖。
3. **UI层**：先实现「列表 + 详情 + 工具栏」基础布局，过滤、搜索、格式化等功能后续迭代。
4. **证书管理**：先支持手动导出证书文件，引导用户手动安装，一键安装功能放在第二阶段。

---

---

## 五、当前落地实现说明
第一阶段（MVP）与原生界面已落地，`cargo test --workspace`、`cargo fmt --check` 通过；`cargo clippy` 仅剩 `proxy-engine` 中未使用的 `ProxyContext::log` 提示。

### 1. 已实现范围
| 模块 | 状态 | 说明 |
|------|------|------|
| `crates/common` | ✅ | `CapturedRequest/Response/Exchange`、`EngineEvent`、统一错误与工具函数 |
| `crates/proxy-engine` | ✅ | HTTP/1.1 明文代理、CONNECT 隧道、HTTPS 基础 MITM（按 SNI 动态签发证书）、gzip/deflate/brotli 解压、内存存储、规则引擎（阻断 / 模拟响应 / 增改首部） |
| `crates/system-bridge` | ✅ | 系统代理开关与根证书安装/移除，按平台条件编译 |
| `crates/desktop` | ✅ | 命令行子命令、引擎生命周期、事件桥、设置持久化、SQLite 历史 / 高亮 / 备注与共享状态机 |
| `crates/desktop-ui` | ✅ | NSH 原生界面：Proxyman 风格三栏布局、虚拟滚动请求表、请求 / 响应检查器、高亮备注、规则页、设置页 |
| `ui-legacy/` | 📦 参考 | 早期 TSX 原型，不参与构建 |

运行时数据流：`ProxyEngine::run`（tokio）→ `broadcast` 事件 → `EventBridge`（`async-channel`）→ `App` 状态机 → UI / 终端输出。
请求完成后由 `desktop::storage` 写入 `~/.packet-sniffer/history.db`（WAL 模式，后台线程写入），启动时恢复最近记录与备注。

### 2. 与原始规划的必要偏差
1. **界面使用 NSH 0.3 的 Rust 组件**：`nsh-core` / `nsh-base` / `nsh-platform-*` 已发布；界面不经过 TSX 或 QuickJS，直接用 `cargo run -p desktop-ui` 启动 macOS AppKit 窗口，`nsh run crates/desktop-ui` 可用于打包。
2. **补充的依赖**：`tokio-rustls`（rustls 的异步 IO 适配，仅 `rustls` 无法做 IO）、`rustls-pemfile`（读取 PEM 根证书）、
   `webpki-roots`（上游证书校验）、`hyper-util`（`TokioIo` 适配）、`http-body-util`（body 组合）。
3. **版本修正**：`rustls-pki-types` 使用 `1.x`（README 中的 `0.1` 与 rustls 0.23 不兼容）；
   `rcgen` 需额外启用 `x509-parser` 特性，才能重新加载已落盘的根证书。
4. **system-bridge 采用命令调用**：按「MVP 起步裁剪建议」第 2 条，macOS 走 `networksetup` / `security`，
   Windows 走 `reg` / `certutil`，Linux 走 `gsettings` / `certutil`，未引入 `security-framework`、`windows`、`dbus` 等重型依赖。
5. **新增两个模块文件**：`protocol/body.rs`（边转发边录制的 body 包装器，保证抓包不改变转发语义）、
   `proxy/upstream.rs`（明文 / TLS 上游连接统一类型）。

### 3. 使用方式
```bash
# 图形界面（NSH 原生窗口，默认自动启动代理）
cargo run -p desktop-ui

# 或者用 NSH 0.3.0+ 运行 / 打包（cargo run 不需要 nsh CLI）
nsh run crates/desktop-ui

# 命令行（headless，事件打印到终端）
cargo run -p desktop -- run --port 8899

# 导出并安装根证书（HTTPS 解密必需）
cargo run -p desktop -- export-ca --out root-ca.pem
cargo run -p desktop -- install-ca

# 接管 / 释放系统代理
cargo run -p desktop -- system-proxy enable
cargo run -p desktop -- system-proxy disable

# 只转发不解密
cargo run -p desktop -- run --no-capture

# 链式代理：系统 → 本引擎(8899) → 已有代理(如 MonoProxy / Clash 的 8118) → 外网
cargo run -p desktop -- run --upstream-proxy 127.0.0.1:8118

# 链式代理 + Basic 认证
cargo run -p desktop -- run --upstream-proxy http://user:pass@10.0.0.2:3128
```
验证（注意 `curl` 默认对 localhost 直连，需要 `--noproxy ""` 才会走代理）：
```bash
curl -x http://127.0.0.1:8899 --noproxy "" http://127.0.0.1:18080/index.html
curl -x http://127.0.0.1:8899 --cacert root-ca.pem https://example.com/
```

> 图形界面通过 `crates/desktop-ui` 启动；命令行模式仍可用 `cargo run -p desktop -- run`，两者共用同一套状态机与事件桥。

### 4. 持续集成与构建产物
GitHub Actions 工作流 [`.github/workflows/build.yml`](.github/workflows/build.yml) 会在推送、Pull Request 和手动触发时：

1. 在 Ubuntu 24.04 上执行 `cargo fmt --check`、`cargo clippy` 与 `cargo test --workspace`；
2. 构建 Linux x86_64、Windows x86_64、macOS Apple Silicon 与 macOS Intel 四个平台的 release 二进制；
3. 每个平台上传独立产物，包含 `packet-sniffer`（CLI）与 `packet-sniffer-ui`（NSH 图形界面）。

构建产物可在仓库的 **Actions → Build → 对应运行 → Artifacts** 下载。本地生成同样的 release 二进制：

```bash
cargo build --release --workspace
```

### 5. 已知限制
- 仅解析 HTTP/1.1；HTTP/2、WebSocket、gRPC 留待后续迭代（ALPN 已固定为 `http/1.1`）。
- 单条 body 默认最多抓取 2 MiB（`--max-body` 可调），超出部分只标记 `body_truncated`，转发不受影响。
- 响应记录在客户端把 body 消费完（或超时）后入库，因此列表里会先出现「进行中」条目。
- 根证书需手动信任；未实现「一键安装 + 系统授权」的完整流程（`install-ca` 已封装平台命令）。
- 规则引擎已支持阻断、模拟响应、请求 / 响应首部增改；断点、请求改写与重发仍未实现。
- 历史记录、高亮与备注已持久化到 SQLite；多选、按规则自动高亮暂未实现。

### 6. 图形界面
`crates/desktop-ui` 是 NSH 原生界面，布局参考 Proxyman：

- **工具栏**：启动 / 停止代理、筛选、清空、代理状态胶囊、排序、导出根证书、设置。
- **侧边栏**：收藏（固定请求、错误请求）、全部请求、按客户端（User-Agent）分组、按域名分组；底部筛选框支持 URL / 域名 / 方法。
- **请求表格**：虚拟滚动，展示序号、URL、客户端、方法、状态、时间、耗时、请求 / 响应大小、HTTPS 与固定标记；HTTP / HTTPS / JSON / 错误类型标签和方法筛选；点击列头可按 URL、客户端、方法、状态、时间、耗时、大小排序。
- **右键菜单**：复制 URL、复制为 cURL、复制请求 / 响应原文、固定、高亮颜色、导出筛选结果、删除单条或清空全部。
- **检查器**：请求与响应并列，支持概要、首部、查询参数、正文、原始 HTTP；顶部可直接选择高亮色并填写备注；响应额外提供 JSON 美化；正文带行号，二进制以十六进制展示。
- **规则引擎**：侧边栏「规则引擎」页新增 / 编辑 / 删除规则，可按方法、URL、域名匹配，执行阻断、模拟响应、增改请求 / 响应首部；应用后自动重启引擎。
- **持久化**：请求完成后写入 SQLite，重启恢复历史记录、高亮与备注；设置页可查看数据库路径、记录数与清空历史。
- **设置页**：监听端口、上游代理、正文上限、HTTPS 解密、系统代理接管、跟随系统 / 浅色 / 深色外观、根证书安装与导出。
- **导出**：工具栏的导出按钮把当前筛选 / 排序结果写成 `~/Downloads/packet-sniffer-requests-<时间>.json`，界面顶部给出结果提示。
- **快捷键**：`⌘F` 聚焦筛选框，`⌘K` 清空请求。

### 7. 链式代理与企业内网
**默认直连**（`upstream_proxy = None`）。需要时再显式开启链式代理：上游还有一层代理
（企业代理、MonoProxy / Clash 这类本地代理客户端）时，用 `--upstream-proxy` 让引擎先经它建隧道：

```bash
cargo run -p desktop -- run --upstream-proxy 127.0.0.1:8118
```

引擎会用 `CONNECT host:port` 在上游代理上打通隧道，再在隧道里做 TLS；
配置项对应 `EngineConfig::with_upstream_proxy(Some(UpstreamProxy))`，支持 `http://user:pass@host:port` 形式的内嵌认证。

**上游证书是自签的**（企业 MITM）时，用 `EngineConfig::with_upstream_extra_root(Some(pem_path))`
把企业根证书加入上游信任库，无需关闭证书校验。

> 注意：如果本机已有代理客户端（如 MonoProxy）在持续改写系统代理，需要先在它的设置里关掉
> 「设为系统代理」开关（保持它监听 8118 即可），再把系统代理指向本引擎的 8899，才能抓全系统流量。
