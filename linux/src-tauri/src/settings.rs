use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn default_on() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub close_launcher_on_instance: bool,
    #[serde(default)]
    pub close_launcher_on_studio: bool,
    pub open_client_on_instance_exit: bool,
    #[serde(default = "default_on")]
    pub vulkan: bool,
    #[serde(default = "default_on")]
    pub dxvk: bool,
    #[serde(default = "default_on")]
    pub esync: bool,
    #[serde(default = "default_on")]
    pub fsync: bool,
    #[serde(default)]
    pub gamemode: bool,
    #[serde(default)]
    pub discord_client_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_launcher_on_instance: false,
            close_launcher_on_studio: false,
            open_client_on_instance_exit: false,
            vulkan: true,
            dxvk: true,
            esync: true,
            fsync: true,
            gamemode: false,
            discord_client_id: String::new(),
        }
    }
}

fn settings_path() -> PathBuf {
    crate::paths::root().join("settings.json")
}

pub fn load() -> Settings {
    let Ok(text) = fs::read_to_string(settings_path()) else {
        return Settings::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(settings: &Settings) {
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(settings_path(), text);
    }
}

pub fn respond(path: &str, query: &str) -> http::Response<Vec<u8>> {
    let body = if path == "/boot" {
        boot_page()
    } else {
        let mut current = load();
        if !query.is_empty() {
            update_from_query(&mut current, query);
            save(&current);
        }
        page(&current)
    };
    http::Response::builder()
        .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(body.into_bytes())
        .unwrap()
}

pub fn update_from_query(settings: &mut Settings, query: &str) {
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let on = value != "0";
        match key {
            "close" => settings.close_launcher_on_instance = on,
            "studio" => settings.close_launcher_on_studio = on,
            "open" => settings.open_client_on_instance_exit = on,
            "vulkan" => settings.vulkan = on,
            "dxvk" => settings.dxvk = on,
            "esync" => settings.esync = on,
            "fsync" => settings.fsync = on,
            "gamemode" => settings.gamemode = on,
            "discord" => settings.discord_client_id = digits(value),
            _ => {}
        }
    }
}

fn boot_page() -> String {
    r#"<!DOCTYPE html>
<html lang="pt">
<head>
<meta charset="utf-8">
<title>Vortex</title>
</head>
<body style="margin:0;background:#12141a"></body>
</html>"#
        .to_string()
}

fn digits(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn checked(on: bool) -> &'static str {
    if on { "checked" } else { "" }
}

