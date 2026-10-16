use std::path::Path;
use std::process::Command;

pub struct Flags {
    pub wine: bool,
    pub vulkan: bool,
    pub dxvk: bool,
    pub webkit: bool,
    pub gamemode: bool,
}

pub fn phrases(flags: Flags) -> Vec<&'static str> {
    let mut lines = Vec::new();
    if !flags.wine {
        lines.push("O Wine ainda não foi baixado.");
    }
    if !flags.vulkan {
        lines.push("Falta um driver Vulkan.");
    }
    if !flags.dxvk {
        lines.push("Falta o DXVK no prefixo.");
    }
    if !flags.webkit {
        lines.push("Falta o WebKitGTK 4.1.");
    }
    if !flags.gamemode {
        lines.push("Falta o gamemoderun. O jogo abre na mesma.");
    }
    lines
}

pub fn gather() -> Flags {
    Flags {
        wine: crate::wine::installed(),
        vulkan: vulkan_present(),
        dxvk: dxvk_ready(),
        webkit: webkit_present(),
        gamemode: crate::runtime::runner_on_path(),
    }
}

fn dxvk_ready() -> bool {
    for name in ["client", "studio"] {
        let prefix = crate::paths::root().join(name).join("prefix");
        if prefix.join("system.reg").is_file()
            && !prefix.join("drive_c/windows/system32/d3d11.dll").is_file()
        {
            return false;
        }
    }
    true
}

fn vulkan_present() -> bool {
    let mut dirs = vec![
        Path::new("/usr/share/vulkan/icd.d").to_path_buf(),
        Path::new("/etc/vulkan/icd.d").to_path_buf(),
    ];
    if let Some(home) = std::env::var_os("HOME").filter(|path| !path.is_empty()) {
        dirs.push(Path::new(&home).join(".local/share/vulkan/icd.d"));
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".json") && !name.contains("lvp") && !name.contains("virtio") {
                return true;
            }
        }
    }
    false
}

fn webkit_present() -> bool {
    for path in [
        "/usr/lib64/libwebkit2gtk-4.1.so.0",
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0",
        "/usr/lib/aarch64-linux-gnu/libwebkit2gtk-4.1.so.0",
        "/usr/lib/libwebkit2gtk-4.1.so.0",
    ] {
        if Path::new(path).exists() {
            return true;
        }
    }
    for bin in ["ldconfig", "/usr/sbin/ldconfig", "/sbin/ldconfig"] {
        let Ok(output) = Command::new(bin).arg("-p").output() else {
            continue;
        };
        if String::from_utf8_lossy(&output.stdout).contains("libwebkit2gtk-4.1.so") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Flags, phrases};

    #[test]
    fn each_gap_has_one_sentence() {
        let lines = phrases(Flags {
            wine: false,
            vulkan: false,
            dxvk: false,
            webkit: false,
            gamemode: false,
        });
        assert_eq!(
            lines,
            [
                "O Wine ainda não foi baixado.",
                "Falta um driver Vulkan.",
                "Falta o DXVK no prefixo.",
                "Falta o WebKitGTK 4.1.",
                "Falta o gamemoderun. O jogo abre na mesma.",
            ]
        );
    }

    #[test]
    fn nothing_missing_is_empty() {
        let lines = phrases(Flags {
            wine: true,
            vulkan: true,
            dxvk: true,
            webkit: true,
            gamemode: true,
        });
        assert!(lines.is_empty());
    }

    #[test]
    fn one_gap_is_one_line() {
        let lines = phrases(Flags {
            wine: true,
            vulkan: true,
            dxvk: true,
            webkit: true,
            gamemode: false,
        });
        assert_eq!(lines, ["Falta o gamemoderun. O jogo abre na mesma."]);
    }
}
