mod audio_server;
mod constants;
mod discord;
mod proxy;
mod proxy_server;
mod server;
mod tray;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use discord::DiscordState;
use server::ServerState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let localhost_port = std::net::TcpListener::bind("localhost:0")
        .expect("no free port")
        .local_addr()
        .unwrap()
        .port();

    tauri::Builder::default()
        .plugin(tauri_plugin_localhost::Builder::new(localhost_port).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .setup(move |app| {
            let cache_dir = app
                .path()
                .app_cache_dir()
                .expect("failed to resolve app cache dir");

            let audio_dir = cache_dir.join("audio");
            std::fs::create_dir_all(&audio_dir).ok();

            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            let (audio_port, proxy_port) = rt.block_on(server::start_all(audio_dir));

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });

            app.manage(Arc::new(ServerState { audio_port, proxy_port }));
            app.manage(Arc::new(DiscordState {
                client: Mutex::new(None),
            }));

            let url: tauri::Url = format!("http://localhost:{localhost_port}").parse().unwrap();
            app.get_webview_window("main").unwrap().navigate(url)?;

            tray::setup_tray(app).expect("failed to setup tray");

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            server::get_server_ports,
            discord::discord_connect,
            discord::discord_disconnect,
            discord::discord_set_activity,
            discord::discord_clear_activity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}