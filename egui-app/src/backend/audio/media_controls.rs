use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata as SmtcMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

use crate::backend::audio::state::AudioState;
use crate::backend::audio::types::MediaCmd;
use crate::backend::events::EventBus;

// TODO(egui-port): 元は `start_media_controls(app: &AppHandle)` で state を
// `handle.state::<AudioState>()` から取得。Tauri の managed-state が無いため
// `bus: EventBus` + `state: Arc<AudioState>` を Phase 2 の `boot()` から明示的に渡す.
pub fn start_media_controls(bus: EventBus, state: std::sync::Arc<AudioState>) {
    let (tx, rx) = std::sync::mpsc::channel::<MediaCmd>();

    *state.media_tx.lock().unwrap() = Some(tx);

    std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            // TODO(egui-port): Tauri の `get_webview_window("main").window_handle()`
            // (Win32 HWND) 相当が無いため `hwnd=None` に縮退。SMTC は hwnd 無しでも
            // 動作する。Phase 2/4 で eframe/winit のネイティブハンドル配線時に復元する.
            #[cfg(not(target_os = "windows"))]
            let hwnd = None;

            #[cfg(target_os = "windows")]
            let hwnd: Option<*mut std::ffi::c_void> = None;

            let config = PlatformConfig {
                display_name: "SoundCloud Desktop",
                dbus_name: "soundcloud_desktop",
                hwnd,
            };

            let mut controls = match MediaControls::new(config) {
                Ok(controls) => controls,
                Err(error) => {
                    eprintln!("[MediaControls] Failed to create: {:?}", error);
                    return;
                }
            };

            let event_bus = bus.clone();
            controls
                .attach(move |event: MediaControlEvent| match event {
                    MediaControlEvent::Play => {
                        event_bus.emit("media:play", &());
                    }
                    MediaControlEvent::Pause => {
                        event_bus.emit("media:pause", &());
                    }
                    MediaControlEvent::Toggle => {
                        event_bus.emit("media:toggle", &());
                    }
                    MediaControlEvent::Next => {
                        event_bus.emit("media:next", &());
                    }
                    MediaControlEvent::Previous => {
                        event_bus.emit("media:prev", &());
                    }
                    MediaControlEvent::SetPosition(MediaPosition(pos)) => {
                        event_bus.emit("media:seek", &pos.as_secs_f64());
                    }
                    MediaControlEvent::Seek(dir) => {
                        let offset = match dir {
                            souvlaki::SeekDirection::Forward => 10.0,
                            souvlaki::SeekDirection::Backward => -10.0,
                        };
                        event_bus.emit("media:seek-relative", &offset);
                    }
                    _ => {}
                })
                .ok();

            loop {
                match rx.recv() {
                    Ok(MediaCmd::SetMetadata {
                        title,
                        artist,
                        cover_url,
                        duration_secs,
                    }) => {
                        controls
                            .set_metadata(SmtcMetadata {
                                title: Some(&title),
                                artist: Some(&artist),
                                cover_url: cover_url.as_deref(),
                                duration: if duration_secs > 0.0 {
                                    Some(Duration::from_secs_f64(duration_secs))
                                } else {
                                    None
                                },
                                ..Default::default()
                            })
                            .ok();
                    }
                    Ok(MediaCmd::SetPlaying(playing)) => {
                        // get_pos() is output time; the OS expects source-timeline position.
                        let pos = state
                            .player
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|player| {
                                Duration::from_secs_f64(crate::backend::audio::engine::source_pos(
                                    &state, player,
                                ))
                            })
                            .unwrap_or_default();
                        let progress = Some(MediaPosition(pos));
                        let playback = if playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        controls.set_playback(playback).ok();
                    }
                    Ok(MediaCmd::SetPosition(secs)) => {
                        let is_playing = state
                            .player
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|player| !player.is_paused() && !player.empty())
                            .unwrap_or(false);
                        let progress = Some(MediaPosition(Duration::from_secs_f64(secs)));
                        let playback = if is_playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        controls.set_playback(playback).ok();
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn media-controls thread");
}
