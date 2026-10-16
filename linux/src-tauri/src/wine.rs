use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

use crate::settings::Settings;

// wow64: the plain amd64 build execs /lib/ld-linux.so.2, missing on this system
const WINE_URL: &str = "https://github.com/Kron4ek/Wine-Builds/releases/download/11.19/wine-11.19-staging-amd64-wow64.tar.xz";
const WINE_SHA: &str = "c89f69cf22f6664e3791210fa3b40c6e9e6d2e27a9aaa466f5a26d9c56664a09";
const WINE_DIR: &str = "wine-11.19-staging-amd64-wow64";
const DXVK_URL: &str =
    "https://github.com/doitsujin/dxvk/releases/download/v3.1.1/dxvk-3.1.1.tar.gz";
const DXVK_SHA: &str = "40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608";
const DXVK_VERSION: &str = "3.1.1";
const XZ_MAGIC: &[u8] = &[0xfd, b'7', b'z', b'X', b'Z', 0x00];
const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b];
const ZIP_MAGIC: &[u8] = &[b'P', b'K', 3, 4];
const ZST_MAGIC: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd];

static DOWNLOAD: Mutex<()> = Mutex::new(());

pub(crate) enum ArchiveKind {
    Xz,
    Gzip,
    Zip,
    Zst,
}

pub(crate) fn lock_download() -> MutexGuard<'static, ()> {
    DOWNLOAD.lock().unwrap_or_else(|err| err.into_inner())
}

fn tool_root(name: &str) -> PathBuf {
    crate::paths::root().join(name)
}

fn wine_bin() -> PathBuf {
    tool_root("wine").join(WINE_DIR).join("bin/wine")
}

pub fn installed() -> bool {
    wine_bin().is_file()
}

fn dxvk_dlls() -> PathBuf {
    tool_root("dxvk")
        .join(format!("dxvk-{DXVK_VERSION}"))
        .join("x64")
}

pub fn binary(progress: &dyn Fn(&str, u8)) -> Result<PathBuf, String> {
    if wine_bin().is_file() {
        return Ok(wine_bin());
    }
    let _guard = lock_download();
    let bin = wine_bin();
    if bin.is_file() {
        return Ok(bin);
    }
    let root = tool_root("wine");
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let archive = root.join("wine.tar.xz");
    save_download(
        WINE_URL,
        &archive,
        "A baixar o Wine",
        ArchiveKind::Xz,
        Some(WINE_SHA),
        "Wine",
        progress,
    )?;
    progress("A extrair o Wine", 0);
    extract(&archive, &root, ArchiveKind::Xz)?;
    let _ = fs::remove_file(&archive);
    progress("A extrair o Wine", 100);
    if bin.is_file() {
        Ok(bin)
    } else {
        Err("Wine em falta depois de extrair".to_string())
    }
}

pub fn prepare_prefix(
    wine_bin: &Path,
    prefix: &Path,
    settings: &Settings,
    progress: &dyn Fn(&str, u8),
) -> Result<(), String> {
    fs::create_dir_all(prefix).map_err(|err| err.to_string())?;
    if !prefix.join("system.reg").is_file() {
        progress("A preparar o prefixo", 0);
        wineboot(wine_bin, prefix)?;
        progress("A preparar o prefixo", 100);
    }
    if settings.dxvk {
        install_dxvk(prefix, progress)?;
    }
    Ok(())
}

pub fn path_env(wine_bin: &Path) -> String {
    let dir = wine_bin.parent().unwrap_or(wine_bin);
    match std::env::var("PATH") {
        Ok(path) => format!("{}:{path}", dir.display()),
        Err(_) => dir.display().to_string(),
    }
}

pub fn win_env(settings: &Settings) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("WINEESYNC", on_off(settings.esync)),
        ("WINEFSYNC", on_off(settings.fsync)),
    ];
    if settings.vulkan {
        env.push(("WGPU_BACKEND", "vulkan".to_string()));
    }
    if let Some(overrides) = dll_overrides(settings) {
        env.push(("WINEDLLOVERRIDES", overrides));
    }
    env
}

fn on_off(on: bool) -> String {
    if on { "1" } else { "0" }.to_string()
}

fn dll_overrides(settings: &Settings) -> Option<String> {
    let mut parts = Vec::new();
    if settings.vulkan {
        // Empty override drops d3d12. Studio then stays on Vulkan; Wine's FXC rejects its D3D12 shaders.
        parts.push("d3d12=");
    }
    if settings.dxvk {
        parts.push("d3d11,dxgi,d3d10core=n");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(";"))
    }
}

