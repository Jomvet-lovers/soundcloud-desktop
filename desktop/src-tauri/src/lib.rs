mod app;
mod audio;
mod discord;
mod import;
mod network;
mod shared;
mod track_cache;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use discord::DiscordState;
use network::server::ServerState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("scproxy", |_ctx, request, responder| {
            let Some(state) = network::proxy::STATE.get() else {
                responder.respond(
                    http::Response::builder()
                        .status(503)
                        .body(b"not ready".to_vec())
                        .unwrap(),
                );
                return;
            };
            state.rt_handle.spawn(async move {
                responder.respond(network::proxy::handle_uri(request).await);
            });
        })
        .setup(move |app| {
            let cache_dir = app
                .path()
                .app_cache_dir()
                .expect("failed to resolve app cache dir");

            let audio_dir = cache_dir.join("audio");
            std::fs::create_dir_all(&audio_dir).ok();

            let assets_dir = cache_dir.join("assets");
            std::fs::create_dir_all(&assets_dir).ok();

            let wallpapers_dir = cache_dir.join("wallpapers");
            std::fs::create_dir_all(&wallpapers_dir).ok();

            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

            network::proxy::STATE
                .set(network::proxy::State {
                    assets_dir,
                    http_client: reqwest::Client::new(),
                    rt_handle: rt.handle().clone(),
                })
                .ok();

            let (static_port, proxy_port) = rt.block_on(network::server::start_all(wallpapers_dir));

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });

            app.manage(Arc::new(ServerState {
                static_port,
                proxy_port,
            }));
            app::diagnostics::mark_session_started(&app.handle());
            app::diagnostics::start_linux_fd_monitor(&app.handle());
            app.manage(Arc::new(DiscordState {
                client: Mutex::new(None),
            }));

            let mut track_cache_state = track_cache::init(audio_dir);
            track_cache_state.app_handle = Some(app.handle().clone());
            app.manage(track_cache_state);

            let audio_state = audio::init();
            app.manage(audio_state);
            audio::start_tick_emitter(app.handle());
            audio::start_media_controls(app.handle());
            audio::start_default_output_monitor(app.handle());

            app::tray::setup_tray(app).expect("failed to setup tray");

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            network::server::get_server_ports,
            app::diagnostics::diagnostics_log,
            discord::discord_connect,
            discord::discord_disconnect,
            discord::discord_set_activity,
            discord::discord_clear_activity,
            audio::audio_load_file,
            audio::audio_load_url,
            audio::audio_play,
            audio::audio_pause,
            audio::audio_stop,
            audio::audio_seek,
            audio::audio_set_volume,
            audio::audio_get_position,
            audio::audio_set_eq,
            audio::audio_set_normalization,
            audio::audio_is_playing,
            audio::audio_set_metadata,
            audio::audio_set_playback_state,
            audio::audio_set_media_position,
            audio::audio_list_devices,
            audio::audio_switch_device,
            audio::audio_set_follow_default_output,
            audio::audio_set_lyrics_timeline,
            audio::audio_clear_lyrics_timeline,
            audio::audio_set_comments_timeline,
            audio::audio_clear_comments_timeline,
            audio::save_track_to_path,
            import::ym_import_start,
            import::ym_import_stop,
            track_cache::track_ensure_cached,
            track_cache::track_is_cached,
            track_cache::track_get_cache_path,
            track_cache::track_preload,
            track_cache::track_cache_size,
            track_cache::track_clear_cache,
            track_cache::track_list_cached,
            track_cache::track_enforce_cache_limit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
