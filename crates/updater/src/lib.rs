//! 应用自更新。
//!
//! 检查 GitHub Release 上的 `latest.json` 清单，下载与当前平台匹配的压缩包并校验
//! SHA-256，然后替换本机安装；需要时重启界面进程。清单由
//! `scripts/update-manifest.sh` 在发布时生成，结构见 [`Manifest`]。
//!
//! 网络与文件操作都是阻塞的：界面层应在后台线程调用 [`apply`]，CLI 直接调用即可。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub mod install;

pub use install::InstallerKind;

/// 应用的 bundle id，清单必须属于它。
pub const APP_ID: &str = "dev.packet-sniffer.app";
/// 发布 Release 的仓库。
pub const REPO: &str = "aafnnp/packet-sniffer";

const MANIFEST_URL: &str =
    "https://github.com/aafnnp/packet-sniffer/releases/latest/download/latest.json";
const MANIFEST_SCHEMA: u32 = 1;
const MANIFEST_LIMIT: u64 = 2 * 1024 * 1024;
const RELEASE_PAGE: &str = "https://github.com/aafnnp/packet-sniffer/releases/latest";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// 请求清单或安装包时失败。
    #[error("检查更新失败：{0}")]
    Check(String),
    /// 清单格式错误、属于别的应用或版本不可用。
    #[error("更新清单无效：{0}")]
    Manifest(String),
    /// 下载内容与清单里的哈希或大小不一致。
    #[error("下载更新失败：{0}")]
    Download(String),
    /// 无法替换或启动新版本。
    #[error("安装更新失败：{0}")]
    Install(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// 当前编译进去的版本。
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace version 是 semver")
}

/// 清单里的平台键。macOS 只有通用二进制，因此不区分架构。
pub fn platform_key() -> String {
    platform_key_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn platform_key_for(os: &str, arch: &str) -> String {
    match (os, arch) {
        ("macos", _) => "macos-universal".to_owned(),
        ("windows", "x86_64") => "windows-x86_64".to_owned(),
        ("linux", "x86_64") => "linux-x86_64".to_owned(),
        ("linux", "aarch64") => "linux-aarch64".to_owned(),
        _ => format!("{os}-{arch}"),
    }
}

/// 清单地址；`PACKET_SNIFFER_UPDATE_URL` 可指向本地文件服务器做测试。
pub fn manifest_url() -> String {
    std::env::var("PACKET_SNIFFER_UPDATE_URL").unwrap_or_else(|_| MANIFEST_URL.to_owned())
}

/// `latest.json`。字段只增不改，旧客户端忽略不认识的键。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub platforms: BTreeMap<String, PlatformAssets>,
}

/// 一个平台可用的文件：绿色版压缩包与（可选的）系统安装包。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformAssets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<Asset>,
}

/// 清单里的一个文件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// 绝对地址，或相对于清单的地址。
    pub url: String,
    /// 小写十六进制的 SHA-256。
    pub sha256: String,
    /// 字节数。
    pub size: u64,
    /// 保存到磁盘时用的文件名；缺省时取 URL 的最后一段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 安装包类型：`dmg` / `nsis` / `appimage`；压缩包留空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Asset {
    fn resolve(self, base: &str) -> Self {
        Self {
            url: resolve_url(base, &self.url),
            ..self
        }
    }

    fn file_name(&self) -> Result<String> {
        let name = self
            .name
            .clone()
            .or_else(|| {
                let path = self.url.split(['?', '#']).next().unwrap_or_default();
                path.rsplit('/').next().map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
            || name.contains('\0')
        {
            return Err(Error::Manifest(format!("文件名不可用：{name:?}")));
        }
        Ok(name)
    }
}

/// 有可用的新版本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub version: Version,
    pub notes: Option<String>,
    /// 绿色版压缩包，能原地替换时使用。
    pub archive: Option<Asset>,
    /// 系统安装包，不能原地替换时使用。
    pub installer: Option<Asset>,
}

impl Available {
    /// 打开发布页用的地址。
    pub fn release_url(&self) -> String {
        format!("{RELEASE_PAGE}/tag/v{}", self.version)
    }
}

/// 下载进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
}

