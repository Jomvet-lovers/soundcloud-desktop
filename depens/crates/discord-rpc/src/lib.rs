//! Discord Rich Presence (десктоп) через `discord-rich-presence`. **Чистая
//! логика, без C-ABI** — наружу её отдаёт `desktop-bridge` (как `sc-bridge`).
//!
//! Зеркалит медиа-хуки движка ([`set_now_playing`](DiscordPresence::set_now_playing)/
//! [`set_playing`](DiscordPresence::set_playing)/[`set_position`](DiscordPresence::set_position)/
//! [`clear`](DiscordPresence::clear)) и пушит активность «Listening to …» с
//! обложкой, кнопкой и таймстемпами (Discord сам тикает прогресс по start/end).
//! Discord не запущен — всё no-op; при обрыве IPC соединение сбрасывается.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use discord_rich_presence::{
    DiscordIpc, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, Button, Timestamps},
};

#[derive(Default)]
struct Meta {
    title: String,
    artist: String,
    cover: Option<String>,
    url: Option<String>,
    duration_secs: i64,
    position_secs: i64,
    playing: bool,
}

pub struct DiscordPresence {
    client_id: String,
    client: Mutex<Option<DiscordIpcClient>>,
    meta: Mutex<Meta>,
}

impl DiscordPresence {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client: Mutex::new(None),
            meta: Mutex::new(Meta::default()),
        }
    }

    /// Подключиться к Discord IPC (идемпотентно). `false` — Discord не запущен.
    pub fn connect(&self) -> bool {
        let mut guard = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_some() {
            return true;
        }
        let mut client = DiscordIpcClient::new(&self.client_id);
        match client.connect() {
            Ok(()) => {
                *guard = Some(client);
                true
            }
            Err(_) => false,
        }
    }

    pub fn set_now_playing(
        &self,
        title: &str,
        artist: &str,
        cover_url: &str,
        track_url: &str,
        duration_secs: i64,
    ) {
        {
            let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
            meta.title = title.to_owned();
            meta.artist = artist.to_owned();
            meta.cover = (!cover_url.is_empty()).then(|| cover_url.to_owned());
            meta.url = (!track_url.is_empty()).then(|| track_url.to_owned());
            meta.duration_secs = duration_secs;
            meta.position_secs = 0;
            meta.playing = true;
        }
        self.push();
    }

    pub fn set_playing(&self, playing: bool) {
        self.meta.lock().unwrap_or_else(|p| p.into_inner()).playing = playing;
        self.push();
    }

    /// Тик позиции — копим без пуша (Discord rate-limited; таймстемпы пересчитаем
    /// на следующем play/pause/смене трека по сохранённой позиции).
    pub fn set_position(&self, position_secs: i64) {
        self.meta.lock().unwrap_or_else(|p| p.into_inner()).position_secs = position_secs;
    }

    pub fn clear(&self) {
        *self.meta.lock().unwrap_or_else(|p| p.into_inner()) = Meta::default();
        let mut guard = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(client) = guard.as_mut() {
            let _ = client.clear_activity();
        }
    }

    /// Перепушить активность из меты. Обрыв IPC → сбрасываем клиент (переподключимся).
    fn push(&self) {
        let meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        let mut guard = self.client.lock().unwrap_or_else(|p| p.into_inner());
        let Some(client) = guard.as_mut() else {
            return;
        };

        let large_image = meta.cover.as_deref().unwrap_or("soundcloud_logo");
        let mut activity = Activity::new()
            .activity_type(ActivityType::Listening)
            .assets(Assets::new().large_image(large_image))
            .details(&meta.title)
            .state(if meta.playing {
                meta.artist.as_str()
            } else {
                "На паузе"
            });

        // Live-прогресс: start..end, Discord тикает сам (только пока играет).
        if meta.playing && meta.duration_secs > 0 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let start = now - meta.position_secs.max(0);
            activity = activity
                .timestamps(Timestamps::new().start(start).end(start + meta.duration_secs));
        }

        let buttons = meta
            .url
            .as_deref()
            .map(|url| vec![Button::new("Слушать в SoundCloud", url)]);
        if let Some(buttons) = buttons {
            activity = activity.buttons(buttons);
        }

        if client.set_activity(activity).is_err() {
            *guard = None;
        }
    }
}
