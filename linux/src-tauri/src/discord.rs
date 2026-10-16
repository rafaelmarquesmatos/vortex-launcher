use std::time::Duration;

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

pub fn start() {
    let id = crate::settings::load().discord_client_id;
    if id.is_empty() {
        return;
    }
    std::thread::spawn(move || hold(&id));
}

fn hold(id: &str) {
    let mut client = match DiscordIpcClient::new(id) {
        Ok(client) => client,
        Err(err) => {
            eprintln!("discord: {err}");
            return;
        }
    };
    if let Err(err) = client.connect() {
        eprintln!("discord: {err}");
        return;
    }
    let activity = activity::Activity::new().details("Vortex Launcher");
    if let Err(err) = client.set_activity(activity) {
        eprintln!("discord: {err}");
        return;
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
