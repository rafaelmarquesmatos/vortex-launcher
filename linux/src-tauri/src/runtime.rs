use std::fs::{self, File};
use std::io::{copy, Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::SystemTime;

use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Product {
    Client,
    Studio,
}

impl Product {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Studio => "studio",
        }
    }

    fn version_url(self) -> &'static str {
        match self {
            Self::Client => "https://playvortex.io/api/client-version",
            Self::Studio => "https://playvortex.io/api/studio-version",
        }
    }

    fn exe_name(self) -> &'static str {
        match self {
            Self::Client => "Vortex.exe",
            Self::Studio => "VortexStudio.exe",
        }
    }

    fn archive_url(self, version: &str) -> String {
        match self {
            // Official redirect. The CDN path can change without this breaking.
            Self::Client => "https://playvortex.io/download/windows".to_string(),
            Self::Studio => format!(
                "https://cdn.playvortex.io/releases/studio/{version}/VortexStudio-Windows.zip"
            ),
        }
    }
}

pub fn launch_scheme(url: &str) -> Option<Product> {
    if url.starts_with("vortex-studio://") {
        Some(Product::Studio)
    } else if url.starts_with("vortex://") {
        Some(Product::Client)
    } else {
        None
    }
}

fn needs_update(local: Option<&str>, remote: &str, exe_exists: bool) -> bool {
    !exe_exists || local.map(str::trim) != Some(remote)
}

// shortcut: roots are the compile-time linux/ folder, switch to the exe dir when we package
fn product_root(product: Product) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(product.dir_name())
}

#[derive(Deserialize)]
struct VersionBody {
    version: String,
}

fn fetch_version(url: &str) -> Result<String, String> {
    let body = ureq::get(url)
        .call()
        .map_err(|err| err.to_string())?
        .into_string()
        .map_err(|err| err.to_string())?;
    let parsed: VersionBody = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    Ok(parsed.version)
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|err| err.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| err.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|err| err.to_string())?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "zip entry left the destination".to_string())?;
        let out = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|err| err.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let mut outfile = File::create(&out).map_err(|err| err.to_string())?;
        copy(&mut entry, &mut outfile).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Some(path);
        }
    }
    None
}

fn ensure_installed(product: Product, progress: &dyn Fn(&str, u8)) -> Result<PathBuf, String> {
    let _guard = crate::wine::lock_download();
    let root = product_root(product);
    let app_dir = root.join("app");
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;

    let remote = fetch_version(product.version_url())?;
    let local = fs::read_to_string(root.join("version")).ok();
    let installed = find_file(&app_dir, product.exe_name());
    if !needs_update(local.as_deref(), &remote, installed.is_some()) {
        return installed.ok_or_else(|| format!("{} exe missing", product.dir_name()));
    }

    eprintln!(
        "{} {} -> {remote}",
        product.dir_name(),
        local.as_deref().map(str::trim).unwrap_or("missing")
    );

    let zip_path = root.join("download.zip");
    let (download_label, extract_label) = match product {
        Product::Client => ("A baixar o client", "A extrair o client"),
        Product::Studio => ("A baixar o Studio", "A extrair o Studio"),
    };
    crate::wine::save_download(
        &product.archive_url(&remote),
        &zip_path,
        download_label,
        crate::wine::ArchiveKind::Zip,
        None,
        download_label,
        progress,
    )?;
    if app_dir.exists() {
        fs::remove_dir_all(&app_dir).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&app_dir).map_err(|err| err.to_string())?;
    progress(extract_label, 0);
    extract_zip(&zip_path, &app_dir)?;
    progress(extract_label, 100);
    let _ = fs::remove_file(&zip_path);

    let exe = find_file(&app_dir, product.exe_name())
        .ok_or_else(|| format!("{} missing after extract", product.exe_name()))?;
    fs::write(root.join("version"), &remote).map_err(|err| err.to_string())?;
    Ok(exe)
}

pub fn is_download_page(url: &str) -> bool {
    let Some(rest) = url.split("://").nth(1) else {
        return false;
    };
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host_port.split('@').next_back().unwrap_or(host_port);
    let host = host.split(':').next().unwrap_or(host);
    if host != "playvortex.io" && host != "www.playvortex.io" {
        return false;
    }
    let path = path.split('#').next().unwrap_or(path);
    path == "download" || path == "download/" || path.starts_with("download?")
}

pub fn is_studio_launch_page(url: &str) -> bool {
    let Some(rest) = url.split("://").nth(1) else {
        return false;
    };
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host_port.split('@').next_back().unwrap_or(host_port);
    let host = host.split(':').next().unwrap_or(host);
    if host != "playvortex.io" && host != "www.playvortex.io" {
        return false;
    }
    let path = path.split('#').next().unwrap_or(path);
    path == "studio/launch" || path == "studio/launch/" || path.starts_with("studio/launch?")
}