fn install_dxvk(prefix: &Path, progress: &dyn Fn(&str, u8)) -> Result<(), String> {
    let system32 = prefix.join("drive_c/windows/system32");
    let stamp = prefix.join("dxvk-version");
    if fs::read_to_string(&stamp).ok().as_deref() == Some(DXVK_VERSION)
        && system32.join("d3d11.dll").is_file()
    {
        return Ok(());
    }
    let dlls = ensure_dxvk_files(progress)?;
    progress("A instalar o DXVK", 0);
    fs::create_dir_all(&system32).map_err(|err| err.to_string())?;
    for name in ["d3d11.dll", "dxgi.dll", "d3d10core.dll"] {
        fs::copy(dlls.join(name), system32.join(name)).map_err(|err| format!("{name}: {err}"))?;
    }
    fs::write(&stamp, DXVK_VERSION).map_err(|err| err.to_string())?;
    progress("A instalar o DXVK", 100);
    Ok(())
}

fn ensure_dxvk_files(progress: &dyn Fn(&str, u8)) -> Result<PathBuf, String> {
    if dxvk_dlls().join("d3d11.dll").is_file() {
        return Ok(dxvk_dlls());
    }
    let _guard = lock_download();
    let dlls = dxvk_dlls();
    if dlls.join("d3d11.dll").is_file() {
        return Ok(dlls);
    }
    let root = tool_root("dxvk");
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let archive = root.join("dxvk.tar.gz");
    save_download(
        DXVK_URL,
        &archive,
        "A baixar o DXVK",
        ArchiveKind::Gzip,
        Some(DXVK_SHA),
        "DXVK",
        progress,
    )?;
    progress("A extrair o DXVK", 0);
    extract(&archive, &root, ArchiveKind::Gzip)?;
    let _ = fs::remove_file(&archive);
    progress("A extrair o DXVK", 100);
    if dlls.join("d3d11.dll").is_file() {
        Ok(dlls)
    } else {
        Err("DXVK em falta depois de extrair".to_string())
    }
}

fn wineboot(wine_bin: &Path, prefix: &Path) -> Result<(), String> {
    let output = Command::new(wine_bin)
        .arg("wineboot")
        .arg("-i")
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .env("PATH", path_env(wine_bin))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("wineboot: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "wineboot: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub(crate) fn save_download(
    url: &str,
    dest: &Path,
    label: &str,
    kind: ArchiveKind,
    sha: Option<&str>,
    name: &str,
    progress: &dyn Fn(&str, u8),
) -> Result<(), String> {
    let partial = partial_path(dest);
    let (done, total) = match fetch(url, &partial, label, progress) {
        Ok(sizes) => sizes,
        Err(err) => {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
    };
    if let Err(err) = accept_partial(&partial, done, total, kind, sha, name) {
        let _ = fs::remove_file(&partial);
        return Err(err);
    }
    fs::rename(&partial, dest).map_err(|err| err.to_string())
}

pub(crate) fn download_file(
    url: &str,
    dest: &Path,
    label: &str,
    progress: &dyn Fn(&str, u8),
) -> Result<(), String> {
    let partial = partial_path(dest);
    let (done, total) = match fetch(url, &partial, label, progress) {
        Ok(sizes) => sizes,
        Err(err) => {
            let _ = fs::remove_file(&partial);
            return Err(err);
        }
    };
    if total > 0 && done != total {
        let _ = fs::remove_file(&partial);
        return Err(format!("download incompleto ({done} de {total})"));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::rename(&partial, dest).map_err(|err| err.to_string())
}

fn partial_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.partial", dest.display()))
}

fn accept_partial(
    path: &Path,
    done: u64,
    total: u64,
    kind: ArchiveKind,
    sha: Option<&str>,
    name: &str,
) -> Result<(), String> {
    let header = read_header(path)?;
    check_download(done, total, &header, kind)?;
    if let Some(expected) = sha {
        verify(path, expected, name, done)?;
    }
    Ok(())
}

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|_| "arquivo do download sumiu".to_string())?;
    let mut header = [0u8; 6];
    let read = file.read(&mut header).map_err(|err| err.to_string())?;
    Ok(header[..read].to_vec())
}

fn check_download(done: u64, total: u64, header: &[u8], kind: ArchiveKind) -> Result<(), String> {
    if total > 0 && done != total {
        return Err(format!("download incompleto ({done} de {total})"));
    }
    let ok = match kind {
        ArchiveKind::Xz => header.starts_with(XZ_MAGIC),
        ArchiveKind::Gzip => header.starts_with(GZIP_MAGIC),
        ArchiveKind::Zip => header.starts_with(ZIP_MAGIC),
        ArchiveKind::Zst => header.starts_with(ZST_MAGIC),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("download não é o pacote ({done} bytes)"))
    }
}

