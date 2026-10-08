//! Phase 2: backend boot。`desktop/src-tauri/src/lib.rs` の `setup()` に対応。
//!
//! 差分:
//! - Tauri の `manage`/`invoke_handler`/tray の代わりに `BootHandle` を返す。
//! - writer WebView (Phase 4) と tray (Phase 5) は起動しない。
//! - オーディオデバイスが無い環境では `audio=None` で起動継続する
//!   (UI は閲覧専用になる)。

use std::sync::{Arc, Mutex};

use crate::backend::app::diagnostics;
use crate::backend::audio::{self, state::AudioState};
use crate::backend::auth::SessionStore;
use crate::backend::direct::DirectState;
use crate::backend::discord::DiscordState;
use crate::backend::events::EventBus;
use crate::backend::{network, paths};

pub struct ServerPorts {
    pub static_port: u16,
    pub proxy_port: u16,
    pub api_port: u16,
}

pub struct BootHandle {
    pub bus: EventBus,
    pub audio: Option<Arc<AudioState>>,
    pub audio_error: Option<String>,
    pub track_cache: crate::backend::track_cache::TrackCacheState,
    pub direct: Arc<DirectState>,
    pub session: Arc<SessionStore>,
    pub discord: Arc<DiscordState>,
    pub servers: ServerPorts,
}

pub fn boot(rt: &tokio::runtime::Runtime, bus: EventBus) -> Result<BootHandle, String> {
    paths::ensure_dirs().map_err(|e| format!("dirs: {e}"))?;
    let cache_dir = paths::app_cache_dir();
    let data_dir = paths::app_data_dir();

    for dir in ["audio", "audio_liked", "audio_incoming", "assets", "wallpapers"]
        .into_iter()
        .map(|d| cache_dir.join(d))
    {
        std::fs::create_dir_all(&dir).ok();
    }
    std::fs::create_dir_all(data_dir.join("images")).ok();

    network::edge::init(data_dir.clone());

    let http_client = sc_fingerprint::client(None)
        .map(|c| (*c).clone())
        .map_err(|e| format!("http client: {e:?}"))?;

    network::proxy::STATE
        .set(network::proxy::State {
            assets_dir: cache_dir.join("assets"),
            http_client: http_client.clone(),
            rt_handle: rt.handle().clone(),
        })
        .ok();

    network::image_cache::STATE
        .set(network::image_cache::ImageCache {
            dir: data_dir.join("images"),
            http_client: http_client.clone(),
        })
        .ok();

    let (static_port, proxy_port) =
        rt.block_on(network::server::start_all(cache_dir.join("wallpapers")));

    let direct = DirectState::init(data_dir.clone(), http_client.clone(), bus.clone());
    let api_port = rt.block_on(crate::backend::direct::routes::start(direct.clone()));

    let session = SessionStore::init(data_dir.clone(), http_client.clone(), rt.handle().clone());
    let discord = Arc::new(DiscordState {
        client: Mutex::new(None),
    });

    let ffmpeg_dir = cache_dir.join("ffmpeg");
    std::fs::create_dir_all(&ffmpeg_dir).ok();
    let mut track_cache = crate::backend::track_cache::init(
        cache_dir.join("audio"),
        cache_dir.join("audio_liked"),
        cache_dir.join("audio_incoming"),
    );
    track_cache.set_app_handle(bus.clone());
    let recovery = track_cache.clone();
    rt.spawn(async move {
        recovery.init_ffmpeg(ffmpeg_dir).await;
        recovery.recover_incoming().await;
    });

    let (audio, audio_error) =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(audio::init)) {
            Ok(state) => {
                let state = Arc::new(state);
                audio::start_tick_emitter(bus.clone(), state.clone());
                // Phase 4 で復元: souvlaki は Windows で HWND 必須のため、
                // eframe/winit ハンドルが取れるまで起動しない。
                // audio::start_media_controls(bus.clone(), state.clone());
                audio::start_default_output_monitor(bus.clone(), state.clone());
                audio::start_fft_thread(bus.clone(), state.analyser_buffer.clone());
                (Some(state), None)
            }
            Err(_) => (None, Some("no audio output device".to_string())),
        };

    diagnostics::mark_session_started();
    diagnostics::start_linux_fd_monitor();

    Ok(BootHandle {
        bus,
        audio,
        audio_error,
        track_cache,
        direct,
        session,
        discord,
        servers: ServerPorts {
            static_port,
            proxy_port,
            api_port,
        },
    })
}
