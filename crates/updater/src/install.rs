//! 把下载好的更新落到本机：解压、替换、重启。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(windows))]
use std::process::Stdio;

use crate::{Error, Outcome, Result};

/// 系统安装包的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerKind {
    Dmg,
    Nsis,
    AppImage,
}

impl InstallerKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "dmg" => Some(Self::Dmg),
            "nsis" | "msi" | "exe" => Some(Self::Nsis),
            "appimage" => Some(Self::AppImage),
            _ => None,
        }
    }
}

impl std::fmt::Display for InstallerKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            InstallerKind::Dmg => "dmg",
            InstallerKind::Nsis => "nsis",
            InstallerKind::AppImage => "appimage",
        })
    }
}

/// 当前进程是否跑在 macOS 的 `.app` 里。这种情况不能用绿色版覆盖二进制，
/// 否则会破坏包签名，应该交给 `.dmg` 安装包。
pub fn is_macos_app_bundle(exe: &Path) -> bool {
    exe.ancestors().any(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    })
}

/// 安装目录是否可写。系统包管理安装的 `/usr/bin`、`Program Files` 通常不可写。
pub fn can_replace_in_place(exe: &Path) -> bool {
    let Some(directory) = exe.parent() else {
        return false;
    };
    let probe = directory.join(format!(".packet-sniffer-write-{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn binary_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["packet-sniffer.exe", "packet-sniffer-ui.exe"]
    }
    #[cfg(not(windows))]
    {
        &["packet-sniffer", "packet-sniffer-ui"]
    }
}

fn ui_binary_name() -> &'static str {
    if cfg!(windows) {
        "packet-sniffer-ui.exe"
    } else {
        "packet-sniffer-ui"
    }
}

/// 解压绿色版压缩包并替换当前可执行文件所在目录里的二进制。
pub fn replace_from_archive(archive: &Path, relaunch: bool) -> Result<Outcome> {
    let exe = std::env::current_exe()
        .map_err(|error| Error::Install(format!("无法定位当前可执行文件：{error}")))?;
    let target = exe
        .parent()
        .ok_or_else(|| Error::Install("当前可执行文件没有父目录".to_owned()))?
        .to_path_buf();
    if is_macos_app_bundle(&exe) {
        return Err(Error::Install(
            "当前运行的是 .app 包，不能覆盖内部二进制".to_owned(),
        ));
    }
    let staging = archive
        .parent()
        .ok_or_else(|| Error::Install("下载目录无效".to_owned()))?
        .join("staging");
    let extracted = extract(archive, &staging)?;
    let current_name = exe
        .file_name()
        .ok_or_else(|| Error::Install("当前可执行文件名无效".to_owned()))?;
    if !extracted
        .iter()
        .any(|path| path.file_name() == Some(current_name))
    {
        return Err(Error::Install(format!(
            "压缩包里没有当前可执行文件 {}",
            current_name.to_string_lossy()
        )));
    }

    #[cfg(windows)]
    {
        schedule_windows(&target, &staging, &extracted, relaunch)
    }
    #[cfg(not(windows))]
    {
        replace_unix(&target, &extracted, relaunch)
    }
}

/// 替换正在运行的 AppImage，然后按需重新启动。
pub fn replace_appimage(downloaded: &Path, relaunch: bool) -> Result<Outcome> {
    let appimage = std::env::var_os("APPIMAGE")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| Error::Install("APPIMAGE 环境变量缺失".to_owned()))?;
    let staged = with_suffix(&appimage, ".new");
    let backup = with_suffix(&appimage, ".old");
    fs::copy(downloaded, &staged)?;
    make_executable(&staged)?;
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    fs::rename(&appimage, &backup)?;
    if let Err(error) = fs::rename(&staged, &appimage) {
        let _ = fs::rename(&backup, &appimage);
        let _ = fs::remove_file(&staged);
        return Err(error.into());
    }
    if relaunch {
        if let Err(error) = relaunch_after_exit(&appimage, std::slice::from_ref(&backup)) {
            let _ = fs::rename(&appimage, &staged);
            let _ = fs::rename(&backup, &appimage);
            return Err(Error::Install(format!("无法启动新版本：{error}")));
        }
    } else {
        let _ = fs::remove_file(&backup);
    }
    Ok(Outcome::Replaced)
}

