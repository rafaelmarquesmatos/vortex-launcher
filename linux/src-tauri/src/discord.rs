use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{Activity, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

const RECONNECT: Duration = Duration::from_secs(30);
pub const PRESENCE_STATE: &str = "via Vortex Launcher";

static TX: Mutex<Option<std::sync::mpsc::Sender<Cmd>>> = Mutex::new(None);

#[derive(Clone)]
enum Cmd {
    Playing { details: String, started: i64 },
    Idle,
}

pub fn start() {
    let id = crate::settings::load().discord_client_id;
    if id.is_empty() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    *TX.lock().unwrap_or_else(|err| err.into_inner()) = Some(tx);
    std::thread::spawn(move || run(rx, id));
}

pub fn playing(details: &str) {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64)
        .unwrap_or(0);
    send(Cmd::Playing {
        details: clip(details),
        started,
    });
}

pub fn idle() {
    send(Cmd::Idle);
}

pub fn details_for(url: &str) -> String {
    let studio = crate::runtime::launch_scheme(url) == Some(crate::runtime::Product::Studio);
    let name = if studio {
        None
    } else {
        game_id(url).and_then(fetch_game_name)
    };
    presence_details(studio, name.as_deref())
}

pub fn presence_details(studio: bool, name: Option<&str>) -> String {
    if studio {
        "No Studio".to_string()
    } else if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        name.to_string()
    } else {
        "A jogar".to_string()
    }
}

pub fn game_id(url: &str) -> Option<u64> {
    let rest = url.split_once("://")?.1;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "game" {
            if let Ok(id) = value.parse() {
                return Some(id);
            }
        }
    }
    let path = path.trim_matches('/');
    let id = path.strip_prefix("play/").unwrap_or(path);
    if id.is_empty() || id.contains('/') {
        return None;
    }
    id.parse().ok()
}

fn fetch_game_name(id: u64) -> Option<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(3))
        .build();
    let url = format!("https://playvortex.io/api/games/{id}");
    let body = agent.get(&url).call().ok()?.into_string().ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    let name = value.get("name")?.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn clip(text: &str) -> String {
    text.chars().take(128).collect()
}

fn send(cmd: Cmd) {
    let guard = TX.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(cmd);
    }
}

fn run(rx: std::sync::mpsc::Receiver<Cmd>, id: String) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut current: Option<Cmd> = None;
    let mut last_attempt: Option<std::time::Instant> = None;
    loop {
        let cmd = match rx.recv_timeout(RECONNECT) {
            Ok(cmd) => Some(cmd),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Some(cmd) = cmd {
            current = Some(cmd);
        }
        let Some(current) = current.clone() else {
            continue;
        };
        let ok = match current {
            Cmd::Idle => client
                .as_mut()
                .map(|client| client.clear_activity().is_ok())
                .unwrap_or(true),
            Cmd::Playing { details, started } => {
                if !ensure(&mut client, &mut last_attempt, &id) {
                    continue;
                }
                let activity = Activity::new()
                    .details(&details)
                    .state(PRESENCE_STATE)
                    .timestamps(Timestamps::new().start(started));
                client
                    .as_mut()
                    .map(|client| client.set_activity(activity).is_ok())
                    .unwrap_or(false)
            }
        };
        if !ok {
            drop_client(&mut client);
        }
    }
    drop_client(&mut client);
}

fn ensure(
    client: &mut Option<DiscordIpcClient>,
    last_attempt: &mut Option<std::time::Instant>,
    id: &str,
) -> bool {
    if client.is_some() {
        return true;
    }
    if let Some(at) = *last_attempt {
        if at.elapsed() < RECONNECT {
            return false;
        }
    }
    *last_attempt = Some(std::time::Instant::now());
    let mut next = match DiscordIpcClient::new(id) {
        Ok(next) => next,
        Err(err) => {
            eprintln!("discord: {err}");
            return false;
        }
    };
    if let Err(err) = next.connect() {
        eprintln!("discord: {err}");
        return false;
    }
    *client = Some(next);
    true
}

fn drop_client(client: &mut Option<DiscordIpcClient>) {
    if let Some(mut client) = client.take() {
        let _ = client.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{PRESENCE_STATE, game_id, presence_details};

    #[test]
    fn game_id_comes_from_query_or_path() {
        assert_eq!(game_id("vortex://play/1"), Some(1));
        assert_eq!(game_id("vortex://play?game=4&token=abc"), Some(4));
        assert_eq!(game_id("vortex-studio://edit/1"), None);
        assert_eq!(game_id("https://playvortex.io"), None);
    }

    #[test]
    fn presence_names_the_game() {
        assert_eq!(presence_details(true, Some("ignored")), "No Studio");
        assert_eq!(presence_details(false, Some("  Parque ")), "Parque");
        assert_eq!(presence_details(false, None), "A jogar");
        assert_eq!(presence_details(false, Some("")), "A jogar");
        assert_eq!(PRESENCE_STATE, "via Vortex Launcher");
    }
}
