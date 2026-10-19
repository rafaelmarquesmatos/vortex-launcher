mod doctor;
mod paths;
mod runtime;
mod scheme;
mod settings;
mod update;
mod wine;

use std::sync::Mutex;

use tauri::Manager;

struct BarState {
    visible: bool,
    label: String,
    percent: u8,
}

static BAR: Mutex<BarState> = Mutex::new(BarState {
    visible: false,
    label: String::new(),
    percent: 0,
});

const TABS: &str = r#"
document.addEventListener('DOMContentLoaded', () => {
  if (location.protocol === 'data:' || location.protocol === 'launcher:' || document.getElementById('vx-tabs')) return;
  const bar = document.createElement('nav');
  bar.id = 'vx-tabs';
  bar.style.cssText = 'position:fixed;bottom:0;left:0;right:0;height:40px;display:flex;gap:8px;align-items:center;padding:0 12px;background:#0c0e13;z-index:2147483647;font:13px sans-serif';
  const site = document.createElement('button');
  site.type = 'button';
  site.textContent = 'Vortex';
  site.style.cssText = 'border:0;background:#2a3142;color:#eee;padding:6px 10px;border-radius:6px';
  const config = document.createElement('button');
  config.type = 'button';
  config.textContent = 'Config';
  config.style.cssText = 'border:0;background:transparent;color:#eee;padding:6px 10px;border-radius:6px;cursor:pointer';
  config.onclick = () => { location.href = 'launcher://localhost/'; };
  bar.append(site, config);
  document.body.appendChild(bar);
  document.body.style.paddingBottom = '48px';
});
"#;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let primary = scheme::claim();
    tauri::Builder::default()
        .register_uri_scheme_protocol("launcher", |_ctx, request| {
            let uri = request.uri();
            settings::respond(uri.path(), uri.query().unwrap_or(""))
        })
        .setup(move |app| {
            scheme::register();
            let scheme::Primary { listener, pending } = primary;
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let mut window = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .expect("main window");
            let prepare_wine = !wine::installed();
            if prepare_wine {
                window.url = tauri::WebviewUrl::External(
                    "launcher://localhost/boot".parse().expect("boot url"),
                );
            }
            let handle = app.handle().clone();
            let popups = handle.clone();
            let main = tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?
                .initialization_script(TABS)
                .on_page_load(|window, payload| {
                    apply_bar(&window);
                    if payload.event() != tauri::webview::PageLoadEvent::Finished {
                        return;
                    }
                    if !runtime::is_studio_launch_page(payload.url().as_str()) {
                        return;
                    }
                    let Ok(home) = tauri::Url::parse("https://playvortex.io/home") else {
                        return;
                    };
                    let _ = window.navigate(home);
                })
                .on_navigation(move |url| !take_url(&handle, url.as_str()))
                .on_new_window(move |url, _features| {
                    if take_url(&popups, url.as_str()) {
                        tauri::webview::NewWindowResponse::Deny
                    } else {
                        tauri::webview::NewWindowResponse::Allow
                    }
                })
                .build()?;
            if prepare_wine {
                let boot = main.app_handle().clone();
                std::thread::spawn(move || prepare_wine_then_open(boot));
            } else {
                spawn_update(main.app_handle().clone());
            }
            let app_handle = main.app_handle().clone();
            if let Some(listener) = listener {
                let background = app_handle.clone();
                std::thread::spawn(move || {
                    for conn in listener.incoming().flatten() {
                        match scheme::incoming(conn) {
                            Some(scheme::Incoming::Focus) => show_main(&background),
                            Some(scheme::Incoming::Launch(url)) => open_url(&background, &url),
                            None => {}
                        }
                    }
                });
            }
            if let Some(url) = pending {
                open_url(&app_handle, &url);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

fn with_main<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
    f: impl FnOnce(tauri::WebviewWindow<R>) + Send + 'static,
) {
    let handle = handle.clone();
    let _ = handle.clone().run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            f(window);
        }
    });
}

fn navigate_main<R: tauri::Runtime>(handle: &tauri::AppHandle<R>, url: &'static str) {
    with_main(handle, move |window| {
        let Ok(url) = tauri::Url::parse(url) else {
            return;
        };
        let _ = window.navigate(url);
    });
}

fn hide_main<R: tauri::Runtime>(handle: &tauri::AppHandle<R>) {
    with_main(handle, |window| {
        let _ = window.hide();
    });
}

fn show_main<R: tauri::Runtime>(handle: &tauri::AppHandle<R>) {
    with_main(handle, |window| {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    });
}

fn take_url<R: tauri::Runtime>(handle: &tauri::AppHandle<R>, url: &str) -> bool {
    if runtime::is_download_page(url) {
        spawn_launch(handle.clone(), "vortex-studio://".to_string(), false);
        return true;
    }
    let url = runtime::normalize_launch_url(url);
    if runtime::launch_scheme(&url).is_some() {
        open_url(handle, &url);
        return true;
    }
    false
}

