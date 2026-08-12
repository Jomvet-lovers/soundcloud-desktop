//! Десктопные системные медиа-контролы (MPRIS на Linux, SMTC на Windows,
//! NowPlaying на macOS) через `souvlaki`. **Чистая логика, без C-ABI** — наружу
//! её отдаёт единый десктоп-FFI крейт `desktop-bridge` (как `sc-bridge` для Core).
//!
//! `souvlaki::MediaControls` не `Send` — держим на выделенном потоке, команды шлём
//! каналом, поэтому тип `Send + Sync`. Медиа-клавиши (inbound) доставляются через
//! колбэк [`MediaKey`], переданный в [`DesktopMediaControls::new`].
//!
//! На Linux работает без оконного хэндла (MPRIS). Для Windows/macOS souvlaki
//! требует `hwnd`/`NSWindow` — пробросим из оболочки отдельно (TODO).

use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("media controls: {0}")]
    Init(String),
}

/// Команда от ОС (медиа-клавиши/системные контролы) → движок.
#[derive(Clone, Copy, Debug)]
pub enum MediaKey {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
}

enum Cmd {
    Metadata {
        title: String,
        artist: String,
        cover: Option<String>,
        duration_ms: i64,
    },
    Playback {
        playing: bool,
        position_ms: i64,
    },
    Clear,
}

/// Текущее состояние воспроизведения для композиции `Playback` (позицию/флаг
/// шлют разными вызовами — нужно помнить обе части).
#[derive(Default, Clone, Copy)]
struct Playstate {
    playing: bool,
    position_ms: i64,
}

/// Системные медиа-контролы. Outbound — [`set_metadata`](Self::set_metadata)/
/// [`set_playing`](Self::set_playing)/[`set_position`](Self::set_position)/
/// [`clear`](Self::clear); inbound — колбэк `on_key` из [`new`](Self::new).
pub struct DesktopMediaControls {
    tx: Mutex<Sender<Cmd>>,
    state: Mutex<Playstate>,
}

impl DesktopMediaControls {
    /// `on_key` зовётся на потоке медиа-контролов при нажатии системной клавиши.
    pub fn new(on_key: impl Fn(MediaKey) + Send + 'static) -> Result<Self, MediaError> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        thread::Builder::new()
            .name("media-controls".into())
            .spawn(move || {
                let config = PlatformConfig {
                    dbus_name: "soundcloud",
                    display_name: "SoundCloud",
                    hwnd: None,
                };
                let mut controls = match MediaControls::new(config) {
                    Ok(controls) => controls,
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("{error:?}")));
                        return;
                    }
                };
                let _ = controls.attach(move |event: MediaControlEvent| {
                    if let Some(key) = map_event(event) {
                        on_key(key);
                    }
                });
                let _ = ready_tx.send(Ok(()));

                for cmd in rx {
                    let _ = match cmd {
                        Cmd::Metadata {
                            title,
                            artist,
                            cover,
                            duration_ms,
                        } => controls.set_metadata(MediaMetadata {
                            title: Some(&title),
                            artist: Some(&artist),
                            cover_url: cover.as_deref(),
                            duration: (duration_ms > 0)
                                .then(|| Duration::from_millis(duration_ms as u64)),
                            ..Default::default()
                        }),
                        Cmd::Playback {
                            playing,
                            position_ms,
                        } => {
                            let progress = Some(MediaPosition(Duration::from_millis(
                                position_ms.max(0) as u64,
                            )));
                            controls.set_playback(if playing {
                                MediaPlayback::Playing { progress }
                            } else {
                                MediaPlayback::Paused { progress }
                            })
                        }
                        Cmd::Clear => controls.set_playback(MediaPlayback::Stopped),
                    };
                }
            })
            .map_err(|e| MediaError::Init(e.to_string()))?;

        ready_rx
            .recv()
            .map_err(|e| MediaError::Init(e.to_string()))?
            .map_err(MediaError::Init)?;
        Ok(Self {
            tx: Mutex::new(tx),
            state: Mutex::new(Playstate::default()),
        })
    }

    /// Новый трек: метаданные (с обложкой и длительностью) + сброс позиции в 0.
    pub fn set_metadata(&self, title: &str, artist: &str, cover_url: &str, duration_ms: i64) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).position_ms = 0;
        self.send(Cmd::Metadata {
            title: title.to_owned(),
            artist: artist.to_owned(),
            cover: (!cover_url.is_empty()).then(|| cover_url.to_owned()),
            duration_ms,
        });
        let playing = self.state.lock().unwrap_or_else(|p| p.into_inner()).playing;
        self.send(Cmd::Playback {
            playing,
            position_ms: 0,
        });
    }

    pub fn set_playing(&self, playing: bool) {
        let position_ms = {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.playing = playing;
            s.position_ms
        };
        self.send(Cmd::Playback {
            playing,
            position_ms,
        });
    }

    pub fn set_position(&self, position_ms: i64) {
        let playing = {
            let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
            s.position_ms = position_ms;
            s.playing
        };
        self.send(Cmd::Playback {
            playing,
            position_ms,
        });
    }

    pub fn clear(&self) {
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = Playstate::default();
        self.send(Cmd::Clear);
    }

    fn send(&self, cmd: Cmd) {
        let tx = self.tx.lock().unwrap_or_else(|poison| poison.into_inner());
        let _ = tx.send(cmd);
    }
}

fn map_event(event: MediaControlEvent) -> Option<MediaKey> {
    Some(match event {
        MediaControlEvent::Play => MediaKey::Play,
        MediaControlEvent::Pause => MediaKey::Pause,
        MediaControlEvent::Toggle => MediaKey::Toggle,
        MediaControlEvent::Next => MediaKey::Next,
        MediaControlEvent::Previous => MediaKey::Previous,
        MediaControlEvent::Stop => MediaKey::Stop,
        _ => return None,
    })
}
