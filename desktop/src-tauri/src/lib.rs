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
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(not(dev))]
    let localhost_port = {
        let port = std::net::TcpListener::bind("localhost:0")
            .expect("no free port")
            .local_addr()
            .unwrap()
            .port();
        builder = builder.plugin(tauri_plugin_localhost::Builder::new(port).build());
        port
    };

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
        .register_asynchronous_uri_scheme_protocol("scproxy", |_ctx, request, responder| {
            let Some(state) = proxy::STATE.get() else {
                responder.respond(
                    http::Response::builder()
                        .status(503)
                        .body(b"not ready".to_vec())
                        .unwrap(),
                );
                return;
            };
            state.rt_handle.spawn(async move {
                responder.respond(proxy::handle_uri(request).await);
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

            proxy::STATE
                .set(proxy::State {
                    assets_dir,
                    http_client: reqwest::Client::new(),
                    rt_handle: rt.handle().clone(),
                })
                .ok();

            let (audio_port, proxy_port) = rt.block_on(server::start_all(audio_dir, wallpapers_dir));

            std::thread::spawn(move || {
                rt.block_on(std::future::pending::<()>());
            });

            app.manage(Arc::new(ServerState {
                audio_port,
                proxy_port,
            }));
            app.manage(Arc::new(DiscordState {
                client: Mutex::new(None),
            }));

            #[cfg(not(dev))]
            {
                let url: tauri::Url =
                    format!("http://localhost:{localhost_port}").parse().unwrap();
                app.get_webview_window("main").unwrap().navigate(url)?;
            }

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
