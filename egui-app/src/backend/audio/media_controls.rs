//! Media controls (SMTC on Windows / MPRIS on Linux) via souvlaki.
//!
//! Windows requires a real HWND and a message pump for SMTC button
//! callbacks, so this module creates a hidden message-only window on its own
//! thread and runs a `PeekMessage` pump alongside the command channel.

use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata as SmtcMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};

use crate::backend::audio::state::AudioState;
use crate::backend::audio::types::MediaCmd;
use crate::backend::events::EventBus;

pub fn start_media_controls(bus: EventBus, state: std::sync::Arc<AudioState>) {
    let (tx, rx) = std::sync::mpsc::channel::<MediaCmd>();

    *state.media_tx.lock().unwrap() = Some(tx);

    std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            let hwnd = create_hidden_window();
            #[cfg(not(target_os = "windows"))]
            let hwnd: Option<*mut std::ffi::c_void> = None;

            let config = PlatformConfig {
                display_name: "SoundCloud Desktop",
                dbus_name: "soundcloud_desktop",
                hwnd,
            };

            let mut controls = match MediaControls::new(config) {
                Ok(controls) => controls,
                Err(error) => {
                    eprintln!("[MediaControls] Failed to create: {error:?}");
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
                #[cfg(target_os = "windows")]
                unsafe {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{
                        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
                    };
                    let mut msg: MSG = std::mem::zeroed();
                    while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(cmd) => apply_command(&mut controls, &state, cmd),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("failed to spawn media-controls thread");
}

fn apply_command(controls: &mut MediaControls, state: &AudioState, cmd: MediaCmd) {
    match cmd {
        MediaCmd::SetMetadata {
            title,
            artist,
            cover_url,
            duration_secs,
        } => {
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
        MediaCmd::SetPlaying(playing) => {
            // get_pos() is output time; the OS expects source-timeline position.
            let pos = state
                .player
                .lock()
                .unwrap()
                .as_ref()
                .map(|player| {
                    Duration::from_secs_f64(crate::backend::audio::engine::source_pos(
                        state, player,
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
        MediaCmd::SetPosition(secs) => {
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
    }
}

/// Hidden message-only window used for SMTC interop. Never shown.
#[cfg(target_os = "windows")]
fn create_hidden_window() -> Option<*mut std::ffi::c_void> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW,
    };
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name: Vec<u16> = "SCDMediaControls\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);
        let window_name: Vec<u16> = "SCD media controls\0".encode_utf16().collect();
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            eprintln!("[MediaControls] hidden window creation failed");
            return None;
        }
        Some(hwnd as *mut std::ffi::c_void)
    }
}
