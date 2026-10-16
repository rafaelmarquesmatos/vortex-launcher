use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[cfg(test)]
pub fn linux_asset(name: &str) -> bool {
    rank(name).is_some()
}

fn rank(name: &str) -> Option<(u8, u8, String)> {
    let lowered = name.to_ascii_lowercase();
    if !lowered.contains("linux") {
        return None;
    }
    let kind = if lowered.ends_with(".appimage") {
        0
    } else if lowered.ends_with(".tar.gz") {
        1
    } else {
        return None;
    };
    let arch = if lowered.contains("x86_64") || lowered.contains("amd64") {
        0
    } else {
        1
    };
    Some((arch, kind, name.to_string()))
}

fn best_asset(assets: &[Asset]) -> Option<&Asset> {
    let mut best: Option<(&Asset, (u8, u8, String))> = None;
    for asset in assets {
        let Some(score) = rank(&asset.name) else {
            continue;
        };
        if best.as_ref().map(|(_, old)| score < *old).unwrap_or(true) {
            best = Some((asset, score));
        }
    }
    best.map(|(asset, _)| asset)
}

#[cfg(test)]
pub fn pick_name<'a>(names: &'a [&'a str]) -> Option<&'a str> {
    let mut best: Option<(&'a str, (u8, u8, String))> = None;
    for name in names {
        let Some(score) = rank(name) else {
            continue;
        };
        if best.as_ref().map(|(_, old)| score < *old).unwrap_or(true) {
            best = Some((*name, score));
        }
    }
    best.map(|(name, _)| name)
}

pub enum Applied {
    Saved(PathBuf),
    Restarting,
}

pub fn apply(asset: &Path) -> Result<Applied, String> {
    if !running_installed() {
        return Ok(Applied::Saved(asset.to_path_buf()));
    }
    let (src, appimage) = payload(asset)?;
    let data = installed_data_dir();
    let dest = if appimage {
        match std::env::var("APPIMAGE") {
            Ok(path) if !path.is_empty() => PathBuf::from(path),
            _ => data.join("vortex-launcher.AppImage"),
        }
    } else {
        let name = src
            .file_name()
            .ok_or_else(|| "nome do arquivo inválido".to_string())?;
        data.join("pkg").join(name)
    };
    let home = std::env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "HOME em falta".to_string())?;
    let wrapper_dest = PathBuf::from(home).join(".local/bin/vortex-launcher");
    let update_dir = asset.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(update_dir).map_err(|err| err.to_string())?;
    let wrapper_src = update_dir.join("wrapper.sh");
    fs::write(&wrapper_src, wrapper_body(&dest, appimage)).map_err(|err| err.to_string())?;
    let script = update_dir.join("apply.sh");
    fs::write(&script, swap_script()).map_err(|err| err.to_string())?;
    let mode = if appimage { "appimage" } else { "binary" };
    Command::new("sh")
        .arg(&script)
        .arg(std::process::id().to_string())
        .arg(&src)
        .arg(&dest)
        .arg(&wrapper_src)
        .arg(&wrapper_dest)
        .arg(mode)
        .arg(&data)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|err| format!("apply: {err}"))?;
    Ok(Applied::Restarting)
}

fn running_installed() -> bool {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|path| !path.is_empty()) {
        if Path::new(&appimage).is_file() {
            return true;
        }
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    exe.starts_with(installed_data_dir())
}

fn installed_data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("vortex-launcher")
}

