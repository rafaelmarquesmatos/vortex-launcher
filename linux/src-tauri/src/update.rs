use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const LATEST: &str =
    "https://api.github.com/repos/rafaelmarquesmatos/vortex-launcher/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn is_newer(local: &str, remote: &str) -> bool {
    let local = parts(local);
    let remote = parts(remote);
    let len = local.len().max(remote.len());
    for index in 0..len {
        let left = local.get(index).copied().unwrap_or(0);
        let right = remote.get(index).copied().unwrap_or(0);
        if right != left {
            return right > left;
        }
    }
    false
}

fn parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

pub fn linux_asset(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("linux") && (name.ends_with(".appimage") || name.ends_with(".tar.gz"))
}

pub fn check(progress: &dyn Fn(&str, u8)) -> Result<Option<PathBuf>, String> {
    let Some(release) = latest()? else {
        return Ok(None);
    };
    if !is_newer(VERSION, &release.tag_name) {
        return Ok(None);
    }
    let Some(asset) = release.assets.iter().find(|asset| linux_asset(&asset.name)) else {
        return Ok(None);
    };
    let name = Path::new(&asset.name)
        .file_name()
        .ok_or_else(|| "nome do arquivo inválido".to_string())?;
    let dest = crate::paths::root().join("update").join(name);
    crate::wine::download_file(
        &asset.browser_download_url,
        &dest,
        "A baixar a atualização",
        progress,
    )?;
    Ok(Some(dest))
}

fn latest() -> Result<Option<Release>, String> {
    match ureq::get(LATEST)
        .set("User-Agent", "VortexLauncher")
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(err) => Err(err.to_string()),
        Ok(response) => {
            let body = response.into_string().map_err(|err| err.to_string())?;
            serde_json::from_str(&body)
                .map(Some)
                .map_err(|err| err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VERSION, is_newer, linux_asset};

    #[test]
    fn newer_version_is_numeric() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "v0.1.1"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
    }

    #[test]
    fn version_matches_manifests() {
        let tauri: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let pkg: serde_json::Value =
            serde_json::from_str(include_str!("../../package.json")).unwrap();
        assert_eq!(tauri["version"].as_str(), Some(VERSION));
        assert_eq!(pkg["version"].as_str(), Some(VERSION));
    }

    #[test]
    fn linux_asset_matches_package_names() {
        assert!(linux_asset("VortexLauncher-linux.AppImage"));
        assert!(linux_asset("vortex-linux.tar.gz"));
        assert!(!linux_asset("VortexLauncher-windows.zip"));
        assert!(!linux_asset("linux-source.zip"));
    }
}