fn fetch(
    url: &str,
    dest: &Path,
    label: &str,
    progress: &dyn Fn(&str, u8),
) -> Result<(u64, u64), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let response = ureq::get(url)
        .set("User-Agent", "VortexLauncher")
        .call()
        .map_err(|err| err.to_string())?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.into_reader();
    let mut file = File::create(dest).map_err(|err| err.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last = 255u8;
    progress(label, 0);
    loop {
        let read = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) => return Err(err.to_string()),
        };
        if file.write_all(&buf[..read]).is_err() {
            return Err("não deu para gravar o download".to_string());
        }
        done += read as u64;
        let pct = percent(done, total);
        if pct != last {
            last = pct;
            progress(label, pct);
        }
    }
    file.flush().map_err(|err| err.to_string())?;
    Ok((done, total))
}

fn verify(path: &Path, expected: &str, name: &str, size: u64) -> Result<(), String> {
    let got = sha256(path)?;
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "hash do {name} não confere (recebido {got}, {size} bytes)"
        ))
    }
}

fn sha256(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err("arquivo do download sumiu".to_string());
    }
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|_| "sha256sum não está no PATH".to_string())?;
    if !output.status.success() {
        return Err("não deu para ler o hash".to_string());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| "não deu para ler o hash".to_string())
}

fn extract(archive: &Path, dest: &Path, kind: ArchiveKind) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|err| err.to_string())?;
    // shortcut: zstd via `tar -xf`; needs a tar that opens zst, which Fedora has
    let flag = match kind {
        ArchiveKind::Xz => "-xJf",
        ArchiveKind::Gzip => "-xzf",
        ArchiveKind::Zip => {
            return Err("zip não se extrai com tar".to_string());
        }
        ArchiveKind::Zst => "-xf",
    };
    let output = Command::new("tar")
        .arg(flag)
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .output()
        .map_err(|err| format!("tar: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "tar: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        0
    } else {
        ((done.saturating_mul(100)) / total).min(100) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchiveKind, check_download, percent, win_env};
    use crate::settings::Settings;

    #[test]
    fn incomplete_download_fails() {
        let err = check_download(10, 100, &[0xfd, b'7', b'z', b'X', b'Z', 0], ArchiveKind::Xz)
            .unwrap_err();
        assert!(err.contains("incompleto"));
        assert!(err.contains("10 de 100"));
    }

    #[test]
    fn xz_and_gzip_magic_pass() {
        assert!(check_download(6, 6, &[0xfd, b'7', b'z', b'X', b'Z', 0], ArchiveKind::Xz).is_ok());
        assert!(check_download(2, 2, &[0x1f, 0x8b], ArchiveKind::Gzip).is_ok());
        assert!(check_download(4, 4, &[0x28, 0xb5, 0x2f, 0xfd], ArchiveKind::Zst).is_ok());
    }

    #[test]
    fn other_header_fails() {
        let err = check_download(15, 15, b"<html>", ArchiveKind::Xz).unwrap_err();
        assert!(err.contains("não é o pacote"));
        assert!(err.contains("15 bytes"));
        let err = check_download(15, 15, b"<html>", ArchiveKind::Gzip).unwrap_err();
        assert!(err.contains("não é o pacote"));
    }

    #[test]
    fn percent_ends_at_100() {
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(5, 10), 50);
        assert_eq!(percent(10, 10), 100);
        assert_eq!(percent(11, 10), 100);
        assert_eq!(percent(1, 0), 0);
    }

    #[test]
    fn graphics_env_defaults_on() {
        let settings = Settings {
            vulkan: true,
            dxvk: true,
            esync: true,
            fsync: true,
            ..Settings::default()
        };
        let env = win_env(&settings);
        assert!(env.contains(&("WGPU_BACKEND", "vulkan".to_string())));
        let overrides = env
            .iter()
            .find(|(key, _)| *key == "WINEDLLOVERRIDES")
            .map(|(_, value)| value.as_str());
        assert_eq!(overrides, Some("d3d12=;d3d11,dxgi,d3d10core=n"));
        assert!(env.contains(&("WINEESYNC", "1".to_string())));
        assert!(env.contains(&("WINEFSYNC", "1".to_string())));
    }

    #[test]
    fn graphics_env_all_off() {
        let settings = Settings {
            vulkan: false,
            dxvk: false,
            esync: false,
            fsync: false,
            ..Settings::default()
        };
        let env = win_env(&settings);
        assert!(
            !env.iter()
                .any(|(key, _)| *key == "WGPU_BACKEND" || *key == "WINEDLLOVERRIDES")
        );
        assert!(env.contains(&("WINEESYNC", "0".to_string())));
        assert!(env.contains(&("WINEFSYNC", "0".to_string())));
    }
}