fn payload(asset: &Path) -> Result<(PathBuf, bool), String> {
    let name = asset
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if name.ends_with(".appimage") {
        return Ok((asset.to_path_buf(), true));
    }
    if !name.ends_with(".tar.gz") {
        return Err("asset inesperado".to_string());
    }
    let unpack = asset.parent().unwrap_or(asset).join("unpack");
    if unpack.exists() {
        fs::remove_dir_all(&unpack).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&unpack).map_err(|err| err.to_string())?;
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(asset)
        .arg("-C")
        .arg(&unpack)
        .output()
        .map_err(|err| format!("tar: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "tar: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let found = find_payload(&unpack)?;
    let appimage = found
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
        .ends_with(".appimage");
    Ok((found, appimage))
}

fn find_payload(dir: &Path) -> Result<PathBuf, String> {
    let mut appimage = None;
    let mut binary = None;
    walk(dir, &mut appimage, &mut binary)?;
    appimage
        .or(binary)
        .ok_or_else(|| "O arquivo não tem o executável".to_string())
}

fn walk(
    dir: &Path,
    appimage: &mut Option<PathBuf>,
    binary: &mut Option<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|err| err.to_string())?;
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.is_dir() {
            walk(&path, appimage, binary)?;
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if name.ends_with(".appimage") && appimage.is_none() {
            *appimage = Some(path);
        } else if binary.is_none() && name.contains("vortex") && !name.ends_with(".tar.gz") {
            *binary = Some(path);
        }
    }
    Ok(())
}

pub fn wrapper_body(dest: &Path, appimage: bool) -> String {
    if appimage {
        format!(
            "#!/bin/sh\nexec \"{}\" --appimage-extract-and-run \"$@\"\n",
            dest.display()
        )
    } else {
        format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", dest.display())
    }
}

pub fn swap_script() -> &'static str {
    r#"#!/bin/sh
pid=$1
src=$2
dest=$3
wrapper_src=$4
wrapper_dest=$5
mode=$6
data=$7
while kill -0 "$pid" 2>/dev/null; do
  sleep 0.2
done
mkdir -p "$(dirname "$dest")" "$(dirname "$wrapper_dest")"
cp -f "$src" "$dest" || exit 1
chmod 755 "$dest"
cp -f "$wrapper_src" "$wrapper_dest" || exit 1
chmod 755 "$wrapper_dest"
if [ "$mode" = appimage ]; then
  rm -rf "$data/pkg"
else
  rm -f "$data/vortex-launcher.AppImage"
fi
exec "$wrapper_dest"
"#
}

#[cfg(test)]
pub fn exe_is_installed(exe: &Path, data: &Path) -> bool {
    exe.starts_with(data)
}

pub fn check(progress: &dyn Fn(&str, u8)) -> Result<Option<PathBuf>, String> {
    let Some(release) = latest()? else {
        return Ok(None);
    };
    if !is_newer(VERSION, &release.tag_name) {
        return Ok(None);
    }
    let Some(asset) = best_asset(&release.assets) else {
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
    use std::path::Path;

    use super::{
        VERSION, exe_is_installed, is_newer, linux_asset, pick_name, swap_script, wrapper_body,
    };

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

    #[test]
    fn appimage_for_this_arch_wins() {
        assert_eq!(
            pick_name(&[
                "vortex-linux.tar.gz",
                "VortexLauncher-linux-aarch64.AppImage",
                "VortexLauncher-linux-x86_64.AppImage",
            ]),
            Some("VortexLauncher-linux-x86_64.AppImage")
        );
        assert_eq!(
            pick_name(&["notes.txt", "vortex-linux-amd64.tar.gz"]),
            Some("vortex-linux-amd64.tar.gz")
        );
    }

    #[test]
    fn wrapper_matches_install_script() {
        let appimage = wrapper_body(Path::new("/data/vortex-launcher.AppImage"), true);
        assert!(appimage.contains("--appimage-extract-and-run"));
        let binary = wrapper_body(Path::new("/data/pkg/vortex"), false);
        assert_eq!(binary, "#!/bin/sh\nexec \"/data/pkg/vortex\" \"$@\"\n");
        assert!(swap_script().contains("kill -0"));
        assert!(swap_script().contains("exec \"$wrapper_dest\""));
    }

    #[test]
    fn installed_exe_lives_under_the_data_dir() {
        let data = Path::new("/home/ada/.local/share/vortex-launcher");
        assert!(exe_is_installed(
            Path::new("/home/ada/.local/share/vortex-launcher/pkg/vortex"),
            data
        ));
        assert!(!exe_is_installed(
            Path::new("/src/linux/src-tauri/target/debug/app"),
            data
        ));
    }

    #[test]
    fn unpacked_appimage_wins() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/vx-unpack-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("inner/Vortex"), b"bin").unwrap();
        std::fs::write(dir.join("VortexLauncher-linux.AppImage"), b"img").unwrap();
        let found = super::find_payload(&dir).unwrap();
        assert!(found.ends_with("VortexLauncher-linux.AppImage"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