pub fn launch(url: &str, progress: &dyn Fn(&str, u8)) -> Result<Child, String> {
    let product = launch_scheme(url).ok_or_else(|| format!("not a launch url: {url}"))?;
    let settings = crate::settings::load();
    let wine_bin = crate::wine::binary(progress)?;
    let exe = ensure_installed(product, progress)?;
    let prefix = product_root(product).join("prefix");
    fs::create_dir_all(&prefix).map_err(|err| err.to_string())?;
    crate::wine::prepare_prefix(&wine_bin, &prefix, &settings, progress)?;

    let mut command = wine_command(&wine_bin, settings.gamemode);
    command
        .arg(&exe)
        .arg(url)
        .env("WINEPREFIX", &prefix)
        .env("PATH", crate::wine::path_env(&wine_bin))
        .stdin(std::process::Stdio::null())
        .process_group(0);
    for (key, value) in crate::wine::win_env(&settings) {
        command.env(key, value);
    }
    if let Some(dir) = exe.parent() {
        command.current_dir(dir);
    }
    command.spawn().map_err(|err| format!("wine: {err}"))
}

fn wine_command(wine_bin: &Path, gamemode: bool) -> Command {
    if use_gamemode(gamemode, runner_on_path()) {
        let mut command = Command::new("gamemoderun");
        command.arg(wine_bin);
        command
    } else {
        Command::new(wine_bin)
    }
}

fn runner_on_path() -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join("gamemoderun").is_file())
}

pub fn use_gamemode(enabled: bool, runner_on_path: bool) -> bool {
    enabled && runner_on_path
}

pub fn note_log_line(seen_game: &mut bool, line: &str) -> bool {
    if line.contains("verified user") && line.contains("for game") {
        *seen_game = true;
    }
    *seen_game && line.contains("couldnt parse the launch url")
}

pub struct LogCursor {
    path: Option<PathBuf>,
    offset: u64,
    seen_game: bool,
}

impl LogCursor {
    pub fn new() -> Self {
        Self {
            path: None,
            offset: 0,
            seen_game: false,
        }
    }

    pub fn poll(&mut self) -> bool {
        let Some(path) = newest_vortex_log() else {
            return false;
        };
        if self.path.as_ref() != Some(&path) {
            self.offset = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            self.path = Some(path);
            return false;
        }
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(_) => return false,
        };
        let len = file.metadata().map(|meta| meta.len()).unwrap_or(self.offset);
        if len < self.offset {
            self.offset = 0;
        }
        if len == self.offset {
            return false;
        }
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut chunk = String::new();
        if file.read_to_string(&mut chunk).is_err() {
            return false;
        }
        self.offset = len;
        chunk.lines().any(|line| note_log_line(&mut self.seen_game, line))
    }
}

fn newest_vortex_log() -> Option<PathBuf> {
    let users = product_root(Product::Client).join("prefix/drive_c/users");
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for user in fs::read_dir(users).ok()?.flatten() {
        let logs = user.path().join("AppData/Local/Vortex/logs");
        for entry in fs::read_dir(logs).ok().into_iter().flatten().flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("vortex.log") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            if best.as_ref().map(|(time, _)| modified > *time).unwrap_or(true) {
                best = Some((modified, entry.path()));
            }
        }
    }
    best.map(|(_, path)| path)
}

pub fn stop_client(child: &mut Child) {
    let pid = child.id();
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{pid}")])
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::{launch_scheme, needs_update, use_gamemode, Product};

    #[test]
    fn missing_install_needs_update() {
        assert!(needs_update(None, "0.7.1", false));
    }

    #[test]
    fn same_version_with_exe_skips_update() {
        assert!(!needs_update(Some("0.7.1\n"), "0.7.1", true));
    }

    #[test]
    fn different_version_updates() {
        assert!(needs_update(Some("0.7.0"), "0.7.1", true));
    }

    #[test]
    fn same_version_without_exe_updates() {
        assert!(needs_update(Some("0.7.1"), "0.7.1", false));
    }

    #[test]
    fn download_page_is_studio_fallback() {
        assert!(super::is_download_page("https://playvortex.io/download"));
        assert!(super::is_download_page("https://www.playvortex.io/download/"));
        assert!(super::is_download_page(
            "https://playvortex.io/download?from=studio"
        ));
        assert!(!super::is_download_page(
            "https://playvortex.io/download/windows"
        ));
        assert!(!super::is_download_page("https://playvortex.io/home"));
    }

    #[test]
    fn studio_launch_page_goes_home() {
        assert!(super::is_studio_launch_page(
            "https://playvortex.io/studio/launch"
        ));
        assert!(super::is_studio_launch_page(
            "https://www.playvortex.io/studio/launch/"
        ));
        assert!(!super::is_studio_launch_page("https://playvortex.io/home"));
        assert!(!super::is_studio_launch_page(
            "https://playvortex.io/studio"
        ));
    }

    #[test]
    fn lobby_after_game_closes_client() {
        let mut seen = false;
        assert!(!super::note_log_line(&mut seen, "couldnt parse the launch url"));
        assert!(!super::note_log_line(
            &mut seen,
            "verified user 1 for game 6"
        ));
        assert!(seen);
        assert!(super::note_log_line(
            &mut seen,
            "couldnt parse the launch url"
        ));
    }

    #[test]
    fn scheme_routes_client_and_studio() {
        assert_eq!(launch_scheme("vortex://play/1"), Some(Product::Client));
        assert_eq!(
            launch_scheme("vortex-studio://edit/1"),
            Some(Product::Studio)
        );
        assert_eq!(launch_scheme("https://playvortex.io"), None);
    }

    #[test]
    fn gamemode_needs_the_runner() {
        assert!(!use_gamemode(false, true));
        assert!(!use_gamemode(true, false));
        assert!(use_gamemode(true, true));
    }
}
