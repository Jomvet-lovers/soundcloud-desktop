mod constants;
mod discord;
mod proxy;
mod server;
mod tray;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use discord::DiscordState;
use server::CacheServerState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            let cache_dir = app
                .path()
                .app_cache_dir()
                .expect("failed to resolve app cache dir");

            let audio_dir = cache_dir.join("audio");
            std::fs::create_dir_all(&audio_dir).ok();

            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            let port = rt.block_on(server::start_cache_server(audio_dir.clone()));

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });

            app.manage(Arc::new(CacheServerState { port }));
            app.manage(Arc::new(DiscordState {
                client: Mutex::new(None),
            }));

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
            server::get_cache_server_port,
            discord::discord_connect,
            discord::discord_disconnect,
            discord::discord_set_activity,
            discord::discord_clear_activity,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