/// 启动系统安装包。
pub fn launch_installer(path: &Path, kind: InstallerKind) -> Result<Outcome> {
    let result = match kind {
        InstallerKind::Dmg => Command::new("open").arg(path).spawn(),
        InstallerKind::Nsis => {
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("msi"))
            {
                Command::new("msiexec").arg("/i").arg(path).spawn()
            } else {
                Command::new(path).spawn()
            }
        }
        InstallerKind::AppImage => {
            make_executable(path)?;
            Command::new(path).spawn()
        }
    };
    result
        .map(|_| Outcome::InstallerLaunched)
        .map_err(|error| Error::Install(format!("无法启动安装程序 {}：{error}", path.display())))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn extract(archive: &Path, staging: &Path) -> Result<Vec<PathBuf>> {
    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::create_dir_all(staging)?;
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extracted = if name.ends_with(".zip") {
        extract_zip(archive, staging)?
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let decoder = flate2::read::GzDecoder::new(fs::File::open(archive)?);
        extract_tar(decoder, staging)?
    } else if name.ends_with(".tar") {
        extract_tar(fs::File::open(archive)?, staging)?
    } else {
        return Err(Error::Install(format!(
            "不认识的更新包格式：{}",
            archive.display()
        )));
    };
    if extracted.is_empty() {
        return Err(Error::Install("更新包里没有可执行文件".to_owned()));
    }
    Ok(extracted)
}

fn wanted(name: &str) -> bool {
    binary_names().contains(&name)
}

fn extract_tar<R: io::Read>(reader: R, staging: &Path) -> Result<Vec<PathBuf>> {
    let mut archive = tar::Archive::new(reader);
    let mut extracted = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?;
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !wanted(name) {
            continue;
        }
        let destination = staging.join(name);
        entry.unpack(&destination)?;
        extracted.push(destination);
    }
    Ok(extracted)
}

fn extract_zip(archive: &Path, staging: &Path) -> Result<Vec<PathBuf>> {
    let file = fs::File::open(archive)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| Error::Install(format!("无法读取 zip：{error}")))?;
    let mut extracted = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::Install(format!("无法读取 zip 条目：{error}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().ok().and_then(|name| {
            Path::new(name.as_ref())
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        });
        let Some(name) = name else {
            continue;
        };
        if !wanted(&name) {
            continue;
        }
        let destination = staging.join(&name);
        let mut output = fs::File::create(&destination)?;
        io::copy(&mut entry, &mut output)?;
        make_executable(&destination)?;
        extracted.push(destination);
    }
    Ok(extracted)
}

#[cfg(not(windows))]
fn replace_unix(target: &Path, extracted: &[PathBuf], relaunch: bool) -> Result<Outcome> {
    let mut installed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for source in extracted {
        let name = source
            .file_name()
            .ok_or_else(|| Error::Install("更新包里的文件名无效".to_owned()))?;
        let destination = target.join(name);
        let staged = target.join(format!(".{}.new", name.to_string_lossy()));
        let backup = target.join(format!(".{}.old", name.to_string_lossy()));

        fs::copy(source, &staged)?;
        make_executable(&staged)?;
        if backup.exists() {
            let _ = fs::remove_file(&backup);
        }
        let had_old = destination.exists();
        if had_old {
            fs::rename(&destination, &backup)?;
        }
        if let Err(error) = fs::rename(&staged, &destination) {
            let _ = fs::remove_file(&staged);
            if had_old {
                let _ = fs::rename(&backup, &destination);
            }
            rollback(&installed);
            return Err(error.into());
        }
        installed.push((destination, backup));
    }

    if relaunch {
        let ui = target.join(ui_binary_name());
        let backups: Vec<PathBuf> = installed.iter().map(|(_, backup)| backup.clone()).collect();
        if let Err(error) = relaunch_after_exit(&ui, &backups) {
            rollback(&installed);
            return Err(Error::Install(format!("无法启动新版本：{error}")));
        }
    } else {
        for (_, backup) in &installed {
            let _ = fs::remove_file(backup);
        }
    }
    Ok(Outcome::Replaced)
}