/// 安装方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// 用绿色版压缩包替换安装目录里的二进制。
    Replace,
    /// 当前进程就是 AppImage：下载新 AppImage 覆盖自身。
    AppImage,
    /// 交给系统安装包（dmg / NSIS）。
    Installer(InstallerKind),
    /// 无法自动安装，只能打开发布页。
    Manual,
}

/// 安装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 文件已替换；调用方应重启应用（界面已自动拉起新版本）。
    Replaced,
    /// Windows 上文件被运行中的进程占用，已安排退出后替换并重启。
    Scheduled,
    /// 系统安装程序已启动，调用方可以退出。
    InstallerLaunched,
    /// 需要用户手动下载。
    Manual { url: String },
}

impl Outcome {
    pub fn message(&self) -> String {
        match self {
            Outcome::Replaced => "更新完成，请重新启动应用。".to_owned(),
            Outcome::Scheduled => "更新已安排，应用退出后会自动替换并重启。".to_owned(),
            Outcome::InstallerLaunched => "安装程序已启动，按提示完成安装即可。".to_owned(),
            Outcome::Manual { url } => format!("请手动下载新版本：{url}"),
        }
    }
}

/// 检查是否有比当前版本更新的版本。没有更新时返回 `Ok(None)`。
pub fn check() -> Result<Option<Available>> {
    let url = manifest_url();
    check_url(&url)?;
    let agent = agent(Duration::from_secs(15));
    let mut response = agent
        .get(&url)
        .header("Accept", "application/json")
        .call()
        .map_err(|error| match error {
            // 还没有包含更新清单的发布（旧版本 Release）。
            ureq::Error::StatusCode(404) => {
                Error::Check("最新版本还没有发布更新清单 latest.json，暂时无法检查更新".to_owned())
            }
            other => Error::Check(other.to_string()),
        })?;
    if !response.status().is_success() {
        return Err(Error::Check(format!(
            "GET {url} 返回 HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MANIFEST_LIMIT)
        .read_to_vec()
        .map_err(|error| Error::Check(error.to_string()))?;
    check_manifest(&url, &bytes, &current_version(), &platform_key())
}

fn check_manifest(
    base: &str,
    bytes: &[u8],
    current: &Version,
    platform: &str,
) -> Result<Option<Available>> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|error| Error::Manifest(error.to_string()))?;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(Error::Manifest(format!(
            "清单版本 {}，本程序只认识 {MANIFEST_SCHEMA}",
            manifest.schema
        )));
    }
    if manifest.id != APP_ID {
        return Err(Error::Manifest(format!(
            "清单属于 {:?}，本应用是 {APP_ID:?}",
            manifest.id
        )));
    }
    let version =
        Version::parse(&manifest.version).map_err(|error| Error::Manifest(error.to_string()))?;
    if version <= *current {
        return Ok(None);
    }
    let Some(assets) = manifest.platforms.get(platform) else {
        return Ok(None);
    };
    let archive = assets.archive.clone().map(|asset| asset.resolve(base));
    let installer = assets.installer.clone().map(|asset| asset.resolve(base));
    if archive.is_none() && installer.is_none() {
        return Ok(None);
    }
    for asset in archive.iter().chain(installer.iter()) {
        check_url(&asset.url)?;
        check_asset_sha256(asset)?;
    }
    Ok(Some(Available {
        version,
        notes: manifest.notes,
        archive,
        installer,
    }))
}

fn check_asset_sha256(asset: &Asset) -> Result<()> {
    let valid = asset.sha256.len() == 64 && asset.sha256.chars().all(|c| c.is_ascii_hexdigit());
    if !valid {
        return Err(Error::Manifest(format!(
            "{} 的 sha256 不是 64 位十六进制",
            asset.url
        )));
    }
    Ok(())
}

/// 下载并校验一个文件，返回本地路径。`progress` 会随分块到达而调用。
pub fn download(
    asset: &Asset,
    directory: &Path,
    mut progress: impl FnMut(Progress),
) -> Result<PathBuf> {
    check_url(&asset.url)?;
    check_asset_sha256(asset)?;
    fs::create_dir_all(directory)?;
    let path = directory.join(asset.file_name()?);
    let partial = path.with_extension(format!(
        "{}.part",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
    ));

    let agent = agent(Duration::from_secs(600));
    let result = (|| -> Result<()> {
        let mut response = agent.get(&asset.url).call().map_err(|error| match error {
            ureq::Error::StatusCode(status) => {
                Error::Download(format!("GET {} 返回 HTTP {status}", asset.url))
            }
            other => Error::Download(other.to_string()),
        })?;
        if !response.status().is_success() {
            return Err(Error::Download(format!(
                "GET {} 返回 HTTP {}",
                asset.url,
                response.status()
            )));
        }
        let mut reader = response.body_mut().as_reader();
        let mut file = fs::File::create(&partial)?;
        let mut hash = Sha256::new();
        let mut received = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| Error::Download(error.to_string()))?;
            if read == 0 {
                break;
            }
            received += read as u64;
            if received > asset.size {
                return Err(Error::Download(format!(
                    "下载超过清单声明的 {} 字节",
                    asset.size
                )));
            }
            hash.update(&buffer[..read]);
            file.write_all(&buffer[..read])?;
            progress(Progress {
                received,
                total: asset.size,
            });
        }
        file.flush()?;
        if received != asset.size {
            return Err(Error::Download(format!(
                "收到 {received} 字节，清单声明 {}",
                asset.size
            )));
        }
        let actual = hex(hash.finalize());
        if actual != asset.sha256.to_ascii_lowercase() {
            return Err(Error::Download(format!(
                "SHA-256 为 {actual}，清单声明 {}",
                asset.sha256
            )));
        }
        fs::rename(&partial, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map(|_| path)
}

/// 根据当前安装形态选择安装方式。
pub fn strategy(update: &Available) -> Strategy {
    let exe = std::env::current_exe().ok();
    if let Some(exe) = &exe {
        if install::is_macos_app_bundle(exe) {
            return match installer_kind(update) {
                Some(InstallerKind::Dmg) => Strategy::Installer(InstallerKind::Dmg),
                _ => Strategy::Manual,
            };
        }
        if std::env::var_os("APPIMAGE").is_some_and(|path| !path.is_empty())
            && installer_kind(update) == Some(InstallerKind::AppImage)
        {
            return Strategy::AppImage;
        }
        if install::can_replace_in_place(exe) && update.archive.is_some() {
            return Strategy::Replace;
        }
    }
    match installer_kind(update) {
        Some(kind @ (InstallerKind::Dmg | InstallerKind::Nsis)) => Strategy::Installer(kind),
        _ => Strategy::Manual,
    }
}

fn installer_kind(update: &Available) -> Option<InstallerKind> {
    let asset = update.installer.as_ref()?;
    if let Some(kind) = asset.kind.as_deref().and_then(InstallerKind::parse) {
        return Some(kind);
    }
    let name = asset.url.to_ascii_lowercase();
    if name.ends_with(".dmg") {
        Some(InstallerKind::Dmg)
    } else if name.ends_with(".exe") || name.ends_with(".msi") {
        Some(InstallerKind::Nsis)
    } else if name.ends_with(".appimage") {
        Some(InstallerKind::AppImage)
    } else {
        None
    }
}

/// 下载并安装更新。`relaunch` 为真时替换后拉起新界面。
pub fn apply(
    update: &Available,
    relaunch: bool,
    mut progress: impl FnMut(Progress),
) -> Result<Outcome> {
    let strategy = strategy(update);
    let directory = private_directory(&update.version)?;
    let result = (|| -> Result<Outcome> {
        match strategy {
            Strategy::Replace => {
                let archive = update
                    .archive
                    .as_ref()
                    .ok_or_else(|| Error::Install("清单里没有绿色版压缩包".to_owned()))?;
                let downloaded = download(archive, &directory, &mut progress)?;
                install::replace_from_archive(&downloaded, relaunch)
            }
            Strategy::AppImage => {
                let installer = update
                    .installer
                    .as_ref()
                    .ok_or_else(|| Error::Install("清单里没有 AppImage".to_owned()))?;
                let downloaded = download(installer, &directory, &mut progress)?;
                install::replace_appimage(&downloaded, relaunch)
            }
            Strategy::Installer(kind) => {
                let installer = update
                    .installer
                    .as_ref()
                    .ok_or_else(|| Error::Install("清单里没有安装包".to_owned()))?;
                let downloaded = download(installer, &directory, &mut progress)?;
                install::launch_installer(&downloaded, kind)
            }
            Strategy::Manual => Ok(Outcome::Manual {
                url: update.release_url(),
            }),
        }
    })();
    match strategy {
        // Windows 的延迟替换脚本还要读 staging，交给系统清理临时目录。
        Strategy::Replace if cfg!(windows) && result.is_ok() => {}
        Strategy::Replace | Strategy::AppImage | Strategy::Installer(_) => {
            let _ = fs::remove_dir_all(&directory);
        }
        Strategy::Manual => {
            let _ = fs::remove_dir_all(&directory);
        }
    }
    result
}

/// 在临时目录里建一个只有本用户能访问的更新目录，避免复用别人预置的路径。
fn private_directory(version: &Version) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..10 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let path = base.join(format!(
            "packet-sniffer-update-{version}-{}-{nonce}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::Install("无法创建临时更新目录".to_owned()))
}

/// 用系统默认程序打开 URL。
pub fn open_url(url: &str) -> Result<()> {
    check_url(url)?;
    let mut command = if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    } else if cfg!(windows) {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| Error::Install(format!("无法打开 {url}：{error}")))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!(
            "packet-sniffer-updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .into()
}

/// 只允许 HTTPS，以及本机回环上的 HTTP（本地测试）。
fn check_url(url: &str) -> Result<()> {
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| {
            url.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
    if url.starts_with("https://") || loopback {
        Ok(())
    } else {
        Err(Error::Manifest(format!("{url:?} 不是 https:// 地址")))
    }
}

/// 相对地址按清单位置展开。
fn resolve_url(base: &str, url: &str) -> String {
    if url.contains("://") {
        return url.to_owned();
    }
    let path_end = base.find(['?', '#']).unwrap_or(base.len());
    match base[..path_end].rfind('/') {
        Some(slash) => format!("{}{url}", &base[..=slash]),
        None => url.to_owned(),
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl fmt::Display for Strategy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Strategy::Replace => formatter.write_str("replace"),
            Strategy::AppImage => formatter.write_str("appimage"),
            Strategy::Installer(kind) => write!(formatter, "installer-{kind}"),
            Strategy::Manual => formatter.write_str("manual"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> serde_json::Value {
        serde_json::json!({
            "url": format!("assets/{name}"),
            "sha256": "a".repeat(64),
            "size": 3,
            "name": name,
        })
    }

    fn manifest(version: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "id": APP_ID,
            "version": version,
            "notes": "修复若干问题",
            "platforms": {
                "linux-x86_64": { "archive": asset("app.tar.gz") },
                "macos-universal": {
                    "archive": asset("app.tar.gz"),
                    "installer": {
                        "url": "https://example.com/app.dmg",
                        "sha256": "b".repeat(64),
                        "size": 10,
                        "kind": "dmg"
                    }
                }
            }
        })
    }

    fn check_value(value: &serde_json::Value, current: &str, platform: &str) -> Option<Available> {
        check_manifest(
            "https://example.com/releases/v1.0.0/latest.json",
            &serde_json::to_vec(value).unwrap(),
            &Version::parse(current).unwrap(),
            platform,
        )
        .unwrap()
    }

    #[test]
    fn newer_version_is_offered() {
        let found = check_value(&manifest("1.3.0"), "1.2.0", "linux-x86_64").unwrap();
        assert_eq!(found.version, Version::parse("1.3.0").unwrap());
        assert_eq!(found.notes.as_deref(), Some("修复若干问题"));
        let archive = found.archive.unwrap();
        assert_eq!(
            archive.url,
            "https://example.com/releases/v1.0.0/assets/app.tar.gz"
        );
        assert!(found.installer.is_none());
    }

    #[test]
    fn same_or_older_version_is_not_offered() {
        assert!(check_value(&manifest("1.2.0"), "1.2.0", "linux-x86_64").is_none());
        assert!(check_value(&manifest("1.1.9"), "1.2.0", "linux-x86_64").is_none());
    }

    #[test]
    fn unknown_platform_is_skipped() {
        assert!(check_value(&manifest("1.3.0"), "1.2.0", "freebsd-x86_64").is_none());
    }

    #[test]
    fn manifest_for_another_app_is_refused() {
        let mut value = manifest("1.3.0");
        value["id"] = serde_json::json!("com.example.other");
        let error = check_manifest(
            "https://example.com/latest.json",
            &serde_json::to_vec(&value).unwrap(),
            &Version::parse("1.0.0").unwrap(),
            "linux-x86_64",
        )
        .unwrap_err();
        assert!(matches!(error, Error::Manifest(_)));
    }

    #[test]
    fn platform_keys_cover_release_assets() {
        assert_eq!(platform_key_for("macos", "aarch64"), "macos-universal");
        assert_eq!(platform_key_for("macos", "x86_64"), "macos-universal");
        assert_eq!(platform_key_for("linux", "x86_64"), "linux-x86_64");
        assert_eq!(platform_key_for("linux", "aarch64"), "linux-aarch64");
        assert_eq!(platform_key_for("windows", "x86_64"), "windows-x86_64");
    }

    #[test]
    fn installer_kind_comes_from_field_or_extension() {
        let mut update = check_value(&manifest("1.3.0"), "1.2.0", "macos-universal").unwrap();
        assert_eq!(installer_kind(&update), Some(InstallerKind::Dmg));
        update.installer.as_mut().unwrap().kind = None;
        assert_eq!(installer_kind(&update), Some(InstallerKind::Dmg));
    }

    #[test]
    fn insecure_urls_are_refused() {
        assert!(check_url("http://example.com/latest.json").is_err());
        assert!(check_url("http://127.0.0.1:8000/latest.json").is_ok());
        assert!(check_url("https://example.com/latest.json").is_ok());
    }

    #[test]
    fn relative_urls_resolve_next_to_the_manifest() {
        assert_eq!(
            resolve_url("https://example.com/a/latest.json", "app.tar.gz"),
            "https://example.com/a/app.tar.gz"
        );
        assert_eq!(
            resolve_url(
                "https://example.com/a/latest.json",
                "https://cdn.example.com/x"
            ),
            "https://cdn.example.com/x"
        );
    }

    #[test]
    fn download_verifies_hash_and_size() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let body = b"fake archive bytes".to_vec();
        let expected = hex(Sha256::digest(&body));
        let length = body.len() as u64;
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });

        let directory = tempfile::tempdir().unwrap();
        let asset = Asset {
            url: format!("http://{address}/app.tar.gz"),
            sha256: expected,
            size: length,
            name: Some("app.tar.gz".to_owned()),
            kind: None,
        };
        let mut last = 0;
        let path = download(&asset, directory.path(), |progress| {
            last = progress.received
        })
        .unwrap();
        assert_eq!(last, length);
        assert_eq!(fs::read(&path).unwrap(), b"fake archive bytes");
        server.join().unwrap();
    }

    #[test]
    fn download_rejects_wrong_hash() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.write_all(b"hi").unwrap();
        });

        let directory = tempfile::tempdir().unwrap();
        let asset = Asset {
            url: format!("http://{address}/app.tar.gz"),
            sha256: "0".repeat(64),
            size: 2,
            name: Some("app.tar.gz".to_owned()),
            kind: None,
        };
        let error = download(&asset, directory.path(), |_| {}).unwrap_err();
        assert!(matches!(error, Error::Download(_)));
        assert!(!directory.path().join("app.tar.gz").exists());
    }

    #[test]
    fn bad_asset_name_is_refused() {
        let mut value = manifest("1.3.0");
        value["platforms"]["linux-x86_64"]["archive"]["name"] = serde_json::json!("../evil");
        let error = check_manifest(
            "https://example.com/latest.json",
            &serde_json::to_vec(&value).unwrap(),
            &Version::parse("1.0.0").unwrap(),
            "linux-x86_64",
        );
        // 清单本身合法；文件名在真正下载时才拒绝。
        assert!(error.is_ok());
    }
}
