use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum Incoming {
    Focus,
    Launch(String),
}

pub struct Primary {
    pub listener: Option<UnixListener>,
    pub pending: Option<String>,
}

pub fn claim() -> Primary {
    let pending = std::env::args().skip(1).find_map(|arg| {
        let url = crate::runtime::normalize_launch_url(&arg);
        crate::runtime::launch_scheme(&url).map(|_| url)
    });
    let sock = socket_path();
    let line = pending.clone().unwrap_or_else(|| "focus".to_string());
    if try_forward(&sock, &line) {
        std::process::exit(0);
    }
    if sock.exists() {
        let _ = fs::remove_file(&sock);
    }
    match UnixListener::bind(&sock) {
        Ok(listener) => {
            let _ = fs::set_permissions(&sock, fs::Permissions::from_mode(0o600));
            Primary {
                listener: Some(listener),
                pending,
            }
        }
        Err(_) => {
            if try_forward(&sock, &line) {
                std::process::exit(0);
            }
            Primary {
                listener: None,
                pending,
            }
        }
    }
}

pub fn register() {
    let Ok(dir) = applications_dir() else {
        return;
    };
    let _ = fs::create_dir_all(&dir);
    let exec = handler_exec();
    let icon = icon_name();
    let entry = desktop_entry(&exec, &icon);
    let path = dir.join("vortex-launcher.desktop");
    if fs::write(&path, entry).is_err() {
        return;
    }
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    for scheme in ["x-scheme-handler/vortex", "x-scheme-handler/vortex-studio"] {
        let _ = Command::new("xdg-mime")
            .args(["default", "vortex-launcher.desktop", scheme])
            .status();
    }
}

pub fn desktop_entry(exec: &str, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Vortex Launcher\n\
         Comment=Launcher Linux para o Vortex\n\
         Exec={exec} %u\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Game;\n\
         MimeType=x-scheme-handler/vortex;x-scheme-handler/vortex-studio;\n"
    )
}

pub fn incoming(mut stream: UnixStream) -> Option<Incoming> {
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf).ok()?;
    if n == 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    let line = text.lines().next()?.trim();
    parse_line(line)
}

pub fn parse_line(line: &str) -> Option<Incoming> {
    if line == "focus" {
        return Some(Incoming::Focus);
    }
    let url = crate::runtime::normalize_launch_url(line);
    if crate::runtime::launch_scheme(&url).is_some() {
        return Some(Incoming::Launch(url));
    }
    None
}

fn socket_path() -> PathBuf {
    crate::paths::root().join("instance.sock")
}

fn try_forward(sock: &Path, line: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(sock) else {
        return false;
    };
    let mut payload = Vec::with_capacity(line.len() + 1);
    payload.extend_from_slice(line.as_bytes());
    payload.push(b'\n');
    stream.write_all(&payload).is_ok()
}

fn applications_dir() -> Result<PathBuf, ()> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(xdg).join("applications"));
    }
    let home = std::env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .ok_or(())?;
    Ok(PathBuf::from(home).join(".local/share/applications"))
}

fn handler_exec() -> String {
    if let Some(home) = std::env::var_os("HOME").filter(|path| !path.is_empty()) {
        let wrapper = PathBuf::from(home).join(".local/bin/vortex-launcher");
        if wrapper.is_file() {
            return wrapper.display().to_string();
        }
    }
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "vortex-launcher".to_string())
}

fn icon_name() -> String {
    let icon = crate::paths::root().join("icon.png");
    if icon.is_file() {
        icon.display().to_string()
    } else {
        "vortex-launcher".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Incoming, desktop_entry, incoming, parse_line, try_forward};
    use std::os::unix::net::UnixListener;

    #[test]
    fn desktop_declares_both_schemes() {
        let entry = desktop_entry("/home/ada/.local/bin/vortex-launcher", "vortex-launcher");
        assert!(entry.contains("Exec=/home/ada/.local/bin/vortex-launcher %u\n"));
        assert!(
            entry.contains("MimeType=x-scheme-handler/vortex;x-scheme-handler/vortex-studio;\n")
        );
    }

    #[test]
    fn line_is_focus_or_launch_url() {
        assert!(matches!(parse_line("focus"), Some(Incoming::Focus)));
        assert!(matches!(
            parse_line("vortex://play/1"),
            Some(Incoming::Launch(url)) if url == "vortex://play/1"
        ));
        assert!(matches!(
            parse_line(r#"window.location = "vortex://play?game=15\u0026token=abc""#),
            Some(Incoming::Launch(url)) if url == "vortex://play?game=15&token=abc"
        ));
        assert!(matches!(
            parse_line("vortex-studio://edit/1"),
            Some(Incoming::Launch(_))
        ));
        assert!(parse_line("https://playvortex.io").is_none());
        assert!(parse_line("focus\nvortx").is_none());
    }

    #[test]
    fn forward_sends_one_line() {
        let path = std::env::temp_dir().join(format!("vx-scheme-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        assert!(try_forward(&path, "vortex://play/9"));
        let (stream, _) = listener.accept().unwrap();
        match incoming(stream) {
            Some(Incoming::Launch(url)) => assert_eq!(url, "vortex://play/9"),
            other => panic!("expected launch, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }
}