fn open_url<R: tauri::Runtime>(handle: &tauri::AppHandle<R>, url: &str) {
    let studio = url.starts_with("vortex-studio://");
    spawn_launch(
        handle.clone(),
        url.to_string(),
        url.starts_with("vortex://"),
    );
    if studio {
        go_home(handle);
    }
}

fn prepare_wine_then_open<R: tauri::Runtime>(handle: tauri::AppHandle<R>) {
    let report = {
        let ui = handle.clone();
        move |label: &str, percent: u8| set_progress(&ui, label, percent)
    };
    match wine::binary(&report) {
        Ok(_) => {
            clear_progress(&handle);
            navigate_main(&handle, "https://playvortex.io");
        }
        Err(err) => {
            set_progress(&handle, &format!("Falhou: {err}"), 0);
            eprintln!("wine: {err}");
        }
    }
    spawn_update(handle);
}

fn spawn_update<R: tauri::Runtime>(handle: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let report = {
            let ui = handle.clone();
            move |label: &str, percent: u8| set_progress(&ui, label, percent)
        };
        match update::check(&report) {
            Ok(Some(path)) => match update::apply(&path) {
                Ok(update::Applied::Restarting) => handle.exit(0),
                Ok(update::Applied::Saved(path)) => set_progress(
                    &handle,
                    &format!("Atualização baixada em {}", path.display()),
                    100,
                ),
                Err(err) => {
                    eprintln!("update: {err}");
                    set_progress(&handle, &format!("Falhou: {err}"), 0);
                }
            },
            Ok(None) => {}
            Err(err) => eprintln!("update: {err}"),
        }
    });
}

fn go_home<R: tauri::Runtime>(handle: &tauri::AppHandle<R>) {
    navigate_main(handle, "https://playvortex.io/home");
}

fn spawn_launch<R: tauri::Runtime>(handle: tauri::AppHandle<R>, url: String, instance: bool) {
    std::thread::spawn(move || {
        let report = {
            let ui = handle.clone();
            move |label: &str, percent: u8| set_progress(&ui, label, percent)
        };
        let mut child = match runtime::launch(&url, &report) {
            Ok(child) => {
                clear_progress(&handle);
                child
            }
            Err(err) => {
                set_progress(&handle, &format!("Falhou: {err}"), 0);
                eprintln!("launch: {err}");
                return;
            }
        };
        if instance {
            let current = settings::load();
            if current.close_launcher_on_instance {
                hide_main(&handle);
            }
            let mut cursor = runtime::LogCursor::new();
            loop {
                if current.open_client_on_instance_exit && cursor.poll() {
                    runtime::stop_client(&mut child);
                    break;
                }
                match child.try_wait() {
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(300)),
                    _ => break,
                }
            }
            if current.close_launcher_on_instance || current.open_client_on_instance_exit {
                show_main(&handle);
            }
            return;
        }
        let current = settings::load();
        if current.close_launcher_on_studio {
            hide_main(&handle);
        }
        let _ = child.wait();
        if current.close_launcher_on_studio {
            show_main(&handle);
        }
    });
}

fn bar_script(label: &str, percent: u8) -> String {
    let label = serde_json::to_string(label).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(() => {{
      let root = document.getElementById('vx-progress');
      if (!root) {{
        root = document.createElement('div');
        root.id = 'vx-progress';
        root.style.cssText = 'position:fixed;left:0;right:0;bottom:48px;z-index:2147483646;padding:12px 16px;background:#0c0e13;color:#eee;font:13px sans-serif';
        root.innerHTML = '<div id="vx-progress-label"></div><div style="margin-top:8px;height:8px;background:#3a4150;border-radius:999px;overflow:hidden"><div id="vx-progress-bar" style="height:100%;width:0;background:#4c8dff"></div></div>';
        (document.body || document.documentElement).appendChild(root);
      }}
      const label = document.getElementById('vx-progress-label');
      const bar = document.getElementById('vx-progress-bar');
      if (label) label.textContent = {label};
      if (bar) bar.style.width = '{percent}%';
    }})()"#
    )
}

fn apply_bar(window: &tauri::WebviewWindow) {
    let bar = BAR.lock().unwrap_or_else(|err| err.into_inner());
    if !bar.visible {
        return;
    }
    let script = bar_script(&bar.label, bar.percent);
    drop(bar);
    let _ = window.eval(script);
}

fn set_progress<R: tauri::Runtime>(handle: &tauri::AppHandle<R>, label: &str, percent: u8) {
    let mut bar = BAR.lock().unwrap_or_else(|err| err.into_inner());
    let first = !bar.visible;
    bar.visible = true;
    bar.label = label.to_string();
    bar.percent = percent;
    let script = bar_script(&bar.label, bar.percent);
    drop(bar);
    with_main(handle, move |window| {
        if first {
            let _ = window.show();
            let _ = window.unminimize();
        }
        let _ = window.eval(script);
    });
}

fn clear_progress<R: tauri::Runtime>(handle: &tauri::AppHandle<R>) {
    let mut bar = BAR.lock().unwrap_or_else(|err| err.into_inner());
    bar.visible = false;
    drop(bar);
    with_main(handle, |window| {
        let _ = window.eval("document.getElementById('vx-progress')?.remove()");
    });
}
