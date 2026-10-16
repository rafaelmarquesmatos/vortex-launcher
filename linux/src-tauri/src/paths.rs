use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let dev_root = dev.join("package.json").is_file().then_some(dev.as_path());
    choose_root(
        std::env::var_os("VORTEX_HOME").as_deref(),
        dev_root,
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

pub fn choose_root(
    vortex_home: Option<&std::ffi::OsStr>,
    dev_root: Option<&Path>,
    xdg_data_home: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> PathBuf {
    if let Some(path) = vortex_home {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Some(dev) = dev_root {
        return dev.to_path_buf();
    }
    let base = xdg_data_home
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| PathBuf::from(path).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("vortex-launcher")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::choose_root;

    #[test]
    fn vortex_home_wins() {
        let root = choose_root(
            Some("/tmp/vortex".as_ref()),
            Some(Path::new("/src/linux")),
            Some("/xdg".as_ref()),
            Some("/home/mark".as_ref()),
        );
        assert_eq!(root, Path::new("/tmp/vortex"));
    }

    #[test]
    fn dev_tree_beats_xdg() {
        let root = choose_root(
            None,
            Some(Path::new("/src/linux")),
            Some("/xdg".as_ref()),
            Some("/home/mark".as_ref()),
        );
        assert_eq!(root, Path::new("/src/linux"));
    }

    #[test]
    fn installed_copy_uses_xdg_then_home() {
        let xdg = choose_root(None, None, Some("/xdg".as_ref()), Some("/home/mark".as_ref()));
        assert_eq!(xdg, Path::new("/xdg/vortex-launcher"));
        let home = choose_root(None, None, None, Some("/home/mark".as_ref()));
        assert_eq!(home, Path::new("/home/mark/.local/share/vortex-launcher"));
        let empty = choose_root(Some("".as_ref()), None, Some("".as_ref()), None);
        assert_eq!(empty, Path::new(".local/share/vortex-launcher"));
    }
}