/// 等本进程退出后再拉起新版本：新界面要绑定同一个代理端口，提前启动会撞端口。
/// 新进程运行 3 秒后删掉备份；若它立刻退出，备份留着供手动恢复。
#[cfg(windows)]
fn relaunch_after_exit(ui: &Path, _backups: &[PathBuf]) -> Result<()> {
    Command::new(ui)
        .spawn()
        .map(|_| ())
        .map_err(|error| Error::Install(format!("无法启动新版本：{error}")))
}

#[cfg(not(windows))]
fn relaunch_after_exit(ui: &Path, backups: &[PathBuf]) -> Result<()> {
    const SCRIPT: &str = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
"$2" &
child=$!
sleep 3
if kill -0 "$child" 2>/dev/null; then
  shift 2
  for backup in "$@"; do rm -f "$backup"; done
fi
wait "$child""#;

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(SCRIPT)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(ui)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for backup in backups {
        command.arg(backup);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| Error::Install(format!("无法启动新版本：{error}")))
}

#[cfg(not(windows))]
fn rollback(installed: &[(PathBuf, PathBuf)]) {
    for (destination, backup) in installed.iter().rev() {
        if backup.exists() {
            let _ = fs::rename(backup, destination);
        }
    }
}

#[cfg(windows)]
fn schedule_windows(
    target: &Path,
    staging: &Path,
    extracted: &[PathBuf],
    relaunch: bool,
) -> Result<Outcome> {
    let script =
        std::env::temp_dir().join(format!("packet-sniffer-update-{}.cmd", std::process::id()));
    let update_dir = staging
        .parent()
        .ok_or_else(|| Error::Install("下载目录无效".to_owned()))?;
    let mut body = String::from("@echo off\r\nsetlocal\r\n");
    body.push_str(&format!("set \"WAIT_PID={}\"\r\n", std::process::id()));
    body.push_str(&format!("set \"TARGET={}\"\r\n", target.display()));
    body.push_str(&format!("set \"STAGE={}\"\r\n", staging.display()));
    body.push_str(":wait\r\n");
    body.push_str("tasklist /FI \"PID eq %WAIT_PID%\" /NH 2>nul | find /I \"%WAIT_PID%\" >nul\r\n");
    body.push_str("if not errorlevel 1 (\r\n  >nul ping -n 2 127.0.0.1\r\n  goto wait\r\n)\r\n");
    body.push_str("set \"FAILED=0\"\r\n");
    for source in extracted {
        if let Some(name) = source.file_name().and_then(|name| name.to_str()) {
            body.push_str(&format!("call :replace \"{name}\"\r\n"));
        }
    }
    body.push_str("if \"%FAILED%\"==\"1\" goto rollback\r\n");
    if relaunch {
        body.push_str("start \"\" \"%TARGET%\\");
        body.push_str(ui_binary_name());
        body.push_str("\"\r\n");
    }
    body.push_str("goto cleanup\r\n");
    body.push_str(":replace\r\n");
    body.push_str("if not exist \"%STAGE%\\%~1\" exit /b 0\r\n");
    body.push_str("if exist \"%TARGET%\\%~1.old\" del /F /Q \"%TARGET%\\%~1.old\" >nul 2>&1\r\n");
    body.push_str(
        "if exist \"%TARGET%\\%~1\" move /Y \"%TARGET%\\%~1\" \"%TARGET%\\%~1.old\" >nul 2>&1\r\n",
    );
    body.push_str("copy /Y \"%STAGE%\\%~1\" \"%TARGET%\\%~1\" >nul 2>&1\r\n");
    body.push_str("if errorlevel 1 set \"FAILED=1\"\r\n");
    body.push_str("exit /b 0\r\n");
    body.push_str(":rollback\r\n");
    for name in binary_names() {
        body.push_str(&format!(
            "if exist \"%TARGET%\\{name}.old\" move /Y \"%TARGET%\\{name}.old\" \"%TARGET%\\{name}\" >nul 2>&1\r\n"
        ));
    }
    body.push_str(":cleanup\r\n");
    body.push_str(">nul ping -n 3 127.0.0.1\r\n");
    body.push_str(&format!(
        "rd /S /Q \"{}\" >nul 2>&1\r\n",
        update_dir.display()
    ));
    body.push_str("del /F /Q \"%~f0\" >nul 2>&1\r\n");
    fs::write(&script, body)?;

    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("cmd")
        .arg("/C")
        .arg(&script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| Error::Install(format!("无法安排更新：{error}")))?;
    Ok(Outcome::Scheduled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn fixtures() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        let file = fs::File::create(directory.path().join("app.tar.gz")).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        for name in binary_names() {
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("./{name}"), &b"test"[..])
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
        directory
    }

    #[test]
    fn tar_archive_yields_only_the_binaries() {
        let directory = fixtures();
        let staging = directory.path().join("staging");
        let extracted = extract(&directory.path().join("app.tar.gz"), &staging).unwrap();
        assert_eq!(extracted.len(), binary_names().len());
        for name in binary_names() {
            assert!(staging.join(name).exists(), "{name} 未解压");
        }
    }

    #[test]
    fn path_traversal_entries_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("evil.tar.gz");
        let file = fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.as_old_mut().name[..13].copy_from_slice(b"../../evil.sh");
        header.set_cksum();
        builder.append(&header, &b"test"[..]).unwrap();

        let name = binary_names()[0];
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_path(name).unwrap();
        header.set_cksum();
        builder.append(&header, &b"test"[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let staging = directory.path().join("staging");
        let extracted = extract(&archive, &staging).unwrap();
        assert_eq!(extracted.len(), 1);
        assert!(staging.join(name).exists());
        assert!(!directory.path().join("evil.sh").exists());
    }

    #[test]
    fn zip_archive_yields_only_the_binaries() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("app.zip");
        let file = fs::File::create(&archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for name in binary_names() {
            writer.start_file(name, options).unwrap();
            writer.write_all(b"test").unwrap();
        }
        writer.finish().unwrap();

        let staging = directory.path().join("staging");
        let extracted = extract(&archive, &staging).unwrap();
        assert_eq!(extracted.len(), binary_names().len());
    }

    #[test]
    fn macos_app_bundle_is_detected() {
        assert!(is_macos_app_bundle(Path::new(
            "/Applications/Packet Sniffer.app/Contents/MacOS/packet-sniffer-ui"
        )));
        assert!(!is_macos_app_bundle(Path::new(
            "/Users/me/Downloads/packet-sniffer-ui"
        )));
    }

    #[cfg(not(windows))]
    #[test]
    fn replace_unix_swaps_files_and_drops_backups() {
        let source = fixtures();
        let staging = source.path().join("staging");
        let extracted = extract(&source.path().join("app.tar.gz"), &staging).unwrap();

        let target = tempfile::tempdir().unwrap();
        for name in binary_names() {
            fs::write(target.path().join(name), b"old").unwrap();
        }
        replace_unix(target.path(), &extracted, false).unwrap();
        for name in binary_names() {
            assert_eq!(fs::read(target.path().join(name)).unwrap(), b"test");
            assert!(!target.path().join(format!(".{name}.old")).exists());
        }

        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(target.path().join(binary_names()[0]))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "替换后的文件应可执行");
    }

    #[test]
    fn writable_directory_is_detected() {
        let directory = tempfile::tempdir().unwrap();
        let exe = directory.path().join("packet-sniffer-ui");
        fs::write(&exe, b"").unwrap();
        assert!(can_replace_in_place(&exe));
    }
}