pub fn page(settings: &Settings) -> String {
    let close = checked(settings.close_launcher_on_instance);
    let studio = checked(settings.close_launcher_on_studio);
    let open = checked(settings.open_client_on_instance_exit);
    let vulkan = checked(settings.vulkan);
    let dxvk = checked(settings.dxvk);
    let esync = checked(settings.esync);
    let fsync = checked(settings.fsync);
    let gamemode = checked(settings.gamemode);
    let discord = &settings.discord_client_id;
    let version = crate::update::VERSION;
    format!(
        r#"<!DOCTYPE html>
<html lang="pt">
<head>
<meta charset="utf-8">
<title>Config</title>
<style>
  body {{ margin: 0; background: #12141a; color: #eee; font: 14px sans-serif; }}
  main {{ padding: 24px 24px 72px; }}
  h2 {{ margin: 28px 0 4px; font-size: 12px; font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; color: #8b93a7; }}
  h2:first-of-type {{ margin-top: 8px; }}
  .row {{ display: grid; grid-template-columns: 44px 1fr; column-gap: 12px; align-items: start; margin: 16px 0; }}
  .copy {{ display: flex; flex-direction: column; gap: 2px; padding-top: 2px; }}
  .desc {{ color: #8b93a7; font-size: 12px; }}
  .switch {{
    -webkit-appearance: none;
    appearance: none;
    width: 44px;
    height: 26px;
    margin: 0;
    border: 0;
    border-radius: 999px;
    background: #3a4150;
    position: relative;
    cursor: pointer;
    flex: none;
  }}
  .switch::after {{
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s ease;
  }}
  input[type=text] {{ margin-top: 8px; width: 100%; max-width: 280px; box-sizing: border-box; background: #0c0e13; color: #eee; border: 1px solid #3a4150; border-radius: 6px; padding: 8px 10px; font: 14px sans-serif; }}
  .switch:checked {{ background: #4c8dff; }}
  .switch:checked::after {{ transform: translateX(18px); }}
  nav {{ position: fixed; bottom: 0; left: 0; right: 0; height: 40px; display: flex; gap: 8px; align-items: center; padding: 0 12px; background: #0c0e13; }}
  a, span.tab {{ border: 0; background: transparent; color: #eee; padding: 6px 10px; border-radius: 6px; text-decoration: none; }}
  .on {{ background: #2a3142; }}
</style>
</head>
<body>
<main>
  <h1>Config</h1>
  <h2>Launcher</h2>
  <label class="row"><input id="close" class="switch" type="checkbox" {close}><span class="copy"><span>Fechar o launcher ao abrir uma instância</span><span class="desc">Esconde a janela quando um jogo abre.</span></span></label>
  <label class="row"><input id="studio" class="switch" type="checkbox" {studio}><span class="copy"><span>Fechar o launcher ao abrir o Studio</span><span class="desc">Esconde a janela quando o Studio abre e mostra de novo quando ele fecha.</span></span></label>
  <label class="row"><input id="open" class="switch" type="checkbox" {open}><span class="copy"><span>Ao sair da partida, fechar o Vortex e voltar ao launcher</span><span class="desc">Depois de uma partida, fecha o Vortex e traz o launcher de volta.</span></span></label>
  <h2>Gráficos</h2>
  <label class="row"><input id="vulkan" class="switch" type="checkbox" {vulkan}><span class="copy"><span>Vulkan</span><span class="desc">O client desenha com Vulkan e deixa o Direct3D 12 desligado.</span></span></label>
  <label class="row"><input id="dxvk" class="switch" type="checkbox" {dxvk}><span class="copy"><span>DXVK</span><span class="desc">Traduz Direct3D 11 para Vulkan dentro do prefixo.</span></span></label>
  <h2>Wine</h2>
  <label class="row"><input id="esync" class="switch" type="checkbox" {esync}><span class="copy"><span>Esync</span><span class="desc">Sincroniza o Wine por eventfd. Serve na maior parte dos kernels.</span></span></label>
  <label class="row"><input id="fsync" class="switch" type="checkbox" {fsync}><span class="copy"><span>Fsync</span><span class="desc">Sincroniza o Wine por futex. Precisa de um kernel com futex2.</span></span></label>
  <label class="row"><input id="gamemode" class="switch" type="checkbox" {gamemode}><span class="copy"><span>GameMode</span><span class="desc">Precisa do pacote gamemode. Sem o binário, o jogo abre na mesma.</span></span></label>
  <h2>Atualização</h2>
  <p class="desc">Versão {version}. Ao abrir, o launcher procura uma release mais nova no GitHub.</p>
  <h2>Discord</h2>
  <label class="row"><span></span><span class="copy"><span>Client ID</span><span class="desc">Vazio, a presença fica desligada. Preenchido, o Discord mostra Vortex Launcher.</span><input id="discord" type="text" value="{discord}" inputmode="numeric"></span></label>
</main>
<nav>
  <a href="https://playvortex.io">Vortex</a>
  <span class="tab on">Config</span>
</nav>
<script>
  function save() {{
    const ids = ['close', 'studio', 'open', 'vulkan', 'dxvk', 'esync', 'fsync', 'gamemode'];
    const discord = encodeURIComponent(document.getElementById('discord').value.trim());
    const query = ids.map((id) => id + '=' + (document.getElementById(id).checked ? '1' : '0')).join('&');
    setTimeout(() => {{
      location.href = 'launcher://localhost/?' + query + '&discord=' + discord;
    }}, 180);
  }}
  for (const id of ['close', 'studio', 'open', 'vulkan', 'dxvk', 'esync', 'fsync', 'gamemode']) {{
    document.getElementById(id).onchange = save;
  }}
  document.getElementById('discord').onchange = save;
</script>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::{update_from_query, Settings};

    #[test]
    fn query_sets_both_toggles() {
        let mut settings = Settings::default();
        update_from_query(&mut settings, "close=1&open=0");
        assert!(settings.close_launcher_on_instance);
        assert!(!settings.open_client_on_instance_exit);
        update_from_query(&mut settings, "open=1");
        assert!(settings.close_launcher_on_instance);
        assert!(settings.open_client_on_instance_exit);
    }

    #[test]
    fn old_file_enables_graphics() {
        let settings: Settings = serde_json::from_str(
            r#"{"close_launcher_on_instance":true,"open_client_on_instance_exit":true}"#,
        )
        .unwrap();
        assert!(settings.close_launcher_on_instance);
        assert!(settings.open_client_on_instance_exit);
        assert!(!settings.close_launcher_on_studio);
        assert!(!settings.gamemode);
        assert!(settings.discord_client_id.is_empty());
        assert!(settings.vulkan && settings.dxvk && settings.esync && settings.fsync);
    }

    #[test]
    fn query_can_turn_graphics_off() {
        let mut settings = Settings::default();
        update_from_query(&mut settings, "vulkan=0&dxvk=0&esync=0&fsync=0");
        assert!(!settings.vulkan && !settings.dxvk && !settings.esync && !settings.fsync);
        update_from_query(&mut settings, "close=1");
        assert!(settings.close_launcher_on_instance);
        assert!(!settings.vulkan);
    }

    #[test]
    fn query_sets_studio_close() {
        let mut settings = Settings::default();
        update_from_query(&mut settings, "studio=1");
        assert!(settings.close_launcher_on_studio);
        assert!(!settings.close_launcher_on_instance);
        update_from_query(&mut settings, "studio=0");
        assert!(!settings.close_launcher_on_studio);
    }

    #[test]
    fn query_sets_gamemode_and_discord_id() {
        let mut settings = Settings::default();
        update_from_query(&mut settings, "gamemode=1&discord=12345abc");
        assert!(settings.gamemode);
        assert_eq!(settings.discord_client_id, "12345");
        update_from_query(&mut settings, "discord=");
        assert!(settings.gamemode);
        assert!(settings.discord_client_id.is_empty());
    }
}
